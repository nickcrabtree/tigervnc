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
                res = protocol::read_server_message(&mut input) => {
                    match res {
                        Ok(msg) => {
                            use crate::protocol::IncomingMessage as IM;
                            match msg {
                                IM::FramebufferUpdate(update) => {
                                    // Log rectangle encodings for diagnostics
                                    let encs: Vec<i32> = update.rectangles.iter().map(|r| r.encoding).collect();
                                    tracing::info!("FramebufferUpdate: {} rects, encodings={:?}", encs.len(), encs);
                                    if let Err(e) = handle_framebuffer_update(&framebuffer, &mut input, update, &events).await {
                                        let _ = events.send(ServerEvent::Error { message: e.to_string() });
                                        let _ = events.send(ServerEvent::ConnectionClosed);
                                        break;
                                    }
                                }
                                IM::SetColorMapEntries(_) => {
                                    // Not implemented: color map modes are uncommon; ignore for now per plan
                                }
                                IM::Bell(_) => {
                                    let _ = events.send(ServerEvent::Bell);
                                }
                                IM::ServerCutText(cut) => {
                                    use bytes::Bytes;
                                    let _ = events.send(ServerEvent::ServerCutText { text: Bytes::from(cut.text) });
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
