//! Framebuffer state management and decoder registry.
//!
//! This module manages the client's framebuffer and provides a registry of
//! encoding decoders to apply server framebuffer update rectangles.

use crate::errors::RfbClientError;
use anyhow::Result as AnyResult;
use rfb_common::Rect;
use rfb_encodings as enc;
use rfb_encodings::{ContentCache, Decoder, MutablePixelBuffer, RfbInStream};
use rfb_pixelbuffer::{ManagedPixelBuffer, PixelBuffer as _, PixelFormat as LocalPixelFormat};
use rfb_protocol::messages::types::{PixelFormat as ServerPixelFormat, Rectangle};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::io::AsyncRead;

/// Registry of decoders keyed by encoding type.
#[derive(Default)]
pub struct DecoderRegistry {
    decoders: HashMap<i32, DecoderEntry>,
}

impl DecoderRegistry {
    /// Create a registry with all standard encodings registered.
    pub fn with_standard() -> Self {
        let mut reg = Self::default();
        reg.register(DecoderEntry::Raw(enc::RawDecoder));
        reg.register(DecoderEntry::CopyRect(enc::CopyRectDecoder));
        reg.register(DecoderEntry::RRE(enc::RREDecoder));
        reg.register(DecoderEntry::Hextile(enc::HextileDecoder));
        reg.register(DecoderEntry::Tight(enc::TightDecoder::default()));
        reg.register(DecoderEntry::ZRLE(enc::ZRLEDecoder::default()));
        reg
    }

    /// Create a registry with all standard encodings plus ContentCache support.
    pub fn with_content_cache(
        cache: Arc<Mutex<ContentCache>>,
        misses: Arc<Mutex<Vec<u64>>>,
    ) -> Self {
        // CRITICAL: Share the ZRLE decoder between registry and CachedRectInit!
        // ZRLE uses a continuous zlib stream across all rectangles in a FramebufferUpdate.
        // If each decoder has its own inflater, subsequent rectangles will fail.
        let zrle_decoder = Arc::new(enc::ZRLEDecoder::default());
        
        let mut reg = Self::default();
        // Register standard encodings with shared ZRLE
        reg.register(DecoderEntry::Raw(enc::RawDecoder));
        reg.register(DecoderEntry::CopyRect(enc::CopyRectDecoder));
        reg.register(DecoderEntry::RRE(enc::RREDecoder));
        reg.register(DecoderEntry::Hextile(enc::HextileDecoder));
        reg.register(DecoderEntry::Tight(enc::TightDecoder::default()));
        reg.register(DecoderEntry::ZRLEShared(zrle_decoder.clone()));
        
        // Register cache decoders with shared ZRLE
        reg.register(DecoderEntry::CachedRect(
            enc::CachedRectDecoder::new_with_miss_reporter(cache.clone(), misses),
        ));
        reg.register(DecoderEntry::CachedRectInit(
            enc::CachedRectInitDecoder::new(cache, zrle_decoder),
        ));
        reg
    }

    /// Register a decoder entry.
    pub fn register(&mut self, decoder: DecoderEntry) {
        self.decoders.insert(decoder.encoding_type(), decoder);
    }

    /// Get a decoder by encoding type.
    pub fn get(&self, encoding: i32) -> Option<&DecoderEntry> {
        self.decoders.get(&encoding)
    }
}

/// A concrete decoder entry wrapper for dynamic dispatch over non-object-safe Decoder.
enum DecoderEntry {
    Raw(enc::RawDecoder),
    CopyRect(enc::CopyRectDecoder),
    RRE(enc::RREDecoder),
    Hextile(enc::HextileDecoder),
    Tight(enc::TightDecoder),
    ZRLE(enc::ZRLEDecoder),
    /// Shared ZRLE decoder (Arc-wrapped for sharing with CachedRectInitDecoder)
    ZRLEShared(Arc<enc::ZRLEDecoder>),
    CachedRect(enc::CachedRectDecoder),
    CachedRectInit(enc::CachedRectInitDecoder),
    PersistentCachedRect(enc::PersistentCachedRectDecoder),
    PersistentCachedRectInit(enc::PersistentCachedRectInitDecoder),
}

impl DecoderEntry {
    fn encoding_type(&self) -> i32 {
        match self {
            Self::Raw(d) => d.encoding_type(),
            Self::CopyRect(d) => d.encoding_type(),
            Self::RRE(d) => d.encoding_type(),
            Self::Hextile(d) => d.encoding_type(),
            Self::Tight(d) => d.encoding_type(),
            Self::ZRLE(d) => d.encoding_type(),
            Self::ZRLEShared(d) => d.encoding_type(),
            Self::CachedRect(d) => d.encoding_type(),
            Self::CachedRectInit(d) => d.encoding_type(),
            Self::PersistentCachedRect(d) => d.encoding_type(),
            Self::PersistentCachedRectInit(d) => d.encoding_type(),
        }
    }

    async fn decode<R: AsyncRead + Unpin>(
        &self,
        stream: &mut RfbInStream<R>,
        rect: &Rectangle,
        pixel_format: &ServerPixelFormat,
        buffer: &mut dyn MutablePixelBuffer,
    ) -> AnyResult<()> {
        match self {
            Self::Raw(d) => d.decode(stream, rect, pixel_format, buffer).await,
            Self::CopyRect(d) => d.decode(stream, rect, pixel_format, buffer).await,
            Self::RRE(d) => d.decode(stream, rect, pixel_format, buffer).await,
            Self::Hextile(d) => d.decode(stream, rect, pixel_format, buffer).await,
            Self::Tight(d) => d.decode(stream, rect, pixel_format, buffer).await,
            Self::ZRLE(d) => d.decode(stream, rect, pixel_format, buffer).await,
            Self::ZRLEShared(d) => d.decode(stream, rect, pixel_format, buffer).await,
            Self::CachedRect(d) => d.decode(stream, rect, pixel_format, buffer).await,
            Self::CachedRectInit(d) => d.decode(stream, rect, pixel_format, buffer).await,
            Self::PersistentCachedRect(d) => d.decode(stream, rect, pixel_format, buffer).await,
            Self::PersistentCachedRectInit(d) => d.decode(stream, rect, pixel_format, buffer).await,
        }
    }
}

/// Framebuffer state and decoder dispatcher.
pub struct Framebuffer {
    /// Local framebuffer buffer in a fixed output pixel format (RGB888).
    buffer: ManagedPixelBuffer,
    /// Server-advertised pixel format (input format for decoders).
    server_pixel_format: ServerPixelFormat,
    /// Decoder registry.
    registry: DecoderRegistry,
    /// Queue of cache IDs that missed during decoding and should be requested.
    pending_misses: Option<Arc<Mutex<Vec<u64>>>>,
}

impl Framebuffer {
    /// Create a new framebuffer with given server pixel format and dimensions.
    ///
    /// The internal buffer uses local RGB888 format for simplicity and broad compatibility.
    pub fn new(width: u16, height: u16, server_pixel_format: ServerPixelFormat) -> Self {
        let local_format = LocalPixelFormat::rgb888();
        let buffer = ManagedPixelBuffer::new(width as u32, height as u32, local_format);
        Self {
            buffer,
            server_pixel_format,
            registry: DecoderRegistry::with_standard(),
            pending_misses: None,
        }
    }

    /// Create a new framebuffer with ContentCache support.
    pub fn with_content_cache(
        width: u16,
        height: u16,
        server_pixel_format: ServerPixelFormat,
        cache: Arc<Mutex<ContentCache>>,
    ) -> Self {
        let local_format = LocalPixelFormat::rgb888();
        let buffer = ManagedPixelBuffer::new(width as u32, height as u32, local_format);
        let misses: Arc<Mutex<Vec<u64>>> = Arc::new(Mutex::new(Vec::new()));
        Self {
            buffer,
            server_pixel_format,
            registry: DecoderRegistry::with_content_cache(cache, misses.clone()),
            pending_misses: Some(misses),
        }
    }

    /// Create a new framebuffer with PersistentCache support.
    pub fn with_persistent_cache(
        width: u16,
        height: u16,
        server_pixel_format: ServerPixelFormat,
        pcache: Arc<Mutex<enc::PersistentClientCache>>,
    ) -> Self {
        let local_format = LocalPixelFormat::rgb888();
        let buffer = ManagedPixelBuffer::new(width as u32, height as u32, local_format);
        let mut reg = DecoderRegistry::with_standard();
        reg.register(DecoderEntry::PersistentCachedRect(
            enc::PersistentCachedRectDecoder::new(pcache.clone()),
        ));
        reg.register(DecoderEntry::PersistentCachedRectInit(
            enc::PersistentCachedRectInitDecoder::new(pcache),
        ));
        Self {
            buffer,
            server_pixel_format,
            registry: reg,
            pending_misses: None,
        }
    }

    /// Create a new framebuffer with both ContentCache and PersistentCache support.
    pub fn with_both_caches(
        width: u16,
        height: u16,
        server_pixel_format: ServerPixelFormat,
        ccache: Arc<Mutex<ContentCache>>,
        pcache: Arc<Mutex<enc::PersistentClientCache>>,
    ) -> Self {
        let local_format = LocalPixelFormat::rgb888();
        let buffer = ManagedPixelBuffer::new(width as u32, height as u32, local_format);
        let misses: Arc<Mutex<Vec<u64>>> = Arc::new(Mutex::new(Vec::new()));
        let mut reg = DecoderRegistry::with_content_cache(ccache, misses.clone());
        reg.register(DecoderEntry::PersistentCachedRect(
            enc::PersistentCachedRectDecoder::new(pcache.clone()),
        ));
        reg.register(DecoderEntry::PersistentCachedRectInit(
            enc::PersistentCachedRectInitDecoder::new(pcache),
        ));
        Self {
            buffer,
            server_pixel_format,
            registry: reg,
            pending_misses: Some(misses),
        }
    }

    /// Returns the current dimensions.
    pub fn size(&self) -> (u16, u16) {
        let (w, h) = self.buffer.dimensions();
        (w as u16, h as u16)
    }

    /// Returns a reference to the underlying buffer.
    pub fn buffer(&self) -> &ManagedPixelBuffer {
        &self.buffer
    }

    /// Returns a mutable reference to the underlying buffer.
    pub fn buffer_mut(&mut self) -> &mut ManagedPixelBuffer {
        &mut self.buffer
    }

    /// Apply a single rectangle update from the server.
    pub async fn apply_rectangle<R: AsyncRead + Unpin>(
        &mut self,
        stream: &mut RfbInStream<R>,
        rect: &Rectangle,
    ) -> Result<(), RfbClientError> {
        match rect.encoding {
            enc::ENCODING_LAST_RECT => {
                // Marker only
                return Ok(());
            }
            enc::ENCODING_DESKTOP_SIZE => {
                // Resize framebuffer
                self.buffer.resize(rect.width as u32, rect.height as u32);
                return Ok(());
            }
            other => {
                let decoder = self
                    .registry
                    .get(other)
                    .ok_or_else(|| RfbClientError::UnsupportedEncoding(other))?;

                // Log selected decoder variant and rectangle details for debugging
                let decoder_name = match decoder {
                    DecoderEntry::Raw(_) => "Raw",
                    DecoderEntry::CopyRect(_) => "CopyRect",
                    DecoderEntry::RRE(_) => "RRE",
                    DecoderEntry::Hextile(_) => "Hextile",
                    DecoderEntry::Tight(_) => "Tight",
                    DecoderEntry::ZRLE(_) => "ZRLE",
                    DecoderEntry::ZRLEShared(_) => "ZRLE",
                    DecoderEntry::CachedRect(_) => "CachedRect",
                    DecoderEntry::CachedRectInit(_) => "CachedRectInit",
                    DecoderEntry::PersistentCachedRect(_) => "PersistentCachedRect",
                    DecoderEntry::PersistentCachedRectInit(_) => "PersistentCachedRectInit",
                };
                tracing::debug!(
                    "Decoder selected: {} (encoding={}) for rect x={}, y={}, w={}, h={}",
                    decoder_name,
                    other,
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height
                );

                let pf = &self.server_pixel_format;
                let buffer: &mut dyn MutablePixelBuffer = &mut self.buffer;

                decoder
                    .decode(stream, rect, pf, buffer)
                    .await
                    .map_err(RfbClientError::Encoding)
            }
        }
    }

    /// Apply an update by streaming from the input (reads header + decodes rectangles).
    pub async fn apply_update_stream<R: AsyncRead + Unpin>(
        &mut self,
        stream: &mut RfbInStream<R>,
    ) -> Result<Vec<Rect>, RfbClientError> {
        // FramebufferUpdate header: 1 byte padding + 2 bytes rect count
        stream.skip(1).await.map_err(|e| {
            RfbClientError::Protocol(format!("failed to read FramebufferUpdate padding: {}", e))
        })?;
        let num_raw = stream.read_u16().await.map_err(|e| {
            RfbClientError::Protocol(format!(
                "failed to read FramebufferUpdate rect count: {}",
                e
            ))
        })?;

        // Framing instrumentation: log FBU start with declared rect count
        tracing::debug!(
            target: "rfb_client::framing",
            "FBU start: declared_rects={}, available_buffer_bytes={}",
            num_raw,
            stream.available()
        );

        let mut damage: Vec<Rect> = Vec::new();
        let mut rects_decoded = 0;

        if num_raw == 0xFFFF {
            // Unknown number of rectangles; terminated by LastRect pseudo-encoding
            loop {
                let buffer_before = stream.available();
                let rect = Rectangle::read_from(stream).await.map_err(|e| {
                    RfbClientError::Protocol(format!("failed to read Rectangle header: {}", e))
                })?;
                tracing::info!(
                    "FramebufferUpdate rect: x={}, y={}, w={}, h={}, encoding={}",
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height,
                    rect.encoding
                );
                if rect.encoding == enc::ENCODING_LAST_RECT {
                    tracing::debug!(
                        target: "rfb_client::framing",
                        "FBU rect {}: LastRect marker (end of update)",
                        rects_decoded
                    );
                    // End of this update
                    break;
                }

                tracing::debug!(
                    target: "rfb_client::framing",
                    "FBU rect {}: enc={} rect=[{},{} {}x{}] buffer_before={}",
                    rects_decoded,
                    rect.encoding,
                    rect.x, rect.y, rect.width, rect.height,
                    buffer_before
                );

                self.apply_rectangle(stream, &rect).await?;

                let buffer_after = stream.available();
                tracing::debug!(
                    target: "rfb_client::framing",
                    "FBU rect {}: decoded, buffer_after={}",
                    rects_decoded,
                    buffer_after
                );

                rects_decoded += 1;

                if rect.encoding >= 0 {
                    damage.push(Rect::new(
                        rect.x as i32,
                        rect.y as i32,
                        rect.width as u32,
                        rect.height as u32,
                    ));
                }
            }
        } else {
            let num = num_raw as usize;
            damage.reserve(num);
            for i in 0..num {
                let buffer_before = stream.available();
                let rect = Rectangle::read_from(stream).await.map_err(|e| {
                    RfbClientError::Protocol(format!("failed to read Rectangle header: {}", e))
                })?;
                tracing::info!(
                    "FramebufferUpdate rect: x={}, y={}, w={}, h={}, encoding={}",
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height,
                    rect.encoding
                );

                tracing::debug!(
                    target: "rfb_client::framing",
                    "FBU rect {}/{}: enc={} rect=[{},{} {}x{}] buffer_before={}",
                    i,
                    num,
                    rect.encoding,
                    rect.x, rect.y, rect.width, rect.height,
                    buffer_before
                );

                self.apply_rectangle(stream, &rect).await?;

                let buffer_after = stream.available();
                tracing::debug!(
                    target: "rfb_client::framing",
                    "FBU rect {}/{}: decoded, buffer_after={}",
                    i,
                    num,
                    buffer_after
                );

                rects_decoded += 1;

                if rect.encoding >= 0 {
                    damage.push(Rect::new(
                        rect.x as i32,
                        rect.y as i32,
                        rect.width as u32,
                        rect.height as u32,
                    ));
                }
            }
        }

        // Framing instrumentation: verify rect count matches
        if (num_raw != 0xFFFF && rects_decoded != num_raw as usize) {
            tracing::warn!(
                target: "rfb_client::framing",
                "FBU end: MISMATCH! declared_rects={} decoded_rects={}",
                num_raw,
                rects_decoded
            );
        } else {
            tracing::debug!(
                target: "rfb_client::framing",
                "FBU end: rects_decoded={} (matches declared count)",
                rects_decoded
            );
        }

        Ok(damage)
    }

    /// Apply multiple rectangles, returning the list of damaged regions for repaint.
    pub async fn apply_update<R: AsyncRead + Unpin>(
        &mut self,
        stream: &mut RfbInStream<R>,
        rects: &[Rectangle],
    ) -> Result<Vec<Rect>, RfbClientError> {
        let mut damage = Vec::with_capacity(rects.len());
        for rect in rects {
            tracing::info!(
                "FramebufferUpdate rect: x={}, y={}, w={}, h={}, encoding={}",
                rect.x,
                rect.y,
                rect.width,
                rect.height,
                rect.encoding
            );
            self.apply_rectangle(stream, rect).await?;
            if rect.encoding >= 0 {
                damage.push(Rect::new(
                    rect.x as i32,
                    rect.y as i32,
                    rect.width as u32,
                    rect.height as u32,
                ));
            }
        }
        Ok(damage)
    }

    /// Drain and return any pending cache miss IDs reported during the last decode.
    pub fn drain_pending_cache_misses(&self) -> Vec<u64> {
        if let Some(m) = &self.pending_misses {
            if let Ok(mut v) = m.lock() {
                let out = v.clone();
                v.clear();
                return out;
            }
        }
        Vec::new()
    }
}
