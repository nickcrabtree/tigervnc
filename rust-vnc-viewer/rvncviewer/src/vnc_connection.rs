use anyhow::Result;
use rfb_client::{Client, ClientBuilder, ClientHandle, Config, ServerEvent};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

/// Manages the VNC client connection and event processing.
pub struct VncConnection {
    /// The active VNC client handle, if connected
    handle: Option<ClientHandle>,
    /// Connection status
    status: ConnectionStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected {
        width: u16,
        height: u16,
        server_name: String,
    },
    Error(String),
}

impl VncConnection {
    /// Creates a new, disconnected VNC connection manager.
    pub fn new() -> Self {
        Self {
            handle: None,
            status: ConnectionStatus::Disconnected,
        }
    }

    /// Attempts to connect to a VNC server.
    ///
    /// This spawns a tokio runtime and connects asynchronously.
    /// The connection runs in the background and events can be polled.
    pub async fn connect(
        &mut self,
        server: &str,
        port: Option<u16>,
        password: Option<String>,
        shared: bool,
    ) -> Result<()> {
        info!("Connecting to VNC server: {}", server);
        self.status = ConnectionStatus::Connecting;

        // Parse server address
        let (host, port) = if let Some(port) = port {
            (server.to_string(), port)
        } else if let Some((h, p)) = server.split_once(':') {
            let port_num: u16 = p.parse().unwrap_or(5900);
            (h.to_string(), port_num)
        } else {
            (server.to_string(), 5900)
        };

        // Build configuration
        let mut config_builder = Config::builder().host(&host).port(port).shared(shared);

        if let Some(pwd) = password {
            config_builder = config_builder.password(&pwd);
        }

        let config = config_builder.build()?;

        // Connect to server
        let client = ClientBuilder::new(config).build().await?;
        let handle = client.handle();

        // Start processing events in background
        tokio::spawn(async move {
            // Client will run until dropped or connection closes
            // We intentionally don't await join() here so it runs in background
        });

        // Check for initial Connected event
        if let Ok(event) = handle.events().recv_timeout(std::time::Duration::from_secs(5)) {
            if let ServerEvent::Connected {
                width,
                height,
                name,
                ..
            } = event
            {
                info!("Connected to {}: {}x{}", name, width, height);
                self.status = ConnectionStatus::Connected {
                    width,
                    height,
                    server_name: name,
                };
                self.handle = Some(handle);
                return Ok(());
            }
        }

        Err(anyhow::anyhow!("Connection failed or timed out"))
    }

    /// Disconnects from the VNC server.
    pub fn disconnect(&mut self) {
        if let Some(handle) = &self.handle {
            let _ = handle.disconnect();
            info!("Disconnected from VNC server");
        }
        self.handle = None;
        self.status = ConnectionStatus::Disconnected;
    }

    /// Returns true if currently connected.
    pub fn is_connected(&self) -> bool {
        matches!(self.status, ConnectionStatus::Connected { .. })
    }

    /// Returns the current connection status.
    pub fn status(&self) -> &ConnectionStatus {
        &self.status
    }

    /// Returns a reference to the client handle, if connected.
    pub fn handle(&self) -> Option<&ClientHandle> {
        self.handle.as_ref()
    }

    /// Polls for server events without blocking.
    ///
    /// Returns the next event if available, or None if no events are ready.
    pub fn poll_event(&self) -> Option<ServerEvent> {
        self.handle.as_ref().and_then(|h| h.try_recv_event())
    }

    /// Sends a key event to the server.
    pub fn send_key(&self, keysym: u32, down: bool) -> Result<()> {
        if let Some(handle) = &self.handle {
            handle.send_key_event(keysym, down)?;
        }
        Ok(())
    }

    /// Sends a pointer event to the server.
    pub fn send_pointer(&self, x: u16, y: u16, buttons: u8) -> Result<()> {
        if let Some(handle) = &self.handle {
            handle.send_pointer_event(x, y, buttons)?;
        }
        Ok(())
    }

    /// Requests a framebuffer update.
    pub fn request_update(&self, incremental: bool) -> Result<()> {
        if let Some(handle) = &self.handle {
            handle.request_update(incremental, None)?;
        }
        Ok(())
    }

    /// Gets the framebuffer size, if connected.
    pub fn framebuffer_size(&self) -> Option<(u32, u32)> {
        self.handle.as_ref().and_then(|h| h.framebuffer_size())
    }

    /// Gets the framebuffer pixels for rendering.
    ///
    /// Returns RGB888 format pixel data.
    pub fn framebuffer_pixels(&self) -> Option<Vec<u8>> {
        self.handle.as_ref().and_then(|h| h.framebuffer_pixels())
    }
}

impl Default for VncConnection {
    fn default() -> Self {
        Self::new()
    }
}
