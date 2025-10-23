//! CopyRect encoding decoder - copy rectangle from another screen location.
//!
//! CopyRect encoding (type 1) is a pseudo-encoding that instructs the client to copy
//! a rectangle from one location on the screen to another. This is highly efficient
//! for operations like window dragging or scrolling, where the content doesn't change
//! but its position does.
//!
//! # Wire Format
//!
//! ```text
//! +----------+----------+
//! | src_x    | src_y    |  2 bytes each (u16, network byte order)
//! +----------+----------+
//! ```
//!
//! The rectangle's `x`, `y`, `width`, and `height` fields specify the **destination**
//! rectangle. The `src_x` and `src_y` fields (read from the stream) specify the
//! **source** location to copy from.
//!
//! # Performance
//!
//! CopyRect is extremely bandwidth-efficient - only 4 bytes are transmitted regardless
//! of the rectangle size. The copy operation is performed entirely on the client side
//! using the existing framebuffer data.
//!
//! # Example
//!
//! ```no_run
//! use rfb_encodings::{Decoder, CopyRectDecoder, ENCODING_COPY_RECT};
//!
//! let decoder = CopyRectDecoder;
//! assert_eq!(decoder.encoding_type(), ENCODING_COPY_RECT);
//! ```
//!
//! # Overlapping Rectangles
//!
//! The decoder supports overlapping source and destination rectangles. The
//! `MutablePixelBuffer::copy_rect()` implementation is required to handle
//! overlaps correctly (typically by using `memmove` semantics).

use crate::{Decoder, MutablePixelBuffer, PixelFormat, Rectangle, RfbInStream, ENCODING_COPY_RECT};
use anyhow::{Context, Result};
use rfb_common::{Point, Rect};
use tokio::io::AsyncRead;

/// Decoder for CopyRect encoding - copy pixels from another screen location.
///
/// This encoding transmits only the source coordinates (4 bytes) and instructs
/// the client to copy a rectangle from the source position to the destination
/// position within the existing framebuffer.
///
/// # Example
///
/// ```no_run
/// # use rfb_encodings::{Decoder, CopyRectDecoder, ENCODING_COPY_RECT};
/// let decoder = CopyRectDecoder;
/// assert_eq!(decoder.encoding_type(), ENCODING_COPY_RECT);
/// ```
pub struct CopyRectDecoder;

impl Decoder for CopyRectDecoder {
    fn encoding_type(&self) -> i32 {
        ENCODING_COPY_RECT
    }

    async fn decode<R: AsyncRead + Unpin>(
        &self,
        stream: &mut RfbInStream<R>,
        rect: &Rectangle,
        _pixel_format: &PixelFormat,
        buffer: &mut dyn MutablePixelBuffer,
    ) -> Result<()> {
        // Empty rectangle - nothing to copy
        if rect.width == 0 || rect.height == 0 {
            return Ok(());
        }

        // Read source position from stream (2 bytes x, 2 bytes y)
        let src_x = stream
            .read_u16()
            .await
            .context("Failed to read CopyRect src_x")?;
        let src_y = stream
            .read_u16()
            .await
            .context("Failed to read CopyRect src_y")?;

        // Create destination rectangle
        let dest = Rect::new(
            rect.x as i32,
            rect.y as i32,
            rect.width as u32,
            rect.height as u32,
        );

        // Calculate offset from dest to source
        let src_offset = Point::new(src_x as i32 - rect.x as i32, src_y as i32 - rect.y as i32);

        // Use the buffer's copy_rect method which handles overlaps correctly
        buffer
            .copy_rect(dest, src_offset)
            .context("Failed to copy rectangle within buffer")?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rfb_pixelbuffer::{ManagedPixelBuffer, PixelBuffer};
    use std::io::Cursor;

    /// Create a simple RGB888 pixel format for testing
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

    /// Helper to set a pixel in the buffer
    fn set_pixel(
        buffer: &mut ManagedPixelBuffer,
        x: i32,
        y: i32,
        r: u8,
        g: u8,
        b: u8,
        a: u8,
    ) -> Result<()> {
        let rect = Rect::new(x, y, 1, 1);
        let pixel_data = vec![b, g, r, a]; // BGRA order
        buffer.image_rect(rect, &pixel_data, 1)
    }

    /// Helper to get a pixel from the buffer
    fn get_pixel(buffer: &ManagedPixelBuffer, x: i32, y: i32) -> [u8; 4] {
        let rect = Rect::new(x, y, 1, 1);
        let mut stride = 0;
        let pixels = buffer.get_buffer(rect, &mut stride).unwrap();
        [pixels[0], pixels[1], pixels[2], pixels[3]]
    }

    #[tokio::test]
    async fn test_copyrect_decoder_type() {
        let decoder = CopyRectDecoder;
        assert_eq!(decoder.encoding_type(), ENCODING_COPY_RECT);
    }

    #[tokio::test]
    async fn test_decode_empty_rectangle() {
        let decoder = CopyRectDecoder;
        let pixel_format = test_pixel_format();
        let buffer_format = rfb_pixelbuffer::PixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(100, 100, buffer_format);

        // Empty rectangle (0x0)
        let rect = Rectangle {
            x: 10,
            y: 10,
            width: 0,
            height: 0,
            encoding: ENCODING_COPY_RECT,
        };

        // Stream with src_x=5, src_y=5 (though it won't be read for empty rect)
        let data: Vec<u8> = vec![0, 5, 0, 5]; // src_x=5, src_y=5
        let cursor = Cursor::new(data);
        let mut stream = RfbInStream::new(cursor);

        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buffer)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_decode_single_pixel() {
        let decoder = CopyRectDecoder;
        let pixel_format = test_pixel_format();
        let buffer_format = rfb_pixelbuffer::PixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(100, 100, buffer_format);

        // Set source pixel at (5, 5) to red
        set_pixel(&mut buffer, 5, 5, 255, 0, 0, 255).unwrap();

        // Copy single pixel from (5, 5) to (10, 10)
        let rect = Rectangle {
            x: 10,
            y: 10,
            width: 1,
            height: 1,
            encoding: ENCODING_COPY_RECT,
        };

        // Stream contains src_x=5, src_y=5
        let data: Vec<u8> = vec![0, 5, 0, 5];
        let cursor = Cursor::new(data);
        let mut stream = RfbInStream::new(cursor);

        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buffer)
            .await;
        assert!(result.is_ok());

        // Verify pixel was copied
        let pixel = get_pixel(&buffer, 10, 10);
        assert_eq!(pixel, [0, 0, 255, 255]); // BGRA: red
    }

    #[tokio::test]
    async fn test_decode_non_overlapping_rectangles() {
        let decoder = CopyRectDecoder;
        let pixel_format = test_pixel_format();
        let buffer_format = rfb_pixelbuffer::PixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(100, 100, buffer_format);

        // Create a 3x2 pattern in the source area (10,10)
        for y in 0..2 {
            for x in 0..3 {
                let r = (x * 50) as u8;
                let g = (y * 100) as u8;
                set_pixel(&mut buffer, 10 + x, 10 + y, r, g, 0, 255).unwrap();
            }
        }

        // Copy 3x2 rectangle from (10, 10) to (50, 50)
        let rect = Rectangle {
            x: 50,
            y: 50,
            width: 3,
            height: 2,
            encoding: ENCODING_COPY_RECT,
        };

        // Stream contains src_x=10, src_y=10
        let data: Vec<u8> = vec![0, 10, 0, 10];
        let cursor = Cursor::new(data);
        let mut stream = RfbInStream::new(cursor);

        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buffer)
            .await;
        assert!(result.is_ok());

        // Verify all pixels were copied correctly
        for y in 0..2 {
            for x in 0..3 {
                let src_pixel = get_pixel(&buffer, 10 + x, 10 + y);
                let dst_pixel = get_pixel(&buffer, 50 + x, 50 + y);
                assert_eq!(
                    src_pixel,
                    dst_pixel,
                    "Pixel at ({}, {}) should match source",
                    50 + x,
                    50 + y
                );
            }
        }
    }

    #[tokio::test]
    async fn test_decode_overlapping_rectangles() {
        let decoder = CopyRectDecoder;
        let pixel_format = test_pixel_format();
        let buffer_format = rfb_pixelbuffer::PixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(100, 100, buffer_format);

        // Create a horizontal pattern at y=10, x=10..15
        for x in 10..15 {
            let r = ((x - 10) * 50) as u8;
            set_pixel(&mut buffer, x, 10, r, 0, 0, 255).unwrap();
        }

        // Copy overlapping: from (10,10) 5x1 to (12,10) 5x1
        // This shifts the pattern right by 2
        let rect = Rectangle {
            x: 12,
            y: 10,
            width: 5,
            height: 1,
            encoding: ENCODING_COPY_RECT,
        };

        // Stream contains src_x=10, src_y=10
        let data: Vec<u8> = vec![0, 10, 0, 10];
        let cursor = Cursor::new(data);
        let mut stream = RfbInStream::new(cursor);

        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buffer)
            .await;
        assert!(result.is_ok());

        // Verify the pattern was shifted correctly
        // Original: [0, 50, 100, 150, 200] at x=10..15
        // After:    [0, 50, 0, 50, 100] at x=10..15
        //                  ^^^^^^^^^^^ copied from x=10..15 to x=12..17
        let pixel_12 = get_pixel(&buffer, 12, 10);
        assert_eq!(pixel_12[2], 0); // Should be first pixel from source

        let pixel_13 = get_pixel(&buffer, 13, 10);
        assert_eq!(pixel_13[2], 50); // Should be second pixel from source
    }

    #[tokio::test]
    async fn test_decode_eof_error() {
        let decoder = CopyRectDecoder;
        let pixel_format = test_pixel_format();
        let buffer_format = rfb_pixelbuffer::PixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(100, 100, buffer_format);

        // Request 2x2 copy
        let rect = Rectangle {
            x: 10,
            y: 10,
            width: 2,
            height: 2,
            encoding: ENCODING_COPY_RECT,
        };

        // Provide incomplete data (only src_x, missing src_y)
        let data: Vec<u8> = vec![0, 5]; // Only 2 bytes, need 4
        let cursor = Cursor::new(data);
        let mut stream = RfbInStream::new(cursor);

        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buffer)
            .await;

        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("Failed to read CopyRect"));
    }

    #[tokio::test]
    async fn test_decode_source_out_of_bounds() {
        let decoder = CopyRectDecoder;
        let pixel_format = test_pixel_format();
        let buffer_format = rfb_pixelbuffer::PixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(10, 10, buffer_format);

        // Destination is valid, but source extends beyond buffer
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 5,
            height: 5,
            encoding: ENCODING_COPY_RECT,
        };

        // Source at (8, 8) with 5x5 size would extend to (13, 13) - out of bounds
        let data: Vec<u8> = vec![0, 8, 0, 8];
        let cursor = Cursor::new(data);
        let mut stream = RfbInStream::new(cursor);

        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buffer)
            .await;

        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("Failed to copy rectangle"));
    }

    #[tokio::test]
    async fn test_decode_destination_out_of_bounds() {
        let decoder = CopyRectDecoder;
        let pixel_format = test_pixel_format();
        let buffer_format = rfb_pixelbuffer::PixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(10, 10, buffer_format);

        // Destination extends beyond buffer bounds
        let rect = Rectangle {
            x: 8,
            y: 8,
            width: 5, // Would extend to x=13, but buffer is only 10 wide
            height: 5,
            encoding: ENCODING_COPY_RECT,
        };

        // Source is valid
        let data: Vec<u8> = vec![0, 0, 0, 0]; // src at (0,0)
        let cursor = Cursor::new(data);
        let mut stream = RfbInStream::new(cursor);

        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buffer)
            .await;

        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("Failed to copy rectangle"));
    }
}
