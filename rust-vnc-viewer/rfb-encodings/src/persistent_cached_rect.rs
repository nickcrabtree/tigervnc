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
