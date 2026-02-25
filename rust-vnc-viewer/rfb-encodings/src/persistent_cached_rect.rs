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
        Self { cache, pending_misses: None }
    }

    pub fn new_with_miss_reporter(
        cache: Arc<Mutex<PersistentClientCache>>,
        misses: Arc<Mutex<Vec<[u8; 16]>>>,
    ) -> Self {
        Self { cache, pending_misses: Some(misses) }
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
        let need_w = rect.width as usize;
        let need_h = rect.height as usize;
        let need_bytes = need_h
            .saturating_mul(entry.stride_pixels)
            .saturating_mul(bpp);
        let incompatible = entry.width < rect.width as u32
            || entry.height < rect.height as u32
            || entry.stride_pixels < need_w
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
            buffer
                .image_rect(dest_rect, &entry.pixels, entry.stride_pixels)
                .context("blit persistent cache hit")?;
            tracing::info!(
                "PersistentCache HIT: rect {}x{} id={:02x?}",
                rect.width,
                rect.height,
                &id
            );
            Ok(())
        } else {
            tracing::warn!("PersistentCache MISS: rect {}x{} id={:02x?}", rect.width, rect.height, &id);
            if let Some(m) = &self.pending_misses {
                if let Ok(mut v) = m.lock() { v.push(id); }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistent_cache::PersistentCachedPixels;
    use rfb_pixelbuffer::{ManagedPixelBuffer, PixelFormat as LocalPixelFormat};
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

        // Stream contains only the 16-byte id.
        let mut stream = RfbInStream::new(Cursor::new(id.to_vec()));

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
        assert!(result.is_ok(), "expected size mismatch to be treated as miss, got: {:?}", result);

        let v = misses.lock().unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0], id);
    }
}
