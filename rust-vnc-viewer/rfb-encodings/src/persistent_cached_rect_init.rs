//! Decoder for PersistentCachedRectInit (encoding 103): hash + actual encoding + pixel data.

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
    tight: TightDecoder,
    zrle: ZRLEDecoder,
}

impl PersistentCachedRectInitDecoder {
    pub fn new(cache: Arc<Mutex<PersistentClientCache>>) -> Self {
        Self {
            cache,
            raw: RawDecoder,
            copyrect: CopyRectDecoder,
            rre: RREDecoder,
            hextile: HextileDecoder,
            tight: TightDecoder::default(),
            zrle: ZRLEDecoder::default(),
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
        // Read 16-byte id + actual encoding (i32)
        let mut id = [0u8; 16];
        stream
            .read_bytes(&mut id)
            .await
            .context("read persistent cache id")?;
        let actual = stream
            .read_i32()
            .await
            .context("read persistent actual encoding")?;

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
                "PersistentCache STORE: rect {}x{} id={:02x?}",
                rect.width,
                rect.height,
                &id
            );
        }

        Ok(())
    }
}
