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
use crate::protocol_trace;
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
/// Read only the next server message type byte.
pub async fn read_message_type<R: AsyncRead + Unpin>(
    instream: &mut RfbInStream<R>,
) -> Result<u8, RfbClientError> {
    let t = instream
        .read_u8()
        .await
        .map_err(|e| RfbClientError::Protocol(format!("failed to read message type: {}", e)))?;
    if protocol_trace::enabled() { protocol_trace::in_msg("ServerMessageType", &format!("type={}", t)); }
    Ok(t)
}

pub async fn read_server_message<R: AsyncRead + Unpin>(
    instream: &mut RfbInStream<R>,
) -> Result<IncomingMessage, RfbClientError> {
    tracing::trace!("Waiting for server message...");
let message_type = instream
        .read_u8()
        .await
        .map_err(|e| RfbClientError::Protocol(format!("failed to read message type: {}", e)))?;
    if protocol_trace::enabled() { protocol_trace::in_msg("ServerMessageType", &format!("type={}", message_type)); }
    tracing::debug!("Received server message type: {}", message_type);

    match message_type {
        0 => {
            tracing::debug!("Parsing FramebufferUpdate message...");
            // Increase timeout to tolerate server encoding delay
            let parse_future = msg::FramebufferUpdate::read_from(instream);
            let result = match tokio::time::timeout(std::time::Duration::from_secs(30), parse_future).await {
                Ok(Ok(fb_update)) => {
                    if protocol_trace::enabled() { protocol_trace::in_msg("FramebufferUpdate", &format!("rects={}", fb_update.rectangles.len())); }
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
            .map(|m| { if protocol_trace::enabled() { protocol_trace::in_msg("SetColorMapEntries", &format!("first={} colors={}", m.first_color, m.colors.len())); } IncomingMessage::SetColorMapEntries(m) })
            .map_err(|e| RfbClientError::Protocol(format!("failed to read SetColorMapEntries: {}", e))),
        2 => msg::Bell::read_from(instream)
            .await
            .map(|m| { if protocol_trace::enabled() { protocol_trace::in_msg("Bell", ""); } IncomingMessage::Bell(m) })
            .map_err(|e| RfbClientError::Protocol(format!("failed to read Bell: {}", e))),
        3 => msg::ServerCutText::read_from(instream)
            .await
            .map(|m| { if protocol_trace::enabled() { protocol_trace::in_msg("ServerCutText", &format!("len={}", m.text.len())); } IncomingMessage::ServerCutText(m) })
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
    if protocol_trace::enabled() { protocol_trace::out_msg("ClientInit", &format!("shared={}", shared)); }
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
    if protocol_trace::enabled() {
        protocol_trace::out_msg(
            "SetPixelFormat",
            &format!(
                "bpp={} depth={} shifts={}/{}/{}",
                msg.pixel_format.bits_per_pixel,
                msg.pixel_format.depth,
                msg.pixel_format.red_shift,
                msg.pixel_format.green_shift,
                msg.pixel_format.blue_shift
            ),
        );
    }
    msg
        .write_to(outstream)
        .map_err(|e| RfbClientError::Protocol(format!("failed to write SetPixelFormat: {}", e)))?;
    tracing::debug!("Wrote SetPixelFormat (bpp={}, depth={}, shifts r/g/b={} {}/{}/{})",
        msg.pixel_format.bits_per_pixel,
        msg.pixel_format.depth,
        msg.pixel_format.red_shift,
        msg.pixel_format.green_shift,
        msg.pixel_format.blue_shift,
        0);
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
    if protocol_trace::enabled() { protocol_trace::out_msg("SetEncodings", &format!("n={}", msg.encodings.len())); }
    tracing::debug!("Wrote SetEncodings: {:?}", msg.encodings);
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
    if protocol_trace::enabled() { protocol_trace::out_msg("FramebufferUpdateRequest", &format!("inc={} rect=({},{} {}x{})", incremental, x, y, width, height)); }
    tracing::debug!("Wrote FramebufferUpdateRequest inc={} rect=({},{} {}x{})",
        incremental, x, y, width, height);
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
    if protocol_trace::enabled() { protocol_trace::out_msg("KeyEvent", &format!("down={} key=0x{:X}", down, key)); }
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
    if protocol_trace::enabled() { protocol_trace::out_msg("PointerEvent", &format!("buttons=0x{:02X} pos=({}, {})", button_mask, x, y)); }
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
    if protocol_trace::enabled() { protocol_trace::out_msg("ClientCutText", &format!("len={}", msg.text.len())); }
    msg.write_to(outstream);
    outstream
        .flush()
        .await
        .map_err(|e| RfbClientError::Transport(e))
}

/// Enable or disable continuous updates over a specified rectangle and flush.
pub async fn write_enable_continuous_updates<W: AsyncWrite + Unpin>(
    outstream: &mut RfbOutStream<W>,
    enable: bool,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
) -> Result<(), RfbClientError> {
    // Message type 150 (client -> server)
    if protocol_trace::enabled() { protocol_trace::out_msg("EnableContinuousUpdates", &format!("enable={} rect=({},{} {}x{})", enable, x, y, width, height)); }
    outstream.write_u8(150);
    outstream.write_u8(if enable { 1 } else { 0 });
    outstream.write_u16(x);
    outstream.write_u16(y);
    outstream.write_u16(width);
    outstream.write_u16(height);
    outstream
        .flush()
        .await
        .map_err(|e| RfbClientError::Transport(e))
}
