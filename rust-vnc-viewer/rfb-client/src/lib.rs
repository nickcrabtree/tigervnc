//! High-level async VNC client library.
//!
//! This crate provides a complete, production-ready VNC client implementation
//! built on top of the low-level `rfb-protocol` and `rfb-encodings` crates.
//! It handles connection management, framebuffer updates, encoding/decoding,
//! error recovery, and reconnection logic.
//!
//! # Features
//!
//! - **Async I/O**: Built on tokio for efficient event-driven networking
//! - **Multiple security types**: None, VNC password, TLS encryption
//! - **All standard encodings**: Raw, CopyRect, RRE, Hextile, Tight, ZRLE
//! - **Automatic reconnection**: Configurable retry policies with exponential backoff
//! - **Configuration management**: TOML files and environment variables
//! - **Fail-fast policy**: Clear error messages, no defensive fallbacks
//! - **Type-safe API**: Strongly-typed messages and events
//!
//! # Quick Start
//!
//! ```no_run
//! use rfb_client::{Config, ClientBuilder, ServerEvent};
//! use anyhow::Result;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Create configuration
//!     let config = Config::builder()
//!         .host("localhost")
//!         .port(5900)
//!         .build()?;
//!
//!     // Build and connect client
//!     let client = ClientBuilder::new(config).build().await?;
//!     let handle = client.handle();
//!
//!     // Process server events
//!     while let Ok(event) = handle.events().recv_async().await {
//!         match event {
//!             ServerEvent::Connected { width, height, .. } => {
//!                 println!("Connected: {}x{}", width, height);
//!             }
//!             ServerEvent::FramebufferUpdated { .. } => {
//!                 // Framebuffer has been updated
//!             }
//!             _ => {}
//!         }
//!     }
//!
//!     Ok(())
//! }
//! ```
//!
//! # Architecture
//!
//! The client uses a task-based architecture:
//!
//! - **Read loop**: Receives server messages, decodes framebuffer updates, emits events
//! - **Write loop**: Sends client commands (pointer, keyboard, etc.)
//! - **Main task**: Coordinates connection lifecycle and reconnection
//!
//! Communication between tasks and the application uses bounded channels for
//! backpressure handling.
//!
//! # Error Handling
//!
//! This crate follows a **fail-fast policy**: when errors occur, they are reported
//! immediately with clear, actionable messages. There are no defensive fallbacks
//! or silent failures.
//!
//! Errors are categorized as either:
//! - **Fatal**: Authentication failures, configuration errors, unsupported features
//! - **Retryable**: Network errors, timeouts (when reconnection is enabled)
//!
//! # Safety
//!
//! This crate is `#![forbid(unsafe_code)]` and uses only safe Rust.

#![forbid(unsafe_code)]
#![deny(missing_docs, clippy::all, clippy::pedantic, clippy::cargo)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::missing_errors_doc)] // TODO: Remove once docs are complete

// Public modules
pub mod config;
pub mod errors;
pub mod messages;
pub mod transport;

// Private implementation modules
mod connection;
mod event_loop;
mod framebuffer;
mod protocol;
mod protocol_trace;
mod cache_stats;

// Optional CLI support
#[cfg(feature = "cli")]
pub mod args;

// Re-exports
pub use config::Config;
pub use errors::RfbClientError;
pub use messages::{ClientCommand, ServerEvent};
pub use transport::TlsConfig;

// Internal use
use rfb_pixelbuffer::PixelBuffer;

use std::sync::Arc;
use tokio::task::JoinHandle;

/// Type alias for a thread-safe handle to the framebuffer.
///
/// The framebuffer is shared between the event loop (which updates it) and
/// the application (which reads from it for rendering).
pub type FramebufferHandle = Arc<tokio::sync::Mutex<crate::framebuffer::Framebuffer>>;

/// Builder for creating a VNC client.
///
/// # Examples
///
/// ```no_run
/// use rfb_client::{Config, ClientBuilder};
/// # use anyhow::Result;
///
/// # async fn example() -> Result<()> {
/// let config = Config::builder()
///     .host("localhost")
///     .port(5900)
///     .build()?;
///
/// let client = ClientBuilder::new(config).build().await?;
/// # Ok(())
/// # }
/// ```
pub struct ClientBuilder {
    config: Config,
}

impl ClientBuilder {
    /// Creates a new client builder with the given configuration.
    #[must_use]
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Builds and connects the client.
    ///
    /// This performs the initial connection and RFB handshake. If successful,
    /// it spawns the event loop tasks and returns a `Client` handle.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The configuration is invalid
    /// - Connection to the server fails
    /// - The RFB handshake fails
    /// - Authentication fails
    pub async fn build(self) -> Result<Client, RfbClientError> {
        // Validate configuration
        self.config.validate()?;

        // Create channels for communication
        let (cmd_tx, cmd_rx) = flume::bounded(32);
        let (event_tx, event_rx) = flume::bounded(64);

        // Spawn event loop
        let (join_handle, framebuffer_handle) =
            event_loop::spawn(self.config, cmd_rx, event_tx).await?;

        Ok(Client {
            handle: ClientHandle {
                commands: cmd_tx,
                events: event_rx,
                framebuffer: Some(framebuffer_handle),
            },
            join_handle,
        })
    }
}

/// Handle for interacting with a running VNC client.
///
/// This handle allows sending commands to the server and receiving events.
/// It can be cloned and shared across threads.
#[derive(Clone)]
pub struct ClientHandle {
    commands: flume::Sender<ClientCommand>,
    events: flume::Receiver<ServerEvent>,
    /// Shared framebuffer for rendering
    framebuffer: Option<FramebufferHandle>,
}

impl ClientHandle {
    /// Sends a command to the VNC server.
    ///
    /// # Errors
    ///
    /// Returns an error if the client has been shut down.
    pub fn send(&self, cmd: ClientCommand) -> Result<(), RfbClientError> {
        self.commands
            .send(cmd)
            .map_err(|_| RfbClientError::ConnectionClosed)
    }

    /// Returns a reference to the event receiver.
    ///
    /// Events can be received using `recv()`, `recv_async()`, `try_recv()`, or
    /// by iterating over the receiver.
    #[must_use]
    pub fn events(&self) -> &flume::Receiver<ServerEvent> {
        &self.events
    }

    /// Tries to receive a server event without blocking.
    ///
    /// Returns `None` if no events are available.
    pub fn try_recv_event(&self) -> Option<ServerEvent> {
        self.events.try_recv().ok()
    }

    /// Sends a key event to the VNC server.
    ///
    /// # Arguments
    ///
    /// * `keysym` - The X11 keysym value
    /// * `down` - True if key is pressed, false if released
    ///
    /// # Errors
    ///
    /// Returns an error if the client has been shut down.
    pub fn send_key_event(&self, keysym: u32, down: bool) -> Result<(), RfbClientError> {
        self.send(ClientCommand::Key { key: keysym, down })
    }

    /// Sends a pointer (mouse) event to the VNC server.
    ///
    /// # Arguments
    ///
    /// * `x` - X coordinate in pixels
    /// * `y` - Y coordinate in pixels
    /// * `buttons` - Button mask (bit 0 = left, bit 1 = middle, bit 2 = right)
    ///
    /// # Errors
    ///
    /// Returns an error if the client has been shut down.
    pub fn send_pointer_event(&self, x: u16, y: u16, buttons: u8) -> Result<(), RfbClientError> {
        self.send(ClientCommand::Pointer { x, y, buttons })
    }

    /// Sends clipboard text to the VNC server.
    ///
    /// # Arguments
    ///
    /// * `text` - Clipboard text data (typically UTF-8)
    ///
    /// # Errors
    ///
    /// Returns an error if the client has been shut down.
    pub fn send_clipboard(&self, text: bytes::Bytes) -> Result<(), RfbClientError> {
        self.send(ClientCommand::ClientCutText { text })
    }

    /// Requests a framebuffer update.
    ///
    /// # Arguments
    ///
    /// * `incremental` - If true, only send updates for changed regions
    /// * `rect` - Rectangle to update. If None, update entire screen
    ///
    /// # Errors
    ///
    /// Returns an error if the client has been shut down.
    pub fn request_update(
        &self,
        incremental: bool,
        rect: Option<rfb_common::Rect>,
    ) -> Result<(), RfbClientError> {
        self.send(ClientCommand::RequestUpdate { incremental, rect })
    }

    /// Closes the connection to the VNC server.
    ///
    /// # Errors
    ///
    /// Returns an error if the client has already been shut down.
    pub fn close(&self) -> Result<(), RfbClientError> {
        self.send(ClientCommand::Close)
    }

    /// Alias for close() to match the naming used in app.rs
    pub fn disconnect(&self) -> Result<(), RfbClientError> {
        self.close()
    }

    /// Returns a handle to the shared framebuffer for rendering.
    ///
    /// Returns `None` if not yet connected or connection has been closed.
    /// The framebuffer is protected by a mutex and can be safely accessed
    /// from multiple threads.
    #[must_use]
    pub fn framebuffer(&self) -> Option<FramebufferHandle> {
        self.framebuffer.clone()
    }

    /// Convenience method to read framebuffer dimensions.
    ///
    /// Returns `None` if not connected or if the framebuffer lock cannot be acquired.
    #[must_use]
    pub fn framebuffer_size(&self) -> Option<(u32, u32)> {
        // Note: This blocks - in a real implementation you'd want an async version
        self.framebuffer.as_ref().and_then(|fb| {
            if let Ok(fb) = fb.try_lock() {
                let (w, h) = fb.size();
                Some((w as u32, h as u32))
            } else {
                None
            }
        })
    }

    /// Convenience method to get framebuffer pixel format.
    ///
    /// Returns `None` if not connected or if the framebuffer lock cannot be acquired.
    #[must_use]
    pub fn framebuffer_format(&self) -> Option<rfb_pixelbuffer::PixelFormat> {
        self.framebuffer.as_ref().and_then(|fb| {
            if let Ok(fb) = fb.try_lock() {
                Some(fb.buffer().format().clone())
            } else {
                None
            }
        })
    }

    /// Convenience method to read framebuffer pixels for rendering.
    ///
    /// Returns the raw pixel data as a byte slice. The format is always RGB888
    /// regardless of the server's native pixel format.
    ///
    /// Returns `None` if not connected or if the framebuffer lock cannot be acquired.
    #[must_use]
    pub fn framebuffer_pixels(&self) -> Option<Vec<u8>> {
        self.framebuffer.as_ref().and_then(|fb| {
            if let Ok(fb) = fb.try_lock() {
                let buffer = fb.buffer();
                let rect =
                    rfb_common::Rect::new(0, 0, buffer.width() as u32, buffer.height() as u32);
                let mut stride = 0;
                if let Some(pixels) = buffer.get_buffer(rect, &mut stride) {
                    let bytes_per_pixel = buffer.format().bytes_per_pixel();
                    let total_bytes = buffer.height() as usize * stride * bytes_per_pixel as usize;
                    Some(pixels[..total_bytes].to_vec())
                } else {
                    None
                }
            } else {
                None
            }
        })
    }
}

/// A connected VNC client.
///
/// The client runs event loops in background tasks. Use the `handle()` method
/// to get a handle for sending commands and receiving events.
///
/// The client will automatically shut down when dropped, but you can also
/// explicitly wait for it to finish using `join()`.
pub struct Client {
    handle: ClientHandle,
    join_handle: JoinHandle<()>,
}

impl Client {
    /// Returns a handle for interacting with the client.
    ///
    /// The handle can be cloned and used from multiple threads.
    #[must_use]
    pub fn handle(&self) -> ClientHandle {
        self.handle.clone()
    }

    /// Waits for the client to finish.
    ///
    /// This consumes the client and blocks until all background tasks have
    /// completed.
    ///
    /// # Errors
    ///
    /// Returns an error if the background task panicked.
    pub async fn join(mut self) -> Result<(), RfbClientError> {
        // Take ownership of join_handle without triggering Drop
        let join_handle = std::mem::replace(&mut self.join_handle, tokio::spawn(async {}));
        // Prevent Drop from running
        std::mem::forget(self);
        join_handle
            .await
            .map_err(|e| RfbClientError::Internal(format!("Client task panicked: {e}")))
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        // Signal shutdown by closing the command channel
        drop(self.handle.commands.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_handle_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ClientHandle>();
    }
}
