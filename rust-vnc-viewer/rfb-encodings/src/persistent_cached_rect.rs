//! Decoder for PersistentCachedRect (encoding 102): reference by 16-byte hash.

use crate::persistent_cache::PersistentClientCache;
use crate::ENCODING_PERSISTENT_CACHED_RECT;
use crate::{Decoder, MutablePixelBuffer, PixelFormat, Rectangle, RfbInStream};
use anyhow::{Context, Result};
use rfb_common::Rect;
use std::sync::{Arc, Mutex};
use tokio::io::AsyncRead;

pub struct PersistentCachedRectDecoder {
    cache: Arc<Mutex<PersistentClientCache>>,
    /// Optional reporter queue to record hashes that missed during decode.
    pending_misses: Option<Arc<Mutex<Vec<[u8; 16]>>>>,
}

impl PersistentCachedRectDecoder {
    pub fn new(cache: Arc<Mutex<PersistentClientCache>>) -> Self {
        Self {
            cache,
            pending_misses: None,
        }
    }

    pub fn new_with_miss_reporter(
        cache: Arc<Mutex<PersistentClientCache>>,
        misses: Arc<Mutex<Vec<[u8; 16]>>>,
    ) -> Self {
        Self {
            cache,
            pending_misses: Some(misses),
        }
    }
}

impl Decoder for PersistentCachedRectDecoder {
    fn encoding_type(&self) -> i32 {
        ENCODING_PERSISTENT_CACHED_RECT
    }

    async fn decode<R: AsyncRead + Unpin>(
        &self,
        stream: &mut RfbInStream<R>,
        rect: &Rectangle,
        _pixel_format: &PixelFormat,
        buffer: &mut dyn MutablePixelBuffer,
    ) -> Result<()> {
        // Read 16-byte cache ID
        let mut id = [0u8; 16];
        stream
            .read_bytes(&mut id)
            .await
            .context("read persistent cache id")?;
        // Offset extension (encoding 102): U16 ox/oy + U16 cachedW/cachedH (network byte order).
        let ox = stream
            .read_u16()
            .await
            .context("read persistent cache ox")?;
        let oy = stream
            .read_u16()
            .await
            .context("read persistent cache oy")?;
        let cached_w = stream
            .read_u16()
            .await
            .context("read persistent cache cachedW")?;
        let cached_h = stream
            .read_u16()
            .await
            .context("read persistent cache cachedH")?;

        // Lookup
        let hit = {
            let mut cache = self
                .cache
                .lock()
                .map_err(|e| anyhow::anyhow!("lock pcache: {}", e))?;
            cache.lookup(&id).cloned()
        };

        if let Some(entry) = hit {
            // Validate cached payload is compatible with destination rectangle.
            // If not, treat as a cache miss (enqueue id) rather than returning a decode error.
            let bpp = entry.format.bytes_per_pixel() as usize;
            let ox = ox as usize;
            let oy = oy as usize;
            let cached_w = cached_w as usize;
            let cached_h = cached_h as usize;
            let need_w = rect.width as usize;
            let need_h = rect.height as usize;
            let req_w = ox.saturating_add(need_w);
            let req_h = oy.saturating_add(need_h);
            let need_bytes = req_h
                .saturating_mul(entry.stride_pixels)
                .saturating_mul(bpp);
            let incompatible = entry.width != cached_w as u32
                || entry.height != cached_h as u32
                || cached_w < req_w
                || cached_h < req_h
                || entry.stride_pixels < req_w
                || entry.format != *buffer.pixel_format()
                || entry.pixels.len() < need_bytes;
            if incompatible {
                tracing::warn!(
                "PersistentCache HIT but incompatible payload: entry={}x{} stride={} bytes={} need={}x{} stride>={} bytes>={} id={:02x?}",
                entry.width,
                entry.height,
                entry.stride_pixels,
                entry.pixels.len(),
                rect.width,
                rect.height,
                need_w,
                need_bytes,
                &id
            );
                if let Some(m) = &self.pending_misses {
                    if let Ok(mut v) = m.lock() {
                        v.push(id);
                    }
                }
                return Ok(());
            }

            // Blit
            let dest_rect = Rect::new(
                rect.x as i32,
                rect.y as i32,
                rect.width as u32,
                rect.height as u32,
            );
            if ox == 0 && oy == 0 {
                buffer
                    .image_rect(dest_rect, &entry.pixels, entry.stride_pixels)
                    .context("blit persistent cache hit")?;
            } else {
                let row_bytes = need_w.saturating_mul(bpp);
                let mut tmp = vec![0u8; need_h.saturating_mul(row_bytes)];
                for row in 0..need_h {
                    let src_off = (oy.saturating_add(row))
                        .saturating_mul(entry.stride_pixels)
                        .saturating_add(ox)
                        .saturating_mul(bpp);
                    let dst_off = row.saturating_mul(row_bytes);
                    tmp[dst_off..dst_off + row_bytes]
                        .copy_from_slice(&entry.pixels[src_off..src_off + row_bytes]);
                }
                buffer
                    .image_rect(dest_rect, &tmp, need_w)
                    .context("blit persistent cache hit (offset)")?;
            }
            tracing::info!(
                "PersistentCache HIT: rect {}x{} id={:02x?}",
                rect.width,
                rect.height,
                &id
            );
            Ok(())
        } else {
            tracing::warn!(
                "PersistentCache MISS: rect {}x{} id={:02x?}",
                rect.width,
                rect.height,
                &id
            );
            if let Some(m) = &self.pending_misses {
                if let Ok(mut v) = m.lock() {
                    v.push(id);
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistent_cache::PersistentCachedPixels;
    use rfb_pixelbuffer::{ManagedPixelBuffer, PixelBuffer, PixelFormat as LocalPixelFormat};
    use std::io::Cursor;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    // RED test (TDD): expected to fail until size-mismatch handling is implemented.
    // Desired behaviour: a cache entry whose stored dimensions/stride are incompatible
    // with the destination rectangle should be treated as a cache miss (enqueue id)
    // rather than returning a decode error.
    #[tokio::test]
    async fn persistent_cached_rect_size_mismatch_is_treated_as_miss() {
        let id = [0xABu8; 16];

        // Create a cache with an entry that is intentionally too small for the target rect.
        // Stored: 2x2 RGB888 (stride 2). Target rect: 4x4.
        let format = LocalPixelFormat::rgb888();
        let entry = PersistentCachedPixels {
            id,
            pixels: vec![0x11u8; 2 * 2 * 3],
            format,
            width: 2,
            height: 2,
            stride_pixels: 2,
            last_used: Instant::now(),
        };

        let mut pc = PersistentClientCache::new(10);
        pc.insert(entry);
        let pc = Arc::new(Mutex::new(pc));

        let misses: Arc<Mutex<Vec<[u8; 16]>>> = Arc::new(Mutex::new(Vec::new()));
        let decoder = PersistentCachedRectDecoder::new_with_miss_reporter(pc, misses.clone());

        // Stream contains only the 16-byte id plus the encoding-102 offset extension.
        let mut payload = Vec::new();
        payload.extend_from_slice(&id);
        payload.extend_from_slice(&0u16.to_be_bytes());
        payload.extend_from_slice(&0u16.to_be_bytes());
        payload.extend_from_slice(&2u16.to_be_bytes());
        payload.extend_from_slice(&2u16.to_be_bytes());
        let mut stream = RfbInStream::new(Cursor::new(payload));

        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
            encoding: ENCODING_PERSISTENT_CACHED_RECT,
        };

        // Destination buffer is 4x4 RGB888.
        let mut buf = ManagedPixelBuffer::new(4, 4, LocalPixelFormat::rgb888());
        let pixel_format = PixelFormat {
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

        // Expected (future): Ok and miss recorded.
        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buf)
            .await;
        assert!(
            result.is_ok(),
            "expected size mismatch to be treated as miss, got: {:?}",
            result
        );

        let v = misses.lock().unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0], id);
    }

    // RED test (TDD): pixel format mismatch on a cache hit should be treated as a miss.
    // Desired behaviour: if cached pixels are not in the buffer's pixel format, the
    // decoder should enqueue the id as a miss and return Ok(()), rather than erroring.
    #[tokio::test]
    async fn persistent_cached_rect_pixel_format_mismatch_is_treated_as_miss() {
        let id = [0xCDu8; 16];

        // Entry is 4x4 but uses RGB565 (2 bytes/pixel) while the destination buffer is RGB888.
        let entry_format = LocalPixelFormat {
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
        let entry = PersistentCachedPixels {
            id,
            pixels: vec![0x22u8; 4 * 4 * 2],
            format: entry_format,
            width: 4,
            height: 4,
            stride_pixels: 4,
            last_used: Instant::now(),
        };

        let mut pc = PersistentClientCache::new(10);
        pc.insert(entry);
        let pc = Arc::new(Mutex::new(pc));

        let misses: Arc<Mutex<Vec<[u8; 16]>>> = Arc::new(Mutex::new(Vec::new()));
        let decoder = PersistentCachedRectDecoder::new_with_miss_reporter(pc, misses.clone());

        let mut payload = Vec::new();
        payload.extend_from_slice(&id);
        payload.extend_from_slice(&0u16.to_be_bytes());
        payload.extend_from_slice(&0u16.to_be_bytes());
        payload.extend_from_slice(&4u16.to_be_bytes());
        payload.extend_from_slice(&4u16.to_be_bytes());
        let mut stream = RfbInStream::new(Cursor::new(payload));
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
            encoding: ENCODING_PERSISTENT_CACHED_RECT,
        };

        let mut buf = ManagedPixelBuffer::new(4, 4, LocalPixelFormat::rgb888());
        let pixel_format = PixelFormat {
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

        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buf)
            .await;
        assert!(
            result.is_ok(),
            "expected pixel format mismatch to be treated as miss, got: {:?}",
            result
        );

        let v = misses.lock().unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0], id);
    }

    // RED test (TDD): PersistentCachedRect offset extension should blit a sub-rectangle.
    // Wire format (per C++): 16-byte CacheKey + U16 ox + U16 oy + U16 cachedW + U16 cachedH.
    #[tokio::test]
    async fn persistent_cached_rect_with_offset_blits_subrect() {
        use rfb_common::Rect;

        let id = [0xEEu8; 16];

        // Cached entry is 4x4 in RGB888. Fill each pixel with a marker value v=y*16+x.
        let format = LocalPixelFormat::rgb888();
        let bpp = format.bytes_per_pixel() as usize;

        let mut pixels: Vec<u8> = Vec::with_capacity(4 * 4 * bpp);
        for y in 0..4u8 {
            for x in 0..4u8 {
                let v = y.wrapping_mul(16).wrapping_add(x);
                for _ in 0..bpp {
                    pixels.push(v);
                }
            }
        }

        let entry = PersistentCachedPixels {
            id,
            pixels,
            format,
            width: 4,
            height: 4,
            stride_pixels: 4,
            last_used: Instant::now(),
        };

        let mut pc = PersistentClientCache::new(10);
        pc.insert(entry);
        let pc = Arc::new(Mutex::new(pc));

        let misses: Arc<Mutex<Vec<[u8; 16]>>> = Arc::new(Mutex::new(Vec::new()));
        let decoder = PersistentCachedRectDecoder::new_with_miss_reporter(pc, misses.clone());

        // Build stream: id + ox/oy + cachedW/cachedH (U16 big-endian).
        let ox: u16 = 1;
        let oy: u16 = 1;
        let cached_w: u16 = 4;
        let cached_h: u16 = 4;
        let mut payload = Vec::new();
        payload.extend_from_slice(&id);
        payload.extend_from_slice(&ox.to_be_bytes());
        payload.extend_from_slice(&oy.to_be_bytes());
        payload.extend_from_slice(&cached_w.to_be_bytes());
        payload.extend_from_slice(&cached_h.to_be_bytes());

        let mut stream = RfbInStream::new(Cursor::new(payload));

        // Request a 2x2 rect; expected source is cached pixels at (1,1).
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
            encoding: ENCODING_PERSISTENT_CACHED_RECT,
        };

        let mut buf = ManagedPixelBuffer::new(2, 2, LocalPixelFormat::rgb888());
        let pixel_format = PixelFormat {
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

        let result = decoder
            .decode(&mut stream, &rect, &pixel_format, &mut buf)
            .await;
        assert!(
            result.is_ok(),
            "expected offset hit to decode ok, got: {:?}",
            result
        );

        // Verify output pixels match cached source at (1,1): 17,18 / 33,34.
        let out_rect = Rect::new(0, 0, 2, 2);
        let mut out_stride = 0usize;
        let out = buf
            .get_buffer(out_rect, &mut out_stride)
            .expect("get_buffer");
        let out_bpp = buf.pixel_format().bytes_per_pixel() as usize;
        let at = |row: usize, col: usize| -> u8 { out[row * out_stride * out_bpp + col * out_bpp] };
        assert_eq!(at(0, 0), 17);
        assert_eq!(at(0, 1), 18);
        assert_eq!(at(1, 0), 33);
        assert_eq!(at(1, 1), 34);

        // Should not record a miss on a valid offset hit.
        assert!(misses.lock().unwrap().is_empty());
    }
}
