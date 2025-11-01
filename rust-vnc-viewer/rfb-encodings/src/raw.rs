//! Raw encoding decoder - uncompressed pixel data.
//!
//! Raw encoding (type 0) is the simplest VNC encoding. It transmits pixels as
//! uncompressed data in the server's pixel format. The decoder reads
//! `width * height * bytes_per_pixel` bytes from the stream and writes them
//! directly to the pixel buffer.
//!
//! # Wire Format
//!
//! ```text
//! +-------------+
//! | Pixel data  |  width * height * bytes_per_pixel bytes
//! +-------------+
//! ```
//!
//! Each pixel is transmitted in the server's pixel format (as negotiated during
//! the ServerInit handshake). No compression or encoding is applied.
//!
//! # Performance
//!
//! Raw encoding is the least efficient in terms of bandwidth (since no compression
//! is used), but it's the simplest to decode and requires minimal CPU. It's typically
//! used as a fallback when other encodings aren't available or suitable.
//!
//! # Example
//!
//! ```no_run
//! use rfb_encodings::{Decoder, RawDecoder, ENCODING_RAW};
//!
//! let decoder = RawDecoder;
//! assert_eq!(decoder.encoding_type(), ENCODING_RAW);
//! ```

use crate::{Decoder, MutablePixelBuffer, PixelFormat, Rectangle, RfbInStream, ENCODING_RAW};
use anyhow::{Context, Result};
use rfb_common::Rect;
use tokio::io::AsyncRead;

/// Decoder for raw (uncompressed) pixel data.
///
/// This is the simplest VNC encoding - pixels are transmitted without any
/// compression or transformation. The decoder reads `width * height * bytes_per_pixel`
/// bytes from the stream and writes them to the buffer.
///
/// # Example
///
/// ```no_run
/// # use rfb_encodings::{Decoder, RawDecoder, ENCODING_RAW};
/// let decoder = RawDecoder;
/// assert_eq!(decoder.encoding_type(), ENCODING_RAW);
/// ```
pub struct RawDecoder;

impl Decoder for RawDecoder {
    fn encoding_type(&self) -> i32 {
        ENCODING_RAW
    }

    async fn decode<R: AsyncRead + Unpin>(
        &self,
        stream: &mut RfbInStream<R>,
        rect: &Rectangle,
        pixel_format: &PixelFormat,
        buffer: &mut dyn MutablePixelBuffer,
    ) -> Result<()> {
        let buffer_before = stream.available();
        tracing::debug!(
            target: "rfb_encodings::framing",
            "Raw decode start: rect=[{},{} {}x{}] buffer_before={}",
            rect.x, rect.y, rect.width, rect.height,
            buffer_before
        );

        // Calculate dimensions and validate
        let width = rect.width as usize;
        let height = rect.height as usize;

        if width == 0 || height == 0 {
            tracing::debug!(
                target: "rfb_encodings::framing",
                "Raw decode end: empty rectangle, bytes_consumed=0, buffer_after={}",
                stream.available()
            );
            return Ok(()); // Empty rectangle - nothing to decode
        }

        let bytes_per_pixel = pixel_format.bytes_per_pixel() as usize;
        let total_bytes = width * height * bytes_per_pixel;

        // Read all pixel data from the stream
        let mut pixel_data = vec![0u8; total_bytes];
        stream
            .read_bytes(&mut pixel_data)
            .await
            .context("Failed to read raw pixel data from stream")?;

        // Get the destination rectangle in the buffer
        let dest_rect = Rect::new(
            rect.x as i32,
            rect.y as i32,
            rect.width as u32,
            rect.height as u32,
        );

        // Write pixel data to buffer using image_rect
        // Stride equals width since we have tightly packed data
        buffer
            .image_rect(dest_rect, &pixel_data, width)
            .context("Failed to write raw pixel data to buffer")?;

        let buffer_after = stream.available();
        tracing::debug!(
            target: "rfb_encodings::framing",
            "Raw decode end: bytes_consumed={}, buffer_after={}",
            buffer_before.saturating_sub(buffer_after),
            buffer_after
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rfb_pixelbuffer::{ManagedPixelBuffer, PixelBuffer};
    use std::io::Cursor;

    /// Create a simple RGB888 pixel format for testing
    /// Note: Using rfb_protocol::messages::types::PixelFormat (from wire format)
    fn test_pixel_format() -> crate::PixelFormat {
        PixelFormat {
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
        }
    }

    #[tokio::test]
    async fn test_raw_decoder_type() {
        let decoder = RawDecoder;
        assert_eq!(decoder.encoding_type(), ENCODING_RAW);
    }

    #[tokio::test]
    async fn test_decode_empty_rectangle() {
        let decoder = RawDecoder;
        let pixel_format = test_pixel_format();
        let buffer_format = rfb_pixelbuffer::PixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(100, 100, buffer_format);

        // Empty rectangle (0x0)
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            encoding: ENCODING_RAW,
        };

        let empty_data: Vec<u8> = vec![];
        let cursor = Cursor::new(empty_data);
        let mut stream = RfbInStream::new(cursor);

        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buffer)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_decode_single_pixel() {
        let decoder = RawDecoder;
        let pixel_format = test_pixel_format();
        // Create buffer with rfb_pixelbuffer::PixelFormat (converted from wire format)
        let buffer_format = rfb_pixelbuffer::PixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(100, 100, buffer_format);

        // Single pixel at (10, 10) - red color
        let rect = Rectangle {
            x: 10,
            y: 10,
            width: 1,
            height: 1,
            encoding: ENCODING_RAW,
        };

        // Red pixel in RGB888: R=255, G=0, B=0, A=255
        let pixel_data = vec![0x00, 0x00, 0xFF, 0xFF]; // BGRA order (little-endian)
        let cursor = Cursor::new(pixel_data);
        let mut stream = RfbInStream::new(cursor);

        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buffer)
            .await;
        assert!(result.is_ok());

        // Verify pixel was written
        let read_rect = Rect::new(10, 10, 1, 1);
        let mut stride = 0;
        let pixels = buffer.get_buffer(read_rect, &mut stride).unwrap();
        // stride is in pixels, and buffer is 100 pixels wide
        // So we get 1 row * 100 pixels * 4 bytes = 400 bytes
        assert_eq!(stride, 100); // Buffer width
        let _bytes_per_pixel = 4;
        // First pixel is at offset 0
        assert_eq!(pixels[0], 0x00); // B
        assert_eq!(pixels[1], 0x00); // G
        assert_eq!(pixels[2], 0xFF); // R
        assert_eq!(pixels[3], 0xFF); // A
    }

    #[tokio::test]
    async fn test_decode_small_rectangle() {
        let decoder = RawDecoder;
        let pixel_format = test_pixel_format();
        let buffer_format = rfb_pixelbuffer::PixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(100, 100, buffer_format);

        // 3x2 rectangle at (5, 5)
        let rect = Rectangle {
            x: 5,
            y: 5,
            width: 3,
            height: 2,
            encoding: ENCODING_RAW,
        };

        // Create test pattern: 6 pixels (3x2)
        let mut pixel_data = vec![0u8; 6 * 4]; // 6 pixels * 4 bytes
        for i in 0..6 {
            pixel_data[i * 4] = (i * 10) as u8; // B
            pixel_data[i * 4 + 1] = (i * 20) as u8; // G
            pixel_data[i * 4 + 2] = (i * 30) as u8; // R
            pixel_data[i * 4 + 3] = 255; // A
        }

        let cursor = Cursor::new(pixel_data.clone());
        let mut stream = RfbInStream::new(cursor);

        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buffer)
            .await;
        assert!(result.is_ok());

        // Verify pixels were written correctly
        let read_rect = Rect::new(5, 5, 3, 2);
        let mut stride = 0;
        let pixels = buffer.get_buffer(read_rect, &mut stride).unwrap();
        assert_eq!(stride, 100); // Buffer width

        let bytes_per_pixel = 4;
        // First pixel at (5,5) is at offset 0 in the returned slice
        assert_eq!(pixels[0], 0); // B
        assert_eq!(pixels[1], 0); // G
        assert_eq!(pixels[2], 0); // R

        // Pixel 5 is at row 1, col 2 (second row, third column)
        // Offset = (1 * stride + 2) * bytes_per_pixel
        let last_offset = (1 * stride + 2) * bytes_per_pixel;
        assert_eq!(pixels[last_offset], 50); // B
        assert_eq!(pixels[last_offset + 1], 100); // G
        assert_eq!(pixels[last_offset + 2], 150); // R
    }

    #[tokio::test]
    async fn test_decode_eof_error() {
        let decoder = RawDecoder;
        let pixel_format = test_pixel_format();
        let buffer_format = rfb_pixelbuffer::PixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(100, 100, buffer_format);

        // Request 2x2 rectangle (16 bytes needed)
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
            encoding: ENCODING_RAW,
        };

        // Provide only 8 bytes (insufficient data)
        let pixel_data = vec![0u8; 8];
        let cursor = Cursor::new(pixel_data);
        let mut stream = RfbInStream::new(cursor);

        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buffer)
            .await;

        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("Failed to read raw pixel data"));
    }

    #[tokio::test]
    async fn test_decode_out_of_bounds() {
        let decoder = RawDecoder;
        let pixel_format = test_pixel_format();
        let buffer_format = rfb_pixelbuffer::PixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(10, 10, buffer_format);

        // Rectangle extends beyond buffer bounds
        let rect = Rectangle {
            x: 8,
            y: 8,
            width: 5, // Would extend to x=13, but buffer is only 10 wide
            height: 5,
            encoding: ENCODING_RAW,
        };

        let pixel_data = vec![0u8; 5 * 5 * 4]; // 25 pixels
        let cursor = Cursor::new(pixel_data);
        let mut stream = RfbInStream::new(cursor);

        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buffer)
            .await;

        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("Failed to write raw pixel data"));
    }

    #[tokio::test]
    async fn test_decode_rgb565_format() {
        let decoder = RawDecoder;

        // RGB565 pixel format (16-bit) - wire format
        let pixel_format = crate::PixelFormat {
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

        // Create buffer with equivalent rfb_pixelbuffer format
        let buffer_format = rfb_pixelbuffer::PixelFormat {
            bits_per_pixel: 16,
            depth: 16,
            big_endian: false,
            true_color: true,
            red_max: 31,
            green_max: 63,
            blue_max: 31,
            red_shift: 11,
            green_shift: 5,
            blue_shift: 0,
        };
        let mut buffer = ManagedPixelBuffer::new(100, 100, buffer_format);

        // Single pixel
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
            encoding: ENCODING_RAW,
        };

        // Red in RGB565: 0b11111_000000_00000 = 0xF800
        let pixel_data = vec![0x00, 0xF8]; // Little-endian
        let cursor = Cursor::new(pixel_data);
        let mut stream = RfbInStream::new(cursor);

        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buffer)
            .await;
        assert!(result.is_ok());

        // Verify pixel was written
        let read_rect = Rect::new(0, 0, 1, 1);
        let mut stride = 0;
        let pixels = buffer.get_buffer(read_rect, &mut stride).unwrap();
        assert_eq!(stride, 100); // Buffer width
                                 // First pixel is at offset 0
        assert_eq!(pixels[0], 0x00);
        assert_eq!(pixels[1], 0xF8);
    }
}
