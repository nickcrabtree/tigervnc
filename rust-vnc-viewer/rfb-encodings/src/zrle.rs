//! ZRLE encoding decoder - Zlib-compressed RLE with 64x64 tiling.
//!
//! ZRLE (Zlib Run-Length Encoding, type 16) combines zlib compression with run-length
//! encoding to achieve excellent compression ratios. It divides rectangles into 64x64
//! pixel tiles (smaller at edges) and uses multiple sub-encoding schemes per tile
//! for optimal efficiency.
//!
//! # Wire Format
//!
//! ```text
//! +------------------+
//! | length           |  4 bytes (u32 big-endian) - length of zlib stream
//! +------------------+
//! | zlib_data        |  'length' bytes of zlib-compressed tile data
//! +------------------+
//! ```
//!
//! After zlib decompression, the data contains tiles in row-major order:
//!
//! ```text
//! For each 64x64 tile (smaller at rectangle edges):
//! +------------------+
//! | subencoding      |  1 byte: bit 7 = RLE flag, bits 0-6 = palette size
//! +------------------+
//! | [tile data]      |  Varies by subencoding (see below)
//! +------------------+
//! ```
//!
//! # 64x64 Tile Grid
//!
//! ```text
//! Rectangle divided into 64x64 tiles:
//!
//!     0    64   128  192  ...
//!   0 +----+----+----+----+
//!     |    |    |    |    |
//!  64 +----+----+----+----+
//!     |    |    |    |    |
//! 128 +----+----+----+----+
//!     |    |    | edge    |
//! ... +----+----+----+----+
//!          edge  tiles
//! ```
//!
//! # Subencoding Byte
//!
//! - **Bit 7**: RLE flag (0 = packed/raw, 1 = RLE)
//! - **Bits 0-6**: Palette size (0-127)
//!   - 0 = no palette (raw or plain RLE)
//!   - 1 = solid tile (single color fill)
//!   - 2-127 = palette with N colors
//!
//! # Seven Tile Modes
//!
//! 1. **Solid (palSize=1)**: Single pixel fills entire tile
//! 2. **Raw (palSize=0, RLE=0)**: Uncompressed pixel data in raster order
//! 3. **Plain RLE (palSize=0, RLE=1)**: RLE without palette
//! 4. **Packed Palette (palSize=2-16, RLE=0)**: 1/2/4-bit indices into palette
//! 5. **Byte-indexed Palette (palSize=17-127, RLE=0)**: 8-bit palette indices
//! 6. **Palette RLE (palSize=2-127, RLE=1)**: RLE with palette indices
//!
//! # CPixel Optimization (24-bit)
//!
//! When pixel format is 32bpp with depth ≤ 24, ZRLE transmits only 3 bytes per pixel:
//!
//! - **isLowCPixel** (little-endian): RGB in bytes [0,1,2], byte [3] = 0
//! - **isHighCPixel** (big-endian): RGB in bytes [1,2,3], byte [0] = 0
//!
//! This optimization saves 25% bandwidth for common TrueColor displays.
//!
//! # RLE Length Encoding
//!
//! Run lengths are encoded as 1 + sum of continuation bytes:
//!
//! ```text
//! Length = 1 + byte0 + byte1 + ... + byteN
//!   where bytes = [255, 255, ..., final]
//!   and final < 255 terminates the sequence
//!
//! Examples:
//!   [10]          → length = 1 + 10 = 11
//!   [255, 100]    → length = 1 + 255 + 100 = 356
//!   [255, 255, 0] → length = 1 + 255 + 255 + 0 = 511
//! ```
//!
//! # Packed Palette Bit Order
//!
//! Indices are packed MSB-first within each byte:
//!
//! ```text
//! For 2-bit indices (4 colors), row of 5 pixels:
//!
//!   Byte 0: [idx0 idx1 idx2 idx3]  (bits 76543210)
//!   Byte 1: [idx4 ---- ---- ----]  (remaining 4 bits unused)
//!
//! For 1-bit indices (2 colors), row of 9 pixels:
//!
//!   Byte 0: [idx0 idx1 idx2 idx3 idx4 idx5 idx6 idx7]
//!   Byte 1: [idx8 ------- ------- -------]  (7 bits unused)
//! ```
//!
//! # Example
//!
//! ```no_run
//! use rfb_encodings::{Decoder, ZRLEDecoder, ENCODING_ZRLE};
//!
//! let decoder = ZRLEDecoder::default();
//! assert_eq!(decoder.encoding_type(), ENCODING_ZRLE);
//! ```

use crate::{Decoder, MutablePixelBuffer, PixelFormat, Rectangle, RfbInStream, ENCODING_ZRLE};
use anyhow::{anyhow, bail, Context, Result};
use flate2::{Decompress, FlushDecompress};
use rfb_common::Rect;
use std::sync::Mutex;
use tokio::io::AsyncRead;

/// ZRLE tile size (64x64 pixels, smaller at rectangle edges).
const TILE_SIZE: u16 = 64;

/// Maximum valid palette size (bit 7 reserved for RLE flag).
const MAX_PALETTE_SIZE: u8 = 127;

/// Decoder for ZRLE encoding.
///
/// This encoding uses zlib compression combined with run-length encoding and palette
/// modes to achieve excellent compression ratios. Rectangles are divided into 64x64
/// tiles, with each tile using one of seven sub-encodings optimized for different
/// content types.
///
/// # Zlib Stream
///
/// **Important**: ZRLE uses a CONTINUOUS zlib stream across multiple rectangles within
/// the same FramebufferUpdate message. Only the first rectangle starts with a zlib header (0x78);
/// subsequent rectangles contain raw deflate continuation data. The zlib inflater state
/// persists across rectangles and is only reset when the decoder is reset or dropped.
///
/// This matches the C++ TigerVNC implementation where `rdr::ZlibInStream zis` is a member
/// variable that maintains state across multiple `decodeRect` calls.
///
/// # Example
///
/// ```no_run
/// # use rfb_encodings::{Decoder, ZRLEDecoder, ENCODING_ZRLE};
/// let decoder = ZRLEDecoder::new();
/// assert_eq!(decoder.encoding_type(), ENCODING_ZRLE);
/// ```
pub struct ZRLEDecoder {
    /// Zlib decompressor state that persists across rectangles within the same FBU.
    /// Uses Mutex for interior mutability and thread safety since Decoder::decode takes &self.
    inflater: Mutex<Decompress>,
}

impl Default for ZRLEDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl ZRLEDecoder {
    /// Create a new ZRLE decoder with a fresh zlib inflater.
    pub fn new() -> Self {
        Self {
            inflater: Mutex::new(Decompress::new(true)), // true = zlib wrapper
        }
    }

    /// Reset the zlib inflater state.
    ///
    /// This should be called at the start of each FramebufferUpdate message to prepare
    /// for a new zlib stream with header. The inflater state persists across rectangles
    /// within a single FBU.
    pub fn reset(&self) {
        self.inflater.lock().unwrap().reset(true); // true = zlib wrapper
    }
}

impl Decoder for ZRLEDecoder {
    fn encoding_type(&self) -> i32 {
        ENCODING_ZRLE
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
            bail!(
                "ZRLE: invalid bits_per_pixel {} (bytes_per_pixel must be 1-4)",
                pixel_format.bits_per_pixel
            );
        }

        // Read compressed data length (u32 big-endian)
        let compressed_len = stream
            .read_u32()
            .await
            .context("ZRLE: failed to read compressed data length")?;

        tracing::debug!(
            "ZRLE: rect [{},{}+{}x{}] compressed_len={}, stream buffer has {} bytes",
            rect.x,
            rect.y,
            rect.width,
            rect.height,
            compressed_len,
            stream.available()
        );

        // Read compressed data
        let mut compressed_data = vec![0u8; compressed_len as usize];
        stream
            .read_bytes(&mut compressed_data)
            .await
            .context("ZRLE: failed to read compressed data")?;

        tracing::debug!(
            "ZRLE: after read, stream buffer has {} bytes remaining",
            stream.available()
        );

        tracing::debug!(
            "ZRLE: read {} compressed bytes, first 16 bytes: {:02x?}",
            compressed_data.len(),
            &compressed_data[..compressed_data.len().min(16)]
        );

        // Decompress the zlib stream
        let decompressed = self
            .decompress_zlib(&compressed_data)
            .context("ZRLE: zlib decompression failed")?;

        tracing::debug!("ZRLE: decompressed {} bytes", decompressed.len());

        // Decode tiles from decompressed data
        let mut cursor = DataCursor::new(&decompressed);
        self.decode_tiles(&mut cursor, rect, pixel_format, buffer, bytes_per_pixel)
            .context("ZRLE: tile decoding failed")?;

        // Verify all data consumed (no trailing bytes)
        let remaining = cursor.remaining();
        if remaining > 0 {
            tracing::warn!(
                "ZRLE: {} trailing bytes after decoding rectangle - data: {:02x?}",
                remaining,
                &cursor.data[cursor.pos..cursor.pos + remaining.min(32)]
            );
            bail!(
                "ZRLE: {} trailing bytes after decoding rectangle",
                remaining
            );
        }
        tracing::debug!("ZRLE: all tile data consumed, no trailing bytes");
        tracing::debug!(
            "ZRLE: decode complete, stream buffer has {} bytes",
            stream.available()
        );

        Ok(())
    }
}

impl ZRLEDecoder {
    /// Decompress zlib-compressed data using the persistent inflater.
    ///
    /// This method uses the inflater state maintained across rectangles within a single FBU.
    /// The first rectangle should have a zlib header (0x78); subsequent rectangles are
    /// raw deflate continuation data. The inflater state carries over automatically.
    fn decompress_zlib(&self, compressed: &[u8]) -> Result<Vec<u8>> {
        let mut decompressed = Vec::new();
        let mut inflater = self.inflater.lock().unwrap();

        // Process all input bytes
        let mut in_pos = 0;
        let mut out_buf = vec![0u8; 64 * 1024]; // 64KB output buffer

        loop {
            let before_in = inflater.total_in();
            let before_out = inflater.total_out();

            let status = inflater
                .decompress(
                    &compressed[in_pos..],
                    &mut out_buf,
                    FlushDecompress::Sync,
                )
                .with_context(|| {
                    format!(
                        "ZRLE: zlib decompression failed (input {} bytes at offset {}, first 16 bytes: {:02x?})",
                        compressed.len(),
                        in_pos,
                        &compressed[..compressed.len().min(16)]
                    )
                })?;

            let consumed = (inflater.total_in() - before_in) as usize;
            let produced = (inflater.total_out() - before_out) as usize;

            in_pos += consumed;
            decompressed.extend_from_slice(&out_buf[..produced]);

            // Check if we're done
            if in_pos >= compressed.len() {
                break;
            }

            // Check for unexpected status
            match status {
                flate2::Status::Ok => continue,
                flate2::Status::BufError => {
                    // Need more output space, continue with fresh buffer
                    continue;
                }
                flate2::Status::StreamEnd => {
                    // Stream ended but we have more input - this shouldn't happen within one rectangle
                    tracing::warn!(
                        "ZRLE: zlib stream ended early, consumed {}/{} bytes",
                        in_pos,
                        compressed.len()
                    );
                    break;
                }
            }
        }

        tracing::trace!(
            "ZRLE: decompressed {} -> {} bytes",
            compressed.len(),
            decompressed.len()
        );

        Ok(decompressed)
    }

    /// Decode all tiles in the rectangle from decompressed data.
    fn decode_tiles(
        &self,
        cursor: &mut DataCursor,
        rect: &Rectangle,
        pixel_format: &PixelFormat,
        buffer: &mut dyn MutablePixelBuffer,
        bytes_per_pixel: u8,
    ) -> Result<()> {
        // Determine CPixel optimization mode
        let cpixel_mode = CPixelMode::detect(pixel_format, bytes_per_pixel);

        // Iterate over 64x64 tiles in row-major order
        let mut ty = 0u16;
        while ty < rect.height {
            let tile_h = std::cmp::min(TILE_SIZE, rect.height - ty);

            let mut tx = 0u16;
            while tx < rect.width {
                let tile_w = std::cmp::min(TILE_SIZE, rect.width - tx);

                // Compute absolute tile position
                let abs_x = rect
                    .x
                    .checked_add(tx)
                    .ok_or_else(|| anyhow!("ZRLE: tile x coordinate overflows"))?;
                let abs_y = rect
                    .y
                    .checked_add(ty)
                    .ok_or_else(|| anyhow!("ZRLE: tile y coordinate overflows"))?;

                // Decode single tile
                self.decode_tile(
                    cursor,
                    (abs_x, abs_y),
                    (tile_w, tile_h),
                    pixel_format,
                    buffer,
                    bytes_per_pixel,
                    &cpixel_mode,
                )
                .with_context(|| {
                    format!(
                        "ZRLE: failed to decode tile at ({}, {}) size {}x{}",
                        tx, ty, tile_w, tile_h
                    )
                })?;

                tx += TILE_SIZE;
            }
            ty += TILE_SIZE;
        }

        Ok(())
    }

    /// Decode a single 64x64 (or smaller) tile.
    #[allow(clippy::too_many_arguments)]
    fn decode_tile(
        &self,
        cursor: &mut DataCursor,
        tile_pos: (u16, u16),
        tile_size: (u16, u16),
        pixel_format: &PixelFormat,
        buffer: &mut dyn MutablePixelBuffer,
        bytes_per_pixel: u8,
        cpixel_mode: &CPixelMode,
    ) -> Result<()> {
        let (_tile_x, _tile_y) = tile_pos;
        let (_tile_w, _tile_h) = tile_size;

        // Read subencoding byte
        let subencoding = cursor
            .read_u8()
            .context("failed to read subencoding byte")?;
        let rle = (subencoding & 0x80) != 0;
        let pal_size = (subencoding & 0x7F) as usize;

        // Validate palette size
        if pal_size as u8 > MAX_PALETTE_SIZE {
            bail!(
                "invalid palette size: {} (max {})",
                pal_size,
                MAX_PALETTE_SIZE
            );
        }

        // Dispatch based on mode
        match (pal_size, rle) {
            (1, _) => {
                // Mode 1: Solid tile (single color fill)
                self.decode_solid_tile(
                    cursor,
                    tile_pos,
                    tile_size,
                    pixel_format,
                    buffer,
                    bytes_per_pixel,
                    cpixel_mode,
                )
            }
            (0, false) => {
                // Mode 2: Raw pixels (no palette, no RLE)
                self.decode_raw_tile(
                    cursor,
                    tile_pos,
                    tile_size,
                    pixel_format,
                    buffer,
                    bytes_per_pixel,
                    cpixel_mode,
                )
            }
            (0, true) => {
                // Mode 3: Plain RLE (no palette)
                self.decode_plain_rle_tile(
                    cursor,
                    tile_pos,
                    tile_size,
                    pixel_format,
                    buffer,
                    bytes_per_pixel,
                    cpixel_mode,
                )
            }
            (2..=16, false) => {
                // Mode 4: Packed palette (1/2/4-bit indices)
                self.decode_packed_palette_tile(
                    cursor,
                    tile_pos,
                    tile_size,
                    pixel_format,
                    buffer,
                    pal_size,
                    bytes_per_pixel,
                    cpixel_mode,
                )
            }
            (17..=127, false) => {
                // Mode 5: Byte-indexed palette (8-bit indices)
                self.decode_byte_palette_tile(
                    cursor,
                    tile_pos,
                    tile_size,
                    pixel_format,
                    buffer,
                    pal_size,
                    bytes_per_pixel,
                    cpixel_mode,
                )
            }
            (2..=127, true) => {
                // Mode 6: Palette RLE
                self.decode_palette_rle_tile(
                    cursor,
                    tile_pos,
                    tile_size,
                    pixel_format,
                    buffer,
                    pal_size,
                    bytes_per_pixel,
                    cpixel_mode,
                )
            }
            _ => bail!(
                "ZRLE: invalid subencoding combination (pal_size={}, rle={})",
                pal_size,
                rle
            ),
        }
    }

    /// Mode 1: Decode solid tile (single color fill).
    #[allow(clippy::too_many_arguments)]
    fn decode_solid_tile(
        &self,
        cursor: &mut DataCursor,
        tile_pos: (u16, u16),
        tile_size: (u16, u16),
        pixel_format: &PixelFormat,
        buffer: &mut dyn MutablePixelBuffer,
        bytes_per_pixel: u8,
        cpixel_mode: &CPixelMode,
    ) -> Result<()> {
        let (tile_x, tile_y) = tile_pos;
        let (tile_w, tile_h) = tile_size;

        // Read single pixel value
        let mut pixel = read_cpixel(cursor, bytes_per_pixel, cpixel_mode)?;

        // ZRLE doesn't encode alpha channel - set to 255 for 32bpp formats
        if bytes_per_pixel == 4 {
            pixel.bytes[3] = 0xFF;
        }

        // Convert to buffer's pixel format
        let pixel_bytes = pixel_to_buffer_format(&pixel, pixel_format, bytes_per_pixel)?;

        // Fill tile with this color
        let tile_rect = Rect::new(tile_x as i32, tile_y as i32, tile_w as u32, tile_h as u32);
        buffer.fill_rect(tile_rect, &pixel_bytes)?;

        Ok(())
    }

    /// Mode 2: Decode raw tile (uncompressed pixel data).
    #[allow(clippy::too_many_arguments)]
    fn decode_raw_tile(
        &self,
        cursor: &mut DataCursor,
        tile_pos: (u16, u16),
        tile_size: (u16, u16),
        pixel_format: &PixelFormat,
        buffer: &mut dyn MutablePixelBuffer,
        bytes_per_pixel: u8,
        cpixel_mode: &CPixelMode,
    ) -> Result<()> {
        let (_tile_x, _tile_y) = tile_pos;
        let (tile_w, tile_h) = tile_size;

        let tile_area = (tile_w as usize)
            .checked_mul(tile_h as usize)
            .ok_or_else(|| anyhow!("tile area overflow"))?;

        // Read all pixels for this tile
        let mut pixels = Vec::with_capacity(tile_area);
        for _ in 0..tile_area {
            let pixel = read_cpixel(cursor, bytes_per_pixel, cpixel_mode)?;
            pixels.push(pixel);
        }

        // Write pixels row by row
        write_pixels_to_buffer(
            &pixels,
            tile_pos,
            tile_size,
            pixel_format,
            buffer,
            bytes_per_pixel,
        )?;

        Ok(())
    }

    /// Mode 3: Decode plain RLE tile (RLE without palette).
    #[allow(clippy::too_many_arguments)]
    fn decode_plain_rle_tile(
        &self,
        cursor: &mut DataCursor,
        tile_pos: (u16, u16),
        tile_size: (u16, u16),
        pixel_format: &PixelFormat,
        buffer: &mut dyn MutablePixelBuffer,
        bytes_per_pixel: u8,
        cpixel_mode: &CPixelMode,
    ) -> Result<()> {
        let (tile_w, tile_h) = tile_size;
        let tile_area = (tile_w as usize)
            .checked_mul(tile_h as usize)
            .ok_or_else(|| anyhow!("tile area overflow"))?;

        let mut pixels = Vec::with_capacity(tile_area);
        let mut count = 0;

        while count < tile_area {
            // Read pixel value
            let pixel = read_cpixel(cursor, bytes_per_pixel, cpixel_mode)?;

            // Read run length (1 + sum of continuation bytes)
            let run_len = read_rle_length(cursor)?;

            // Validate run doesn't exceed tile
            if count + run_len > tile_area {
                bail!(
                    "RLE run length {} exceeds remaining pixels {} (tile area {})",
                    run_len,
                    tile_area - count,
                    tile_area
                );
            }

            // Emit run
            for _ in 0..run_len {
                pixels.push(pixel.clone());
            }
            count += run_len;
        }

        // Write pixels to buffer
        write_pixels_to_buffer(
            &pixels,
            tile_pos,
            tile_size,
            pixel_format,
            buffer,
            bytes_per_pixel,
        )?;

        Ok(())
    }

    /// Mode 4: Decode packed palette tile (1/2/4-bit indices).
    #[allow(clippy::too_many_arguments)]
    fn decode_packed_palette_tile(
        &self,
        cursor: &mut DataCursor,
        tile_pos: (u16, u16),
        tile_size: (u16, u16),
        pixel_format: &PixelFormat,
        buffer: &mut dyn MutablePixelBuffer,
        pal_size: usize,
        bytes_per_pixel: u8,
        cpixel_mode: &CPixelMode,
    ) -> Result<()> {
        let (tile_w, tile_h) = tile_size;

        // Read palette
        let palette = read_palette(cursor, pal_size, bytes_per_pixel, cpixel_mode)?;

        // Determine bits per pixel based on palette size
        let bpp = if pal_size <= 2 {
            1
        } else if pal_size <= 4 {
            2
        } else {
            4 // pal_size <= 16
        };

        let tile_area = (tile_w as usize)
            .checked_mul(tile_h as usize)
            .ok_or_else(|| anyhow!("tile area overflow"))?;
        let mut pixels = Vec::with_capacity(tile_area);

        // Decode packed indices row by row
        for _ in 0..tile_h {
            let row_bits = (tile_w as usize)
                .checked_mul(bpp)
                .ok_or_else(|| anyhow!("row bits overflow"))?;
            let row_bytes = row_bits.div_ceil(8);

            let packed = cursor
                .read_exact(row_bytes)
                .context("failed to read packed palette row")?;

            // Unpack indices (MSB-first within each byte)
            let mut bit_offset = 0;
            for _ in 0..tile_w {
                let byte_idx = bit_offset / 8;
                let bit_idx = 7 - (bit_offset % 8);
                let mask = ((1 << bpp) - 1) << (bit_idx - (bpp - 1));
                let index = ((packed[byte_idx] & mask) >> (bit_idx - (bpp - 1))) as usize;

                if index >= pal_size {
                    bail!(
                        "packed palette index {} out of range (pal_size {})",
                        index,
                        pal_size
                    );
                }

                pixels.push(palette[index].clone());
                bit_offset += bpp;
            }
        }

        // Write pixels to buffer
        write_pixels_to_buffer(
            &pixels,
            tile_pos,
            tile_size,
            pixel_format,
            buffer,
            bytes_per_pixel,
        )?;

        Ok(())
    }

    /// Mode 5: Decode byte-indexed palette tile (8-bit indices).
    #[allow(clippy::too_many_arguments)]
    fn decode_byte_palette_tile(
        &self,
        cursor: &mut DataCursor,
        tile_pos: (u16, u16),
        tile_size: (u16, u16),
        pixel_format: &PixelFormat,
        buffer: &mut dyn MutablePixelBuffer,
        pal_size: usize,
        bytes_per_pixel: u8,
        cpixel_mode: &CPixelMode,
    ) -> Result<()> {
        let (tile_w, tile_h) = tile_size;
        let tile_area = (tile_w as usize)
            .checked_mul(tile_h as usize)
            .ok_or_else(|| anyhow!("tile area overflow"))?;

        // Read palette
        let palette = read_palette(cursor, pal_size, bytes_per_pixel, cpixel_mode)?;

        // Read indices
        let indices = cursor
            .read_exact(tile_area)
            .context("failed to read byte palette indices")?;

        let mut pixels = Vec::with_capacity(tile_area);
        for &index in indices {
            let idx = index as usize;
            if idx >= pal_size {
                bail!(
                    "byte palette index {} out of range (pal_size {})",
                    idx,
                    pal_size
                );
            }
            pixels.push(palette[idx].clone());
        }

        // Write pixels to buffer
        write_pixels_to_buffer(
            &pixels,
            tile_pos,
            tile_size,
            pixel_format,
            buffer,
            bytes_per_pixel,
        )?;

        Ok(())
    }

    /// Mode 6: Decode palette RLE tile.
    #[allow(clippy::too_many_arguments)]
    fn decode_palette_rle_tile(
        &self,
        cursor: &mut DataCursor,
        tile_pos: (u16, u16),
        tile_size: (u16, u16),
        pixel_format: &PixelFormat,
        buffer: &mut dyn MutablePixelBuffer,
        pal_size: usize,
        bytes_per_pixel: u8,
        cpixel_mode: &CPixelMode,
    ) -> Result<()> {
        let (tile_w, tile_h) = tile_size;
        let tile_area = (tile_w as usize)
            .checked_mul(tile_h as usize)
            .ok_or_else(|| anyhow!("tile area overflow"))?;

        // Read palette
        let palette = read_palette(cursor, pal_size, bytes_per_pixel, cpixel_mode)?;

        let mut pixels = Vec::with_capacity(tile_area);
        let mut count = 0;

        while count < tile_area {
            // Read code byte
            let code = cursor
                .read_u8()
                .context("failed to read palette RLE code")?;

            let (index, run_len) = if (code & 0x80) == 0 {
                // Single pixel
                (code as usize, 1)
            } else {
                // RLE run: index in bits 0-6, read run length
                let index = (code & 0x7F) as usize;
                let run_len = read_rle_length(cursor)?;
                (index, run_len)
            };

            // Validate index
            if index >= pal_size {
                bail!(
                    "palette RLE index {} out of range (pal_size {})",
                    index,
                    pal_size
                );
            }

            // Validate run doesn't exceed tile
            if count + run_len > tile_area {
                bail!(
                    "RLE run length {} exceeds remaining pixels {} (tile area {})",
                    run_len,
                    tile_area - count,
                    tile_area
                );
            }

            // Emit run
            for _ in 0..run_len {
                pixels.push(palette[index].clone());
            }
            count += run_len;
        }

        // Write pixels to buffer
        write_pixels_to_buffer(
            &pixels,
            tile_pos,
            tile_size,
            pixel_format,
            buffer,
            bytes_per_pixel,
        )?;

        Ok(())
    }
}

/// Byte cursor for reading from decompressed data.
struct DataCursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> DataCursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    fn read_u8(&mut self) -> Result<u8> {
        if self.pos >= self.data.len() {
            bail!("cursor EOF: need 1 byte, have {}", self.remaining());
        }
        let val = self.data[self.pos];
        self.pos += 1;
        Ok(val)
    }

    fn read_exact(&mut self, count: usize) -> Result<&'a [u8]> {
        if self.pos + count > self.data.len() {
            bail!(
                "cursor EOF: need {} bytes, have {}",
                count,
                self.remaining()
            );
        }
        let slice = &self.data[self.pos..self.pos + count];
        self.pos += count;
        Ok(slice)
    }
}

/// CPixel optimization mode for 24-bit pixels in 32bpp format.
#[derive(Debug, Clone, Copy)]
enum CPixelMode {
    /// No optimization: use full bytes_per_pixel
    None,
    /// Little-endian 32bpp, depth ≤ 24: RGB in bytes [0,1,2]
    LowC,
    /// Big-endian 32bpp, depth ≤ 24: RGB in bytes [1,2,3]
    HighC,
}

impl CPixelMode {
    fn detect(pf: &PixelFormat, bpp: u8) -> Self {
        if bpp != 4 || pf.depth > 24 {
            return Self::None;
        }

        // Check if RGB fits in 24 bits by calculating max pixel value
        // This simulates encoding white (0xFFFF, 0xFFFF, 0xFFFF) in the pixel format
        let r = ((0xFFFFu32 * pf.red_max as u32) / 0xFFFF) << pf.red_shift;
        let g = ((0xFFFFu32 * pf.green_max as u32) / 0xFFFF) << pf.green_shift;
        let b = ((0xFFFFu32 * pf.blue_max as u32) / 0xFFFF) << pf.blue_shift;
        let max_pixel = r | g | b;

        let fits_low_3 = max_pixel < (1 << 24);
        let fits_high_3 = (max_pixel & 0xFF) == 0;

        if fits_low_3 && pf.big_endian == 0 {
            Self::LowC
        } else if fits_high_3 && pf.big_endian != 0 {
            Self::HighC
        } else {
            Self::None
        }
    }
}

/// Internal pixel representation (up to 4 bytes).
#[derive(Debug, Clone)]
struct CPixel {
    bytes: [u8; 4],
}

impl CPixel {
    fn new(bytes: [u8; 4], _len: u8) -> Self {
        Self { bytes }
    }
}

/// Read a single CPixel from the cursor.
fn read_cpixel(cursor: &mut DataCursor, bytes_per_pixel: u8, mode: &CPixelMode) -> Result<CPixel> {
    match mode {
        CPixelMode::LowC => {
            // Read 3 bytes into [0,1,2], set [3]=0
            let data = cursor.read_exact(3)?;
            Ok(CPixel::new([data[0], data[1], data[2], 0], 4))
        }
        CPixelMode::HighC => {
            // Read 3 bytes into [1,2,3], set [0]=0
            let data = cursor.read_exact(3)?;
            Ok(CPixel::new([0, data[0], data[1], data[2]], 4))
        }
        CPixelMode::None => {
            // Read full bytes_per_pixel
            let data = cursor.read_exact(bytes_per_pixel as usize)?;
            let mut bytes = [0u8; 4];
            bytes[..bytes_per_pixel as usize].copy_from_slice(data);
            Ok(CPixel::new(bytes, bytes_per_pixel))
        }
    }
}

/// Read a palette of CPixels.
fn read_palette(
    cursor: &mut DataCursor,
    pal_size: usize,
    bytes_per_pixel: u8,
    mode: &CPixelMode,
) -> Result<Vec<CPixel>> {
    let mut palette = Vec::with_capacity(pal_size);
    for _ in 0..pal_size {
        palette.push(read_cpixel(cursor, bytes_per_pixel, mode)?);
    }
    Ok(palette)
}

/// Read RLE run length (1 + sum of continuation bytes).
fn read_rle_length(cursor: &mut DataCursor) -> Result<usize> {
    let mut length = 1usize;
    loop {
        let byte = cursor.read_u8().context("failed to read RLE length byte")?;
        length = length
            .checked_add(byte as usize)
            .ok_or_else(|| anyhow!("RLE length overflow"))?;
        if byte != 255 {
            break;
        }
    }
    Ok(length)
}

/// Convert CPixel to buffer's pixel format.
fn pixel_to_buffer_format(
    cpixel: &CPixel,
    _pixel_format: &PixelFormat,
    bytes_per_pixel: u8,
) -> Result<Vec<u8>> {
    // CPixel is in server's pixel format - let buffer handle conversion via fill_rect
    Ok(cpixel.bytes[..bytes_per_pixel as usize].to_vec())
}

/// Write pixels to buffer row by row.
fn write_pixels_to_buffer(
    pixels: &[CPixel],
    tile_pos: (u16, u16),
    tile_size: (u16, u16),
    _pixel_format: &PixelFormat,
    buffer: &mut dyn MutablePixelBuffer,
    bytes_per_pixel: u8,
) -> Result<()> {
    let (tile_x, tile_y) = tile_pos;
    let (tile_w, tile_h) = tile_size;

    // Extract pixel data in server's format
    let mut pixel_data = Vec::with_capacity(pixels.len() * bytes_per_pixel as usize);
    for pixel in pixels {
        // ZRLE doesn't encode alpha channel - set to 255 for 32bpp formats
        if bytes_per_pixel == 4 {
            pixel_data.extend_from_slice(&pixel.bytes[0..3]);
            pixel_data.push(0xFF); // Alpha = 255
        } else {
            pixel_data.extend_from_slice(&pixel.bytes[..bytes_per_pixel as usize]);
        }
    }

    // Write to buffer as a rectangle - buffer handles pixel format conversion
    let tile_rect = Rect::new(tile_x as i32, tile_y as i32, tile_w as u32, tile_h as u32);
    buffer.image_rect(tile_rect, &pixel_data, tile_w as usize)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use rfb_pixelbuffer::{ManagedPixelBuffer, PixelBuffer, PixelFormat as PBPixelFormat};
    use std::io::Write;

    /// Helper to create a test PixelFormat (32bpp RGB888, depth=32 to disable CPixel mode).
    fn test_pixel_format() -> PixelFormat {
        PixelFormat {
            bits_per_pixel: 32,
            depth: 32, // Use depth=32 to disable CPixel 3-byte optimization
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

    /// Helper to create zlib-compressed ZRLE data.
    fn make_zrle_data(payload: &[u8]) -> Vec<u8> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(payload).unwrap();
        let compressed = encoder.finish().unwrap();

        let mut result = Vec::new();
        result.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
        result.extend_from_slice(&compressed);
        result
    }

    #[tokio::test]
    async fn test_solid_tile_1x1() {
        let decoder = ZRLEDecoder::default();
        let pf = test_pixel_format();

        // Tile: palSize=1, pixel=[0xFF, 0x00, 0x00, 0x00] (red)
        let tile_data = vec![1, 0xFF, 0x00, 0x00, 0x00];
        let zrle_data = make_zrle_data(&tile_data);

        let mut stream = RfbInStream::new(std::io::Cursor::new(zrle_data));
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
            encoding: ENCODING_ZRLE,
        };

        let pb_pf = PBPixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(1, 1, pb_pf);

        decoder
            .decode(&mut stream, &rect, &pf, &mut buffer)
            .await
            .unwrap();

        // Verify pixel is red
        let mut stride = 0;
        let data = buffer
            .get_buffer(Rect::new(0, 0, 1, 1), &mut stride)
            .unwrap();
        assert_eq!(&data[0..4], &[0xFF, 0x00, 0x00, 0xFF]);
    }

    #[tokio::test]
    async fn test_raw_tile_2x2() {
        let decoder = ZRLEDecoder::default();
        let pf = test_pixel_format();

        // Tile: palSize=0, RLE=0, 4 pixels (red, green, blue, black)
        let tile_data = vec![
            0, // palSize=0, RLE=0
            0xFF, 0x00, 0x00, 0x00, // red
            0x00, 0xFF, 0x00, 0x00, // green
            0x00, 0x00, 0xFF, 0x00, // blue
            0x00, 0x00, 0x00, 0x00, // black
        ];
        let zrle_data = make_zrle_data(&tile_data);

        let mut stream = RfbInStream::new(std::io::Cursor::new(zrle_data));
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
            encoding: ENCODING_ZRLE,
        };

        let pb_pf = PBPixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(2, 2, pb_pf);

        decoder
            .decode(&mut stream, &rect, &pf, &mut buffer)
            .await
            .unwrap();

        // Verify colors
        let mut stride = 0;
        let data = buffer
            .get_buffer(Rect::new(0, 0, 2, 2), &mut stride)
            .unwrap();
        assert_eq!(stride, 2);
        assert_eq!(&data[0..4], &[0xFF, 0x00, 0x00, 0xFF]); // red
        assert_eq!(&data[4..8], &[0x00, 0xFF, 0x00, 0xFF]); // green
        assert_eq!(&data[8..12], &[0x00, 0x00, 0xFF, 0xFF]); // blue
        assert_eq!(&data[12..16], &[0x00, 0x00, 0x00, 0xFF]); // black
    }

    #[tokio::test]
    async fn test_plain_rle_with_runs() {
        let decoder = ZRLEDecoder::default();
        let pf = test_pixel_format();

        // Tile: palSize=0, RLE=1, 3x3 with runs
        // Run 1: red x 5 (length=1+4)
        // Run 2: blue x 4 (length=1+3)
        let tile_data = vec![
            0x80, // palSize=0, RLE=1
            0xFF, 0x00, 0x00, 0x00, 4, // red x 5
            0x00, 0x00, 0xFF, 0x00, 3, // blue x 4
        ];
        let zrle_data = make_zrle_data(&tile_data);

        let mut stream = RfbInStream::new(std::io::Cursor::new(zrle_data));
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 3,
            height: 3,
            encoding: ENCODING_ZRLE,
        };

        let pb_pf = PBPixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(3, 3, pb_pf);

        decoder
            .decode(&mut stream, &rect, &pf, &mut buffer)
            .await
            .unwrap();

        // Verify: first 5 pixels red, next 4 blue
        let mut stride = 0;
        let data = buffer
            .get_buffer(Rect::new(0, 0, 3, 3), &mut stride)
            .unwrap();
        for i in 0..5 {
            let offset = i * 4;
            assert_eq!(
                &data[offset..offset + 3],
                &[0xFF, 0x00, 0x00],
                "pixel {} should be red",
                i
            );
        }
        for i in 5..9 {
            let offset = i * 4;
            assert_eq!(
                &data[offset..offset + 3],
                &[0x00, 0x00, 0xFF],
                "pixel {} should be blue",
                i
            );
        }
    }

    #[tokio::test]
    async fn test_packed_palette_2bit() {
        let decoder = ZRLEDecoder::default();
        let pf = test_pixel_format();

        // Tile: palSize=4 (2-bit indices), 4x1
        // Palette: [red, green, blue, white]
        // Indices: [0, 1, 2, 3]
        // Packed: 0b00_01_10_11 = 0x1B
        let tile_data = vec![
            4, // palSize=4, RLE=0
            0xFF, 0x00, 0x00, 0x00, // red
            0x00, 0xFF, 0x00, 0x00, // green
            0x00, 0x00, 0xFF, 0x00, // blue
            0xFF, 0xFF, 0xFF, 0x00, // white
            0x1B, // packed indices
        ];
        let zrle_data = make_zrle_data(&tile_data);

        let mut stream = RfbInStream::new(std::io::Cursor::new(zrle_data));
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 4,
            height: 1,
            encoding: ENCODING_ZRLE,
        };

        let pb_pf = PBPixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(4, 1, pb_pf);

        decoder
            .decode(&mut stream, &rect, &pf, &mut buffer)
            .await
            .unwrap();

        // Verify colors
        let mut stride = 0;
        let data = buffer
            .get_buffer(Rect::new(0, 0, 4, 1), &mut stride)
            .unwrap();
        assert_eq!(&data[0..4], &[0xFF, 0x00, 0x00, 0xFF]); // red
        assert_eq!(&data[4..8], &[0x00, 0xFF, 0x00, 0xFF]); // green
        assert_eq!(&data[8..12], &[0x00, 0x00, 0xFF, 0xFF]); // blue
        assert_eq!(&data[12..16], &[0xFF, 0xFF, 0xFF, 0xFF]); // white
    }

    #[tokio::test]
    async fn test_byte_palette() {
        let decoder = ZRLEDecoder::default();
        let pf = test_pixel_format();

        // Tile: palSize=17, 2x1
        // Palette: 17 colors (only use first 2)
        // Indices: [0, 1]
        let mut tile_data = vec![17]; // palSize=17, RLE=0

        // 17 palette entries (red, green, then 15 dummy)
        tile_data.extend_from_slice(&[0xFF, 0x00, 0x00, 0x00]); // red
        tile_data.extend_from_slice(&[0x00, 0xFF, 0x00, 0x00]); // green
        for _ in 0..15 {
            tile_data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // dummy
        }

        // Indices
        tile_data.extend_from_slice(&[0, 1]);

        let zrle_data = make_zrle_data(&tile_data);

        let mut stream = RfbInStream::new(std::io::Cursor::new(zrle_data));
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 2,
            height: 1,
            encoding: ENCODING_ZRLE,
        };

        let pb_pf = PBPixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(2, 1, pb_pf);

        decoder
            .decode(&mut stream, &rect, &pf, &mut buffer)
            .await
            .unwrap();

        // Verify colors
        let mut stride = 0;
        let data = buffer
            .get_buffer(Rect::new(0, 0, 2, 1), &mut stride)
            .unwrap();
        assert_eq!(&data[0..4], &[0xFF, 0x00, 0x00, 0xFF]); // red
        assert_eq!(&data[4..8], &[0x00, 0xFF, 0x00, 0xFF]); // green
    }

    #[tokio::test]
    async fn test_palette_rle() {
        let decoder = ZRLEDecoder::default();
        let pf = test_pixel_format();

        // Tile: palSize=2, RLE=1, 1x6
        // Palette: [red, blue]
        // Codes: [0] (red x1), [0x80 | 1, 3] (blue x4), [0] (red x1)
        let tile_data = vec![
            0x82, // palSize=2, RLE=1
            0xFF, 0x00, 0x00, 0x00, // red
            0x00, 0x00, 0xFF, 0x00, // blue
            0,    // red x1
            0x81, 3, // blue x4 (index 1, length 1+3)
            0, // red x1
        ];
        let zrle_data = make_zrle_data(&tile_data);

        let mut stream = RfbInStream::new(std::io::Cursor::new(zrle_data));
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 6,
            height: 1,
            encoding: ENCODING_ZRLE,
        };

        let pb_pf = PBPixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(6, 1, pb_pf);

        decoder
            .decode(&mut stream, &rect, &pf, &mut buffer)
            .await
            .unwrap();

        // Verify pattern: red, blue, blue, blue, blue, red
        let mut stride = 0;
        let data = buffer
            .get_buffer(Rect::new(0, 0, 6, 1), &mut stride)
            .unwrap();
        assert_eq!(&data[0..4], &[0xFF, 0x00, 0x00, 0xFF]); // red
        assert_eq!(&data[4..8], &[0x00, 0x00, 0xFF, 0xFF]); // blue
        assert_eq!(&data[8..12], &[0x00, 0x00, 0xFF, 0xFF]); // blue
        assert_eq!(&data[12..16], &[0x00, 0x00, 0xFF, 0xFF]); // blue
        assert_eq!(&data[16..20], &[0x00, 0x00, 0xFF, 0xFF]); // blue
        assert_eq!(&data[20..24], &[0xFF, 0x00, 0x00, 0xFF]); // red
    }

    #[tokio::test]
    async fn test_empty_rectangle() {
        let decoder = ZRLEDecoder::default();
        let pf = test_pixel_format();
        let pb_pf = PBPixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(10, 10, pb_pf);

        let zrle_data = make_zrle_data(&[]);
        let mut stream = RfbInStream::new(std::io::Cursor::new(zrle_data));
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 0,
            height: 10,
            encoding: ENCODING_ZRLE,
        };

        // Should succeed without reading data
        decoder
            .decode(&mut stream, &rect, &pf, &mut buffer)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_error_eof_reading_pixel() {
        let decoder = ZRLEDecoder::default();
        let pf = test_pixel_format();

        // Subencoding byte 128 = palSize=0, RLE=1 (plain RLE)
        // But no pixel data follows - should get EOF error
        let tile_data = vec![128];
        let zrle_data = make_zrle_data(&tile_data);

        let mut stream = RfbInStream::new(std::io::Cursor::new(zrle_data));
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
            encoding: ENCODING_ZRLE,
        };

        let pb_pf = PBPixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(1, 1, pb_pf);

        let result = decoder.decode(&mut stream, &rect, &pf, &mut buffer).await;
        assert!(result.is_err());
        let err_str = format!("{:?}", result.unwrap_err());
        assert!(err_str.contains("EOF") || err_str.contains("need"));
    }

    #[tokio::test]
    async fn test_error_rle_run_exceeds_tile() {
        let decoder = ZRLEDecoder::default();
        let pf = test_pixel_format();

        // Tile: palSize=0, RLE=1, 1x1 but run length = 5
        let tile_data = vec![
            0x80, // palSize=0, RLE=1
            0xFF, 0x00, 0x00, 0x00, 4, // red x 5 (but tile is only 1 pixel!)
        ];
        let zrle_data = make_zrle_data(&tile_data);

        let mut stream = RfbInStream::new(std::io::Cursor::new(zrle_data));
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
            encoding: ENCODING_ZRLE,
        };

        let pb_pf = PBPixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(1, 1, pb_pf);

        let result = decoder.decode(&mut stream, &rect, &pf, &mut buffer).await;
        assert!(result.is_err());
        let err_str = format!("{:?}", result.unwrap_err());
        assert!(err_str.contains("exceeds remaining pixels"));
    }

    #[tokio::test]
    async fn test_error_palette_index_out_of_range() {
        let decoder = ZRLEDecoder::default();
        let pf = test_pixel_format();

        // Tile: palSize=2, byte-indexed mode (17), index 5 (>2)
        let mut tile_data = vec![17]; // palSize=17
        tile_data.extend_from_slice(&[0xFF, 0x00, 0x00, 0x00]); // red
        tile_data.extend_from_slice(&[0x00, 0xFF, 0x00, 0x00]); // green
        for _ in 0..15 {
            tile_data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // dummy
        }
        tile_data.push(20); // invalid index

        let zrle_data = make_zrle_data(&tile_data);

        let mut stream = RfbInStream::new(std::io::Cursor::new(zrle_data));
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
            encoding: ENCODING_ZRLE,
        };

        let pb_pf = PBPixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(1, 1, pb_pf);

        let result = decoder.decode(&mut stream, &rect, &pf, &mut buffer).await;
        assert!(result.is_err());
        let err_str = format!("{:?}", result.unwrap_err());
        assert!(err_str.contains("out of range"));
    }

    #[tokio::test]
    async fn test_long_rle_run_with_255_continuation() {
        let decoder = ZRLEDecoder::default();
        let pf = test_pixel_format();

        // Test RLE length encoding with 255 continuation byte
        // 1x100 rectangle (2 tiles: 64x1 + 36x1)
        // Tile 1: RLE run of 64 red pixels (length = 1 + 63)
        // Tile 2: RLE run of 36 red pixels (length = 1 + 35)
        let mut tile_data = Vec::new();
        // Tile 1 (64 pixels)
        tile_data.push(0x80); // palSize=0, RLE=1
        tile_data.extend_from_slice(&[0xFF, 0x00, 0x00, 0x00, 63]); // red x 64 (1+63)
                                                                    // Tile 2 (36 pixels)
        tile_data.push(0x80); // palSize=0, RLE=1
        tile_data.extend_from_slice(&[0xFF, 0x00, 0x00, 0x00, 35]); // red x 36 (1+35)

        let zrle_data = make_zrle_data(&tile_data);

        let mut stream = RfbInStream::new(std::io::Cursor::new(zrle_data));
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 100,
            height: 1,
            encoding: ENCODING_ZRLE,
        };

        let pb_pf = PBPixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(100, 1, pb_pf);

        decoder
            .decode(&mut stream, &rect, &pf, &mut buffer)
            .await
            .unwrap();

        // Verify all 100 pixels are red
        let mut stride = 0;
        let data = buffer
            .get_buffer(Rect::new(0, 0, 100, 1), &mut stride)
            .unwrap();
        for i in 0..100 {
            let offset = i * 4;
            assert_eq!(
                &data[offset..offset + 3],
                &[0xFF, 0x00, 0x00],
                "pixel {} should be red",
                i
            );
        }
    }

    #[tokio::test]
    async fn test_multiple_tiles() {
        let decoder = ZRLEDecoder::default();
        let pf = test_pixel_format();

        // Rectangle 128x1 (2 tiles: 64x1 each)
        // Tile 1: solid red (palSize=1)
        // Tile 2: solid blue (palSize=1)
        let mut tile_data = Vec::new();
        tile_data.push(1); // Tile 1: palSize=1
        tile_data.extend_from_slice(&[0xFF, 0x00, 0x00, 0x00]); // red
        tile_data.push(1); // Tile 2: palSize=1
        tile_data.extend_from_slice(&[0x00, 0x00, 0xFF, 0x00]); // blue

        let zrle_data = make_zrle_data(&tile_data);

        let mut stream = RfbInStream::new(std::io::Cursor::new(zrle_data));
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 128,
            height: 1,
            encoding: ENCODING_ZRLE,
        };

        let pb_pf = PBPixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(128, 1, pb_pf);

        decoder
            .decode(&mut stream, &rect, &pf, &mut buffer)
            .await
            .unwrap();

        // Verify first 64 pixels are red, next 64 are blue
        let mut stride = 0;
        let data = buffer
            .get_buffer(Rect::new(0, 0, 128, 1), &mut stride)
            .unwrap();
        for i in 0..64 {
            let offset = i * 4;
            assert_eq!(
                &data[offset..offset + 3],
                &[0xFF, 0x00, 0x00],
                "pixel {} should be red",
                i
            );
        }
        for i in 64..128 {
            let offset = i * 4;
            assert_eq!(
                &data[offset..offset + 3],
                &[0x00, 0x00, 0xFF],
                "pixel {} should be blue",
                i
            );
        }
    }
}
