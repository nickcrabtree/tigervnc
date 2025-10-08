//! RFB (Remote Framebuffer) protocol implementation.
//!
//! This crate provides the core networking and protocol layer for VNC client connections.
//! It handles socket connections, I/O streams, message serialization/deserialization,
//! and the RFB protocol handshake.
//!
//! # Modules
//!
//! - [`socket`] - Socket abstractions (TCP, Unix domain)
//! - More modules coming in Phase 2 implementation...
//!
//! # Examples
//!
//! ```no_run
//! use rfb_protocol::{TcpSocket, VncSocket};
//!
//! # async fn example() -> anyhow::Result<()> {
//! // Connect to a VNC server
//! let socket = TcpSocket::connect("localhost", 5900).await?;
//! println!("Connected to: {}", socket.peer_endpoint());
//! # Ok(())
//! # }
//! ```

pub mod socket;

// Re-export commonly used types
pub use socket::{VncSocket, TcpSocket};

#[cfg(unix)]
pub use socket::UnixSocket;
