//! RFB protocol message types.
//!
//! This module provides types and parsers for all RFB (Remote Framebuffer) protocol messages
//! exchanged between client and server. Messages are categorized into:
//!
//! - **Core types** ([`types`]) - Shared types like PixelFormat, Rectangle, and encoding constants
//! - **Server messages** ([`server`]) - Messages sent from server to client
//! - **Client messages** ([`client`]) - Messages sent from client to server
//!
//! # Wire Format Rules
//!
//! All messages follow these invariants:
//!
//! 1. **Big-endian byte order** - All multi-byte integers use network byte order
//! 2. **Strict boolean validation** - Boolean fields must be exactly 0 or 1 (any other value is an error)
//! 3. **Padding validation** - Padding bytes must be zero
//! 4. **Fail-fast errors** - Invalid data results in errors, no defensive fallbacks
//!
//! # Message Parsing Limitations (Task 2.4)
//!
//! **Important**: In this implementation phase, `FramebufferUpdate` only parses rectangle
//! headers (x, y, width, height, encoding). The encoding-specific pixel data payloads are
//! **not** consumed or parsed here, as that depends on decoder implementations in Phase 3.
//!
//! Callers must handle encoding payloads separately after receiving rectangle headers.
//!
//! # Examples
//!
//! ```no_run
//! use rfb_protocol::messages::types::PixelFormat;
//! use rfb_protocol::messages::client::ClientInit;
//!
//! // Create a client init message (shared connection)
//! let client_init = ClientInit { shared: true };
//! ```

pub mod client;
pub mod server;
pub mod types;

// Re-export commonly used types
pub use types::{
    PixelFormat, Rectangle, ENCODING_COPYRECT, ENCODING_HEXTILE, ENCODING_RAW, ENCODING_RRE,
    ENCODING_TIGHT, ENCODING_ZRLE,
};

pub use server::{
    Bell, ColorMapEntry, FramebufferUpdate, ServerCutText, ServerInit, SetColorMapEntries,
};

pub use client::{
    ClientCutText, ClientInit, FramebufferUpdateRequest, KeyEvent, PointerEvent, SetEncodings,
    SetPixelFormat,
};

use crate::io::{RfbInStream, RfbOutStream};
use tokio::io::{AsyncRead, AsyncWrite};

/// All client-to-server RFB message types.
#[derive(Debug, Clone)]
pub enum ClientMessage {
    SetPixelFormat(SetPixelFormat),
    SetEncodings(SetEncodings),
    FramebufferUpdateRequest(FramebufferUpdateRequest),
    KeyEvent(KeyEvent),
    PointerEvent(PointerEvent),
    ClientCutText(ClientCutText),
}

impl ClientMessage {
    /// Write this message to an output stream.
    pub fn write_to<W: AsyncWrite + Unpin>(
        &self,
        stream: &mut RfbOutStream<W>,
    ) -> std::io::Result<()> {
        match self {
            ClientMessage::SetPixelFormat(msg) => msg.write_to(stream),
            ClientMessage::SetEncodings(msg) => {
                msg.write_to(stream);
                Ok(())
            }
            ClientMessage::FramebufferUpdateRequest(msg) => {
                msg.write_to(stream);
                Ok(())
            }
            ClientMessage::KeyEvent(msg) => {
                msg.write_to(stream);
                Ok(())
            }
            ClientMessage::PointerEvent(msg) => {
                msg.write_to(stream);
                Ok(())
            }
            ClientMessage::ClientCutText(msg) => {
                msg.write_to(stream);
                Ok(())
            }
        }
    }
}

/// All server-to-client RFB message types.
#[derive(Debug, Clone)]
pub enum ServerMessage {
    FramebufferUpdate(FramebufferUpdate),
    SetColorMapEntries(SetColorMapEntries),
    Bell,
    ServerCutText(ServerCutText),
}

impl ServerMessage {
    /// Read a server message from an input stream.
    pub async fn read_from<R: AsyncRead + Unpin>(
        stream: &mut RfbInStream<R>,
    ) -> std::io::Result<Self> {
        let msg_type = stream.read_u8().await?;
        match msg_type {
            0 => Ok(ServerMessage::FramebufferUpdate(
                FramebufferUpdate::read_from(stream).await?,
            )),
            1 => Ok(ServerMessage::SetColorMapEntries(
                SetColorMapEntries::read_from(stream).await?,
            )),
            2 => Ok(ServerMessage::Bell),
            3 => Ok(ServerMessage::ServerCutText(
                ServerCutText::read_from(stream).await?,
            )),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown server message type: {}", msg_type),
            )),
        }
    }
}
