//! Client-to-server RFB messages.
//!
//! This module defines all messages sent from the VNC client to the server.

use super::types::PixelFormat;
use crate::io::{RfbInStream, RfbOutStream};
use tokio::io::{AsyncRead, AsyncWrite};

/// ClientInit message - client initialization.
///
/// Sent by the client after security handshake. Indicates whether the
/// client wants a shared or exclusive connection.
///
/// # Wire Format
///
/// - 1 byte: shared flag (0 = exclusive, 1 = shared)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientInit {
    pub shared: bool,
}

impl ClientInit {
    /// Read ClientInit from an RFB input stream.
    pub async fn read_from<R: AsyncRead + Unpin>(
        stream: &mut RfbInStream<R>,
    ) -> std::io::Result<Self> {
        let shared_flag = stream.read_u8().await?;
        if shared_flag > 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("shared flag must be 0 or 1, got {}", shared_flag),
            ));
        }
        Ok(Self {
            shared: shared_flag == 1,
        })
    }

    /// Write ClientInit to an RFB output stream.
    pub fn write_to<W: AsyncWrite + Unpin>(&self, stream: &mut RfbOutStream<W>) {
        stream.write_u8(if self.shared { 1 } else { 0 });
    }
}

/// SetPixelFormat message - change pixel format.
///
/// Tells the server to use a different pixel format for framebuffer updates.
///
/// # Wire Format
///
/// - 1 byte: message type (0)
/// - 3 bytes: padding
/// - 16 bytes: PixelFormat
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetPixelFormat {
    pub pixel_format: PixelFormat,
}

impl SetPixelFormat {
    /// Read SetPixelFormat from an RFB input stream.
    pub async fn read_from<R: AsyncRead + Unpin>(
        stream: &mut RfbInStream<R>,
    ) -> std::io::Result<Self> {
        stream.skip(3).await?; // padding
        let pixel_format = PixelFormat::read_from(stream).await?;
        Ok(Self { pixel_format })
    }

    /// Write SetPixelFormat to an RFB output stream.
    pub fn write_to<W: AsyncWrite + Unpin>(
        &self,
        stream: &mut RfbOutStream<W>,
    ) -> std::io::Result<()> {
        stream.write_u8(0); // message type
        stream.write_u8(0); // padding
        stream.write_u8(0); // padding
        stream.write_u8(0); // padding
        self.pixel_format.write_to(stream)?;
        Ok(())
    }
}

/// SetEncodings message - declare supported encodings.
///
/// Tells the server which encoding types the client supports, in order of preference.
///
/// # Wire Format
///
/// - 1 byte: message type (2)
/// - 1 byte: padding
/// - 2 bytes: number of encodings
/// - N * 4 bytes: encoding types (signed i32 each)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetEncodings {
    pub encodings: Vec<i32>,
}

impl SetEncodings {
    /// Read SetEncodings from an RFB input stream.
    pub async fn read_from<R: AsyncRead + Unpin>(
        stream: &mut RfbInStream<R>,
    ) -> std::io::Result<Self> {
        stream.skip(1).await?; // padding
        let num_encodings = stream.read_u16().await? as usize;

        let mut encodings = Vec::with_capacity(num_encodings);
        for _ in 0..num_encodings {
            encodings.push(stream.read_i32().await?);
        }

        Ok(Self { encodings })
    }

    /// Write SetEncodings to an RFB output stream.
    pub fn write_to<W: AsyncWrite + Unpin>(&self, stream: &mut RfbOutStream<W>) {
        stream.write_u8(2); // message type
        stream.write_u8(0); // padding
        stream.write_u16(self.encodings.len() as u16);

        for encoding in &self.encodings {
            stream.write_i32(*encoding);
        }
    }
}

/// FramebufferUpdateRequest message - request screen update.
///
/// Requests the server to send a framebuffer update for a specific region.
///
/// # Wire Format
///
/// - 1 byte: message type (3)
/// - 1 byte: incremental (0 = full update, 1 = incremental)
/// - 2 bytes: x position
/// - 2 bytes: y position
/// - 2 bytes: width
/// - 2 bytes: height
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FramebufferUpdateRequest {
    pub incremental: bool,
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl FramebufferUpdateRequest {
    /// Read FramebufferUpdateRequest from an RFB input stream.
    pub async fn read_from<R: AsyncRead + Unpin>(
        stream: &mut RfbInStream<R>,
    ) -> std::io::Result<Self> {
        let incremental_flag = stream.read_u8().await?;
        if incremental_flag > 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("incremental flag must be 0 or 1, got {}", incremental_flag),
            ));
        }

        Ok(Self {
            incremental: incremental_flag == 1,
            x: stream.read_u16().await?,
            y: stream.read_u16().await?,
            width: stream.read_u16().await?,
            height: stream.read_u16().await?,
        })
    }

    /// Write FramebufferUpdateRequest to an RFB output stream.
    pub fn write_to<W: AsyncWrite + Unpin>(&self, stream: &mut RfbOutStream<W>) {
        stream.write_u8(3); // message type
        stream.write_u8(if self.incremental { 1 } else { 0 });
        stream.write_u16(self.x);
        stream.write_u16(self.y);
        stream.write_u16(self.width);
        stream.write_u16(self.height);
    }
}

/// KeyEvent message - keyboard input.
///
/// Sends a key press or release event to the server.
///
/// # Wire Format
///
/// - 1 byte: message type (4)
/// - 1 byte: down flag (0 = up, 1 = down)
/// - 2 bytes: padding
/// - 4 bytes: keysym (X11 keysym value)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub down: bool,
    pub key: u32, // X11 keysym
}

impl KeyEvent {
    /// Read KeyEvent from an RFB input stream.
    pub async fn read_from<R: AsyncRead + Unpin>(
        stream: &mut RfbInStream<R>,
    ) -> std::io::Result<Self> {
        let down_flag = stream.read_u8().await?;
        if down_flag > 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("down flag must be 0 or 1, got {}", down_flag),
            ));
        }

        stream.skip(2).await?; // padding

        Ok(Self {
            down: down_flag == 1,
            key: stream.read_u32().await?,
        })
    }

    /// Write KeyEvent to an RFB output stream.
    pub fn write_to<W: AsyncWrite + Unpin>(&self, stream: &mut RfbOutStream<W>) {
        stream.write_u8(4); // message type
        stream.write_u8(if self.down { 1 } else { 0 });
        stream.write_u8(0); // padding
        stream.write_u8(0); // padding
        stream.write_u32(self.key);
    }
}

/// PointerEvent message - mouse input.
///
/// Sends mouse position and button state to the server.
///
/// # Wire Format
///
/// - 1 byte: message type (5)
/// - 1 byte: button mask (bitfield: bit 0 = button 1, bit 1 = button 2, etc.)
/// - 2 bytes: x position
/// - 2 bytes: y position
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerEvent {
    pub button_mask: u8,
    pub x: u16,
    pub y: u16,
}

impl PointerEvent {
    /// Read PointerEvent from an RFB input stream.
    pub async fn read_from<R: AsyncRead + Unpin>(
        stream: &mut RfbInStream<R>,
    ) -> std::io::Result<Self> {
        Ok(Self {
            button_mask: stream.read_u8().await?,
            x: stream.read_u16().await?,
            y: stream.read_u16().await?,
        })
    }

    /// Write PointerEvent to an RFB output stream.
    pub fn write_to<W: AsyncWrite + Unpin>(&self, stream: &mut RfbOutStream<W>) {
        stream.write_u8(5); // message type
        stream.write_u8(self.button_mask);
        stream.write_u16(self.x);
        stream.write_u16(self.y);
    }
}

/// ClientCutText message - clipboard update from client.
///
/// Sends clipboard text from the client to the server.
///
/// # Wire Format
///
/// - 1 byte: message type (6)
/// - 3 bytes: padding
/// - 4 bytes: text length
/// - N bytes: text (Latin-1 encoding)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientCutText {
    pub text: String,
}

/// RequestCachedData message - request full data for a missing cache ID (ContentCache protocol).
///
/// # Wire Format
/// - 1 byte: message type (254)
/// - 8 bytes: cache_id (u64, big-endian)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestCachedData {
    pub cache_id: u64,
}

impl RequestCachedData {
    /// Write RequestCachedData to an RFB output stream.
    pub fn write_to<W: AsyncWrite + Unpin>(&self, stream: &mut RfbOutStream<W>) {
        stream.write_u8(254); // msgTypeRequestCachedData
        stream.write_u64(self.cache_id);
    }
}

impl ClientCutText {
    /// Read ClientCutText from an RFB input stream.
    pub async fn read_from<R: AsyncRead + Unpin>(
        stream: &mut RfbInStream<R>,
    ) -> std::io::Result<Self> {
        stream.skip(3).await?; // padding
        let length = stream.read_u32().await? as usize;

        let mut text_bytes = vec![0u8; length];
        stream.read_bytes(&mut text_bytes).await?;

        // RFB uses Latin-1 encoding for cut text
        let text = String::from_utf8_lossy(&text_bytes).to_string();

        Ok(Self { text })
    }

    /// Write ClientCutText to an RFB output stream.
    pub fn write_to<W: AsyncWrite + Unpin>(&self, stream: &mut RfbOutStream<W>) {
        stream.write_u8(6); // message type
        stream.write_u8(0); // padding
        stream.write_u8(0); // padding
        stream.write_u8(0); // padding
        stream.write_u32(self.text.len() as u32);
        stream.write_bytes(self.text.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::types::*;
    use std::io::Cursor;

    #[tokio::test]
    async fn test_client_init_shared() {
        let original = ClientInit { shared: true };

        let mut buffer = Vec::new();
        let mut out_stream = RfbOutStream::new(&mut buffer);
        original.write_to(&mut out_stream);
        out_stream.flush().await.unwrap();

        let mut in_stream = RfbInStream::new(Cursor::new(buffer));
        let read_back = ClientInit::read_from(&mut in_stream).await.unwrap();

        assert_eq!(original, read_back);
    }

    #[tokio::test]
    async fn test_client_init_exclusive() {
        let original = ClientInit { shared: false };

        let mut buffer = Vec::new();
        let mut out_stream = RfbOutStream::new(&mut buffer);
        original.write_to(&mut out_stream);
        out_stream.flush().await.unwrap();

        let mut in_stream = RfbInStream::new(Cursor::new(buffer));
        let read_back = ClientInit::read_from(&mut in_stream).await.unwrap();

        assert_eq!(original, read_back);
    }

    #[tokio::test]
    async fn test_client_init_invalid_flag() {
        let data = vec![2u8]; // Invalid: must be 0 or 1
        let mut stream = RfbInStream::new(Cursor::new(data));
        let result = ClientInit::read_from(&mut stream).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_set_pixel_format() {
        let original = SetPixelFormat {
            pixel_format: PixelFormat {
                bits_per_pixel: 32,
                depth: 24,
                big_endian: 0,
                true_color: 1,
                red_max: 255,
                green_max: 255,
                blue_max: 255,
                red_shift: 16,
                green_shift: 8,
                blue_shift: 0,
            },
        };

        let mut buffer = Vec::new();
        let mut out_stream = RfbOutStream::new(&mut buffer);
        original.write_to(&mut out_stream).unwrap();
        out_stream.flush().await.unwrap();

        let mut in_stream = RfbInStream::new(Cursor::new(&buffer[1..]));
        let read_back = SetPixelFormat::read_from(&mut in_stream).await.unwrap();

        assert_eq!(original, read_back);
    }

    #[tokio::test]
    async fn test_set_encodings() {
        let original = SetEncodings {
            encodings: vec![ENCODING_RAW, ENCODING_COPYRECT, ENCODING_ZRLE],
        };

        let mut buffer = Vec::new();
        let mut out_stream = RfbOutStream::new(&mut buffer);
        original.write_to(&mut out_stream);
        out_stream.flush().await.unwrap();

        let mut in_stream = RfbInStream::new(Cursor::new(&buffer[1..]));
        let read_back = SetEncodings::read_from(&mut in_stream).await.unwrap();

        assert_eq!(original, read_back);
    }

    #[tokio::test]
    async fn test_framebuffer_update_request_incremental() {
        let original = FramebufferUpdateRequest {
            incremental: true,
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };

        let mut buffer = Vec::new();
        let mut out_stream = RfbOutStream::new(&mut buffer);
        original.write_to(&mut out_stream);
        out_stream.flush().await.unwrap();

        let mut in_stream = RfbInStream::new(Cursor::new(&buffer[1..]));
        let read_back = FramebufferUpdateRequest::read_from(&mut in_stream)
            .await
            .unwrap();

        assert_eq!(original, read_back);
    }

    #[tokio::test]
    async fn test_framebuffer_update_request_full() {
        let original = FramebufferUpdateRequest {
            incremental: false,
            x: 100,
            y: 200,
            width: 640,
            height: 480,
        };

        let mut buffer = Vec::new();
        let mut out_stream = RfbOutStream::new(&mut buffer);
        original.write_to(&mut out_stream);
        out_stream.flush().await.unwrap();

        let mut in_stream = RfbInStream::new(Cursor::new(&buffer[1..]));
        let read_back = FramebufferUpdateRequest::read_from(&mut in_stream)
            .await
            .unwrap();

        assert_eq!(original, read_back);
    }

    #[tokio::test]
    async fn test_key_event_down() {
        let original = KeyEvent {
            down: true,
            key: 0x0061, // 'a' key
        };

        let mut buffer = Vec::new();
        let mut out_stream = RfbOutStream::new(&mut buffer);
        original.write_to(&mut out_stream);
        out_stream.flush().await.unwrap();

        let mut in_stream = RfbInStream::new(Cursor::new(&buffer[1..]));
        let read_back = KeyEvent::read_from(&mut in_stream).await.unwrap();

        assert_eq!(original, read_back);
    }

    #[tokio::test]
    async fn test_key_event_up() {
        let original = KeyEvent {
            down: false,
            key: 0xFF0D, // Return key
        };

        let mut buffer = Vec::new();
        let mut out_stream = RfbOutStream::new(&mut buffer);
        original.write_to(&mut out_stream);
        out_stream.flush().await.unwrap();

        let mut in_stream = RfbInStream::new(Cursor::new(&buffer[1..]));
        let read_back = KeyEvent::read_from(&mut in_stream).await.unwrap();

        assert_eq!(original, read_back);
    }

    #[tokio::test]
    async fn test_pointer_event() {
        let original = PointerEvent {
            button_mask: 0b00000001, // Left button pressed
            x: 500,
            y: 300,
        };

        let mut buffer = Vec::new();
        let mut out_stream = RfbOutStream::new(&mut buffer);
        original.write_to(&mut out_stream);
        out_stream.flush().await.unwrap();

        let mut in_stream = RfbInStream::new(Cursor::new(&buffer[1..]));
        let read_back = PointerEvent::read_from(&mut in_stream).await.unwrap();

        assert_eq!(original, read_back);
    }

    #[tokio::test]
    async fn test_pointer_event_multiple_buttons() {
        let original = PointerEvent {
            button_mask: 0b00000011, // Left and right buttons
            x: 1000,
            y: 800,
        };

        let mut buffer = Vec::new();
        let mut out_stream = RfbOutStream::new(&mut buffer);
        original.write_to(&mut out_stream);
        out_stream.flush().await.unwrap();

        let mut in_stream = RfbInStream::new(Cursor::new(&buffer[1..]));
        let read_back = PointerEvent::read_from(&mut in_stream).await.unwrap();

        assert_eq!(original, read_back);
    }

    #[tokio::test]
    async fn test_client_cut_text() {
        let original = ClientCutText {
            text: "Copy this text".to_string(),
        };

        let mut buffer = Vec::new();
        let mut out_stream = RfbOutStream::new(&mut buffer);
        original.write_to(&mut out_stream);
        out_stream.flush().await.unwrap();

        let mut in_stream = RfbInStream::new(Cursor::new(&buffer[1..]));
        let read_back = ClientCutText::read_from(&mut in_stream).await.unwrap();

        assert_eq!(original, read_back);
    }

    #[tokio::test]
    async fn test_client_cut_text_empty() {
        let original = ClientCutText {
            text: String::new(),
        };

        let mut buffer = Vec::new();
        let mut out_stream = RfbOutStream::new(&mut buffer);
        original.write_to(&mut out_stream);
        out_stream.flush().await.unwrap();

        let mut in_stream = RfbInStream::new(Cursor::new(&buffer[1..]));
        let read_back = ClientCutText::read_from(&mut in_stream).await.unwrap();

        assert_eq!(original, read_back);
    }
}
