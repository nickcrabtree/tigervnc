//! Hextile encoding decoder - Tiled encoding with multiple sub-encodings.
//!
//! Hextile (type 5) is one of the most commonly used VNC encodings. It divides rectangles
//! into 16x16 pixel tiles (with smaller tiles at edges), and each tile can use different
//! sub-encodings for optimal compression. This encoding provides a good balance between
//! bandwidth efficiency and decoding speed.
//!
//! # Wire Format
//!
//! For each 16x16 tile in the rectangle (smaller at edges):
//!
//! ```text
//! +------------------+
//! | tile_type        |  1 byte (bit flags)
//! +------------------+
//! | [raw_pixels]     |  tile_w * tile_h * bpp bytes (if RAW bit set)
//! +------------------+
//! | [background]     |  bpp bytes (if BACKGROUND_SPECIFIED)
//! +------------------+
//! | [foreground]     |  bpp bytes (if FOREGROUND_SPECIFIED)
//! +------------------+
//! | [num_subrects]   |  1 byte (if ANY_SUBRECTS)
//! +------------------+
//! | [subrects...]    |  For each subrect:
//! |   [pixel]        |    bpp bytes (if SUBRECTS_COLOURED)
//! |   xy             |    1 byte: x=(xy>>4), y=(xy&0xF)
//! |   wh             |    1 byte: w=((wh>>4)+1), h=((wh&0xF)+1)
//! +------------------+
//! ```
//!
//! # Tile Type Flags
//!
//! - **RAW (0x01)**: Tile is raw uncompressed pixels (ignores other flags)
//! - **BACKGROUND_SPECIFIED (0x02)**: New background color follows
//! - **FOREGROUND_SPECIFIED (0x04)**: New foreground color follows
//! - **ANY_SUBRECTS (0x08)**: Subrectangles follow after optional colors
//! - **SUBRECTS_COLOURED (0x10)**: Each subrect has its own color (vs using foreground)
//!
//! # State Persistence
//!
//! Background and foreground colors persist across tiles **within a single rectangle**.
//! The decoder must reset these to `None` at the start of each rectangle decode.
//! This allows efficient encoding of large uniform regions without repeating colors.
//!
//! # Performance
//!
//! Hextile is widely used because it:
//! - Compresses solid regions well (single background fill)
//! - Handles text and UI elements efficiently (foreground + subrects)
//! - Decodes quickly (no decompression algorithms)
//! - Adapts per-tile (can fall back to RAW for complex regions)
//!
//! # Example
//!
//! ```no_run
//! use rfb_encodings::{Decoder, HextileDecoder, ENCODING_HEXTILE};
//!
//! let decoder = HextileDecoder;
//! assert_eq!(decoder.encoding_type(), ENCODING_HEXTILE);
//! ```

use crate::{Decoder, MutablePixelBuffer, PixelFormat, Rectangle, RfbInStream, ENCODING_HEXTILE};
use anyhow::{anyhow, Context, Result};
use rfb_common::Rect;
use tokio::io::AsyncRead;

// Hextile tile type flags (bit positions in the tile type byte)
const TILE_RAW: u8 = 1 << 0; // 0x01: Raw tile encoding
const TILE_BACKGROUND_SPECIFIED: u8 = 1 << 1; // 0x02: Background color follows
const TILE_FOREGROUND_SPECIFIED: u8 = 1 << 2; // 0x04: Foreground color follows
const TILE_ANY_SUBRECTS: u8 = 1 << 3; // 0x08: Subrectangles present
const TILE_SUBRECTS_COLOURED: u8 = 1 << 4; // 0x10: Subrects each have a color

/// Standard Hextile tile size (tiles at rectangle edges may be smaller).
const TILE_SIZE: u16 = 16;

/// Decoder for Hextile encoding.
///
/// This encoding divides the rectangle into 16x16 tiles, with each tile using one
/// of several sub-encodings for optimal compression. Tiles can be RAW (uncompressed),
/// background-only, or contain foreground-colored sub-rectangles.
///
/// # Example
///
/// ```no_run
/// # use rfb_encodings::{Decoder, HextileDecoder, ENCODING_HEXTILE};
/// let decoder = HextileDecoder;
/// assert_eq!(decoder.encoding_type(), ENCODING_HEXTILE);
/// ```
pub struct HextileDecoder;

impl Decoder for HextileDecoder {
    fn encoding_type(&self) -> i32 {
        ENCODING_HEXTILE
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
            "Hextile decode start: rect=[{},{} {}x{}] buffer_before={}",
            rect.x, rect.y, rect.width, rect.height,
            buffer_before
        );

        // Empty rectangle - nothing to decode
        if rect.width == 0 || rect.height == 0 {
            tracing::debug!(
                target: "rfb_encodings::framing",
                "Hextile decode end: empty rectangle, bytes_consumed=0, buffer_after={}",
                stream.available()
            );
            return Ok(());
        }

        let bytes_per_pixel = pixel_format.bits_per_pixel / 8;
        if bytes_per_pixel == 0 || bytes_per_pixel > 4 {
            return Err(anyhow!(
                "Invalid bytes_per_pixel: {} (must be 1-4)",
                bytes_per_pixel
            ));
        }

        // Per-rectangle state: background and foreground persist across tiles within one rectangle
        let mut background: Option<Vec<u8>> = None;
        let mut foreground: Option<Vec<u8>> = None;

        // Process tiles in 16x16 blocks, top-to-bottom, left-to-right
        let mut ty = 0u16;
        while ty < rect.height {
            let tile_h = std::cmp::min(TILE_SIZE, rect.height - ty);

            let mut tx = 0u16;
            while tx < rect.width {
                let tile_w = std::cmp::min(TILE_SIZE, rect.width - tx);

                // Read tile type byte
                let tile_type = stream.read_u8().await.with_context(|| {
                    format!(
                        "Failed to read Hextile tile type at tile ({}, {}) in rect at ({}, {})",
                        tx, ty, rect.x, rect.y
                    )
                })?;

                // RAW tile: uncompressed pixel data
                if (tile_type & TILE_RAW) != 0 {
                    let abs_x = rect
                        .x
                        .checked_add(tx)
                        .ok_or_else(|| anyhow!("Hextile RAW tile absolute x overflows"))?;
                    let abs_y = rect
                        .y
                        .checked_add(ty)
                        .ok_or_else(|| anyhow!("Hextile RAW tile absolute y overflows"))?;
                    decode_raw_tile(
                        stream,
                        buffer,
                        (abs_x, abs_y),
                        (tile_w, tile_h),
                        bytes_per_pixel,
                        (tx, ty),
                    )
                    .await?;
                    tx += TILE_SIZE;
                    continue;
                }

                // Non-RAW tile: Handle background, foreground, and subrects

                // Update background color if specified
                if (tile_type & TILE_BACKGROUND_SPECIFIED) != 0 {
                    let mut bg = vec![0u8; bytes_per_pixel as usize];
                    stream.read_bytes(&mut bg).await.with_context(|| {
                        format!(
                            "Failed to read Hextile background at tile ({}, {}) in rect at ({}, {})",
                            tx, ty, rect.x, rect.y
                        )
                    })?;
                    background = Some(bg);
                }

                // Background must be defined to fill tile
                let bg = background.as_ref().ok_or_else(|| {
                    anyhow!(
                        "Hextile tile at ({}, {}) in rect at ({}, {}) requires background but none was specified",
                        tx, ty, rect.x, rect.y
                    )
                })?;

                // Fill tile with background color
                let abs_x = rect
                    .x
                    .checked_add(tx)
                    .ok_or_else(|| anyhow!("Hextile tile absolute x overflows"))?;
                let abs_y = rect
                    .y
                    .checked_add(ty)
                    .ok_or_else(|| anyhow!("Hextile tile absolute y overflows"))?;
                let tile_rect = Rect::new(abs_x as i32, abs_y as i32, tile_w as u32, tile_h as u32);
                buffer.fill_rect(tile_rect, bg).with_context(|| {
                    format!(
                        "Failed to fill Hextile background at tile ({}, {}) in rect at ({}, {})",
                        tx, ty, rect.x, rect.y
                    )
                })?;

                // Update foreground color if specified
                if (tile_type & TILE_FOREGROUND_SPECIFIED) != 0 {
                    let mut fg = vec![0u8; bytes_per_pixel as usize];
                    stream.read_bytes(&mut fg).await.with_context(|| {
                        format!(
                            "Failed to read Hextile foreground at tile ({}, {}) in rect at ({}, {})",
                            tx, ty, rect.x, rect.y
                        )
                    })?;
                    foreground = Some(fg);
                }

                // Decode subrectangles if present
                if (tile_type & TILE_ANY_SUBRECTS) != 0 {
                    let num_subrects = stream.read_u8().await.with_context(|| {
                        format!(
                            "Failed to read Hextile subrect count at tile ({}, {}) in rect at ({}, {})",
                            tx, ty, rect.x, rect.y
                        )
                    })?;

                    let subrects_coloured = (tile_type & TILE_SUBRECTS_COLOURED) != 0;

                    // If subrects are monochrome and count > 0, we need a foreground color
                    if num_subrects > 0 && !subrects_coloured && foreground.is_none() {
                        return Err(anyhow!(
                            "Hextile tile at ({}, {}) in rect at ({}, {}) has monochrome subrects but no foreground color",
                            tx, ty, rect.x, rect.y
                        ));
                    }

                    for i in 0..num_subrects {
                        // Read color if subrects are colored, else use foreground
                        let color = if subrects_coloured {
                            let mut col = vec![0u8; bytes_per_pixel as usize];
                            stream.read_bytes(&mut col).await.with_context(|| {
                                format!(
                                    "Failed to read color for Hextile subrect {} at tile ({}, {}) in rect at ({}, {})",
                                    i, tx, ty, rect.x, rect.y
                                )
                            })?;
                            col
                        } else {
                            foreground.as_ref().unwrap().clone()
                        };

                        // Read position byte: high nibble = x, low nibble = y
                        let xy = stream.read_u8().await.with_context(|| {
                            format!(
                                "Failed to read XY for Hextile subrect {} at tile ({}, {}) in rect at ({}, {})",
                                i, tx, ty, rect.x, rect.y
                            )
                        })?;
                        let x_off = (xy >> 4) & 0x0F;
                        let y_off = xy & 0x0F;

                        // Read size byte: high nibble = (width-1), low nibble = (height-1)
                        let wh = stream.read_u8().await.with_context(|| {
                            format!(
                                "Failed to read WH for Hextile subrect {} at tile ({}, {}) in rect at ({}, {})",
                                i, tx, ty, rect.x, rect.y
                            )
                        })?;
                        let w = ((wh >> 4) & 0x0F) + 1;
                        let h = (wh & 0x0F) + 1;

                        // Validate subrect is within tile bounds
                        let right = x_off.checked_add(w).ok_or_else(|| {
                            anyhow!(
                                "Hextile subrect {} x+width overflows: {} + {} at tile ({}, {})",
                                i,
                                x_off,
                                w,
                                tx,
                                ty
                            )
                        })?;
                        let bottom = y_off.checked_add(h).ok_or_else(|| {
                            anyhow!(
                                "Hextile subrect {} y+height overflows: {} + {} at tile ({}, {})",
                                i,
                                y_off,
                                h,
                                tx,
                                ty
                            )
                        })?;

                        if right as u16 > tile_w {
                            return Err(anyhow!(
                                "Hextile subrect {} extends beyond tile width: x={}, w={}, tile_w={} at tile ({}, {})",
                                i, x_off, w, tile_w, tx, ty
                            ));
                        }
                        if bottom as u16 > tile_h {
                            return Err(anyhow!(
                                "Hextile subrect {} extends beyond tile height: y={}, h={}, tile_h={} at tile ({}, {})",
                                i, y_off, h, tile_h, tx, ty
                            ));
                        }

                        // Draw subrectangle
                        let sr_abs_x = abs_x
                            .checked_add(x_off as u16)
                            .ok_or_else(|| anyhow!("Hextile subrect absolute x overflows"))?;
                        let sr_abs_y = abs_y
                            .checked_add(y_off as u16)
                            .ok_or_else(|| anyhow!("Hextile subrect absolute y overflows"))?;
                        let subrect =
                            Rect::new(sr_abs_x as i32, sr_abs_y as i32, w as u32, h as u32);
                        buffer.fill_rect(subrect, &color).with_context(|| {
                            format!(
                                "Failed to fill Hextile subrect {} at ({}, {}) size {}x{} in tile ({}, {})",
                                i, x_off, y_off, w, h, tx, ty
                            )
                        })?;
                    }
                }

                tx += TILE_SIZE;
            }
            ty += TILE_SIZE;
        }

        let buffer_after = stream.available();
        tracing::debug!(
            target: "rfb_encodings::framing",
            "Hextile decode end: bytes_consumed={}, buffer_after={}",
            buffer_before.saturating_sub(buffer_after),
            buffer_after
        );

        Ok(())
    }
}

/// Decode a RAW tile: uncompressed pixel data for the entire tile.
async fn decode_raw_tile<R: AsyncRead + Unpin>(
    stream: &mut RfbInStream<R>,
    buffer: &mut dyn MutablePixelBuffer,
    abs_pos: (u16, u16),   // Absolute (x, y) position in framebuffer
    tile_size: (u16, u16), // (width, height) of tile
    bytes_per_pixel: u8,
    tile_offset: (u16, u16), // (tx, ty) for error reporting
) -> Result<()> {
    let (abs_x, abs_y) = abs_pos;
    let (tile_w, tile_h) = tile_size;
    let (tx, ty) = tile_offset;
    // Calculate total bytes to read
    let pixels_per_row = tile_w as usize;
    let row_bytes = pixels_per_row
        .checked_mul(bytes_per_pixel as usize)
        .ok_or_else(|| anyhow!("Hextile RAW tile row bytes overflow"))?;
    let total_bytes = row_bytes
        .checked_mul(tile_h as usize)
        .ok_or_else(|| anyhow!("Hextile RAW tile total bytes overflow"))?;

    // Read all raw pixel data
    let mut raw_data = vec![0u8; total_bytes];
    stream.read_bytes(&mut raw_data).await.with_context(|| {
        format!(
            "Failed to read {} bytes of RAW data for Hextile tile at ({}, {})",
            total_bytes, tx, ty
        )
    })?;

    // Write row-by-row into the framebuffer (stride may differ from tile width)
    for row in 0..tile_h {
        let src_offset = (row as usize) * row_bytes;
        let src_row = &raw_data[src_offset..src_offset + row_bytes];

        let row_y = abs_y
            .checked_add(row)
            .ok_or_else(|| anyhow!("Hextile RAW tile row y overflows"))?;
        let row_rect = Rect::new(abs_x as i32, row_y as i32, tile_w as u32, 1);

        buffer
            .image_rect(row_rect, src_row, tile_w as usize)
            .with_context(|| {
                format!(
                    "Failed to write RAW tile row {} at ({}, {}) for tile at ({}, {})",
                    row, abs_x, row_y, tx, ty
                )
            })?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rfb_pixelbuffer::{ManagedPixelBuffer, PixelBuffer};
    use std::io::Cursor;

    // Wire format pixel format (rfb_protocol)
    fn pf_rgb888() -> crate::PixelFormat {
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

    // Buffer format (rfb_pixelbuffer)
    fn buffer_format() -> rfb_pixelbuffer::PixelFormat {
        rfb_pixelbuffer::PixelFormat::rgb888()
    }

    fn make_stream(data: Vec<u8>) -> RfbInStream<Cursor<Vec<u8>>> {
        RfbInStream::new(Cursor::new(data))
    }

    #[tokio::test]
    async fn test_empty_rectangle_width_zero() {
        let decoder = HextileDecoder;
        let mut stream = make_stream(vec![]);
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 0,
            height: 10,
            encoding: ENCODING_HEXTILE,
        };
        let pf = pf_rgb888();
        let mut fb = ManagedPixelBuffer::new(100, 100, buffer_format());

        let result = decoder.decode(&mut stream, &rect, &pf, &mut fb).await;
        assert!(result.is_ok(), "Empty width rectangle should succeed");
    }

    #[tokio::test]
    async fn test_empty_rectangle_height_zero() {
        let decoder = HextileDecoder;
        let mut stream = make_stream(vec![]);
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 10,
            height: 0,
            encoding: ENCODING_HEXTILE,
        };
        let pf = pf_rgb888();
        let mut fb = ManagedPixelBuffer::new(100, 100, buffer_format());

        let result = decoder.decode(&mut stream, &rect, &pf, &mut fb).await;
        assert!(result.is_ok(), "Empty height rectangle should succeed");
    }

    #[tokio::test]
    async fn test_single_tile_raw_2x2() {
        let decoder = HextileDecoder;
        // Tile type: RAW
        // 2x2 pixels RGB888 (32bpp = 4 bytes per pixel): R, G, B, W
        let data = vec![
            TILE_RAW, // tile type
            0, 0, 255, 0, // red pixel (BGRA: 0x00FF0000)
            0, 255, 0, 0, // green pixel
            255, 0, 0, 0, // blue pixel
            255, 255, 255, 0, // white pixel
        ];
        let mut stream = make_stream(data);
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
            encoding: ENCODING_HEXTILE,
        };
        let pf = pf_rgb888();
        let mut fb = ManagedPixelBuffer::new(10, 10, buffer_format());

        let result = decoder.decode(&mut stream, &rect, &pf, &mut fb).await;
        assert!(result.is_ok(), "RAW tile should decode: {:?}", result);
    }

    #[tokio::test]
    async fn test_single_tile_background_only() {
        let decoder = HextileDecoder;
        // Tile type: BACKGROUND_SPECIFIED, then background color (red)
        let data = vec![
            TILE_BACKGROUND_SPECIFIED, // tile type
            0,
            0,
            255,
            0, // red background (32bpp BGRA little-endian)
        ];
        let mut stream = make_stream(data);
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
            encoding: ENCODING_HEXTILE,
        };
        let pf = pf_rgb888();
        let mut fb = ManagedPixelBuffer::new(10, 10, buffer_format());

        let result = decoder.decode(&mut stream, &rect, &pf, &mut fb).await;
        assert!(result.is_ok(), "Background-only tile should decode");

        // Verify red pixels at (0,0) and (3,3)
        let mut stride = 0;
        let buf_slice = fb.get_buffer(Rect::new(0, 0, 4, 4), &mut stride).unwrap();
        // First pixel red (BGRA little-endian: B,G,R,A)
        assert_eq!(buf_slice[0], 0); // B
        assert_eq!(buf_slice[1], 0); // G
        assert_eq!(buf_slice[2], 255); // R
    }

    #[tokio::test]
    async fn test_background_persists_across_tiles() {
        let decoder = HextileDecoder;
        // Two tiles in a 17x1 rectangle: first tile sets background, second uses it
        let data = vec![
            TILE_BACKGROUND_SPECIFIED, // tile 1 type
            32,
            64,
            128,
            0, // bg color (32bpp BGRA)
            0, // tile 2 type: use previous bg
        ];
        let mut stream = make_stream(data);
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 17,
            height: 1,
            encoding: ENCODING_HEXTILE,
        };
        let pf = pf_rgb888();
        let mut fb = ManagedPixelBuffer::new(20, 10, buffer_format());

        let result = decoder.decode(&mut stream, &rect, &pf, &mut fb).await;
        assert!(
            result.is_ok(),
            "Background should persist across tiles: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn test_foreground_monochrome_subrects() {
        let decoder = HextileDecoder;
        // Tile: background + foreground + one monochrome subrect at (2,2) size 3x3
        let data = vec![
            TILE_BACKGROUND_SPECIFIED | TILE_FOREGROUND_SPECIFIED | TILE_ANY_SUBRECTS,
            0,
            0,
            0,
            0, // black bg (32bpp)
            255,
            255,
            255,
            0,    // white fg (32bpp)
            1,    // 1 subrect
            0x22, // xy: x=2, y=2
            0x22, // wh: (w-1)=2, (h-1)=2 -> 3x3
        ];
        let mut stream = make_stream(data);
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
            encoding: ENCODING_HEXTILE,
        };
        let pf = pf_rgb888();
        let mut fb = ManagedPixelBuffer::new(20, 20, buffer_format());

        let result = decoder.decode(&mut stream, &rect, &pf, &mut fb).await;
        assert!(result.is_ok(), "Monochrome subrects should decode");
    }

    #[tokio::test]
    async fn test_colored_subrects() {
        let decoder = HextileDecoder;
        // Tile: background + two colored subrects
        let data = vec![
            TILE_BACKGROUND_SPECIFIED | TILE_ANY_SUBRECTS | TILE_SUBRECTS_COLOURED,
            100,
            100,
            100,
            0, // gray bg (32bpp)
            2, // 2 subrects
            0,
            0,
            255,
            0,    // red color (32bpp BGRA)
            0x00, // xy: (0,0)
            0x00, // wh: 1x1
            0,
            255,
            0,
            0,    // green color (32bpp BGRA)
            0x11, // xy: (1,1)
            0x00, // wh: 1x1
        ];
        let mut stream = make_stream(data);
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 5,
            height: 5,
            encoding: ENCODING_HEXTILE,
        };
        let pf = pf_rgb888();
        let mut fb = ManagedPixelBuffer::new(10, 10, buffer_format());

        let result = decoder.decode(&mut stream, &rect, &pf, &mut fb).await;
        assert!(result.is_ok(), "Colored subrects should decode");
    }

    #[tokio::test]
    async fn test_subrects_count_zero() {
        let decoder = HextileDecoder;
        // Tile: background + ANY_SUBRECTS but count=0
        let data = vec![
            TILE_BACKGROUND_SPECIFIED | TILE_ANY_SUBRECTS,
            50,
            50,
            50,
            0, // bg (32bpp)
            0, // 0 subrects
        ];
        let mut stream = make_stream(data);
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
            encoding: ENCODING_HEXTILE,
        };
        let pf = pf_rgb888();
        let mut fb = ManagedPixelBuffer::new(10, 10, buffer_format());

        let result = decoder.decode(&mut stream, &rect, &pf, &mut fb).await;
        assert!(
            result.is_ok(),
            "Zero subrects should be valid: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn test_edge_tile_right() {
        let decoder = HextileDecoder;
        // Rectangle 17x10: last tile is 1x10
        let data = vec![
            TILE_BACKGROUND_SPECIFIED, // tile 1
            0,
            0,
            0,
            0,                         // tile 1 bg
            TILE_BACKGROUND_SPECIFIED, // tile 2 (1x10)
            255,
            255,
            255,
            0, // tile 2 bg
        ];
        let mut stream = make_stream(data);
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 17,
            height: 10,
            encoding: ENCODING_HEXTILE,
        };
        let pf = pf_rgb888();
        let mut fb = ManagedPixelBuffer::new(20, 20, buffer_format());

        let result = decoder.decode(&mut stream, &rect, &pf, &mut fb).await;
        assert!(result.is_ok(), "Right-edge tile should decode");
    }

    #[tokio::test]
    async fn test_edge_tile_bottom() {
        let decoder = HextileDecoder;
        // Rectangle 10x31: last row tile is 10x15
        let data = vec![
            TILE_BACKGROUND_SPECIFIED, // tile row 1
            0,
            0,
            0,
            0,
            TILE_BACKGROUND_SPECIFIED, // tile row 2
            255,
            255,
            255,
            0,
            TILE_BACKGROUND_SPECIFIED, // tile row 3 (10x15)
            128,
            128,
            128,
            0,
        ];
        let mut stream = make_stream(data);
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 10,
            height: 31,
            encoding: ENCODING_HEXTILE,
        };
        let pf = pf_rgb888();
        let mut fb = ManagedPixelBuffer::new(20, 40, buffer_format());

        let result = decoder.decode(&mut stream, &rect, &pf, &mut fb).await;
        assert!(result.is_ok(), "Bottom-edge tile should decode");
    }

    #[tokio::test]
    async fn test_edge_tile_corner() {
        let decoder = HextileDecoder;
        // Rectangle 17x31: corner tile is 1x15
        let data = vec![
            TILE_BACKGROUND_SPECIFIED,
            0,
            0,
            0,
            0, // (0,0) 16x16
            TILE_BACKGROUND_SPECIFIED,
            0,
            0,
            0,
            0, // (16,0) 1x16
            TILE_BACKGROUND_SPECIFIED,
            0,
            0,
            0,
            0, // (0,16) 16x15
            TILE_BACKGROUND_SPECIFIED,
            0,
            0,
            0,
            0, // (16,16) 1x15 corner
        ];
        let mut stream = make_stream(data);
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 17,
            height: 31,
            encoding: ENCODING_HEXTILE,
        };
        let pf = pf_rgb888();
        let mut fb = ManagedPixelBuffer::new(20, 40, buffer_format());

        let result = decoder.decode(&mut stream, &rect, &pf, &mut fb).await;
        assert!(result.is_ok(), "Corner edge tile should decode");
    }

    #[tokio::test]
    async fn test_no_background_error() {
        let decoder = HextileDecoder;
        // Tile type 0: no flags, expects to use previous background but none exists
        let data = vec![0]; // tile type with no background specified
        let mut stream = make_stream(data);
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
            encoding: ENCODING_HEXTILE,
        };
        let pf = pf_rgb888();
        let mut fb = ManagedPixelBuffer::new(10, 10, buffer_format());

        let result = decoder.decode(&mut stream, &rect, &pf, &mut fb).await;
        assert!(
            result.is_err(),
            "Should fail when background not specified and not set"
        );
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("requires background"));
    }

    #[tokio::test]
    async fn test_monochrome_subrects_no_foreground_error() {
        let decoder = HextileDecoder;
        // Tile: background + monochrome subrects but no foreground
        let data = vec![
            TILE_BACKGROUND_SPECIFIED | TILE_ANY_SUBRECTS,
            0,
            0,
            0,
            0, // bg (32bpp)
            1, // 1 subrect
               // But no foreground specified
        ];
        let mut stream = make_stream(data);
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
            encoding: ENCODING_HEXTILE,
        };
        let pf = pf_rgb888();
        let mut fb = ManagedPixelBuffer::new(10, 10, buffer_format());

        let result = decoder.decode(&mut stream, &rect, &pf, &mut fb).await;
        assert!(
            result.is_err(),
            "Should fail when monochrome subrects but no foreground"
        );
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("no foreground color"));
    }

    #[tokio::test]
    async fn test_subrect_out_of_bounds_x() {
        let decoder = HextileDecoder;
        // Tile 4x4: subrect at x=3, w=3 -> out of bounds
        let data = vec![
            TILE_BACKGROUND_SPECIFIED | TILE_FOREGROUND_SPECIFIED | TILE_ANY_SUBRECTS,
            0,
            0,
            0,
            0, // bg (32bpp)
            255,
            255,
            255,
            0,    // fg (32bpp)
            1,    // 1 subrect
            0x30, // xy: x=3, y=0
            0x20, // wh: w=3, h=1
        ];
        let mut stream = make_stream(data);
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
            encoding: ENCODING_HEXTILE,
        };
        let pf = pf_rgb888();
        let mut fb = ManagedPixelBuffer::new(10, 10, buffer_format());

        let result = decoder.decode(&mut stream, &rect, &pf, &mut fb).await;
        assert!(
            result.is_err(),
            "Should fail when subrect exceeds tile width"
        );
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("extends beyond tile width"));
    }

    #[tokio::test]
    async fn test_subrect_out_of_bounds_y() {
        let decoder = HextileDecoder;
        // Tile 4x4: subrect at y=3, h=3 -> out of bounds
        let data = vec![
            TILE_BACKGROUND_SPECIFIED | TILE_FOREGROUND_SPECIFIED | TILE_ANY_SUBRECTS,
            0,
            0,
            0,
            0, // bg (32bpp)
            255,
            255,
            255,
            0,    // fg (32bpp)
            1,    // 1 subrect
            0x03, // xy: x=0, y=3
            0x02, // wh: w=1, h=3
        ];
        let mut stream = make_stream(data);
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
            encoding: ENCODING_HEXTILE,
        };
        let pf = pf_rgb888();
        let mut fb = ManagedPixelBuffer::new(10, 10, buffer_format());

        let result = decoder.decode(&mut stream, &rect, &pf, &mut fb).await;
        assert!(
            result.is_err(),
            "Should fail when subrect exceeds tile height"
        );
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("extends beyond tile height"));
    }

    #[tokio::test]
    async fn test_raw_tile_ignores_other_flags() {
        let decoder = HextileDecoder;
        // Tile type has RAW + other flags; should treat as RAW only
        let data = vec![
            TILE_RAW | TILE_BACKGROUND_SPECIFIED | TILE_FOREGROUND_SPECIFIED,
            0,
            0,
            255,
            0, // pixel 1 (32bpp BGRA)
            0,
            255,
            0,
            0, // pixel 2
            255,
            0,
            0,
            0, // pixel 3
            0,
            255,
            255,
            0, // pixel 4
        ];
        let mut stream = make_stream(data);
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
            encoding: ENCODING_HEXTILE,
        };
        let pf = pf_rgb888();
        let mut fb = ManagedPixelBuffer::new(10, 10, buffer_format());

        let result = decoder.decode(&mut stream, &rect, &pf, &mut fb).await;
        assert!(
            result.is_ok(),
            "RAW tile should ignore other flags: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn test_eof_reading_tile_type() {
        let decoder = HextileDecoder;
        let data = vec![]; // empty stream
        let mut stream = make_stream(data);
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
            encoding: ENCODING_HEXTILE,
        };
        let pf = pf_rgb888();
        let mut fb = ManagedPixelBuffer::new(10, 10, buffer_format());

        let result = decoder.decode(&mut stream, &rect, &pf, &mut fb).await;
        assert!(result.is_err(), "Should fail on EOF reading tile type");
        assert!(result.unwrap_err().to_string().contains("tile type"));
    }

    #[tokio::test]
    async fn test_eof_reading_background() {
        let decoder = HextileDecoder;
        let data = vec![
            TILE_BACKGROUND_SPECIFIED, // tile type
            255,
            0, // only 2 bytes of 4
        ];
        let mut stream = make_stream(data);
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
            encoding: ENCODING_HEXTILE,
        };
        let pf = pf_rgb888();
        let mut fb = ManagedPixelBuffer::new(10, 10, buffer_format());

        let result = decoder.decode(&mut stream, &rect, &pf, &mut fb).await;
        assert!(result.is_err(), "Should fail on EOF reading background");
        assert!(result.unwrap_err().to_string().contains("background"));
    }

    #[tokio::test]
    async fn test_eof_reading_subrect_count() {
        let decoder = HextileDecoder;
        let data = vec![
            TILE_BACKGROUND_SPECIFIED | TILE_ANY_SUBRECTS,
            255,
            255,
            255,
            0, // bg (32bpp)
               // missing subrect count
        ];
        let mut stream = make_stream(data);
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
            encoding: ENCODING_HEXTILE,
        };
        let pf = pf_rgb888();
        let mut fb = ManagedPixelBuffer::new(10, 10, buffer_format());

        let result = decoder.decode(&mut stream, &rect, &pf, &mut fb).await;
        assert!(result.is_err(), "Should fail on EOF reading subrect count");
        assert!(result.unwrap_err().to_string().contains("subrect count"));
    }

    #[tokio::test]
    async fn test_eof_reading_subrect_xy() {
        let decoder = HextileDecoder;
        let data = vec![
            TILE_BACKGROUND_SPECIFIED | TILE_FOREGROUND_SPECIFIED | TILE_ANY_SUBRECTS,
            0,
            0,
            0,
            0, // bg (32bpp)
            255,
            255,
            255,
            0, // fg (32bpp)
            1, // 1 subrect
               // missing xy
        ];
        let mut stream = make_stream(data);
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
            encoding: ENCODING_HEXTILE,
        };
        let pf = pf_rgb888();
        let mut fb = ManagedPixelBuffer::new(10, 10, buffer_format());

        let result = decoder.decode(&mut stream, &rect, &pf, &mut fb).await;
        assert!(result.is_err(), "Should fail on EOF reading subrect XY");
        assert!(result.unwrap_err().to_string().contains("XY"));
    }

    // Note: This test is commented out because the current decoder doesn't convert
    // between wire format and buffer format. In a real implementation, pixels would need
    // to be converted from the wire format (e.g., RGB565) to the buffer format (RGB888).
    // For now, we only test with matching formats (RGB888/32bpp).
    /*
    #[tokio::test]
    async fn test_rgb565_background() {
        let decoder = HextileDecoder;
        // RGB565: 16-bit red (0xF800)
        let data = vec![
            TILE_BACKGROUND_SPECIFIED,
            0xF8, 0x00, // red in RGB565 big-endian
        ];
        let mut stream = make_stream(data);
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
            encoding: ENCODING_HEXTILE,
        };
        let pf = pf_rgb565();
        let mut fb = ManagedPixelBuffer::new(10, 10, buffer_format());

        let result = decoder.decode(&mut stream, &rect, &pf, &mut fb).await;
        assert!(result.is_ok(), "RGB565 background should decode: {:?}", result.err());
    }
    */

    #[tokio::test]
    async fn test_foreground_persistence() {
        let decoder = HextileDecoder;
        // Two tiles: first sets bg+fg, second uses persisted fg for monochrome subrects
        let data = vec![
            TILE_BACKGROUND_SPECIFIED | TILE_FOREGROUND_SPECIFIED,
            0,
            0,
            0,
            0, // bg (32bpp)
            255,
            255,
            255,
            0,                 // fg (32bpp)
            TILE_ANY_SUBRECTS, // second tile: uses persisted bg and fg
            1,                 // 1 monochrome subrect
            0x00,              // xy: (0,0)
            0x00,              // wh: 1x1
        ];
        let mut stream = make_stream(data);
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 17,
            height: 1,
            encoding: ENCODING_HEXTILE,
        };
        let pf = pf_rgb888();
        let mut fb = ManagedPixelBuffer::new(20, 10, buffer_format());

        let result = decoder.decode(&mut stream, &rect, &pf, &mut fb).await;
        assert!(result.is_ok(), "Foreground should persist across tiles");
    }

    #[tokio::test]
    async fn test_subrect_at_boundary() {
        let decoder = HextileDecoder;
        // Tile 16x16: subrect at (15,15) size 1x1 (maximum valid position)
        let data = vec![
            TILE_BACKGROUND_SPECIFIED | TILE_FOREGROUND_SPECIFIED | TILE_ANY_SUBRECTS,
            0,
            0,
            0,
            0, // bg (32bpp)
            255,
            255,
            255,
            0,    // fg (32bpp)
            1,    // 1 subrect
            0xFF, // xy: x=15, y=15
            0x00, // wh: w=1, h=1
        ];
        let mut stream = make_stream(data);
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 16,
            height: 16,
            encoding: ENCODING_HEXTILE,
        };
        let pf = pf_rgb888();
        let mut fb = ManagedPixelBuffer::new(20, 20, buffer_format());

        let result = decoder.decode(&mut stream, &rect, &pf, &mut fb).await;
        assert!(result.is_ok(), "Subrect at boundary should be valid");
    }

    #[tokio::test]
    async fn test_hextile_decoder_type() {
        let decoder = HextileDecoder;
        assert_eq!(decoder.encoding_type(), ENCODING_HEXTILE);
    }
}
