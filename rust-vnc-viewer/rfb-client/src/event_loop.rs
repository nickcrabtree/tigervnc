//! Event loop coordination: read loop, write loop, and reconnection logic.

use crate::{
    config::Config,
    connection,
    errors::RfbClientError,
    framebuffer::Framebuffer,
    messages::{ClientCommand, ServerEvent},
    protocol,
};
use rfb_common::Rect;
use rfb_protocol::messages::{server::FramebufferUpdate, types::Rectangle};
use tokio::select;
use tokio::task::JoinHandle;

/// Spawn the client event loop.
///
/// This establishes a connection, creates the framebuffer, and starts read/write loops.
pub async fn spawn(
    config: Config,
    commands: flume::Receiver<ClientCommand>,
    events: flume::Sender<ServerEvent>,
) -> Result<JoinHandle<()>, RfbClientError> {
    // Establish connection and get streams + server init
    let conn = connection::establish(&config).await?;
    let width = conn.server_init.framebuffer_width;
    let height = conn.server_init.framebuffer_height;
    let name = conn.server_init.name.clone();
    let pixel_format = conn.server_init.pixel_format.clone();

    // Initialize framebuffer with server pixel format
    let mut framebuffer = Framebuffer::new(width, height, pixel_format.clone());

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

    // Initial SetEncodings based on config
    let _ = protocol::write_set_encodings(&mut output, config.display.encodings.clone()).await;

    // Spawn a task to run the main loop (read + write via select)
    let handle = tokio::spawn(async move {
        // Use async recv to avoid blocking
        loop {
            select! {
                // Prefer reading server messages to keep buffers flowing
                res = protocol::read_server_message(&mut input) => {
                    match res {
                        Ok(msg) => {
                            use crate::protocol::IncomingMessage as IM;
                            match msg {
                                IM::FramebufferUpdate(update) => {
                                    if let Err(e) = handle_framebuffer_update(&mut framebuffer, &mut input, update, &events).await {
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

    Ok(handle)
}

async fn handle_framebuffer_update<R: tokio::io::AsyncRead + Unpin>(
    framebuffer: &mut Framebuffer,
    input: &mut rfb_protocol::io::RfbInStream<R>,
    update: FramebufferUpdate,
    events: &flume::Sender<ServerEvent>,
) -> Result<(), RfbClientError> {
    // Apply all rectangles using decoders
    let damage = framebuffer.apply_update(input, &update.rectangles).await?;
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
