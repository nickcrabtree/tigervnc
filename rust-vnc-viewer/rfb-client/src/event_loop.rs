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
        // Periodic incremental update requester (best-effort)
        let mut periodic = tokio::time::interval(std::time::Duration::from_millis(250));

        // Send initial protocol messages from within the task
        // 1) SetPixelFormat to 32bpp true-color little-endian RGB888 (like C++ viewer)
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

        // 2) SetEncodings
        tracing::info!("Sending SetEncodings: {:?}", encodings);
        if let Err(e) = protocol::write_set_encodings(&mut output, encodings).await {
            tracing::error!("Failed to send SetEncodings: {}", e);
            return;
        }

        // 3) Request initial full framebuffer update
        tracing::info!("Requesting initial framebuffer update: {}x{}", fb_width, fb_height);
        if let Err(e) = protocol::write_framebuffer_update_request(&mut output, false, 0, 0, fb_width, fb_height).await {
            tracing::error!("Failed to send FramebufferUpdateRequest: {}", e);
            return;
        }

        tracing::info!("Event loop task started, entering main loop");
        // Use async recv to avoid blocking
        let mut iteration = 0u64;
        loop {
            if iteration % 100 == 1 {
                tracing::debug!("Event loop iteration {}", iteration);
            }
            select! {
                // Prefer reading server messages to keep buffers flowing
                res = protocol::read_message_type(&mut input) => {
                    match res {
                        Ok(msg_type) => {
                            tracing::debug!("Server message type: {}", msg_type);
                            match msg_type {
                                0 => {
                                    // FramebufferUpdate: pipeline next incremental request, then stream-decode
                                    tracing::debug!("Pipelining incremental FramebufferUpdateRequest");
                                    let _ = protocol::write_framebuffer_update_request(&mut output, true, 0, 0, fb_width, fb_height).await;
                                    let damage = {
                                        let mut fb = framebuffer.lock().await;
                                        match fb.apply_update_stream(&mut input).await {
                                            Ok(d) => d,
                                            Err(e) => {
                                                let _ = events.send(ServerEvent::Error { message: e.to_string() });
                                                let _ = events.send(ServerEvent::ConnectionClosed);
                                                break;
                                            }
                                        }
                                    };
                                    if !damage.is_empty() {
                                        let _ = events.send(ServerEvent::FramebufferUpdated { damage });
                                    }
                                }
                                1 => {
                                    // SetColorMapEntries - currently ignored
                                    // We still need to consume the payload to stay in sync
                                    let _ = rfb_protocol::messages::server::SetColorMapEntries::read_from(&mut input).await;
                                }
                                2 => {
                                    let _ = events.send(ServerEvent::Bell);
                                }
                                3 => {
                                    if let Ok(cut) = rfb_protocol::messages::server::ServerCutText::read_from(&mut input).await {
                                        use bytes::Bytes;
                                        let _ = events.send(ServerEvent::ServerCutText { text: Bytes::from(cut.text) });
                                    }
                                }
                                150 => {
                                    // EndOfContinuousUpdates (server->client). No payload.
                                }
                                248 => {
                                    // ServerFence: read padding(3), flags(u32), len(u8), payload[len]
                                    use tokio::io::AsyncReadExt as _;
                                    // We don't have direct helpers for small reads here; reuse RfbInStream
                                    // Read 3 bytes padding by skipping
                                    let _ = input.skip(3).await;
                                    // Read flags (u32) and length (u8)
                                    if let Ok(_flags) = input.read_u32().await {
                                        if let Ok(len) = input.read_u8().await {
                                            // Read len bytes
                                            let mut buf = vec![0u8; len as usize];
                                            let _ = input.read_bytes(&mut buf).await;
                                        }
                                    }
                                }
                                _ => {
                                    // Unknown or unsupported server message: ignore to keep connection alive
                                    tracing::debug!("Ignoring unsupported server message type: {}", msg_type);
                                }
                            }
                        }
                        Err(e) => {
                            // Report and exit on error (fail-fast)
                            let _ = events.send(ServerEvent::Error { message: e.to_string() });
                            let _ = events.send(ServerEvent::ConnectionClosed);
                            break;
                        }
                    }
                }

                cmd = commands.recv_async() => {
                    match cmd {
                        Ok(command) => {
                            if let Err(e) = handle_command(&mut output, &events, command).await {
                                let _ = events.send(ServerEvent::Error { message: e.to_string() });
                                let _ = events.send(ServerEvent::ConnectionClosed);
                                break;
                            }
                        }
                        Err(_) => {
                            // Command channel closed by application
                            let _ = events.send(ServerEvent::ConnectionClosed);
                            break;
                        }
                    }
                }

                _ = periodic.tick() => {
                    tracing::debug!("Periodic incremental FramebufferUpdateRequest");
                    let _ = protocol::write_framebuffer_update_request(&mut output, true, 0, 0, fb_width, fb_height).await;
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
