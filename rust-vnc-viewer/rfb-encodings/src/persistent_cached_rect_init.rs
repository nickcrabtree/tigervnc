//! Decoder for PersistentCachedRectInit (encoding 103): 64-bit cache ID + actual encoding + pixel data.

use crate::persistent_cache::{PersistentCachedPixels, PersistentClientCache};
use crate::ENCODING_PERSISTENT_CACHED_RECT_INIT;
use crate::{
    CopyRectDecoder, Decoder, HextileDecoder, MutablePixelBuffer, PixelFormat, RREDecoder,
    RawDecoder, Rectangle, RfbInStream, TightDecoder, ZRLEDecoder, ENCODING_COPY_RECT,
    ENCODING_HEXTILE, ENCODING_RAW, ENCODING_RRE, ENCODING_TIGHT, ENCODING_ZRLE,
};
use anyhow::{Context, Result};
use rfb_common::Rect;
use std::sync::{Arc, Mutex};
use tokio::io::AsyncRead;

pub struct PersistentCachedRectInitDecoder {
    cache: Arc<Mutex<PersistentClientCache>>,
    raw: RawDecoder,
    copyrect: CopyRectDecoder,
    rre: RREDecoder,
    hextile: HextileDecoder,
    tight: Arc<TightDecoder>,
    zrle: Arc<ZRLEDecoder>,
}

impl PersistentCachedRectInitDecoder {
    pub fn new(cache: Arc<Mutex<PersistentClientCache>>) -> Self {
        Self::new_with_shared_state(
            cache,
            Arc::new(TightDecoder::default()),
            Arc::new(ZRLEDecoder::default()),
        )
    }

    pub fn new_with_shared_state(
        cache: Arc<Mutex<PersistentClientCache>>,
        tight: Arc<TightDecoder>,
        zrle: Arc<ZRLEDecoder>,
    ) -> Self {
        Self {
            cache,
            raw: RawDecoder,
            copyrect: CopyRectDecoder,
            rre: RREDecoder,
            hextile: HextileDecoder,
            tight,
            zrle,
        }
    }
}

impl Decoder for PersistentCachedRectInitDecoder {
    fn encoding_type(&self) -> i32 {
        ENCODING_PERSISTENT_CACHED_RECT_INIT
    }

    async fn decode<R: AsyncRead + Unpin>(
        &self,
        stream: &mut RfbInStream<R>,
        rect: &Rectangle,
        pixel_format: &PixelFormat,
        buffer: &mut dyn MutablePixelBuffer,
    ) -> Result<()> {
        // PersistentCache INIT wire format:
        //   16-byte CacheKey + inner encoding (legacy/current server)
        // with a possible v2 extension:
        //   16-byte CacheKey + flags + optional canonical PixelFormat + inner encoding.
        //
        // Parse the legacy/current form first. The previous implementation consumed a
        // flags byte unconditionally; when the server omitted that byte, this ate the
        // first byte of the inner encoding and shifted one payload byte into the
        // encoding value (e.g. Tight=7 became 1792).
        let id = stream
            .read_u64()
            .await
            .context("read persistent cache key high")?;
        let _id_low = stream
            .read_u64()
            .await
            .context("read persistent cache key low")?;
        let mut actual = stream
            .read_i32()
            .await
            .context("read persistent actual encoding")?;

        fn is_sane_inner_encoding(enc: i32) -> bool {
            matches!(
                enc,
                ENCODING_RAW
                    | ENCODING_COPY_RECT
                    | ENCODING_RRE
                    | ENCODING_HEXTILE
                    | ENCODING_TIGHT
                    | ENCODING_ZRLE
            )
        }

        if !is_sane_inner_encoding(actual) {
            // Fallback for v2 headers. We have already consumed four bytes after the
            // key; in v2 these are flags plus the first three bytes of either the
            // PixelFormat or the inner encoding. Only the native_format flag is
            // currently meaningful. For flags=0 v2 headers the wire format is
            // ambiguous with legacy Raw; current server runs omit flags unless needed.
            let flags = ((actual as u32) >> 24) as u8;
            if flags & 0xFE != 0 {
                anyhow::bail!("Unsupported PersistentCachedRectInit flags {:#04x}", flags);
            }
            if flags & 0x01 != 0 {
                // Consume the remaining 13 bytes of the canonical 16-byte PixelFormat;
                // the first 3 bytes were already consumed as part of `actual` above.
                for _ in 0..13 {
                    let _ = stream
                        .read_u8()
                        .await
                        .context("read persistent native PixelFormat tail")?;
                }
            }
            actual = stream
                .read_i32()
                .await
                .context("read persistent v2 actual encoding")?;
            if !is_sane_inner_encoding(actual) {
                anyhow::bail!(
                    "Unsupported inner encoding {} for PersistentCachedRectInit",
                    actual
                );
            }
        }

        // Build a Rectangle for inner decoder
        let actual_rect = Rectangle {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
            encoding: actual,
        };

        // Dispatch
        match actual {
            ENCODING_RAW => {
                self.raw
                    .decode(stream, &actual_rect, pixel_format, buffer)
                    .await?
            }
            ENCODING_COPY_RECT => {
                self.copyrect
                    .decode(stream, &actual_rect, pixel_format, buffer)
                    .await?
            }
            ENCODING_RRE => {
                self.rre
                    .decode(stream, &actual_rect, pixel_format, buffer)
                    .await?
            }
            ENCODING_HEXTILE => {
                self.hextile
                    .decode(stream, &actual_rect, pixel_format, buffer)
                    .await?
            }
            ENCODING_TIGHT => {
                self.tight
                    .decode(stream, &actual_rect, pixel_format, buffer)
                    .await?
            }
            ENCODING_ZRLE => {
                self.zrle
                    .decode(stream, &actual_rect, pixel_format, buffer)
                    .await?
            }
            _ => anyhow::bail!(
                "Unsupported inner encoding {} for PersistentCachedRectInit",
                actual
            ),
        }

        // Extract pixels from buffer and store
        let dest_rect = Rect::new(
            rect.x as i32,
            rect.y as i32,
            rect.width as u32,
            rect.height as u32,
        );
        let mut stride_pixels = 0usize;
        if let Some(pixels) = buffer.get_buffer(dest_rect, &mut stride_pixels) {
            let bpp = buffer.pixel_format().bytes_per_pixel() as usize;
            let byte_len = rect.height as usize * stride_pixels * bpp;
            let entry = PersistentCachedPixels {
                id,
                pixels: pixels[..byte_len].to_vec(),
                format: *buffer.pixel_format(),
                width: rect.width as u32,
                height: rect.height as u32,
                stride_pixels,
                last_used: std::time::Instant::now(),
            };
            let mut cache = self
                .cache
                .lock()
                .map_err(|e| anyhow::anyhow!("lock pcache: {}", e))?;
            cache.insert(entry);
            tracing::info!(
                "PersistentCache MISS STORE: rect {}x{} id={:016x}",
                rect.width,
                rect.height,
                &id
            );
        }

        Ok(())
    }
}
