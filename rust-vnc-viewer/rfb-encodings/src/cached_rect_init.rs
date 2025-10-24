//! CachedRectInit decoder - handle cache misses and store new content.
//!
//! When the server sends a CachedRectInit, it's transmitting new content that
//! should be cached for future reference. This decoder reads the cache_id and
//! actual_encoding, dispatches to the appropriate decoder for the pixel data,
//! then stores the decoded result in the cache.
//!
//! # Protocol Flow
//!
//! 1. Server sends Rectangle with encoding = ENCODING_CACHED_RECT_INIT
//! 2. This decoder reads cache_id (8 bytes) and actual_encoding (4 bytes)
//! 3. Dispatches to appropriate decoder based on actual_encoding
//! 4. Stores decoded pixels in cache under cache_id
//! 5. Blits decoded pixels to framebuffer
//!
//! # Caching Strategy
//!
//! - Only cache content that's likely to repeat (based on size thresholds)
//! - Use content hash as cache_id to ensure addressability
//! - Store in local RGB888 format for fast blitting regardless of server format

use crate::{
    Decoder, MutablePixelBuffer, PixelFormat, Rectangle, RfbInStream,
    RawDecoder, CopyRectDecoder, RREDecoder, HextileDecoder, TightDecoder, ZRLEDecoder,
    ENCODING_RAW, ENCODING_COPY_RECT, ENCODING_RRE, ENCODING_HEXTILE, ENCODING_TIGHT, ENCODING_ZRLE,
};
use crate::content_cache::{ContentCache, CachedPixels};
use anyhow::{Context, Result};
use rfb_protocol::messages::cache::CachedRectInit;
use rfb_protocol::messages::types::ENCODING_CACHED_RECT_INIT;
use std::sync::{Arc, Mutex};
use tokio::io::AsyncRead;
use rfb_common::Rect;

/// CachedRectInit decoder - handles cache misses and stores new content.
///
/// When the server sends new content with a cache ID, this decoder:
/// 1. Reads the cache_id and actual_encoding
/// 2. Dispatches to the appropriate decoder for the pixel data
/// 3. Stores the decoded result in the cache for future reference
/// 4. Blits the pixels to the framebuffer
pub struct CachedRectInitDecoder {
    /// Shared cache instance for storing decoded pixels.
    cache: Arc<Mutex<ContentCache>>,
    
    /// Raw decoder for ENCODING_RAW.
    raw_decoder: RawDecoder,
    
    /// CopyRect decoder for ENCODING_COPY_RECT.
    copyrect_decoder: CopyRectDecoder,
    
    /// RRE decoder for ENCODING_RRE.
    rre_decoder: RREDecoder,
    
    /// Hextile decoder for ENCODING_HEXTILE.
    hextile_decoder: HextileDecoder,
    
    /// Tight decoder for ENCODING_TIGHT.
    tight_decoder: TightDecoder,
    
    /// ZRLE decoder for ENCODING_ZRLE.
    zrle_decoder: ZRLEDecoder,
}

impl CachedRectInitDecoder {
    /// Create a new CachedRectInit decoder with the given cache.
    ///
    /// The cache should be shared with CachedRect decoder and other
    /// components that need cache access.
    pub fn new(cache: Arc<Mutex<ContentCache>>) -> Self {
        Self {
            cache,
            raw_decoder: RawDecoder,
            copyrect_decoder: CopyRectDecoder,
            rre_decoder: RREDecoder,
            hextile_decoder: HextileDecoder,
            tight_decoder: TightDecoder::default(),
            zrle_decoder: ZRLEDecoder::default(),
        }
    }

    /// Get a reference to the shared cache.
    ///
    /// Useful for statistics reporting or cache management.
    pub fn cache(&self) -> &Arc<Mutex<ContentCache>> {
        &self.cache
    }
    
    /// Dispatch to the appropriate decoder based on encoding type.
    async fn decode_with_actual_encoding<R: AsyncRead + Unpin>(
        &self,
        stream: &mut RfbInStream<R>,
        rect: &Rectangle,
        actual_encoding: i32,
        pixel_format: &PixelFormat,
        buffer: &mut dyn MutablePixelBuffer,
    ) -> Result<()> {
        match actual_encoding {
            ENCODING_RAW => {
                self.raw_decoder.decode(stream, rect, pixel_format, buffer).await
            }
            ENCODING_COPY_RECT => {
                self.copyrect_decoder.decode(stream, rect, pixel_format, buffer).await
            }
            ENCODING_RRE => {
                self.rre_decoder.decode(stream, rect, pixel_format, buffer).await
            }
            ENCODING_HEXTILE => {
                self.hextile_decoder.decode(stream, rect, pixel_format, buffer).await
            }
            ENCODING_TIGHT => {
                self.tight_decoder.decode(stream, rect, pixel_format, buffer).await
            }
            ENCODING_ZRLE => {
                self.zrle_decoder.decode(stream, rect, pixel_format, buffer).await
            }
            _ => {
                anyhow::bail!(
                    "Unsupported actual_encoding {} in CachedRectInit for rect {}x{} at ({},{})",
                    actual_encoding, rect.width, rect.height, rect.x, rect.y
                )
            }
        }
    }
}

impl Decoder for CachedRectInitDecoder {
    fn encoding_type(&self) -> i32 {
        ENCODING_CACHED_RECT_INIT
    }

    async fn decode<R: AsyncRead + Unpin>(
        &self,
        stream: &mut RfbInStream<R>,
        rect: &Rectangle,
        pixel_format: &PixelFormat,
        buffer: &mut dyn MutablePixelBuffer,
    ) -> Result<()> {
        // Read the CachedRectInit header (cache_id + actual_encoding)
        let cached_rect_init = CachedRectInit::read_from(stream)
            .await
            .context("Failed to read CachedRectInit from stream")?;

        tracing::debug!(
            "CachedRectInit: cache_id={}, actual_encoding={}, rect={}x{} at ({},{})",
            cached_rect_init.cache_id,
            cached_rect_init.actual_encoding,
            rect.width, rect.height,
            rect.x, rect.y
        );

        // Create a temporary buffer to decode into and then extract pixels for caching
        // We decode into the main buffer first, then extract the pixels
        let dest_rect = Rect::new(
            rect.x as i32,
            rect.y as i32,
            rect.width as u32,
            rect.height as u32,
        );

        // Decode the pixel data using the appropriate decoder
        // We create a modified Rectangle with the actual encoding for the nested decoder
        let actual_rect = Rectangle {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
            encoding: cached_rect_init.actual_encoding,
        };

        self.decode_with_actual_encoding(
            stream,
            &actual_rect,
            cached_rect_init.actual_encoding,
            pixel_format,
            buffer,
        )
        .await
        .with_context(|| {
            format!(
                "Failed to decode actual_encoding {} for CachedRectInit cache_id={}",
                cached_rect_init.actual_encoding, cached_rect_init.cache_id
            )
        })?;

        // Extract the decoded pixels from the buffer to store in cache
        let mut stride = 0;
        if let Some(pixels) = buffer.get_buffer(dest_rect, &mut stride) {
            // Calculate the bytes we need (remember: stride is in pixels, not bytes!)
            let format = buffer.pixel_format();
            let bytes_per_pixel = format.bytes_per_pixel() as usize;
            let pixel_data_len = rect.height as usize * stride * bytes_per_pixel;
            
            // Create cached pixel entry
            let cached_pixels = CachedPixels::new(
                cached_rect_init.cache_id,
                pixels[..pixel_data_len].to_vec(),
                format.clone(),
                rect.width as u32,
                rect.height as u32,
                stride,
            );

            // Store in cache
            {
                let mut cache = self.cache
                    .lock()
                    .map_err(|e| anyhow::anyhow!("Failed to lock ContentCache: {}", e))?;
                
                cache.insert(cached_rect_init.cache_id, cached_pixels)
                    .with_context(|| {
                        format!("Failed to store cache_id {} in ContentCache", cached_rect_init.cache_id)
                    })?;
            }

            tracing::debug!(
                "ContentCache STORE: cache_id={}, {} bytes stored for rect {}x{} at ({},{})",
                cached_rect_init.cache_id,
                pixel_data_len,
                rect.width, rect.height,
                rect.x, rect.y
            );
        } else {
            tracing::warn!(
                "Failed to extract pixels from buffer for caching, cache_id={} will not be stored",
                cached_rect_init.cache_id
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content_cache::ContentCache;
    use rfb_pixelbuffer::{PixelFormat, ManagedPixelBuffer};
    use rfb_protocol::io::RfbOutStream;
    use std::io::Cursor;

    #[tokio::test]
    async fn test_cached_rect_init_decoder_raw() {
        let cache = Arc::new(Mutex::new(ContentCache::new(100)));
        let decoder = CachedRectInitDecoder::new(cache.clone());

        let mut buffer = ManagedPixelBuffer::new(1024, 768, PixelFormat::rgb888());

        // Create test data: CachedRectInit header + Raw pixel data
        let test_cache_id = 98765u64;
        let cached_rect_init = CachedRectInit::new(test_cache_id, ENCODING_RAW);
        
        // Raw pixel data: 2x2 red pixels (4 pixels * 4 bytes each = 16 bytes)
        let raw_pixels = vec![
            0xFF, 0x00, 0x00, 0xFF,  // Red pixel 1
            0xFF, 0x00, 0x00, 0xFF,  // Red pixel 2
            0xFF, 0x00, 0x00, 0xFF,  // Red pixel 3
            0xFF, 0x00, 0x00, 0xFF,  // Red pixel 4
        ];

        // Create stream data
        let mut stream_data = Vec::new();
        let mut out_stream = RfbOutStream::new(&mut stream_data);
        cached_rect_init.write_to(&mut out_stream).unwrap();
        stream_data.extend_from_slice(&raw_pixels);
        
        let mut stream = RfbInStream::new(Cursor::new(stream_data));

        // Create rectangle
        let rect = Rectangle {
            x: 50,
            y: 50,
            width: 2,
            height: 2,
            encoding: ENCODING_CACHED_RECT_INIT,
        };

        // Decode should succeed
        let wire_format = crate::PixelFormat {
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
        let result = decoder.decode(
            &mut stream,
            &rect,
            &wire_format,
            &mut buffer,
        ).await;
        assert!(result.is_ok());

        // Verify that data was stored in cache
        let stats = {
            let c = cache.lock().unwrap();
            c.stats()
        };
        assert_eq!(stats.entries, 1);

        // Verify we can look up the cached data
        let cached_pixels = {
            let mut c = cache.lock().unwrap();
            c.lookup(test_cache_id).map(|cp| (cp.width, cp.height, cp.cache_id))
        };
        assert!(cached_pixels.is_some());
        let (width, height, cache_id) = cached_pixels.unwrap();
        assert_eq!(width, 2);
        assert_eq!(height, 2);
        assert_eq!(cache_id, test_cache_id);
    }

    #[tokio::test]
    async fn test_cached_rect_init_decoder_unsupported_encoding() {
        let cache = Arc::new(Mutex::new(ContentCache::new(100)));
        let decoder = CachedRectInitDecoder::new(cache);
        let mut buffer = ManagedPixelBuffer::new(1024, 768, PixelFormat::rgb888());

        // Create CachedRectInit with unsupported encoding
        let cached_rect_init = CachedRectInit::new(11111, 999); // Invalid encoding
        
        let mut stream_data = Vec::new();
        let mut out_stream = RfbOutStream::new(&mut stream_data);
        cached_rect_init.write_to(&mut out_stream).unwrap();
        // No pixel data needed since this should fail early
        
        let mut stream = RfbInStream::new(Cursor::new(stream_data));

        let rect = Rectangle {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
            encoding: ENCODING_CACHED_RECT_INIT,
        };

        // Decode should fail
        let wire_format = crate::PixelFormat {
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
        let result = decoder.decode(&mut stream, &rect, &wire_format, &mut buffer).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unsupported actual_encoding"));
    }

    #[test]
    fn test_cached_rect_init_decoder_encoding_type() {
        let cache = Arc::new(Mutex::new(ContentCache::new(1)));
        let decoder = CachedRectInitDecoder::new(cache);
        assert_eq!(decoder.encoding_type(), ENCODING_CACHED_RECT_INIT);
    }
}