//! Core RFB protocol types.
//!
//! This module defines fundamental types used throughout the RFB protocol:
//! - [`PixelFormat`] - Describes pixel format (bit depths, color channels, endianness)
//! - [`Rectangle`] - Rectangle header with encoding type
//! - Encoding constants for different compression/encoding schemes

use crate::io::{RfbInStream, RfbOutStream};
use tokio::io::{AsyncRead, AsyncWrite};

/// RFB pixel format specification.
///
/// Describes how pixels are encoded in the framebuffer, including:
/// - Bits per pixel and color depth
/// - RGB channel sizes and bit positions
/// - Byte order (big/little endian)
///
/// # Wire Format
///
/// PixelFormat is 16 bytes on the wire:
/// - 1 byte: bits_per_pixel
/// - 1 byte: depth
/// - 1 byte: big_endian (0 or 1)
/// - 1 byte: true_color (0 or 1)
/// - 2 bytes: red_max
/// - 2 bytes: green_max
/// - 2 bytes: blue_max
/// - 1 byte: red_shift
/// - 1 byte: green_shift
/// - 1 byte: blue_shift
/// - 3 bytes: padding (must be zero)
///
/// # Examples
///
/// ```
/// use rfb_protocol::messages::types::PixelFormat;
///
/// // Standard 32-bit RGB format
/// let pf = PixelFormat {
///     bits_per_pixel: 32,
///     depth: 24,
///     big_endian: 0,
///     true_color: 1,
///     red_max: 255,
///     green_max: 255,
///     blue_max: 255,
///     red_shift: 16,
///     green_shift: 8,
///     blue_shift: 0,
/// };
///
/// assert_eq!(pf.bytes_per_pixel(), 4);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PixelFormat {
    pub bits_per_pixel: u8,
    pub depth: u8,
    pub big_endian: u8, // Boolean: must be 0 or 1
    pub true_color: u8, // Boolean: must be 0 or 1
    pub red_max: u16,
    pub green_max: u16,
    pub blue_max: u16,
    pub red_shift: u8,
    pub green_shift: u8,
    pub blue_shift: u8,
}

impl PixelFormat {
    /// Calculate bytes per pixel (1, 2, 3, or 4).
    ///
    /// # Examples
    ///
    /// ```
    /// # use rfb_protocol::messages::types::PixelFormat;
    /// let pf = PixelFormat {
    ///     bits_per_pixel: 32,
    ///     depth: 24,
    ///     big_endian: 0,
    ///     true_color: 1,
    ///     red_max: 255, green_max: 255, blue_max: 255,
    ///     red_shift: 16, green_shift: 8, blue_shift: 0,
    /// };
    /// assert_eq!(pf.bytes_per_pixel(), 4);
    /// ```
    pub fn bytes_per_pixel(&self) -> u8 {
        self.bits_per_pixel.div_ceil(8)
    }

    /// Read a PixelFormat from an RFB input stream.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - EOF is reached before all 16 bytes are read
    /// - Boolean fields (big_endian, true_color) are not 0 or 1
    /// - Padding bytes are not zero
    pub async fn read_from<R: AsyncRead + Unpin>(
        stream: &mut RfbInStream<R>,
    ) -> std::io::Result<Self> {
        let bits_per_pixel = stream.read_u8().await?;
        let depth = stream.read_u8().await?;
        let big_endian = stream.read_u8().await?;
        let true_color = stream.read_u8().await?;

        // Validate booleans strictly
        if big_endian > 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("big_endian must be 0 or 1, got {}", big_endian),
            ));
        }
        if true_color > 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("true_color must be 0 or 1, got {}", true_color),
            ));
        }

        let red_max = stream.read_u16().await?;
        let green_max = stream.read_u16().await?;
        let blue_max = stream.read_u16().await?;
        let red_shift = stream.read_u8().await?;
        let green_shift = stream.read_u8().await?;
        let blue_shift = stream.read_u8().await?;

        // Read and validate padding (3 bytes, must be zero)
        let mut padding = [0u8; 3];
        stream.read_bytes(&mut padding).await?;
        if padding != [0, 0, 0] {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("padding must be zero, got {:?}", padding),
            ));
        }

        Ok(Self {
            bits_per_pixel,
            depth,
            big_endian,
            true_color,
            red_max,
            green_max,
            blue_max,
            red_shift,
            green_shift,
            blue_shift,
        })
    }

    /// Write this PixelFormat to an RFB output stream.
    ///
    /// # Errors
    ///
    /// Returns an error if boolean fields are not 0 or 1.
    pub fn write_to<W: AsyncWrite + Unpin>(
        &self,
        stream: &mut RfbOutStream<W>,
    ) -> std::io::Result<()> {
        // Validate booleans before writing
        if self.big_endian > 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("big_endian must be 0 or 1, got {}", self.big_endian),
            ));
        }
        if self.true_color > 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("true_color must be 0 or 1, got {}", self.true_color),
            ));
        }

        stream.write_u8(self.bits_per_pixel);
        stream.write_u8(self.depth);
        stream.write_u8(self.big_endian);
        stream.write_u8(self.true_color);
        stream.write_u16(self.red_max);
        stream.write_u16(self.green_max);
        stream.write_u16(self.blue_max);
        stream.write_u8(self.red_shift);
        stream.write_u8(self.green_shift);
        stream.write_u8(self.blue_shift);
        // 3 bytes padding (must be zero)
        stream.write_u8(0);
        stream.write_u8(0);
        stream.write_u8(0);

        Ok(())
    }
}

/// Rectangle header for framebuffer updates.
///
/// Describes a rectangular region of the screen along with the encoding
/// type used for its pixel data.
///
/// # Wire Format
///
/// Rectangle header is 12 bytes:
/// - 2 bytes: x position
/// - 2 bytes: y position
/// - 2 bytes: width
/// - 2 bytes: height
/// - 4 bytes: encoding type (signed i32)
///
/// # Note
///
/// The Rectangle struct only contains the header. The actual pixel data
/// follows and must be parsed according to the encoding type by separate
/// decoder implementations (Phase 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rectangle {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    pub encoding: i32,
}

impl Rectangle {
    /// Read a Rectangle header from an RFB input stream.
    ///
    /// **Note**: This only reads the 12-byte header. The encoding-specific
    /// pixel data that follows must be handled separately.
    pub async fn read_from<R: AsyncRead + Unpin>(
        stream: &mut RfbInStream<R>,
    ) -> std::io::Result<Self> {
        Ok(Self {
            x: stream.read_u16().await?,
            y: stream.read_u16().await?,
            width: stream.read_u16().await?,
            height: stream.read_u16().await?,
            encoding: stream.read_i32().await?,
        })
    }

    /// Write a Rectangle header to an RFB output stream.
    pub fn write_to<W: AsyncWrite + Unpin>(&self, stream: &mut RfbOutStream<W>) {
        stream.write_u16(self.x);
        stream.write_u16(self.y);
        stream.write_u16(self.width);
        stream.write_u16(self.height);
        stream.write_i32(self.encoding);
    }
}

//
// Encoding type constants
//

/// Raw encoding - uncompressed pixel data.
pub const ENCODING_RAW: i32 = 0;

/// CopyRect encoding - copy from another screen region.
pub const ENCODING_COPYRECT: i32 = 1;

/// RRE (Rise-and-Run-length Encoding).
pub const ENCODING_RRE: i32 = 2;

/// Hextile encoding - 16x16 tile-based compression.
pub const ENCODING_HEXTILE: i32 = 5;

/// Tight encoding - JPEG and zlib compression.
pub const ENCODING_TIGHT: i32 = 7;

/// ZRLE (Zlib Run-Length Encoding).
pub const ENCODING_ZRLE: i32 = 16;

//
// ContentCache encoding types
//

/// CachedRect encoding - reference to cached content (20 bytes: cache_id only).
/// Server sends this when content is already in client's cache.
pub const ENCODING_CACHED_RECT: i32 = -512; // 0xFFFFFE00

/// CachedRectInit encoding - initial transmission with cache ID.
/// Server sends this for new content, includes cache_id + actual encoding + pixel data.
pub const ENCODING_CACHED_RECT_INIT: i32 = -511; // 0xFFFFFE01

//
// Pseudo-encodings (for capability negotiation)
//

/// Pseudo-encoding to advertise ContentCache support.
/// Client includes this in SetEncodings to enable ContentCache protocol.
pub const PSEUDO_ENCODING_CONTENT_CACHE: i32 = -496; // 0xFFFFFE10

//
// Security type constants
//

/// No security - no authentication required.
pub const SECURITY_TYPE_NONE: u8 = 1;

/// VNC authentication - challenge-response with password.
pub const SECURITY_TYPE_VNC_AUTH: u8 = 2;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[tokio::test]
    async fn test_pixelformat_bytes_per_pixel() {
        let pf = PixelFormat {
            bits_per_pixel: 8,
            depth: 8,
            big_endian: 0,
            true_color: 1,
            red_max: 7,
            green_max: 7,
            blue_max: 3,
            red_shift: 0,
            green_shift: 3,
            blue_shift: 6,
        };
        assert_eq!(pf.bytes_per_pixel(), 1);

        let pf = PixelFormat {
            bits_per_pixel: 16,
            depth: 16,
            big_endian: 0,
            true_color: 1,
            red_max: 31,
            green_max: 63,
            blue_max: 31,
            red_shift: 11,
            green_shift: 5,
            blue_shift: 0,
        };
        assert_eq!(pf.bytes_per_pixel(), 2);

        let pf = PixelFormat {
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
        };
        assert_eq!(pf.bytes_per_pixel(), 4);
    }

    #[tokio::test]
    async fn test_pixelformat_round_trip() {
        let original = PixelFormat {
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
        };

        // Write to buffer
        let mut buffer = Vec::new();
        let mut out_stream = RfbOutStream::new(&mut buffer);
        original.write_to(&mut out_stream).unwrap();
        out_stream.flush().await.unwrap();

        // Read back
        let mut in_stream = RfbInStream::new(Cursor::new(buffer));
        let read_back = PixelFormat::read_from(&mut in_stream).await.unwrap();

        assert_eq!(original, read_back);
    }

    #[tokio::test]
    async fn test_pixelformat_invalid_boolean() {
        // big_endian = 2 (invalid)
        let data = vec![
            32, 24, 2, 1, // bits_per_pixel, depth, big_endian (INVALID), true_color
            0, 255, 0, 255, 0, 255, // red_max, green_max, blue_max
            16, 8, 0, // red_shift, green_shift, blue_shift
            0, 0, 0, // padding
        ];
        let mut stream = RfbInStream::new(Cursor::new(data));
        let result = PixelFormat::read_from(&mut stream).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_pixelformat_invalid_padding() {
        // padding = [1, 0, 0] (invalid)
        let data = vec![
            32, 24, 0, 1, // bits_per_pixel, depth, big_endian, true_color
            0, 255, 0, 255, 0, 255, // red_max, green_max, blue_max
            16, 8, 0, // red_shift, green_shift, blue_shift
            1, 0, 0, // padding (INVALID - first byte non-zero)
        ];
        let mut stream = RfbInStream::new(Cursor::new(data));
        let result = PixelFormat::read_from(&mut stream).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_rectangle_round_trip() {
        let original = Rectangle {
            x: 100,
            y: 200,
            width: 640,
            height: 480,
            encoding: ENCODING_RAW,
        };

        // Write to buffer
        let mut buffer = Vec::new();
        let mut out_stream = RfbOutStream::new(&mut buffer);
        original.write_to(&mut out_stream);
        out_stream.flush().await.unwrap();

        // Read back
        let mut in_stream = RfbInStream::new(Cursor::new(buffer));
        let read_back = Rectangle::read_from(&mut in_stream).await.unwrap();

        assert_eq!(original, read_back);
    }
}
