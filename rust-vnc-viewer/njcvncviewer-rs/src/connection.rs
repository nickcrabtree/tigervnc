use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender};
use rfb_encodings::{CopyRectDecoder, Decoder, RawDecoder, ENCODING_COPY_RECT, ENCODING_RAW};
use rfb_pixelbuffer::{ManagedPixelBuffer, MutablePixelBuffer, PixelFormat};
use rfb_protocol::{
    connection::{ConnectionState, RfbConnection},
    handshake,
    messages::{self, ClientMessage, ServerMessage},
    TcpSocket, VncSocket,
};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Messages from connection thread to GUI thread
#[derive(Debug, Clone)]
pub enum ConnectionEvent {
    /// Connection state changed
    StateChanged(ConnectionState),

    /// Connected and received server info
    Connected {
        width: u16,
        height: u16,
        pixel_format: PixelFormat,
        server_name: String,
    },

    /// Framebuffer update received
    FramebufferUpdate {
        buffer: Arc<ManagedPixelBuffer>,
    },

    /// Error occurred
    Error(String),

    /// Connection closed
    Disconnected,
}

/// Messages from GUI thread to connection thread
#[derive(Debug, Clone)]
pub enum ConnectionCommand {
    /// Request framebuffer update
    RequestUpdate {
        incremental: bool,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
    },

    /// Send key event
    KeyEvent { down: bool, key: u32 },

    /// Send pointer event
    PointerEvent {
        button_mask: u8,
        x: u16,
        y: u16,
    },

    /// Disconnect
    Disconnect,
}

/// Concrete decoder type that dispatches to the appropriate decoder implementation
enum DecoderImpl {
    Raw(RawDecoder),
    CopyRect(CopyRectDecoder),
}

impl DecoderImpl {
    async fn decode<R: tokio::io::AsyncRead + Unpin>(
        &self,
        stream: &mut rfb_protocol::io::RfbInStream<R>,
        rect: &rfb_protocol::messages::types::Rectangle,
        pixel_format: &rfb_protocol::messages::types::PixelFormat,
        buffer: &mut dyn MutablePixelBuffer,
    ) -> Result<()> {
        match self {
            DecoderImpl::Raw(d) => d.decode(stream, rect, pixel_format, buffer).await,
            DecoderImpl::CopyRect(d) => d.decode(stream, rect, pixel_format, buffer).await,
        }
    }
}

/// Registry of decoders keyed by encoding type
struct DecoderRegistry {
    decoders: HashMap<i32, DecoderImpl>,
}

impl DecoderRegistry {
    fn new() -> Self {
        let mut decoders = HashMap::new();
        decoders.insert(ENCODING_RAW, DecoderImpl::Raw(RawDecoder));
        decoders.insert(ENCODING_COPY_RECT, DecoderImpl::CopyRect(CopyRectDecoder));
        Self { decoders }
    }

    fn get(&self, encoding: i32) -> Option<&DecoderImpl> {
        self.decoders.get(&encoding)
    }
}

pub struct ConnectionManager {
    event_tx: Sender<ConnectionEvent>,
    command_rx: Receiver<ConnectionCommand>,
    host: String,
    port: u16,
    shared: bool,
    decoders: DecoderRegistry,
}

impl ConnectionManager {
    pub fn new(
        event_tx: Sender<ConnectionEvent>,
        command_rx: Receiver<ConnectionCommand>,
        host: String,
        port: u16,
        shared: bool,
    ) -> Self {
        Self {
            event_tx,
            command_rx,
            host,
            port,
            shared,
            decoders: DecoderRegistry::new(),
        }
    }

    pub async fn run(self) {
        if let Err(e) = self.run_inner().await {
            error!("Connection error: {:#}", e);
            let _ = self
                .event_tx
                .send(ConnectionEvent::Error(format!("{:#}", e)));
        }

        let _ = self.event_tx.send(ConnectionEvent::Disconnected);
    }

    async fn run_inner(&self) -> Result<()> {
        info!("Connecting to {}:{}", self.host, self.port);

        // Connect socket
        let socket = TcpSocket::connect(&self.host, self.port)
            .await
            .context("Failed to connect")?;

        let peer_addr = socket.peer_endpoint();
        info!("Connected to {}", peer_addr);

        let (reader, writer) = tokio::io::split(socket);
        let mut conn = RfbConnection::new(reader, writer);
        conn.set_peer_address(peer_addr);

        // Perform handshake
        self.send_event(ConnectionEvent::StateChanged(
            ConnectionState::ProtocolVersion,
        ));

        conn.begin_handshake()
            .context("Failed to begin handshake")?;

        // Protocol version negotiation
        debug!("Negotiating protocol version");
        let version = {
            let (instream, outstream) = conn.streams();
            handshake::negotiate_version(instream, outstream)
                .await
                .context("Protocol version negotiation failed")?
        };
        info!("Negotiated protocol version: {:?}", version);

        // Security negotiation  
        self.send_event(ConnectionEvent::StateChanged(ConnectionState::Security));
        debug!("Negotiating security");
        {
            let (instream, outstream) = conn.streams();
            handshake::negotiate_security(instream, outstream, version)
                .await
                .context("Security negotiation failed")?
        };
        info!("Security negotiation successful");

        // Send ClientInit
        self.send_event(ConnectionEvent::StateChanged(ConnectionState::ClientInit));
        debug!("Sending ClientInit (shared={})", self.shared);
        handshake::send_client_init(conn.outstream(), self.shared)
            .await
            .context("Failed to send ClientInit")?;

        // Receive ServerInit
        self.send_event(ConnectionEvent::StateChanged(ConnectionState::ServerInit));
        debug!("Waiting for ServerInit");
        let server_init = handshake::recv_server_init(conn.instream())
            .await
            .context("Failed to receive ServerInit")?;

        info!(
            "Server: {} ({}x{})",
            server_init.name, server_init.framebuffer_width, server_init.framebuffer_height
        );

        // Store server info
        conn.set_dimensions(
            server_init.framebuffer_width,
            server_init.framebuffer_height,
        );
        conn.set_server_name(server_init.name.clone());

        // Store both protocol and pixelbuffer versions of pixel format
        let protocol_pixel_format = server_init.pixel_format.clone();
        let pixel_format: PixelFormat = server_init.pixel_format.into();

        // Send Connected event
        self.send_event(ConnectionEvent::Connected {
            width: server_init.framebuffer_width,
            height: server_init.framebuffer_height,
            pixel_format,
            server_name: server_init.name,
        });

        // Transition to Normal state
        conn.transition_to(ConnectionState::Normal)
            .context("Failed to transition to Normal state")?;
        self.send_event(ConnectionEvent::StateChanged(ConnectionState::Normal));

        // Set pixel format (use server's default for now)
        let set_pixel_format = ClientMessage::SetPixelFormat(messages::SetPixelFormat {
            pixel_format: protocol_pixel_format.clone(),
        });
        set_pixel_format
            .write_to(conn.outstream())
            .context("Failed to write SetPixelFormat")?;
        conn.outstream()
            .flush()
            .await
            .context("Failed to flush SetPixelFormat")?;

        // Set encodings (Raw and CopyRect for now)
        let set_encodings = ClientMessage::SetEncodings(messages::SetEncodings {
            encodings: vec![0, 1], // Raw=0, CopyRect=1
        });
        set_encodings
            .write_to(conn.outstream())
            .context("Failed to write SetEncodings")?;
        conn.outstream()
            .flush()
            .await
            .context("Failed to flush SetEncodings")?;

        // Request initial full framebuffer update
        let request_update = ClientMessage::FramebufferUpdateRequest(
            messages::FramebufferUpdateRequest {
                incremental: false,
                x: 0,
                y: 0,
                width: server_init.framebuffer_width,
                height: server_init.framebuffer_height,
            },
        );
        request_update
            .write_to(conn.outstream())
            .context("Failed to write initial FramebufferUpdateRequest")?;
        conn.outstream()
            .flush()
            .await
            .context("Failed to flush initial FramebufferUpdateRequest")?;

        info!("Handshake complete, entering main loop");

        // Main event loop
        self.main_loop(conn, protocol_pixel_format, pixel_format).await
    }

    async fn main_loop<R, W>(
        &self,
        mut conn: RfbConnection<R, W>,
        protocol_pixel_format: rfb_protocol::messages::types::PixelFormat,
        pixel_format: PixelFormat,
    ) -> Result<()>
    where
        R: tokio::io::AsyncRead + Unpin,
        W: tokio::io::AsyncWrite + Unpin,
    {
        // Create pixel buffer
        let width = conn.dimensions().unwrap().0;
        let height = conn.dimensions().unwrap().1;
        let mut pixel_buffer =
            ManagedPixelBuffer::new(width as u32, height as u32, pixel_format);

        loop {
            // Check for commands from GUI
            if let Ok(cmd) = self.command_rx.try_recv() {
                match cmd {
                    ConnectionCommand::Disconnect => {
                        info!("Disconnect requested");
                        break;
                    }
                    ConnectionCommand::RequestUpdate {
                        incremental,
                        x,
                        y,
                        width,
                        height,
                    } => {
                        let request = ClientMessage::FramebufferUpdateRequest(
                            messages::FramebufferUpdateRequest {
                                incremental,
                                x,
                                y,
                                width,
                                height,
                            },
                        );
                        request.write_to(conn.outstream())?;
                        conn.outstream().flush().await?;
                    }
                    ConnectionCommand::KeyEvent { down, key } => {
                        let key_event =
                            ClientMessage::KeyEvent(messages::KeyEvent { down, key });
                        key_event.write_to(conn.outstream())?;
                        conn.outstream().flush().await?;
                    }
                    ConnectionCommand::PointerEvent { button_mask, x, y } => {
                        let pointer_event = ClientMessage::PointerEvent(messages::PointerEvent {
                            button_mask,
                            x,
                            y,
                        });
                        pointer_event.write_to(conn.outstream())?;
                        conn.outstream().flush().await?;
                    }
                }
            }

            // Check for server messages (with timeout)
            match tokio::time::timeout(
                tokio::time::Duration::from_millis(100),
                ServerMessage::read_from(conn.instream()),
            )
            .await
            {
                Ok(Ok(msg)) => {
                    self.handle_server_message(msg, &mut pixel_buffer, &protocol_pixel_format, &mut conn)
                        .await?;
                }
                Ok(Err(e)) => {
                    error!("Failed to read server message: {:#}", e);
                    return Err(e.into());
                }
                Err(_) => {
                    // Timeout - continue loop
                }
            }
        }

        Ok(())
    }

    async fn handle_server_message<R, W>(
        &self,
        msg: ServerMessage,
        pixel_buffer: &mut ManagedPixelBuffer,
        pixel_format: &rfb_protocol::messages::types::PixelFormat,
        conn: &mut RfbConnection<R, W>,
    ) -> Result<()>
    where
        R: tokio::io::AsyncRead + Unpin,
        W: tokio::io::AsyncWrite + Unpin,
    {
        match msg {
            ServerMessage::FramebufferUpdate(update) => {
                debug!(
                    "FramebufferUpdate: {} rectangles",
                    update.rectangles.len()
                );

                // Decode each rectangle
                for rect in &update.rectangles {
                    debug!(
                        "Rectangle: x={}, y={}, w={}, h={}, encoding={}",
                        rect.x, rect.y, rect.width, rect.height, rect.encoding
                    );

                    // Look up decoder for this encoding type
                    if let Some(decoder) = self.decoders.get(rect.encoding) {
                        decoder
                            .decode(conn.instream(), rect, pixel_format, pixel_buffer)
                            .await
                            .with_context(|| {
                                format!(
                                    "Failed to decode rectangle at ({},{}) {}x{} with encoding {}",
                                    rect.x, rect.y, rect.width, rect.height, rect.encoding
                                )
                            })?;
                    } else {
                        warn!(
                            "No decoder for encoding {} (rect at {},{} {}x{})",
                            rect.encoding, rect.x, rect.y, rect.width, rect.height
                        );
                        // Skip this rectangle - we can't decode it
                        // This will cause the connection to fail if the server sends
                        // an encoding we don't support
                        return Err(anyhow::anyhow!(
                            "Unsupported encoding: {}",
                            rect.encoding
                        ));
                    }
                }

                // Send updated buffer to GUI
                self.send_event(ConnectionEvent::FramebufferUpdate {
                    buffer: Arc::new(pixel_buffer.clone()),
                });
            }
            ServerMessage::SetColorMapEntries(_) => {
                warn!("SetColorMapEntries not implemented");
            }
            ServerMessage::Bell => {
                debug!("Bell received");
                // TODO: Play system bell sound
            }
            ServerMessage::ServerCutText(text) => {
                debug!("ServerCutText received: {} bytes", text.text.len());
                // TODO: Update clipboard
            }
        }

        Ok(())
    }

    fn send_event(&self, event: ConnectionEvent) {
        if let Err(e) = self.event_tx.send(event) {
            warn!("Failed to send event: {}", e);
        }
    }
}
