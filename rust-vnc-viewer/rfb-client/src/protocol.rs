//! Protocol message helpers for sending/receiving RFB messages.
//!
//! This module provides convenience functions for reading server messages
//! and writing client messages using the buffered RFB streams from
//! `rfb-protocol`.
//!
//! The helpers are intentionally thin wrappers over the low-level
//! `rfb_protocol::messages` types, enforcing the project's fail-fast
//! policy and returning rich `RfbClientError` values.

use crate::errors::RfbClientError;
use rfb_protocol::io::{RfbInStream, RfbOutStream};
use rfb_protocol::messages as msg;
use tokio::io::{AsyncRead, AsyncWrite};

/// Incoming server message (high-level wrapper around rfb-protocol messages).
#[derive(Debug, Clone, PartialEq)]
pub enum IncomingMessage {
    /// Framebuffer update with rectangle headers.
    FramebufferUpdate(msg::FramebufferUpdate),
    /// Color map entries update.
    SetColorMapEntries(msg::SetColorMapEntries),
    /// Bell notification.
    Bell(msg::Bell),
    /// Clipboard text from server.
    ServerCutText(msg::ServerCutText),
}

/// Read the next server message by dispatching on the message type byte.
///
/// This function reads a single byte to determine the server-to-client message
/// type, then parses the remainder using the appropriate `read_from` helper.
///
/// Message type mapping (RFB 3.8):
/// - 0: FramebufferUpdate
/// - 1: SetColorMapEntries
/// - 2: Bell
/// - 3: ServerCutText
pub async fn read_server_message<R: AsyncRead + Unpin>(
    instream: &mut RfbInStream<R>,
) -> Result<IncomingMessage, RfbClientError> {
    tracing::trace!("Waiting for server message...");
    let message_type = instream
        .read_u8()
        .await
        .map_err(|e| RfbClientError::Protocol(format!("failed to read message type: {}", e)))?;
    
    tracing::debug!("Received server message type: {}", message_type);

    match message_type {
        0 => {
            tracing::debug!("Parsing FramebufferUpdate message...");
            // Increase timeout to tolerate server encoding delay
            let parse_future = msg::FramebufferUpdate::read_from(instream);
            let result = match tokio::time::timeout(std::time::Duration::from_secs(30), parse_future).await {
                Ok(Ok(fb_update)) => {
                    tracing::debug!("FramebufferUpdate parsed successfully with {} rectangles", fb_update.rectangles.len());
                    Ok(IncomingMessage::FramebufferUpdate(fb_update))
                },
                Ok(Err(e)) => {
                    tracing::error!("Failed to parse FramebufferUpdate: {}", e);
                    Err(RfbClientError::Protocol(format!("failed to read FramebufferUpdate: {}", e)))
                },
                Err(_timeout) => {
                    tracing::error!("Timeout (30s) parsing FramebufferUpdate - possible hang or slow network");
                    Err(RfbClientError::Protocol("timeout reading FramebufferUpdate".to_string()))
                }
            };
            result
        }
        1 => msg::SetColorMapEntries::read_from(instream)
            .await
            .map(IncomingMessage::SetColorMapEntries)
            .map_err(|e| RfbClientError::Protocol(format!("failed to read SetColorMapEntries: {}", e))),
        2 => msg::Bell::read_from(instream)
            .await
            .map(IncomingMessage::Bell)
            .map_err(|e| RfbClientError::Protocol(format!("failed to read Bell: {}", e))),
        3 => msg::ServerCutText::read_from(instream)
            .await
            .map(IncomingMessage::ServerCutText)
            .map_err(|e| RfbClientError::Protocol(format!("failed to read ServerCutText: {}", e))),
        other => Err(RfbClientError::UnexpectedMessage(format!(
            "unsupported server message type: {}",
            other
        ))),
    }
}

/// Write a ClientInit message (shared/exclusive session) and flush.
pub async fn write_client_init<W: AsyncWrite + Unpin>(
    outstream: &mut RfbOutStream<W>,
    shared: bool,
) -> Result<(), RfbClientError> {
    let msg = msg::ClientInit { shared };
    msg.write_to(outstream);
    outstream
        .flush()
        .await
        .map_err(|e| RfbClientError::Transport(e))
}

/// Write SetPixelFormat and flush.
pub async fn write_set_pixel_format<W: AsyncWrite + Unpin>(
    outstream: &mut RfbOutStream<W>,
    pixel_format: msg::PixelFormat,
) -> Result<(), RfbClientError> {
    let msg = msg::SetPixelFormat { pixel_format };
    msg
        .write_to(outstream)
        .map_err(|e| RfbClientError::Protocol(format!("failed to write SetPixelFormat: {}", e)))?;
    outstream
        .flush()
        .await
        .map_err(|e| RfbClientError::Transport(e))
}

/// Write SetEncodings with preferred encoding order and flush.
pub async fn write_set_encodings<W: AsyncWrite + Unpin>(
    outstream: &mut RfbOutStream<W>,
    encodings: Vec<i32>,
) -> Result<(), RfbClientError> {
    let msg = msg::SetEncodings { encodings };
    msg.write_to(outstream);
    outstream
        .flush()
        .await
        .map_err(|e| RfbClientError::Transport(e))
}

/// Write a FramebufferUpdateRequest and flush.
pub async fn write_framebuffer_update_request<W: AsyncWrite + Unpin>(
    outstream: &mut RfbOutStream<W>,
    incremental: bool,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
) -> Result<(), RfbClientError> {
    let msg = msg::FramebufferUpdateRequest {
        incremental,
        x,
        y,
        width,
        height,
    };
    msg.write_to(outstream);
    outstream
        .flush()
        .await
        .map_err(|e| RfbClientError::Transport(e))
}

/// Write a KeyEvent (press or release) and flush.
pub async fn write_key_event<W: AsyncWrite + Unpin>(
    outstream: &mut RfbOutStream<W>,
    key: u32,
    down: bool,
) -> Result<(), RfbClientError> {
    let msg = msg::KeyEvent { down, key };
    msg.write_to(outstream);
    outstream
        .flush()
        .await
        .map_err(|e| RfbClientError::Transport(e))
}

/// Write a PointerEvent (mouse) and flush.
pub async fn write_pointer_event<W: AsyncWrite + Unpin>(
    outstream: &mut RfbOutStream<W>,
    button_mask: u8,
    x: u16,
    y: u16,
) -> Result<(), RfbClientError> {
    let msg = msg::PointerEvent { button_mask, x, y };
    msg.write_to(outstream);
    outstream
        .flush()
        .await
        .map_err(|e| RfbClientError::Transport(e))
}

/// Write ClientCutText and flush.
pub async fn write_client_cut_text<W: AsyncWrite + Unpin>(
    outstream: &mut RfbOutStream<W>,
    text: &str,
) -> Result<(), RfbClientError> {
    let msg = msg::ClientCutText {
        text: text.to_string(),
    };
    msg.write_to(outstream);
    outstream
        .flush()
        .await
        .map_err(|e| RfbClientError::Transport(e))
}
