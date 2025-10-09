//! RRE encoding decoder - Rise-and-Run-length Encoding.
//!
//! RRE (Rise-and-Run-length Encoding, type 2) is a simple VNC encoding that
//! represents rectangular regions as a background color plus a list of solid-color
//! sub-rectangles. This encoding is efficient for screens with large areas of
//! uniform color, such as desktop backgrounds or flat UI elements.
//!
//! # Wire Format
//!
//! ```text
//! +------------------+
//! | num_subrects     |  4 bytes (u32, network byte order)
//! +------------------+
//! | background_pixel |  bytes_per_pixel bytes
//! +------------------+
//! | Subrectangle 1   |
//! |   pixel          |  bytes_per_pixel bytes
//! |   x              |  2 bytes (u16)
//! |   y              |  2 bytes (u16)
//! |   width          |  2 bytes (u16)
//! |   height         |  2 bytes (u16)
//! +------------------+
//! | Subrectangle 2   |
//! |   ...            |
//! +------------------+
//! | Subrectangle N   |
//! |   ...            |
//! +------------------+
//! ```
//!
//! The decoder first fills the entire rectangle with the background color, then
//! overwrites specific sub-regions with their respective colors.
//!
//! # Performance
//!
//! RRE is more bandwidth-efficient than Raw encoding for scenes with large
//! solid-color regions. However, it's less efficient than more sophisticated
//! encodings like Hextile or Tight for complex images.
//!
//! # Example
//!
//! ```no_run
//! use rfb_encodings::{Decoder, RREDecoder, ENCODING_RRE};
//!
//! let decoder = RREDecoder;
//! assert_eq!(decoder.encoding_type(), ENCODING_RRE);
//! ```

use crate::{Decoder, MutablePixelBuffer, PixelFormat, Rectangle, RfbInStream, ENCODING_RRE};
use anyhow::{anyhow, Context, Result};
use rfb_common::Rect;
use tokio::io::AsyncRead;

/// Decoder for RRE (Rise-and-Run-length Encoding).
///
/// This encoding transmits a background color followed by a list of solid-color
/// sub-rectangles. The decoder fills the entire rectangle with the background
/// color, then overwrites each sub-rectangle with its specified color.
///
/// # Example
///
/// ```no_run
/// # use rfb_encodings::{Decoder, RREDecoder, ENCODING_RRE};
/// let decoder = RREDecoder;
/// assert_eq!(decoder.encoding_type(), ENCODING_RRE);
/// ```
pub struct RREDecoder;

impl Decoder for RREDecoder {
    fn encoding_type(&self) -> i32 {
        ENCODING_RRE
    }

    async fn decode<R: AsyncRead + Unpin>(
        &self,
        stream: &mut RfbInStream<R>,
        rect: &Rectangle,
        pixel_format: &PixelFormat,
        buffer: &mut dyn MutablePixelBuffer,
    ) -> Result<()> {
        // Empty rectangle - nothing to decode
        if rect.width == 0 || rect.height == 0 {
            return Ok(());
        }

        let bytes_per_pixel = pixel_format.bits_per_pixel / 8;
        if bytes_per_pixel == 0 || bytes_per_pixel > 4 {
            return Err(anyhow!(
                "Invalid bytes_per_pixel: {} (must be 1-4)",
                bytes_per_pixel
            ));
        }

        // Read number of sub-rectangles
        let num_subrects = stream
            .read_u32()
            .await
            .context("Failed to read RRE num_subrects")?;

        // Read background pixel
        let mut bg_pixel = vec![0u8; bytes_per_pixel as usize];
        stream
            .read_bytes(&mut bg_pixel)
            .await
            .context("Failed to read RRE background pixel")?;

        // Fill entire rectangle with background color
        let dest_rect = Rect::new(
            rect.x as i32,
            rect.y as i32,
            rect.width as u32,
            rect.height as u32,
        );
        buffer
            .fill_rect(dest_rect, &bg_pixel)
            .context("Failed to fill background in RRE decode")?;

        // Decode each sub-rectangle
        for i in 0..num_subrects {
            // Read sub-rectangle pixel color
            let mut pixel = vec![0u8; bytes_per_pixel as usize];
            stream
                .read_bytes(&mut pixel)
                .await
                .with_context(|| format!("Failed to read pixel for RRE subrect {}", i))?;

            // Read sub-rectangle coordinates
            let x = stream
                .read_u16()
                .await
                .with_context(|| format!("Failed to read x for RRE subrect {}", i))?;
            let y = stream
                .read_u16()
                .await
                .with_context(|| format!("Failed to read y for RRE subrect {}", i))?;
            let width = stream
                .read_u16()
                .await
                .with_context(|| format!("Failed to read width for RRE subrect {}", i))?;
            let height = stream
                .read_u16()
                .await
                .with_context(|| format!("Failed to read height for RRE subrect {}", i))?;

            // Validate coordinates are within the main rectangle
            let right = x
                .checked_add(width)
                .ok_or_else(|| anyhow!("RRE subrect {} x+width overflows: {} + {}", i, x, width))?;
            let bottom = y.checked_add(height).ok_or_else(|| {
                anyhow!("RRE subrect {} y+height overflows: {} + {}", i, y, height)
            })?;

            if right > rect.width {
                return Err(anyhow!(
                    "RRE subrect {} extends beyond rectangle width: x={}, width={}, rect.width={}",
                    i,
                    x,
                    width,
                    rect.width
                ));
            }
            if bottom > rect.height {
                return Err(anyhow!(
                    "RRE subrect {} extends beyond rectangle height: y={}, height={}, rect.height={}",
                    i,
                    y,
                    height,
                    rect.height
                ));
            }

            // Skip zero-area sub-rectangles (they don't affect the output)
            if width == 0 || height == 0 {
                continue;
            }

            // Convert relative coordinates to absolute buffer coordinates
            let abs_x = rect
                .x
                .checked_add(x)
                .ok_or_else(|| anyhow!("RRE subrect {} absolute x overflows", i))?;
            let abs_y = rect
                .y
                .checked_add(y)
                .ok_or_else(|| anyhow!("RRE subrect {} absolute y overflows", i))?;

            let subrect = Rect::new(abs_x as i32, abs_y as i32, width as u32, height as u32);
            buffer.fill_rect(subrect, &pixel).with_context(|| {
                format!(
                    "Failed to fill RRE subrect {} at ({}, {}) size {}x{}",
                    i, abs_x, abs_y, width, height
                )
            })?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rfb_pixelbuffer::{ManagedPixelBuffer, PixelBuffer};
    use std::io::Cursor;

    /// Create a simple RGB888 pixel format for testing (wire format)
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

    /// Helper to create an RRE packet for testing
    fn make_rre_packet(_bpp: u8, bg: &[u8], subrects: &[(&[u8], u16, u16, u16, u16)]) -> Vec<u8> {
        let mut data = Vec::new();

        // Number of sub-rectangles (u32, big-endian)
        data.extend_from_slice(&(subrects.len() as u32).to_be_bytes());

        // Background pixel
        data.extend_from_slice(bg);

        // Each sub-rectangle
        for (pixel, x, y, w, h) in subrects {
            data.extend_from_slice(pixel);
            data.extend_from_slice(&x.to_be_bytes());
            data.extend_from_slice(&y.to_be_bytes());
            data.extend_from_slice(&w.to_be_bytes());
            data.extend_from_slice(&h.to_be_bytes());
        }

        data
    }

    /// Helper to get a pixel from the buffer
    fn get_pixel(buffer: &ManagedPixelBuffer, x: i32, y: i32) -> [u8; 4] {
        let rect = Rect::new(x, y, 1, 1);
        let mut stride = 0;
        let pixels = buffer.get_buffer(rect, &mut stride).unwrap();
        [pixels[0], pixels[1], pixels[2], pixels[3]]
    }

    #[tokio::test]
    async fn test_rre_decoder_type() {
        let decoder = RREDecoder;
        assert_eq!(decoder.encoding_type(), ENCODING_RRE);
    }

    #[tokio::test]
    async fn test_decode_empty_rectangle() {
        let decoder = RREDecoder;
        let pixel_format = test_pixel_format();
        let buffer_format = rfb_pixelbuffer::PixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(100, 100, buffer_format);

        // Empty rectangle (0x0)
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            encoding: ENCODING_RRE,
        };

        let data = vec![]; // No data needed for empty rect
        let cursor = Cursor::new(data);
        let mut stream = RfbInStream::new(cursor);

        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buffer)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_decode_background_only() {
        let decoder = RREDecoder;
        let pixel_format = test_pixel_format();
        let buffer_format = rfb_pixelbuffer::PixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(100, 100, buffer_format);

        // 10x10 rectangle with blue background, no sub-rectangles
        let rect = Rectangle {
            x: 5,
            y: 5,
            width: 10,
            height: 10,
            encoding: ENCODING_RRE,
        };

        let bg = &[0, 0, 255, 255]; // Blue in BGRA
        let data = make_rre_packet(4, bg, &[]);
        let cursor = Cursor::new(data);
        let mut stream = RfbInStream::new(cursor);

        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buffer)
            .await;
        assert!(result.is_ok());

        // Verify background color
        let pixel = get_pixel(&buffer, 5, 5);
        assert_eq!(pixel, [0, 0, 255, 255]);
        let pixel = get_pixel(&buffer, 14, 14);
        assert_eq!(pixel, [0, 0, 255, 255]);
    }

    #[tokio::test]
    async fn test_decode_single_subrectangle() {
        let decoder = RREDecoder;
        let pixel_format = test_pixel_format();
        let buffer_format = rfb_pixelbuffer::PixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(100, 100, buffer_format);

        // 10x10 rectangle with blue background and one red 3x3 sub-rectangle at (2,2)
        let rect = Rectangle {
            x: 10,
            y: 10,
            width: 10,
            height: 10,
            encoding: ENCODING_RRE,
        };

        // BGRA: Blue=255, Green=0, Red=0, Alpha=255
        let blue_bg = &[255, 0, 0, 255]; // Blue in BGRA
        let red_sub = &[0, 0, 255, 255]; // Red in BGRA

        let data = make_rre_packet(4, blue_bg, &[(red_sub, 2, 2, 3, 3)]);
        let cursor = Cursor::new(data);
        let mut stream = RfbInStream::new(cursor);

        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buffer)
            .await;
        assert!(result.is_ok());

        // Verify background (top-left corner)
        let pixel = get_pixel(&buffer, 10, 10);
        assert_eq!(pixel, [255, 0, 0, 255]); // Blue

        // Verify sub-rectangle (at 12,12 - which is rect.x+2, rect.y+2)
        let pixel = get_pixel(&buffer, 12, 12);
        assert_eq!(pixel, [0, 0, 255, 255]); // Red

        // Verify sub-rectangle edge
        let pixel = get_pixel(&buffer, 14, 14);
        assert_eq!(pixel, [0, 0, 255, 255]); // Red (3x3 goes from 12-14)

        // Verify outside sub-rectangle is still background
        let pixel = get_pixel(&buffer, 15, 15);
        assert_eq!(pixel, [255, 0, 0, 255]); // Blue
    }

    #[tokio::test]
    async fn test_decode_multiple_subrectangles() {
        let decoder = RREDecoder;
        let pixel_format = test_pixel_format();
        let buffer_format = rfb_pixelbuffer::PixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(100, 100, buffer_format);

        // 20x20 rectangle with white background and three colored sub-rectangles
        let rect = Rectangle {
            x: 10,
            y: 10,
            width: 20,
            height: 20,
            encoding: ENCODING_RRE,
        };

        let white: &[u8] = &[255, 255, 255, 255]; // White
        let red: &[u8] = &[0, 0, 255, 255]; // Red in BGRA
        let green: &[u8] = &[0, 255, 0, 255]; // Green in BGRA
        let blue: &[u8] = &[255, 0, 0, 255]; // Blue in BGRA

        let subrects: &[(&[u8], u16, u16, u16, u16)] = &[
            (red, 0, 0, 5, 5),    // Red square at top-left
            (green, 15, 0, 5, 5), // Green square at top-right
            (blue, 7, 15, 6, 5),  // Blue rectangle at bottom-center
        ];

        let data = make_rre_packet(4, white, subrects);
        let cursor = Cursor::new(data);
        let mut stream = RfbInStream::new(cursor);

        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buffer)
            .await;
        assert!(result.is_ok());

        // Verify red sub-rectangle
        let pixel = get_pixel(&buffer, 10, 10); // rect.x + 0, rect.y + 0
        assert_eq!(pixel, [0, 0, 255, 255]);

        // Verify green sub-rectangle
        let pixel = get_pixel(&buffer, 25, 10); // rect.x + 15, rect.y + 0
        assert_eq!(pixel, [0, 255, 0, 255]);

        // Verify blue sub-rectangle
        let pixel = get_pixel(&buffer, 17, 25); // rect.x + 7, rect.y + 15
        assert_eq!(pixel, [255, 0, 0, 255]);

        // Verify white background between sub-rectangles
        // Red goes from (10,10) to (14,14), Green from (25,10) to (29,14)
        // So (20, 10) should be white background in the gap
        let pixel = get_pixel(&buffer, 20, 10); // Between red and green
        assert_eq!(pixel, [255, 255, 255, 255]);
    }

    #[tokio::test]
    async fn test_decode_subrect_at_boundary() {
        let decoder = RREDecoder;
        let pixel_format = test_pixel_format();
        let buffer_format = rfb_pixelbuffer::PixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(100, 100, buffer_format);

        // 10x10 rectangle with sub-rectangle exactly touching right and bottom edges
        let rect = Rectangle {
            x: 10,
            y: 10,
            width: 10,
            height: 10,
            encoding: ENCODING_RRE,
        };

        let white: &[u8] = &[255, 255, 255, 255];
        let red: &[u8] = &[0, 0, 255, 255];

        // Sub-rectangle at (7, 7) with size 3x3 - exactly touches bottom-right corner
        let subrects: &[(&[u8], u16, u16, u16, u16)] = &[(red, 7, 7, 3, 3)];

        let data = make_rre_packet(4, white, subrects);
        let cursor = Cursor::new(data);
        let mut stream = RfbInStream::new(cursor);

        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buffer)
            .await;
        assert!(result.is_ok());

        // Verify corner pixel
        let pixel = get_pixel(&buffer, 19, 19); // rect.x + 9, rect.y + 9
        assert_eq!(pixel, [0, 0, 255, 255]);
    }

    #[tokio::test]
    async fn test_decode_zero_sized_subrect() {
        let decoder = RREDecoder;
        let pixel_format = test_pixel_format();
        let buffer_format = rfb_pixelbuffer::PixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(100, 100, buffer_format);

        // Sub-rectangle with zero width
        let rect = Rectangle {
            x: 10,
            y: 10,
            width: 10,
            height: 10,
            encoding: ENCODING_RRE,
        };

        let white: &[u8] = &[255, 255, 255, 255];
        let red: &[u8] = &[0, 0, 255, 255];

        let subrects: &[(&[u8], u16, u16, u16, u16)] = &[(red, 5, 5, 0, 5)]; // Zero width

        let data = make_rre_packet(4, white, subrects);
        let cursor = Cursor::new(data);
        let mut stream = RfbInStream::new(cursor);

        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buffer)
            .await;
        assert!(result.is_ok()); // Should not crash

        // Verify background is unchanged
        let pixel = get_pixel(&buffer, 15, 15);
        assert_eq!(pixel, [255, 255, 255, 255]);
    }

    #[tokio::test]
    async fn test_decode_eof_reading_num_subrects() {
        let decoder = RREDecoder;
        let pixel_format = test_pixel_format();
        let buffer_format = rfb_pixelbuffer::PixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(100, 100, buffer_format);

        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
            encoding: ENCODING_RRE,
        };

        // Empty data - can't read num_subrects
        let data = vec![];
        let cursor = Cursor::new(data);
        let mut stream = RfbInStream::new(cursor);

        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buffer)
            .await;

        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("num_subrects"));
    }

    #[tokio::test]
    async fn test_decode_eof_reading_background() {
        let decoder = RREDecoder;
        let pixel_format = test_pixel_format();
        let buffer_format = rfb_pixelbuffer::PixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(100, 100, buffer_format);

        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
            encoding: ENCODING_RRE,
        };

        // Only num_subrects, no background pixel
        let mut data = Vec::new();
        data.extend_from_slice(&0u32.to_be_bytes()); // num_subrects = 0
                                                     // Missing background pixel (should be 4 bytes)

        let cursor = Cursor::new(data);
        let mut stream = RfbInStream::new(cursor);

        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buffer)
            .await;

        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("background"));
    }

    #[tokio::test]
    async fn test_decode_eof_reading_subrect() {
        let decoder = RREDecoder;
        let pixel_format = test_pixel_format();
        let buffer_format = rfb_pixelbuffer::PixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(100, 100, buffer_format);

        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
            encoding: ENCODING_RRE,
        };

        // num_subrects=1, background, but incomplete subrect data
        let mut data = Vec::new();
        data.extend_from_slice(&1u32.to_be_bytes()); // num_subrects = 1
        data.extend_from_slice(&[255, 255, 255, 255]); // background
        data.extend_from_slice(&[0, 0, 255, 255]); // subrect pixel
                                                   // Missing x, y, width, height

        let cursor = Cursor::new(data);
        let mut stream = RfbInStream::new(cursor);

        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buffer)
            .await;

        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("subrect"));
    }

    #[tokio::test]
    async fn test_decode_subrect_out_of_bounds() {
        let decoder = RREDecoder;
        let pixel_format = test_pixel_format();
        let buffer_format = rfb_pixelbuffer::PixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(100, 100, buffer_format);

        // 10x10 rectangle
        let rect = Rectangle {
            x: 10,
            y: 10,
            width: 10,
            height: 10,
            encoding: ENCODING_RRE,
        };

        let white: &[u8] = &[255, 255, 255, 255];
        let red: &[u8] = &[0, 0, 255, 255];

        // Sub-rectangle extends beyond main rectangle: x=8, width=5 -> right=13 > 10
        let subrects: &[(&[u8], u16, u16, u16, u16)] = &[(red, 8, 0, 5, 5)];

        let data = make_rre_packet(4, white, subrects);
        let cursor = Cursor::new(data);
        let mut stream = RfbInStream::new(cursor);

        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buffer)
            .await;

        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("beyond rectangle"));
    }

    #[tokio::test]
    async fn test_decode_subrect_overflow() {
        let decoder = RREDecoder;
        let pixel_format = test_pixel_format();
        let buffer_format = rfb_pixelbuffer::PixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(100, 100, buffer_format);

        let rect = Rectangle {
            x: 10,
            y: 10,
            width: 10,
            height: 10,
            encoding: ENCODING_RRE,
        };

        let white: &[u8] = &[255, 255, 255, 255];
        let red: &[u8] = &[0, 0, 255, 255];

        // Sub-rectangle with x + width causing overflow
        let subrects: &[(&[u8], u16, u16, u16, u16)] = &[(red, 65535, 0, 65535, 5)];

        let data = make_rre_packet(4, white, subrects);
        let cursor = Cursor::new(data);
        let mut stream = RfbInStream::new(cursor);

        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buffer)
            .await;

        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("overflow"));
    }

    #[tokio::test]
    async fn test_decode_rgb565_format() {
        let decoder = RREDecoder;

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

        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
            encoding: ENCODING_RRE,
        };

        // Blue background: 0b00000_000000_11111 = 0x001F
        let blue_bg: &[u8] = &[0x1F, 0x00]; // Little-endian
                                            // Red sub-rect: 0b11111_000000_00000 = 0xF800
        let red_sub: &[u8] = &[0x00, 0xF8]; // Little-endian

        let subrects: &[(&[u8], u16, u16, u16, u16)] = &[(red_sub, 3, 3, 4, 4)];

        let data = make_rre_packet(2, blue_bg, subrects);
        let cursor = Cursor::new(data);
        let mut stream = RfbInStream::new(cursor);

        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buffer)
            .await;
        assert!(result.is_ok());

        // Verify background
        let pixel = get_pixel(&buffer, 0, 0);
        assert_eq!(pixel[0], 0x1F);
        assert_eq!(pixel[1], 0x00);

        // Verify sub-rectangle
        let pixel = get_pixel(&buffer, 3, 3);
        assert_eq!(pixel[0], 0x00);
        assert_eq!(pixel[1], 0xF8);
    }
}
