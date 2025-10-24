//! CachedRect decoder - handle cache hits for ContentCache protocol.
//!
//! When the server sends a CachedRect, it's indicating that the client should
//! already have the pixel data in its cache. This decoder looks up the cache_id
//! and blits the cached pixels directly to the framebuffer.
//!
//! # Protocol Flow
//!
//! 1. Server sends Rectangle with encoding = ENCODING_CACHED_RECT
//! 2. This decoder reads the 8-byte cache_id
//! 3. Looks up the cache_id in the ContentCache
//! 4. If **hit**: Blits cached pixels to framebuffer (fast path)
//! 5. If **miss**: Returns error to trigger framebuffer refresh
//!
//! # Performance
//!
//! - **Bandwidth**: 20 bytes total (12-byte header + 8-byte cache_id)
//! - **CPU**: Zero decode cost (memory blit vs decompression)
//! - **Latency**: Sub-millisecond cache lookup vs decode time

use crate::{Decoder, MutablePixelBuffer, PixelFormat, Rectangle, RfbInStream};
use crate::content_cache::ContentCache;
use anyhow::{Context, Result};
use rfb_protocol::messages::cache::CachedRect;
use rfb_protocol::messages::types::ENCODING_CACHED_RECT;
use std::sync::{Arc, Mutex};
use tokio::io::AsyncRead;
use rfb_common::Rect;

/// CachedRect decoder - handles cache hits for ContentCache protocol.
///
/// When the server believes the client has cached pixel data, it sends
/// a CachedRect with only the cache_id. This decoder looks up the cached
/// data and blits it directly to the framebuffer.
pub struct CachedRectDecoder {
    /// Shared cache instance for looking up cached pixels.
    cache: Arc<Mutex<ContentCache>>,
}

impl CachedRectDecoder {
    /// Create a new CachedRect decoder with the given cache.
    ///
    /// The cache should be shared with CachedRectInit decoder and
    /// other components that need cache access.
    pub fn new(cache: Arc<Mutex<ContentCache>>) -> Self {
        Self { cache }
    }

    /// Get a reference to the shared cache.
    ///
    /// Useful for statistics reporting or cache management.
    pub fn cache(&self) -> &Arc<Mutex<ContentCache>> {
        &self.cache
    }
}

impl Decoder for CachedRectDecoder {
    fn encoding_type(&self) -> i32 {
        ENCODING_CACHED_RECT
    }

    async fn decode<R: AsyncRead + Unpin>(
        &self,
        stream: &mut RfbInStream<R>,
        rect: &Rectangle,
        _pixel_format: &PixelFormat,
        buffer: &mut dyn MutablePixelBuffer,
    ) -> Result<()> {
        // Read the CachedRect message (just the cache_id)
        let cached_rect = CachedRect::read_from(stream)
            .await
            .context("Failed to read CachedRect from stream")?;

        // Look up the cached pixels and clone them to avoid borrowing issues
        let cache_hit = {
            let mut cache = self.cache
                .lock()
                .map_err(|e| anyhow::anyhow!("Failed to lock ContentCache: {}", e))?;
            
            cache.lookup(cached_rect.cache_id).cloned()
        };

        match cache_hit {
            Some(cached_pixels) => {
                // Cache hit! Blit the cached pixels to the framebuffer
                let dest_rect = Rect::new(
                    rect.x as i32,
                    rect.y as i32,
                    rect.width as u32,
                    rect.height as u32,
                );

                // Verify dimensions match what the server claims
                if cached_pixels.width != rect.width as u32 || cached_pixels.height != rect.height as u32 {
                    anyhow::bail!(
                        "Cached pixel dimensions {}x{} don't match rectangle {}x{}",
                        cached_pixels.width, cached_pixels.height,
                        rect.width, rect.height
                    );
                }

                // Blit cached pixels to framebuffer
                buffer.image_rect(dest_rect, &cached_pixels.pixels, cached_pixels.stride)
                    .with_context(|| {
                        format!(
                            "Failed to blit cached pixels (cache_id={}) to framebuffer at {:?}",
                            cached_rect.cache_id, dest_rect
                        )
                    })?;

                tracing::debug!(
                    "ContentCache HIT: cache_id={}, rect={}x{} at ({},{}), {} bytes → framebuffer",
                    cached_rect.cache_id,
                    rect.width, rect.height,
                    rect.x, rect.y,
                    cached_pixels.pixels.len()
                );

                Ok(())
            }
            None => {
                // Cache miss! The client needs to request a refresh
                tracing::warn!(
                    "ContentCache MISS: cache_id={} not found for rect {}x{} at ({},{})",
                    cached_rect.cache_id,
                    rect.width, rect.height,
                    rect.x, rect.y
                );

                // Return an error to signal that the client should request a refresh
                anyhow::bail!(
                    "Cache miss for cache_id {}: rectangle {}x{} at ({},{}) not in cache",
                    cached_rect.cache_id, rect.width, rect.height, rect.x, rect.y
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content_cache::{ContentCache, CachedPixels};
    use rfb_pixelbuffer::{PixelFormat, ManagedPixelBuffer};
    use rfb_protocol::io::RfbOutStream;
    use std::io::Cursor;

    #[tokio::test]
    async fn test_cached_rect_decoder_hit() {
        // Create cache and populate with test data
        let cache = Arc::new(Mutex::new(ContentCache::new(100))); // 100MB limit
        
        let test_cache_id = 12345u64;
        let test_pixels: Vec<u8> = (0..64 * 64).flat_map(|_| vec![0xFF, 0x00, 0x00, 0xFF]).collect(); // Red pixels
        let cached_pixels = CachedPixels::new(
            test_cache_id,
            test_pixels.clone(),
            PixelFormat::rgb888(),
            64, 64, 64
        );
        
        {
            let mut c = cache.lock().unwrap();
            c.insert(test_cache_id, cached_pixels).unwrap();
        }

        // Create decoder
        let decoder = CachedRectDecoder::new(cache.clone());

        // Create test buffer
        let mut buffer = ManagedPixelBuffer::new(1024, 768, PixelFormat::rgb888());

        // Create stream with CachedRect data
        let cached_rect = CachedRect::new(test_cache_id);
        let mut stream_data = Vec::new();
        let mut out_stream = RfbOutStream::new(&mut stream_data);
        cached_rect.write_to(&mut out_stream).unwrap();
        let mut stream = RfbInStream::new(Cursor::new(stream_data));

        // Create rectangle
        let rect = Rectangle {
            x: 100,
            y: 100,
            width: 64,
            height: 64,
            encoding: ENCODING_CACHED_RECT,
        };

        // Decode should succeed (cache hit)
        let result = decoder.decode(&mut stream, &rect, &PixelFormat::rgb888(), &mut buffer).await;
        assert!(result.is_ok());

        // Verify cache statistics
        let stats = {
            let c = cache.lock().unwrap();
            c.stats()
        };
        assert_eq!(stats.hit_count, 1);
        assert_eq!(stats.miss_count, 0);
    }

    #[tokio::test]
    async fn test_cached_rect_decoder_miss() {
        // Create empty cache
        let cache = Arc::new(Mutex::new(ContentCache::new(100)));
        let decoder = CachedRectDecoder::new(cache.clone());

        // Create test buffer
        let mut buffer = ManagedPixelBuffer::new(1024, 768, PixelFormat::rgb888());

        // Create stream with CachedRect data for non-existent cache_id
        let missing_cache_id = 99999u64;
        let cached_rect = CachedRect::new(missing_cache_id);
        let mut stream_data = Vec::new();
        let mut out_stream = RfbOutStream::new(&mut stream_data);
        cached_rect.write_to(&mut out_stream).unwrap();
        let mut stream = RfbInStream::new(Cursor::new(stream_data));

        // Create rectangle
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 32,
            height: 32,
            encoding: ENCODING_CACHED_RECT,
        };

        // Decode should fail (cache miss)
        let result = decoder.decode(&mut stream, &rect, &PixelFormat::rgb888(), &mut buffer).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Cache miss"));

        // Verify cache statistics
        let stats = {
            let c = cache.lock().unwrap();
            c.stats()
        };
        assert_eq!(stats.hit_count, 0);
        assert_eq!(stats.miss_count, 1);
    }

    #[tokio::test]
    async fn test_cached_rect_decoder_dimension_mismatch() {
        // Create cache with 64x64 pixel data
        let cache = Arc::new(Mutex::new(ContentCache::new(100)));
        
        let test_cache_id = 54321u64;
        let test_pixels: Vec<u8> = (0..64 * 64).flat_map(|_| vec![0x00, 0xFF, 0x00, 0xFF]).collect(); // Green pixels
        let cached_pixels = CachedPixels::new(
            test_cache_id,
            test_pixels,
            PixelFormat::rgb888(),
            64, 64, 64
        );
        
        {
            let mut c = cache.lock().unwrap();
            c.insert(test_cache_id, cached_pixels).unwrap();
        }

        let decoder = CachedRectDecoder::new(cache);
        let mut buffer = ManagedPixelBuffer::new(1024, 768, PixelFormat::rgb888());

        // Create stream with CachedRect data
        let cached_rect = CachedRect::new(test_cache_id);
        let mut stream_data = Vec::new();
        let mut out_stream = RfbOutStream::new(&mut stream_data);
        cached_rect.write_to(&mut out_stream).unwrap();
        let mut stream = RfbInStream::new(Cursor::new(stream_data));

        // Create rectangle with DIFFERENT dimensions than cached data
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 32,  // Cached data is 64x64, but rectangle claims 32x32
            height: 32,
            encoding: ENCODING_CACHED_RECT,
        };

        // Decode should fail (dimension mismatch)
        let result = decoder.decode(&mut stream, &rect, &PixelFormat::rgb888(), &mut buffer).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("don't match"));
    }

    #[test]
    fn test_cached_rect_decoder_encoding_type() {
        let cache = Arc::new(Mutex::new(ContentCache::new(1)));
        let decoder = CachedRectDecoder::new(cache);
        assert_eq!(decoder.encoding_type(), ENCODING_CACHED_RECT);
    }
}