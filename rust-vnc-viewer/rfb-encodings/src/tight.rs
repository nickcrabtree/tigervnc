//! Tight encoding decoder - JPEG or zlib compression with filtering.
//!
//! Tight (type 7) is the most commonly used VNC encoding in production. It achieves excellent
//! compression ratios through a combination of JPEG for photo-like regions, zlib for other
//! content, palette modes for indexed color, and gradient prediction filtering for better
//! compression of continuous-tone images.
//!
//! # Wire Format
//!
//! ```text
//! +------------------+
//! | compression_ctl  |  1 byte (4 reset bits + 4 compression type bits)
//! +------------------+
//! | [encoding data]  |  Varies by compression type (see below)
//! +------------------+
//! ```
//!
//! # Compression Control Byte
//!
//! Lower 4 bits (0-3): **Zlib stream reset flags** (one bit per stream 0-3).
//! - If bit N is set: reset zlib stream N before decoding this rectangle.
//!
//! Upper 4 bits (4-7): **Compression type**.
//! - `0x08` (FILL): Solid color fill (most efficient for uniform regions)
//! - `0x09` (JPEG): JPEG-compressed image data
//! - `0x00-0x03`: Basic compression using zlib stream 0-3
//!
//! # Compression Types
//!
//! ## Fill Mode (0x08)
//!
//! Single solid color fills the entire rectangle:
//! - If truecolor depth ≥ 24: 3 bytes RGB
//! - Otherwise: `bits_per_pixel / 8` bytes in native format
//!
//! ## JPEG Mode (0x09)
//!
//! JPEG-compressed image:
//! 1. Compact length (1-3 bytes)
//! 2. JPEG data (length bytes)
//! 3. Decode JPEG to RGB8, then convert to pixel format
//!
//! ## Basic Mode (0x00-0x03)
//!
//! Optionally filtered data, optionally zlib-compressed:
//!
//! 1. **If explicit filter flag (0x04) is set in comp_ctl:**
//!    - Read 1 byte filter_id:
//!      - `0x00` (COPY): RGB888 data (3 bytes/pixel)
//!      - `0x01` (PALETTE): Indexed color (2-256 palette entries)
//!      - `0x02` (GRADIENT): Gradient-predicted RGB888
//!
//! 2. **Data reading:**
//!    - If data_size < 12: read uncompressed
//!    - Otherwise: read compact length, then compressed data, decompress with zlib
//!
//! 3. **Filtering:**
//!    - COPY: convert RGB888 or native pixels to buffer format
//!    - PALETTE: expand palette indices to full pixels
//!    - GRADIENT: reconstruct RGB888 using gradient prediction, then convert
//!
//! # Compact Length Encoding
//!
//! Variable-length integer (1-3 bytes):
//! - If byte0 < 0x80: length = byte0
//! - Else: length = (byte0 & 0x7F) | (byte1 << 7)
//!   - If byte1 < 0x80: done
//!   - Else: length |= (byte2 << 14)
//!
//! # Zlib Streams
//!
//! Tight maintains 4 independent zlib decompression streams (stream IDs 0-3).
//! Lower 2 bits of comp_ctl (in basic mode) select which stream to use.
//! Streams preserve history across rectangles until explicitly reset.
//!
//! # Example
//!
//! ```no_run
//! use rfb_encodings::{Decoder, TightDecoder, ENCODING_TIGHT};
//!
//! let decoder = TightDecoder::default();
//! assert_eq!(decoder.encoding_type(), ENCODING_TIGHT);
//! ```

use crate::{Decoder, MutablePixelBuffer, PixelFormat, Rectangle, RfbInStream, ENCODING_TIGHT};
use anyhow::{anyhow, bail, Context, Result};
use flate2::Decompress;
use rfb_common::Rect;
use std::io::Cursor;
use std::sync::Mutex;
use tokio::io::AsyncRead;

// Tight compression control flags (upper nibble of comp_ctl)
const TIGHT_EXPLICIT_FILTER: u8 = 0x04;
const TIGHT_FILL: u8 = 0x08;
const TIGHT_JPEG: u8 = 0x09;
const TIGHT_MAX_SUBENCODING: u8 = 0x09;

// Tight filter types (for basic compression with explicit filter)
const TIGHT_FILTER_COPY: u8 = 0x00;
const TIGHT_FILTER_PALETTE: u8 = 0x01;
const TIGHT_FILTER_GRADIENT: u8 = 0x02;

// Tight encoding limits
const TIGHT_MAX_WIDTH: u16 = 2048;
const TIGHT_MIN_TO_COMPRESS: usize = 12;

/// Decoder for Tight encoding.
///
/// This is the most sophisticated and widely-used VNC encoding. It combines JPEG compression
/// for photo-like regions, zlib for other content, palette modes for indexed colors, and
/// gradient prediction for continuous-tone images. Tight achieves excellent compression
/// ratios while maintaining reasonable decoding speed.
///
/// # Zlib Streams
///
/// Maintains 4 independent zlib decompression streams (IDs 0-3) that preserve history
/// across rectangles. The compression control byte indicates which stream to use and
/// when to reset streams.
///
/// # Example
///
/// ```no_run
/// # use rfb_encodings::{Decoder, TightDecoder, ENCODING_TIGHT};
/// let decoder = TightDecoder::default();
/// assert_eq!(decoder.encoding_type(), ENCODING_TIGHT);
/// ```
pub struct TightDecoder {
    /// Four independent zlib decompression streams (None = not yet initialized).
    /// Streams preserve dictionary across rectangles until explicitly reset.
    /// Uses Mutex for thread-safe interior mutability since decode() takes &self.
    zlib_streams: Mutex<[Option<Decompress>; 4]>,
}

impl Default for TightDecoder {
    fn default() -> Self {
        Self {
            zlib_streams: Mutex::new([None, None, None, None]),
        }
    }
}

impl TightDecoder {
    /// Read a Tight compact length value (1-3 bytes).
    ///
    /// Returns the decoded length as a usize.
    async fn read_compact_length<R: AsyncRead + Unpin>(
        stream: &mut RfbInStream<R>,
    ) -> Result<usize> {
        let b0 = stream
            .read_u8()
            .await
            .context("Failed to read compact length byte 0")?;

        if (b0 & 0x80) == 0 {
            return Ok(b0 as usize);
        }

        let b1 = stream
            .read_u8()
            .await
            .context("Failed to read compact length byte 1")?;
        let mut length = ((b0 & 0x7F) as usize) | ((b1 as usize) << 7);

        if (b1 & 0x80) == 0 {
            return Ok(length);
        }

        let b2 = stream
            .read_u8()
            .await
            .context("Failed to read compact length byte 2")?;
        length |= (b2 as usize) << 14;

        Ok(length)
    }

    /// Decompress zlib-compressed data using the specified stream.
    ///
    /// # Arguments
    /// * `stream_id` - Zlib stream ID (0-3)
    /// * `compressed_data` - The compressed bytes
    /// * `expected_size` - Expected decompressed size
    ///
    /// # Returns
    /// Decompressed data as Vec<u8>
    fn decompress_zlib(
        &self,
        stream_id: usize,
        compressed_data: &[u8],
        expected_size: usize,
    ) -> Result<Vec<u8>> {
        let mut streams = self.zlib_streams.lock().unwrap();

        // Initialize stream on first use
        if streams[stream_id].is_none() {
            streams[stream_id] = Some(Decompress::new(true));
        }

        let decompressor = streams[stream_id]
            .as_mut()
            .expect("Zlib stream should be initialized");

        let mut output = vec![0u8; expected_size];
        let before_out = decompressor.total_out();

        decompressor
            .decompress(compressed_data, &mut output, flate2::FlushDecompress::Sync)
            .with_context(|| {
                format!(
                    "Tight: zlib decompression failed for stream {} (in={}, expected_out={})",
                    stream_id,
                    compressed_data.len(),
                    expected_size
                )
            })?;

        let output_size = (decompressor.total_out() - before_out) as usize;
        if output_size != expected_size {
            bail!(
                "Tight: zlib stream {} produced {} bytes but expected {}",
                stream_id,
                output_size,
                expected_size
            );
        }

        output.truncate(output_size);
        Ok(output)
    }

    /// Apply COPY filter: convert RGB888 or native pixels to buffer format.
    fn filter_copy(
        &self,
        data: &[u8],
        rect: &Rectangle,
        pixel_format: &PixelFormat,
        buffer: &mut dyn MutablePixelBuffer,
        rgb888_mode: bool,
    ) -> Result<()> {
        let width = rect.width as usize;
        let height = rect.height as usize;
        let dest_rect = Rect::new(rect.x as i32, rect.y as i32, width as u32, height as u32);

        if rgb888_mode {
            // Data is RGB888 (3 bytes/pixel) - need to convert to buffer's format
            // For now, use image_rect which handles conversion
            // Create temporary buffer in BGRA format for image_rect
            let bytes_per_pixel = (pixel_format.bits_per_pixel / 8) as usize;
            let expected_size = width * height * 3;
            if data.len() < expected_size {
                bail!(
                    "Tight COPY RGB888: insufficient data ({} < {})",
                    data.len(),
                    expected_size
                );
            }

            // Convert RGB888 to target pixel format
            let mut converted = vec![0u8; width * height * bytes_per_pixel];
            for y in 0..height {
                for x in 0..width {
                    let src_idx = (y * width + x) * 3;
                    let dst_idx = (y * width + x) * bytes_per_pixel;
                    let r = data[src_idx];
                    let g = data[src_idx + 1];
                    let b = data[src_idx + 2];

                    // Pack into pixel format
                    self.pack_rgb_to_pixel(
                        r,
                        g,
                        b,
                        pixel_format,
                        &mut converted[dst_idx..dst_idx + bytes_per_pixel],
                    )?;
                }
            }

            buffer.image_rect(dest_rect, &converted, width)?;
        } else {
            // Data is in native pixel format - direct copy
            buffer.image_rect(dest_rect, data, width)?;
        }

        Ok(())
    }

    /// Apply PALETTE filter: expand palette indices to pixels.
    async fn filter_palette<R: AsyncRead + Unpin>(
        &self,
        stream: &mut RfbInStream<R>,
        rect: &Rectangle,
        pixel_format: &PixelFormat,
        buffer: &mut dyn MutablePixelBuffer,
        data: &[u8],
    ) -> Result<()> {
        // Read palette size (1 byte: count - 1)
        let pal_size_minus_one = stream
            .read_u8()
            .await
            .context("Failed to read Tight palette size")?;
        let palette_size = (pal_size_minus_one as usize) + 1;

        if !(2..=256).contains(&palette_size) {
            bail!(
                "Tight PALETTE: invalid palette size {} (must be 2-256)",
                palette_size
            );
        }

        // Read palette colors as RGB888 (3 bytes each)
        let mut palette_rgb = vec![0u8; palette_size * 3];
        stream
            .read_bytes(&mut palette_rgb)
            .await
            .context("Failed to read Tight palette RGB data")?;

        // Convert palette to pixel format
        let bytes_per_pixel = (pixel_format.bits_per_pixel / 8) as usize;
        let mut palette_pixels = vec![0u8; palette_size * bytes_per_pixel];
        for i in 0..palette_size {
            let r = palette_rgb[i * 3];
            let g = palette_rgb[i * 3 + 1];
            let b = palette_rgb[i * 3 + 2];
            self.pack_rgb_to_pixel(
                r,
                g,
                b,
                pixel_format,
                &mut palette_pixels[i * bytes_per_pixel..(i + 1) * bytes_per_pixel],
            )?;
        }

        let width = rect.width as usize;
        let height = rect.height as usize;

        // Decode indices and expand to pixels
        let mut pixels = vec![0u8; width * height * bytes_per_pixel];

        if palette_size == 2 {
            // 1-bit indices (packed 8 per byte, MSB first)
            let mut data_idx = 0;
            for y in 0..height {
                let mut x = 0;
                while x < width {
                    if data_idx >= data.len() {
                        bail!("Tight PALETTE 2-color: unexpected end of index data");
                    }
                    let bits = data[data_idx];
                    data_idx += 1;

                    for bit in (0..8).rev() {
                        if x >= width {
                            break;
                        }
                        let pal_idx = ((bits >> bit) & 1) as usize;
                        let pixel_start = (y * width + x) * bytes_per_pixel;
                        pixels[pixel_start..pixel_start + bytes_per_pixel].copy_from_slice(
                            &palette_pixels
                                [pal_idx * bytes_per_pixel..(pal_idx + 1) * bytes_per_pixel],
                        );
                        x += 1;
                    }
                }
            }
        } else {
            // 8-bit indices (1 byte per pixel)
            for y in 0..height {
                for x in 0..width {
                    let data_idx = y * width + x;
                    if data_idx >= data.len() {
                        bail!("Tight PALETTE: unexpected end of index data");
                    }
                    let pal_idx = data[data_idx] as usize;
                    if pal_idx >= palette_size {
                        bail!(
                            "Tight PALETTE: index {} out of range (palette size {})",
                            pal_idx,
                            palette_size
                        );
                    }
                    let pixel_start = (y * width + x) * bytes_per_pixel;
                    pixels[pixel_start..pixel_start + bytes_per_pixel].copy_from_slice(
                        &palette_pixels[pal_idx * bytes_per_pixel..(pal_idx + 1) * bytes_per_pixel],
                    );
                }
            }
        }

        // Write to buffer
        let dest_rect = Rect::new(rect.x as i32, rect.y as i32, width as u32, height as u32);
        buffer.image_rect(dest_rect, &pixels, width)?;

        Ok(())
    }

    /// Apply GRADIENT filter: reconstruct RGB888 using gradient prediction.
    fn filter_gradient(
        &self,
        data: &[u8],
        rect: &Rectangle,
        pixel_format: &PixelFormat,
        buffer: &mut dyn MutablePixelBuffer,
    ) -> Result<()> {
        // Gradient filter requires truecolor (16 or 32 bpp)
        if pixel_format.bits_per_pixel == 8 {
            bail!("Tight gradient filter requires truecolor (16/32 bpp), got 8 bpp");
        }

        let width = rect.width as usize;
        let height = rect.height as usize;
        let expected_size = width * height * 3;

        if data.len() < expected_size {
            bail!(
                "Tight GRADIENT: insufficient data ({} < {})",
                data.len(),
                expected_size
            );
        }

        // Gradient prediction: reconstruct RGB888 from deltas
        let mut prev_row = vec![0u8; width * 3];
        let mut curr_row = vec![0u8; width * 3];

        let bytes_per_pixel = (pixel_format.bits_per_pixel / 8) as usize;
        let mut pixels = vec![0u8; width * height * bytes_per_pixel];

        for y in 0..height {
            for x in 0..width {
                let src_idx = (y * width + x) * 3;

                for c in 0..3 {
                    let left = if x > 0 { curr_row[(x - 1) * 3 + c] } else { 0 };
                    let top = prev_row[x * 3 + c];
                    let top_left = if x > 0 { prev_row[(x - 1) * 3 + c] } else { 0 };

                    let predicted = left.wrapping_add(top).wrapping_sub(top_left);
                    let decoded = predicted.wrapping_add(data[src_idx + c]);
                    curr_row[x * 3 + c] = decoded;
                }

                // Convert reconstructed RGB to pixel format
                let r = curr_row[x * 3];
                let g = curr_row[x * 3 + 1];
                let b = curr_row[x * 3 + 2];
                let pixel_start = (y * width + x) * bytes_per_pixel;
                self.pack_rgb_to_pixel(
                    r,
                    g,
                    b,
                    pixel_format,
                    &mut pixels[pixel_start..pixel_start + bytes_per_pixel],
                )?;
            }

            // Swap rows for next iteration
            std::mem::swap(&mut prev_row, &mut curr_row);
        }

        // Write to buffer
        let dest_rect = Rect::new(rect.x as i32, rect.y as i32, width as u32, height as u32);
        buffer.image_rect(dest_rect, &pixels, width)?;

        Ok(())
    }

    /// Pack RGB888 values into the target pixel format.
    fn pack_rgb_to_pixel(
        &self,
        r: u8,
        g: u8,
        b: u8,
        pf: &PixelFormat,
        out: &mut [u8],
    ) -> Result<()> {
        let bytes_per_pixel = (pf.bits_per_pixel / 8) as usize;
        if out.len() < bytes_per_pixel {
            bail!("Output buffer too small for pixel");
        }

        // Scale RGB values to fit format bit depths
        let r_val = (r as u32) * (pf.red_max as u32) / 255;
        let g_val = (g as u32) * (pf.green_max as u32) / 255;
        let b_val = (b as u32) * (pf.blue_max as u32) / 255;

        // Pack into pixel value
        let pixel = (r_val << pf.red_shift) | (g_val << pf.green_shift) | (b_val << pf.blue_shift);

        // Write to output in correct endianness
        match bytes_per_pixel {
            1 => out[0] = pixel as u8,
            2 => {
                if pf.big_endian != 0 {
                    out[0..2].copy_from_slice(&(pixel as u16).to_be_bytes());
                } else {
                    out[0..2].copy_from_slice(&(pixel as u16).to_le_bytes());
                }
            }
            4 => {
                if pf.big_endian != 0 {
                    out[0..4].copy_from_slice(&pixel.to_be_bytes());
                } else {
                    out[0..4].copy_from_slice(&pixel.to_le_bytes());
                }
            }
            _ => bail!("Unsupported bytes_per_pixel: {}", bytes_per_pixel),
        }

        Ok(())
    }
}

impl Decoder for TightDecoder {
    fn encoding_type(&self) -> i32 {
        ENCODING_TIGHT
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
            "Tight decode start: rect=[{},{} {}x{}] buffer_before={}",
            rect.x, rect.y, rect.width, rect.height,
            buffer_before
        );

        // Empty rectangle - nothing to decode
        if rect.width == 0 || rect.height == 0 {
            tracing::debug!(
                target: "rfb_encodings::framing",
                "Tight decode end: empty rectangle, bytes_consumed=0, buffer_after={}",
                stream.available()
            );
            return Ok(());
        }

        // Check width limit
        if rect.width > TIGHT_MAX_WIDTH {
            bail!(
                "Tight: rectangle too wide ({} > {} max)",
                rect.width,
                TIGHT_MAX_WIDTH
            );
        }

        // Read compression control byte
        let comp_ctl = stream.read_u8().await.with_context(|| {
            format!(
                "Failed to read Tight compression control at ({}, {})",
                rect.x, rect.y
            )
        })?;

        // Handle stream resets (lower 4 bits)
        {
            let mut streams = self.zlib_streams.lock().unwrap();
            for i in 0..4 {
                if (comp_ctl & (1 << i)) != 0 {
                    tracing::debug!(
                        "Tight: reset zlib stream {} requested (comp_ctl={:#04x})",
                        i, comp_ctl
                    );
                    streams[i] = None; // Reset stream
                }
            }
        }

        // Determine compression type (upper 4 bits)
        let comp_type = comp_ctl >> 4;
        
        tracing::debug!(
            "Tight: comp_ctl={:#04x} comp_type={:#x} reset_bits={:#x}",
            comp_ctl, comp_type, comp_ctl & 0x0F
        );

        // Handle FILL mode
        if comp_type == TIGHT_FILL {
            let bytes_per_pixel = (pixel_format.bits_per_pixel / 8) as usize;
            let mut pixel_data = vec![0u8; bytes_per_pixel];

            // For truecolor depth >= 24, read 3 bytes RGB
            if pixel_format.depth >= 24 && pixel_format.true_color != 0 {
                let mut rgb = [0u8; 3];
                stream
                    .read_bytes(&mut rgb)
                    .await
                    .context("Failed to read Tight FILL RGB data")?;
                self.pack_rgb_to_pixel(rgb[0], rgb[1], rgb[2], pixel_format, &mut pixel_data)?;
            } else {
                stream
                    .read_bytes(&mut pixel_data)
                    .await
                    .context("Failed to read Tight FILL pixel data")?;
            }

            // Fill rectangle with solid color
            let fill_rect = Rect::new(
                rect.x as i32,
                rect.y as i32,
                rect.width as u32,
                rect.height as u32,
            );
            buffer
                .fill_rect(fill_rect, &pixel_data)
                .context("Failed to fill Tight FILL rectangle")?;

            let buffer_after = stream.available();
            tracing::debug!(
                target: "rfb_encodings::framing",
                "Tight decode end (FILL): bytes_consumed={}, buffer_after={}",
                buffer_before.saturating_sub(buffer_after),
                buffer_after
            );
            return Ok(());
        }

        // Handle JPEG mode
        if comp_type == TIGHT_JPEG {
            let jpeg_len = Self::read_compact_length(stream).await?;
            let mut jpeg_data = vec![0u8; jpeg_len];
            stream
                .read_bytes(&mut jpeg_data)
                .await
                .with_context(|| format!("Failed to read {} bytes of Tight JPEG data", jpeg_len))?;

            // Decode JPEG
            let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(&jpeg_data));
            let pixels = decoder
                .decode()
                .context("Failed to decode Tight JPEG data")?;
            let metadata = decoder
                .info()
                .ok_or_else(|| anyhow!("JPEG decoder missing metadata"))?;

            // Validate dimensions
            if metadata.width != rect.width || metadata.height != rect.height {
                bail!(
                    "Tight JPEG: dimension mismatch (JPEG {}x{} vs rect {}x{})",
                    metadata.width,
                    metadata.height,
                    rect.width,
                    rect.height
                );
            }

            // Convert RGB to pixel format
            let width = rect.width as usize;
            let height = rect.height as usize;
            let bytes_per_pixel = (pixel_format.bits_per_pixel / 8) as usize;
            let mut converted = vec![0u8; width * height * bytes_per_pixel];

            for y in 0..height {
                for x in 0..width {
                    let src_idx = (y * width + x) * 3;
                    let dst_idx = (y * width + x) * bytes_per_pixel;
                    let r = pixels[src_idx];
                    let g = pixels[src_idx + 1];
                    let b = pixels[src_idx + 2];
                    self.pack_rgb_to_pixel(
                        r,
                        g,
                        b,
                        pixel_format,
                        &mut converted[dst_idx..dst_idx + bytes_per_pixel],
                    )?;
                }
            }

            let dest_rect = Rect::new(rect.x as i32, rect.y as i32, width as u32, height as u32);
            buffer.image_rect(dest_rect, &converted, width)?;

            let buffer_after = stream.available();
            tracing::debug!(
                target: "rfb_encodings::framing",
                "Tight decode end (JPEG): bytes_consumed={}, buffer_after={}",
                buffer_before.saturating_sub(buffer_after),
                buffer_after
            );
            return Ok(());
        }

        // Validate compression type for BASIC mode
        if comp_type > TIGHT_MAX_SUBENCODING {
            bail!(
                "Tight: invalid compression type {} (max {})",
                comp_type,
                TIGHT_MAX_SUBENCODING
            );
        }

        // Handle BASIC mode (zlib-compressed with optional filter)
        let stream_id = (comp_ctl >> 4) & 0x03;
        // Explicit filter is bit 2 of upper nibble (bit 6 of comp_ctl)
        let explicit_filter = (comp_ctl & 0x40) != 0;

        let width = rect.width as usize;
        let height = rect.height as usize;
        let bytes_per_pixel = (pixel_format.bits_per_pixel / 8) as usize;

        // Determine filter and data size
        // For BASIC mode without explicit filter, data is RGB888 (3 bpp)
        let rgb888_implicit = !explicit_filter;  // Track if RGB888 format is implicit
        let (filter_type, data_size, use_palette) = if explicit_filter {
            let filter_id = stream
                .read_u8()
                .await
                .context("Failed to read Tight filter ID")?;

            match filter_id {
                TIGHT_FILTER_COPY => {
                    // RGB888 format (3 bytes/pixel)
                    (filter_id, width * height * 3, false)
                }
                TIGHT_FILTER_PALETTE => {
                    // Palette mode - size determined after reading palette
                    (filter_id, 0, true)
                }
                TIGHT_FILTER_GRADIENT => {
                    // Gradient prediction (3 bytes/pixel RGB888)
                    (filter_id, width * height * 3, false)
                }
                _ => bail!("Tight: invalid filter type {}", filter_id),
            }
        } else {
            // No explicit filter - implicit COPY in RGB888 format (3 bytes/pixel)
            // Tight protocol: BASIC mode without filter bit always uses RGB888,
            // regardless of negotiated pixel format
            let data_size = width * height * 3;  // Always RGB888
            tracing::debug!(
                "Tight BASIC (no filter/RGB888): width={} height={} data_size={}",
                width, height, data_size
            );
            (TIGHT_FILTER_COPY, data_size, false)  // Not palette mode
        };

        // Special handling for palette mode
        if use_palette {
            // For palette, we need to read the palette first to determine data size
            // This is handled inside filter_palette
            // For now, read the data based on palette size

            // Peek palette size to calculate data size
            let pal_size_byte = stream
                .read_u8()
                .await
                .context("Failed to read palette size for data calculation")?;
            let palette_size = (pal_size_byte as usize) + 1;

            // Calculate index data size
            let index_data_size = if palette_size == 2 {
                height * width.div_ceil(8) // 1 bit per pixel, packed
            } else {
                width * height // 1 byte per pixel
            };

            // Account for palette RGB data
            let palette_rgb_size = palette_size * 3;
            let total_data_size = index_data_size;

            // Read or decompress data
            let data = if total_data_size < TIGHT_MIN_TO_COMPRESS {
                let mut d = vec![0u8; total_data_size];
                stream
                    .read_bytes(&mut d)
                    .await
                    .context("Failed to read uncompressed Tight palette index data")?;
                d
            } else {
                let compressed_len = Self::read_compact_length(stream).await?;
                let mut compressed = vec![0u8; compressed_len];
                stream
                    .read_bytes(&mut compressed)
                    .await
                    .context("Failed to read compressed Tight palette index data")?;
                self.decompress_zlib(stream_id as usize, &compressed, total_data_size)?
            };

            // Create a temporary stream with palette size byte + RGB data + index data
            let mut temp_data = vec![pal_size_byte];
            // We need to read the palette RGB data too
            let mut palette_rgb = vec![0u8; palette_rgb_size];
            stream
                .read_bytes(&mut palette_rgb)
                .await
                .context("Failed to read palette RGB")?;
            temp_data.extend_from_slice(&palette_rgb);

            let cursor = Cursor::new(temp_data);
            let mut temp_stream = RfbInStream::new(cursor);

            let result = self
                .filter_palette(&mut temp_stream, rect, pixel_format, buffer, &data)
                .await;

            let buffer_after = stream.available();
            tracing::debug!(
                target: "rfb_encodings::framing",
                "Tight decode end (PALETTE): bytes_consumed={}, buffer_after={}",
                buffer_before.saturating_sub(buffer_after),
                buffer_after
            );
            return result;
        }

        // Read or decompress data
        let data = if data_size < TIGHT_MIN_TO_COMPRESS {
            let mut d = vec![0u8; data_size];
            stream.read_bytes(&mut d).await.with_context(|| {
                format!(
                    "Failed to read {} bytes of uncompressed Tight data",
                    data_size
                )
            })?;
            d
        } else {
            let compressed_len = Self::read_compact_length(stream).await?;
            tracing::debug!(
                "Tight: reading {} compressed bytes for {} uncompressed bytes (stream_id={})",
                compressed_len, data_size, stream_id
            );
            let mut compressed = vec![0u8; compressed_len];
            stream.read_bytes(&mut compressed).await.with_context(|| {
                format!(
                    "Failed to read {} bytes of compressed Tight data",
                    compressed_len
                )
            })?;
            tracing::debug!("Tight: decompressing {} bytes", compressed_len);
            self.decompress_zlib(stream_id as usize, &compressed, data_size)?
        };

        // Apply filter
        match filter_type {
            TIGHT_FILTER_COPY => {
                // RGB888 mode if explicit filter OR implicit (no filter byte in BASIC mode)
                let rgb888_mode = explicit_filter || rgb888_implicit;
                self.filter_copy(&data, rect, pixel_format, buffer, rgb888_mode)?;
            }
            TIGHT_FILTER_GRADIENT => {
                self.filter_gradient(&data, rect, pixel_format, buffer)?;
            }
            _ => bail!("Unexpected filter type {} in non-palette path", filter_type),
        }

        let buffer_after = stream.available();
        tracing::debug!(
            target: "rfb_encodings::framing",
            "Tight decode end (BASIC): filter_type={}, bytes_consumed={}, buffer_after={}",
            filter_type,
            buffer_before.saturating_sub(buffer_after),
            buffer_after
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use rfb_pixelbuffer::ManagedPixelBuffer;
    use std::io::{Cursor, Write};

    fn test_pixel_format() -> PixelFormat {
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

    fn zlib_compress(data: &[u8]) -> Vec<u8> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    #[tokio::test]
    async fn test_tight_decoder_type() {
        let decoder = TightDecoder::default();
        assert_eq!(decoder.encoding_type(), ENCODING_TIGHT);
    }

    #[tokio::test]
    async fn test_empty_rectangle() {
        let decoder = TightDecoder::default();
        let pixel_format = test_pixel_format();
        let buffer_format = rfb_pixelbuffer::PixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(100, 100, buffer_format);

        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            encoding: ENCODING_TIGHT,
        };

        let data: Vec<u8> = vec![];
        let cursor = Cursor::new(data);
        let mut stream = RfbInStream::new(cursor);

        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buffer)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_fill_mode_rgb888() {
        let decoder = TightDecoder::default();
        let pixel_format = test_pixel_format();
        let buffer_format = rfb_pixelbuffer::PixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(100, 100, buffer_format);

        let rect = Rectangle {
            x: 10,
            y: 10,
            width: 5,
            height: 5,
            encoding: ENCODING_TIGHT,
        };

        // FILL mode (0x08 << 4) with RGB888: R=255, G=0, B=0
        let mut data = vec![0x80]; // FILL mode
        data.extend_from_slice(&[255, 0, 0]); // Red in RGB

        let cursor = Cursor::new(data);
        let mut stream = RfbInStream::new(cursor);

        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buffer)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_rectangle_too_wide() {
        let decoder = TightDecoder::default();
        let pixel_format = test_pixel_format();
        let buffer_format = rfb_pixelbuffer::PixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(3000, 100, buffer_format);

        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 2049, // > TIGHT_MAX_WIDTH
            height: 10,
            encoding: ENCODING_TIGHT,
        };

        let data = vec![0x00]; // Doesn't matter, should fail before reading
        let cursor = Cursor::new(data);
        let mut stream = RfbInStream::new(cursor);

        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buffer)
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too wide"));
    }

    #[tokio::test]
    async fn test_invalid_compression_type() {
        let decoder = TightDecoder::default();
        let pixel_format = test_pixel_format();
        let buffer_format = rfb_pixelbuffer::PixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(100, 100, buffer_format);

        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
            encoding: ENCODING_TIGHT,
        };

        // Invalid compression type (0x0A > TIGHT_MAX_SUBENCODING)
        let data = vec![0xA0];
        let cursor = Cursor::new(data);
        let mut stream = RfbInStream::new(cursor);

        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buffer)
            .await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("invalid compression type"));
    }

    #[tokio::test]
    async fn test_compact_length_1_byte() {
        let data = vec![0x7F]; // 127
        let cursor = Cursor::new(data);
        let mut stream = RfbInStream::new(cursor);
        let len = TightDecoder::read_compact_length(&mut stream)
            .await
            .unwrap();
        assert_eq!(len, 127);
    }

    #[tokio::test]
    async fn test_compact_length_2_bytes() {
        let data = vec![0x80, 0x01]; // 128
        let cursor = Cursor::new(data);
        let mut stream = RfbInStream::new(cursor);
        let len = TightDecoder::read_compact_length(&mut stream)
            .await
            .unwrap();
        assert_eq!(len, 128);
    }

    #[tokio::test]
    async fn test_compact_length_3_bytes() {
        let data = vec![0xFF, 0xFF, 0x03]; // 65535
        let cursor = Cursor::new(data);
        let mut stream = RfbInStream::new(cursor);
        let len = TightDecoder::read_compact_length(&mut stream)
            .await
            .unwrap();
        assert_eq!(len, 65535);
    }

    #[tokio::test]
    async fn test_basic_copy_uncompressed() {
        let decoder = TightDecoder::default();
        let pixel_format = test_pixel_format();
        let buffer_format = rfb_pixelbuffer::PixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(100, 100, buffer_format);

        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 2,
            height: 1,
            encoding: ENCODING_TIGHT,
        };

        // Basic mode stream 0 with explicit COPY filter (RGB888), uncompressed
        // 2x1 pixels = 2 pixels * 3 bytes = 6 bytes (< MIN_TO_COMPRESS = 12)
        let mut data = vec![0x04]; // Basic mode stream 0 + EXPLICIT_FILTER flag
        data.push(0x00); // TIGHT_FILTER_COPY
                         // 2 pixels in RGB888 format
        data.extend_from_slice(&[255, 0, 0]); // Red
        data.extend_from_slice(&[0, 255, 0]); // Green

        let cursor = Cursor::new(data);
        let mut stream = RfbInStream::new(cursor);

        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buffer)
            .await;
        if let Err(e) = &result {
            eprintln!("Test error: {:#}", e);
        }
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_zlib_decompression() {
        let decoder = TightDecoder::default();

        let original = b"Hello, Tight encoding with zlib compression!";
        let compressed = zlib_compress(original);

        let decompressed = decoder
            .decompress_zlib(0, &compressed, original.len())
            .unwrap();
        assert_eq!(&decompressed[..], &original[..]);
    }

    #[tokio::test]
    async fn test_stream_reset() {
        let decoder = TightDecoder::default();

        // Initialize stream 0
        let data1 = b"First data";
        let compressed1 = zlib_compress(data1);
        let _result1 = decoder
            .decompress_zlib(0, &compressed1, data1.len())
            .unwrap();

        // Stream 0 now has history
        assert!(decoder.zlib_streams.lock().unwrap()[0].is_some());

        // Reset stream 0 (bit 0 set in lower nibble)
        decoder.zlib_streams.lock().unwrap()[0] = None;
        assert!(decoder.zlib_streams.lock().unwrap()[0].is_none());

        // Decompress new data with fresh stream
        let data2 = b"Second data after reset";
        let compressed2 = zlib_compress(data2);
        let result2 = decoder
            .decompress_zlib(0, &compressed2, data2.len())
            .unwrap();
        assert_eq!(&result2[..], &data2[..]);
    }

    #[tokio::test]
    async fn test_gradient_filter_rejects_8bpp() {
        let decoder = TightDecoder::default();

        let pixel_format_8bpp = PixelFormat {
            bits_per_pixel: 8,
            depth: 8,
            big_endian: 0,
            true_color: 0,
            red_max: 7,
            green_max: 7,
            blue_max: 3,
            red_shift: 0,
            green_shift: 3,
            blue_shift: 6,
        };

        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
            encoding: ENCODING_TIGHT,
        };

        let data = vec![0u8; 2 * 2 * 3];
        let buffer_format = rfb_pixelbuffer::PixelFormat::rgb888();
        let mut buffer = ManagedPixelBuffer::new(100, 100, buffer_format);

        let result = decoder.filter_gradient(&data, &rect, &pixel_format_8bpp, &mut buffer);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("gradient filter requires truecolor"));
    }
}
