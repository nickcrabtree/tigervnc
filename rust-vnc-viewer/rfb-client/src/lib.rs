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
#![deny(
    missing_docs,
    clippy::all,
    clippy::pedantic,
    clippy::cargo
)]
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

// Optional CLI support
#[cfg(feature = "cli")]
pub mod args;

// Re-exports
pub use config::Config;
pub use errors::RfbClientError;
pub use messages::{ClientCommand, ServerEvent};
pub use transport::TlsConfig;

use std::sync::Arc;
use tokio::task::JoinHandle;

/// Type alias for a thread-safe handle to the framebuffer.
///
/// The framebuffer is shared between the event loop (which updates it) and
/// the application (which reads from it for rendering).
pub type FramebufferHandle = Arc<parking_lot::Mutex<rfb_pixelbuffer::ManagedPixelBuffer>>;

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
        let handle = event_loop::spawn(self.config, cmd_rx, event_tx).await?;

        Ok(Client {
            handle: ClientHandle {
                commands: cmd_tx,
                events: event_rx,
            },
            join_handle: handle,
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

    /// Closes the connection to the VNC server.
    ///
    /// # Errors
    ///
    /// Returns an error if the client has already been shut down.
    pub fn close(&self) -> Result<(), RfbClientError> {
        self.send(ClientCommand::Close)
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
