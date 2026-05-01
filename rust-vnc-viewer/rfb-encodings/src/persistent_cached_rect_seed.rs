//! Decoder for CachedRectSeed (encoding 105): seed PersistentCache from framebuffer pixels.
use crate::persistent_cache::{PersistentCachedPixels, PersistentClientCache};
use crate::{
    Decoder, MutablePixelBuffer, PixelFormat, Rectangle, RfbInStream, ENCODING_CACHED_RECT_SEED,
};
use anyhow::{bail, Context, Result};
use rfb_common::Rect;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::io::AsyncRead;
pub struct PersistentCachedRectSeedDecoder {
    cache: Arc<Mutex<PersistentClientCache>>,
}
impl PersistentCachedRectSeedDecoder {
    pub fn new(cache: Arc<Mutex<PersistentClientCache>>) -> Self {
        Self { cache }
    }
}
#[allow(async_fn_in_trait)]
impl Decoder for PersistentCachedRectSeedDecoder {
    fn encoding_type(&self) -> i32 {
        ENCODING_CACHED_RECT_SEED
    }
    async fn decode<R: AsyncRead + Unpin>(
        &self,
        stream: &mut RfbInStream<R>,
        rect: &Rectangle,
        _pixel_format: &PixelFormat,
        buffer: &mut dyn MutablePixelBuffer,
    ) -> Result<()> {
        let mut id = [0u8; 16];
        stream
            .read_bytes(&mut id)
            .await
            .context("read CachedRectSeed id")?;
        let dest_rect = Rect::new(
            rect.x as i32,
            rect.y as i32,
            rect.width as u32,
            rect.height as u32,
        );
        let mut stride_pixels = 0usize;
        let Some(pixels) = buffer.get_buffer(dest_rect, &mut stride_pixels) else {
            bail!("CachedRectSeed rect outside framebuffer");
        };
        let format = buffer.pixel_format();
        let bpp = format.bytes_per_pixel() as usize;
        let byte_len = rect.height as usize * stride_pixels * bpp;
        if pixels.len() < byte_len {
            bail!("CachedRectSeed framebuffer slice too short");
        }
        self.cache
            .lock()
            .map_err(|_| anyhow::anyhow!("persistent cache mutex poisoned"))?
            .insert(PersistentCachedPixels {
                id,
                pixels: pixels[..byte_len].to_vec(),
                format: *format,
                width: rect.width as u32,
                height: rect.height as u32,
                stride_pixels,
                last_used: Instant::now(),
            });
        Ok(())
    }
}
