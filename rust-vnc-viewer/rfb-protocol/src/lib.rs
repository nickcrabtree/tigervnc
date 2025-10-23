//! RFB (Remote Framebuffer) protocol implementation.
//!
//! This crate provides the core networking and protocol layer for VNC client connections.
//! It handles socket connections, I/O streams, message serialization/deserialization,
//! and the RFB protocol handshake.
//!
//! # Modules
//!
//! - [`socket`] - Socket abstractions (TCP, Unix domain)
//! - [`io`] - Buffered I/O streams (RfbInStream, RfbOutStream)
//! - [`connection`] - Connection state machine and lifecycle management
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

pub mod connection;
pub mod handshake;
pub mod io;
pub mod messages;
pub mod socket;

// Re-export commonly used types
pub use connection::{ConnectionState, RfbConnection};
pub use io::{RfbInStream, RfbOutStream};
pub use messages::{ClientMessage, ServerMessage};
pub use socket::{TcpSocket, VncSocket};
pub use handshake::NegotiatedVersion;

#[cfg(unix)]
pub use socket::UnixSocket;
