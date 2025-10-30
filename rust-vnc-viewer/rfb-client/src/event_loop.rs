//! Event loop coordination: read loop, write loop, and reconnection logic.

use crate::{
    config::Config,
    connection,
    errors::RfbClientError,
    framebuffer::Framebuffer,
    messages::{ClientCommand, ServerEvent},
    protocol,
    FramebufferHandle,
};
use rfb_encodings::ContentCache;
use std::sync::Mutex;
use rfb_protocol::messages::server::FramebufferUpdate;
use std::sync::Arc;
use tokio::select;
use tokio::task::JoinHandle;

/// Spawn the client event loop.
///
/// This establishes a connection, creates the framebuffer, and starts read/write loops.
/// Returns both the join handle and the shared framebuffer handle.
pub async fn spawn(
    config: Config,
    commands: flume::Receiver<ClientCommand>,
    events: flume::Sender<ServerEvent>,
) -> Result<(JoinHandle<()>, FramebufferHandle), RfbClientError> {
    // Establish connection and get streams + server init
    let conn = connection::establish(&config).await?;
    let width = conn.server_init.framebuffer_width;
    let height = conn.server_init.framebuffer_height;
    let name = conn.server_init.name.clone();
    let pixel_format = conn.server_init.pixel_format.clone();

    // Initialize shared framebuffer with server pixel format and optional caches
    let framebuffer = if config.persistent_cache.enabled {
        let pcache = Arc::new(Mutex::new(rfb_encodings::PersistentClientCache::new(config.persistent_cache.size_mb)));
        Arc::new(tokio::sync::Mutex::new(
            Framebuffer::with_persistent_cache(width, height, pixel_format.clone(), pcache)
        ))
    } else if config.content_cache.enabled {
        // Create ContentCache instance
        let cache = Arc::new(Mutex::new(ContentCache::new(config.content_cache.size_mb)));
        Arc::new(tokio::sync::Mutex::new(
            Framebuffer::with_content_cache(width, height, pixel_format.clone(), cache)
        ))
    } else {
        Arc::new(tokio::sync::Mutex::new(
            Framebuffer::new(width, height, pixel_format.clone())
        ))
    };
    let framebuffer_handle = framebuffer.clone();

    // Notify application of successful connection
    let _ = events.send(ServerEvent::Connected {
        width,
        height,
        name,
        pixel_format: pixel_format.clone(),
    });

    // Split streams for loops (they are already buffered types)
    let mut input = conn.input; // RfbInStream<...>
    let mut output = conn.output; // RfbOutStream<...>

    // Capture config values needed in the spawned task
    let encodings = config.effective_encodings();
    let fb_width = width;
    let fb_height = height;

    // Spawn a task to run the main loop (read + write via select)
    let handle = tokio::spawn(async move {
        use std::time::Instant;
        let start_time = Instant::now();
        // Periodic incremental update requester (best-effort)
        let mut periodic = tokio::time::interval(std::time::Duration::from_millis(250));
        let mut last_update = Instant::now();
        let mut last_request = Instant::now();

        // Send initial protocol messages from within the task
        // 1) SetEncodings (C++ sends this before SetPixelFormat)
        tracing::info!("Sending SetEncodings: {:?}", encodings);
        if let Err(e) = protocol::write_set_encodings(&mut output, encodings).await {
            tracing::error!("Failed to send SetEncodings: {}", e);
            return;
        }

        // 2) SetPixelFormat to 32bpp true-color little-endian RGB888 (like C++ viewer)
        let desired_pf = rfb_protocol::messages::types::PixelFormat {
            bits_per_pixel: 32,
            depth: 24,
            big_endian: 0,
            true_color: 1,
            red_max: 255,
            green_max: 255,
            blue_max: 255,
            red_shift: 16,
            green_shift: 8,
            blue_shift: 0,
        };
        if let Err(e) = protocol::write_set_pixel_format(&mut output, desired_pf).await {
            tracing::error!("Failed to send SetPixelFormat: {}", e);
            return;
        }

        // 3) Enable ContinuousUpdates (server-driven updates) over full framebuffer
        // Matches C++ viewer behavior and helps drive higher update cadence
        if let Err(e) = protocol::write_enable_continuous_updates(&mut output, true, 0, 0, fb_width, fb_height).await {
            tracing::warn!("Failed to enable ContinuousUpdates: {}", e);
        } else {
            tracing::debug!("Enabled ContinuousUpdates over 0,0 {}x{}", fb_width, fb_height);
        }

        // 4) Request initial full framebuffer update
        tracing::info!("Requesting initial framebuffer update: {}x{}", fb_width, fb_height);
        if let Err(e) = protocol::write_framebuffer_update_request(&mut output, false, 0, 0, fb_width, fb_height).await {
            tracing::error!("Failed to send FramebufferUpdateRequest: {}", e);
            return;
        }

        // Enter main loop directly; rely on timeout-based nudging to drive updates
        tracing::info!("MAIN: entering main loop");
        let mut iteration = 0u64;
        let mut last_heartbeat = Instant::now();
        loop {
            iteration = iteration.wrapping_add(1);
            if last_heartbeat.elapsed() > std::time::Duration::from_secs(2) {
                tracing::info!("MAIN: alive iter={} last_update={}ms last_request={}ms", iteration, last_update.elapsed().as_millis(), last_request.elapsed().as_millis());
                last_heartbeat = Instant::now();
            }
            // Read next server message with a timeout; prefer high-level parsing for 0..3
            tracing::info!("MAIN: iter={} waiting for server message", iteration);
            match tokio::time::timeout(std::time::Duration::from_millis(500), protocol::read_server_message(&mut input)).await {
                Ok(Ok(msg)) => {
                    use crate::protocol::IncomingMessage::*;
                    match msg {
                        FramebufferUpdate(update) => {
                            tracing::info!("MAIN: got FramebufferUpdate ({} rects)", update.rectangles.len());
                            // Pipeline next incremental request, then decode
                            let _ = protocol::write_framebuffer_update_request(&mut output, true, 0, 0, fb_width, fb_height).await;
                            if let Err(e) = handle_framebuffer_update(&framebuffer, &mut input, update, &events).await {
                                let _ = events.send(ServerEvent::Error { message: e.to_string() });
                                let _ = events.send(ServerEvent::ConnectionClosed);
                                break;
                            }
                            last_update = Instant::now();
                        }
                        SetColorMapEntries(_cm) => {
                            // Nudge the server to keep updates flowing
                            let _ = protocol::write_framebuffer_update_request(&mut output, true, 0, 0, fb_width, fb_height).await;
                            last_request = Instant::now();
                        }
                        Bell(_) => {
                            let _ = events.send(ServerEvent::Bell);
                            let _ = protocol::write_framebuffer_update_request(&mut output, true, 0, 0, fb_width, fb_height).await;
                            last_request = Instant::now();
                        }
                        ServerCutText(sc) => {
                            use bytes::Bytes;
                            let _ = events.send(ServerEvent::ServerCutText { text: Bytes::from(sc.text) });
                            let _ = protocol::write_framebuffer_update_request(&mut output, true, 0, 0, fb_width, fb_height).await;
                            last_request = Instant::now();
                        }
                    }
                }
                Ok(Err(crate::errors::RfbClientError::UnexpectedMessage(s))) => {
                    // Likely a non-0..3 message (e.g., 150/248). Extract the type number.
                    tracing::info!("MAIN: UnexpectedMessage from read_server_message: {}", s);
                    let mt_opt = s.split(':').last().and_then(|t| t.trim().parse::<u32>().ok());
                    if let Some(mt) = mt_opt {
                        match mt {
                            150 => {
                                tracing::info!("MAIN: treating as EndOfContinuousUpdates (150) -> re-enabling CU and requesting FULL+incremental");
                                let _ = protocol::write_enable_continuous_updates(&mut output, true, 0, 0, fb_width, fb_height).await;
                                let _ = protocol::write_framebuffer_update_request(&mut output, false, 0, 0, fb_width, fb_height).await;
                                let _ = protocol::write_framebuffer_update_request(&mut output, true, 0, 0, fb_width, fb_height).await;
                                last_request = Instant::now();
                            }
                            248 => {
                                tracing::info!("MAIN: treating as ServerFence (248) -> re-enabling CU and requesting FULL+incremental");
                                use tokio::io::AsyncReadExt as _;
                                let _ = input.skip(3).await; // padding
                                if let Ok(_flags) = input.read_u32().await {
                                    if let Ok(len) = input.read_u8().await {
                                        let mut buf = vec![0u8; len as usize];
                                        let _ = input.read_bytes(&mut buf).await;
                                    }
                                }
                                let _ = protocol::write_enable_continuous_updates(&mut output, true, 0, 0, fb_width, fb_height).await;
                                let _ = protocol::write_framebuffer_update_request(&mut output, false, 0, 0, fb_width, fb_height).await;
                                let _ = protocol::write_framebuffer_update_request(&mut output, true, 0, 0, fb_width, fb_height).await;
                                last_request = Instant::now();
                            }
                            other => {
                                tracing::debug!("MAIN: unsupported server message type {} (ignored)", other);
                            }
                        }
                    } else {
                        tracing::debug!("MAIN: could not parse message type from UnexpectedMessage: {}", s);
                    }
                }
                Ok(Err(e)) => {
                    // Report and exit on error (fail-fast)
                    tracing::info!("MAIN: exiting on read error: {}", e);
                    let _ = events.send(ServerEvent::Error { message: e.to_string() });
                    let _ = events.send(ServerEvent::ConnectionClosed);
                    break;
                }
                Err(_elapsed) => {
                    // Timeout waiting for server message: proactively request an update
                    tracing::info!("MAIN: timeout 500ms -> sending {} FBU req", if last_update.elapsed() > std::time::Duration::from_secs(1) { "FULL" } else { "incremental" });
                    let _ = protocol::write_framebuffer_update_request(
                        &mut output,
                        !(last_update.elapsed() > std::time::Duration::from_secs(1)),
                        0, 0, fb_width, fb_height
                    ).await;
                    last_request = Instant::now();
                    if let Ok(command) = commands.try_recv() {
                        if let Err(e) = handle_command(&mut output, &events, command).await {
                            tracing::info!("MAIN: exiting on command error: {}", e);
                            let _ = events.send(ServerEvent::Error { message: e.to_string() });
                            let _ = events.send(ServerEvent::ConnectionClosed);
                            break;
                        }
                    }
                }
            }
        }
    });

    Ok((handle, framebuffer_handle))
}

async fn handle_framebuffer_update<R: tokio::io::AsyncRead + Unpin>(
    framebuffer: &FramebufferHandle,
    input: &mut rfb_protocol::io::RfbInStream<R>,
    update: FramebufferUpdate,
    events: &flume::Sender<ServerEvent>,
) -> Result<(), RfbClientError> {
    // Apply all rectangles using decoders
    let damage = {
        let mut fb = framebuffer.lock().await;
        fb.apply_update(input, &update.rectangles).await?
    };
    if !damage.is_empty() {
        let _ = events.send(ServerEvent::FramebufferUpdated { damage });
    }
    Ok(())
}

async fn handle_command<W: tokio::io::AsyncWrite + Unpin>(
    output: &mut rfb_protocol::io::RfbOutStream<W>,
    events: &flume::Sender<ServerEvent>,
    command: ClientCommand,
) -> Result<(), RfbClientError> {
    match command {
        ClientCommand::RequestUpdate { incremental, rect } => {
            let (x, y, w, h) = match rect {
                Some(r) => (r.x as u16, r.y as u16, r.width as u16, r.height as u16),
                None => (0, 0, u16::MAX, u16::MAX),
            };
            protocol::write_framebuffer_update_request(output, incremental, x, y, w, h).await?;
        }
        ClientCommand::Pointer { x, y, buttons } => {
            protocol::write_pointer_event(output, buttons, x, y).await?;
        }
        ClientCommand::Key { key, down } => {
            protocol::write_key_event(output, key, down).await?;
        }
        ClientCommand::ClientCutText { text } => {
            let s = String::from_utf8_lossy(&text).to_string();
            protocol::write_client_cut_text(output, &s).await?;
        }
        ClientCommand::Close => {
            // Graceful shutdown: notify and return error to break loop
            let _ = events.send(ServerEvent::ConnectionClosed);
            return Err(RfbClientError::ConnectionClosed);
        }
    }
    Ok(())
}
