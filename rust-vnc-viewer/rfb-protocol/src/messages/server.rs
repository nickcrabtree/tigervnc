//! Server-to-client RFB messages.
//!
//! This module defines all messages sent from the VNC server to the client.

use super::types::{PixelFormat, Rectangle};
use crate::io::{RfbInStream, RfbOutStream};
use tokio::io::{AsyncRead, AsyncWrite};

/// ServerInit message - initial server parameters.
///
/// Sent by the server after the ClientInit message. Provides framebuffer
/// dimensions, pixel format, and desktop name.
///
/// # Wire Format
///
/// - 2 bytes: framebuffer width
/// - 2 bytes: framebuffer height
/// - 16 bytes: PixelFormat
/// - 4 bytes: name length
/// - N bytes: name string (UTF-8)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInit {
    pub framebuffer_width: u16,
    pub framebuffer_height: u16,
    pub pixel_format: PixelFormat,
    pub name: String,
}

impl ServerInit {
    /// Read ServerInit from an RFB input stream.
    pub async fn read_from<R: AsyncRead + Unpin>(
        stream: &mut RfbInStream<R>,
    ) -> std::io::Result<Self> {
        let framebuffer_width = stream.read_u16().await?;
        let framebuffer_height = stream.read_u16().await?;
        let pixel_format = PixelFormat::read_from(stream).await?;
        let name_length = stream.read_u32().await? as usize;

        let mut name_bytes = vec![0u8; name_length];
        stream.read_bytes(&mut name_bytes).await?;
        let name = String::from_utf8(name_bytes).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid UTF-8: {}", e),
            )
        })?;

        Ok(Self {
            framebuffer_width,
            framebuffer_height,
            pixel_format,
            name,
        })
    }

    /// Write ServerInit to an RFB output stream.
    pub fn write_to<W: AsyncWrite + Unpin>(
        &self,
        stream: &mut RfbOutStream<W>,
    ) -> std::io::Result<()> {
        stream.write_u16(self.framebuffer_width);
        stream.write_u16(self.framebuffer_height);
        self.pixel_format.write_to(stream)?;
        stream.write_u32(self.name.len() as u32);
        stream.write_bytes(self.name.as_bytes());
        Ok(())
    }
}

/// FramebufferUpdate message - screen update with rectangle headers.
///
/// **Important**: This struct only contains rectangle headers. The encoding-specific
/// pixel data that follows each rectangle is **not** parsed here and must be handled
/// by encoding decoders in Phase 3.
///
/// # Wire Format
///
/// - 1 byte: message type (0)
/// - 1 byte: padding
/// - 2 bytes: number of rectangles
/// - For each rectangle: 12-byte Rectangle header (x, y, width, height, encoding)
///   - Encoding-specific pixel data follows (not parsed here)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FramebufferUpdate {
    pub rectangles: Vec<Rectangle>,
}

impl FramebufferUpdate {
    /// Read FramebufferUpdate from an RFB input stream.
    ///
    /// **Note**: This only reads rectangle headers. Callers must separately
    /// handle encoding-specific pixel data for each rectangle based on its
    /// encoding type.
    pub async fn read_from<R: AsyncRead + Unpin>(
        stream: &mut RfbInStream<R>,
    ) -> std::io::Result<Self> {
        // Skip message type (already read by caller) and padding
        stream.skip(1).await?; // padding

        let num_rectangles = stream.read_u16().await? as usize;
        let mut rectangles = Vec::with_capacity(num_rectangles);

        for _ in 0..num_rectangles {
            rectangles.push(Rectangle::read_from(stream).await?);
        }

        Ok(Self { rectangles })
    }

    /// Write FramebufferUpdate to an RFB output stream.
    ///
    /// **Note**: This only writes the message header and rectangle headers.
    /// Encoding-specific pixel data must be written separately after calling
    /// this method.
    pub fn write_to<W: AsyncWrite + Unpin>(&self, stream: &mut RfbOutStream<W>) {
        stream.write_u8(0); // message type
        stream.write_u8(0); // padding
        stream.write_u16(self.rectangles.len() as u16);

        for rect in &self.rectangles {
            rect.write_to(stream);
        }
    }
}

/// Color map entry (RGB triplet).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorMapEntry {
    pub red: u16,
    pub green: u16,
    pub blue: u16,
}

/// SetColorMapEntries message - update color map.
///
/// Used for palette-based color modes (not common in modern VNC).
///
/// # Wire Format
///
/// - 1 byte: message type (1)
/// - 1 byte: padding
/// - 2 bytes: first color index
/// - 2 bytes: number of colors
/// - For each color: 6 bytes (red u16, green u16, blue u16)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetColorMapEntries {
    pub first_color: u16,
    pub colors: Vec<ColorMapEntry>,
}

impl SetColorMapEntries {
    /// Read SetColorMapEntries from an RFB input stream.
    pub async fn read_from<R: AsyncRead + Unpin>(
        stream: &mut RfbInStream<R>,
    ) -> std::io::Result<Self> {
        stream.skip(1).await?; // padding
        let first_color = stream.read_u16().await?;
        let num_colors = stream.read_u16().await? as usize;

        let mut colors = Vec::with_capacity(num_colors);
        for _ in 0..num_colors {
            colors.push(ColorMapEntry {
                red: stream.read_u16().await?,
                green: stream.read_u16().await?,
                blue: stream.read_u16().await?,
            });
        }

        Ok(Self {
            first_color,
            colors,
        })
    }

    /// Write SetColorMapEntries to an RFB output stream.
    pub fn write_to<W: AsyncWrite + Unpin>(&self, stream: &mut RfbOutStream<W>) {
        stream.write_u8(1); // message type
        stream.write_u8(0); // padding
        stream.write_u16(self.first_color);
        stream.write_u16(self.colors.len() as u16);

        for color in &self.colors {
            stream.write_u16(color.red);
            stream.write_u16(color.green);
            stream.write_u16(color.blue);
        }
    }
}

/// Bell message - audible notification.
///
/// Signals that a bell/beep should be sounded.
///
/// # Wire Format
///
/// - 1 byte: message type (2)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bell;

impl Bell {
    /// Read Bell from an RFB input stream (no additional data).
    pub async fn read_from<R: AsyncRead + Unpin>(
        _stream: &mut RfbInStream<R>,
    ) -> std::io::Result<Self> {
        // Bell has no body, just the message type
        Ok(Self)
    }

    /// Write Bell to an RFB output stream.
    pub fn write_to<W: AsyncWrite + Unpin>(&self, stream: &mut RfbOutStream<W>) {
        stream.write_u8(2); // message type
    }
}

/// ServerCutText message - clipboard update from server.
///
/// Sends clipboard text from the server to the client.
///
/// # Wire Format
///
/// - 1 byte: message type (3)
/// - 3 bytes: padding
/// - 4 bytes: text length
/// - N bytes: text (Latin-1 encoding)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerCutText {
    pub text: String,
}

impl ServerCutText {
    /// Read ServerCutText from an RFB input stream.
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

    /// Write ServerCutText to an RFB output stream.
    pub fn write_to<W: AsyncWrite + Unpin>(&self, stream: &mut RfbOutStream<W>) {
        stream.write_u8(3); // message type
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
    async fn test_server_init_round_trip() {
        let original = ServerInit {
            framebuffer_width: 1920,
            framebuffer_height: 1080,
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
            name: "Test Desktop".to_string(),
        };

        let mut buffer = Vec::new();
        let mut out_stream = RfbOutStream::new(&mut buffer);
        original.write_to(&mut out_stream).unwrap();
        out_stream.flush().await.unwrap();

        let mut in_stream = RfbInStream::new(Cursor::new(buffer));
        let read_back = ServerInit::read_from(&mut in_stream).await.unwrap();

        assert_eq!(original, read_back);
    }

    #[tokio::test]
    async fn test_framebuffer_update_headers() {
        let original = FramebufferUpdate {
            rectangles: vec![
                Rectangle {
                    x: 0,
                    y: 0,
                    width: 100,
                    height: 100,
                    encoding: ENCODING_RAW,
                },
                Rectangle {
                    x: 100,
                    y: 100,
                    width: 200,
                    height: 150,
                    encoding: ENCODING_TIGHT,
                },
            ],
        };

        let mut buffer = Vec::new();
        let mut out_stream = RfbOutStream::new(&mut buffer);
        original.write_to(&mut out_stream);
        out_stream.flush().await.unwrap();

        // Read back (skip message type byte that was written)
        let mut in_stream = RfbInStream::new(Cursor::new(&buffer[1..]));
        let read_back = FramebufferUpdate::read_from(&mut in_stream).await.unwrap();

        assert_eq!(original, read_back);
    }

    #[tokio::test]
    async fn test_set_colormap_entries() {
        let original = SetColorMapEntries {
            first_color: 10,
            colors: vec![
                ColorMapEntry {
                    red: 65535,
                    green: 0,
                    blue: 0,
                },
                ColorMapEntry {
                    red: 0,
                    green: 65535,
                    blue: 0,
                },
            ],
        };

        let mut buffer = Vec::new();
        let mut out_stream = RfbOutStream::new(&mut buffer);
        original.write_to(&mut out_stream);
        out_stream.flush().await.unwrap();

        let mut in_stream = RfbInStream::new(Cursor::new(&buffer[1..]));
        let read_back = SetColorMapEntries::read_from(&mut in_stream).await.unwrap();

        assert_eq!(original, read_back);
    }

    #[tokio::test]
    async fn test_bell() {
        let bell = Bell;

        let mut buffer = Vec::new();
        let mut out_stream = RfbOutStream::new(&mut buffer);
        bell.write_to(&mut out_stream);
        out_stream.flush().await.unwrap();

        assert_eq!(buffer, vec![2]); // Just message type

        let mut in_stream = RfbInStream::new(Cursor::new(&buffer[1..]));
        let read_back = Bell::read_from(&mut in_stream).await.unwrap();

        assert_eq!(bell, read_back);
    }

    #[tokio::test]
    async fn test_server_cut_text_with_content() {
        let original = ServerCutText {
            text: "Hello, clipboard!".to_string(),
        };

        let mut buffer = Vec::new();
        let mut out_stream = RfbOutStream::new(&mut buffer);
        original.write_to(&mut out_stream);
        out_stream.flush().await.unwrap();

        let mut in_stream = RfbInStream::new(Cursor::new(&buffer[1..]));
        let read_back = ServerCutText::read_from(&mut in_stream).await.unwrap();

        assert_eq!(original, read_back);
    }

    #[tokio::test]
    async fn test_server_cut_text_empty() {
        let original = ServerCutText {
            text: String::new(),
        };

        let mut buffer = Vec::new();
        let mut out_stream = RfbOutStream::new(&mut buffer);
        original.write_to(&mut out_stream);
        out_stream.flush().await.unwrap();

        let mut in_stream = RfbInStream::new(Cursor::new(&buffer[1..]));
        let read_back = ServerCutText::read_from(&mut in_stream).await.unwrap();

        assert_eq!(original, read_back);
    }
}
