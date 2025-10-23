//! Framebuffer state management and decoder registry.
//!
//! This module manages the client's framebuffer and provides a registry of
//! encoding decoders to apply server framebuffer update rectangles.

use crate::errors::RfbClientError;
use anyhow::Result as AnyResult;
use rfb_common::Rect;
use rfb_encodings as enc;
use rfb_encodings::{Decoder, MutablePixelBuffer, RfbInStream};
use rfb_pixelbuffer::{ManagedPixelBuffer, PixelBuffer as _, PixelFormat as LocalPixelFormat};
use rfb_protocol::messages::types::{PixelFormat as ServerPixelFormat, Rectangle};
use std::collections::HashMap;
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
                self.buffer
                    .resize(rect.width as u32, rect.height as u32);
                return Ok(());
            }
            other => {
                let decoder = self
                    .registry
                    .get(other)
                    .ok_or_else(|| RfbClientError::UnsupportedEncoding(other))?;

                let pf = &self.server_pixel_format;
                let buffer: &mut dyn MutablePixelBuffer = &mut self.buffer;

                decoder
                    .decode(stream, rect, pf, buffer)
                    .await
                    .map_err(RfbClientError::Encoding)
            }
        }
    }

    /// Apply multiple rectangles, returning the list of damaged regions for repaint.
    pub async fn apply_update<R: AsyncRead + Unpin>(
        &mut self,
        stream: &mut RfbInStream<R>,
        rects: &[Rectangle],
    ) -> Result<Vec<Rect>, RfbClientError> {
        let mut damage = Vec::with_capacity(rects.len());
        for rect in rects {
            self.apply_rectangle(stream, rect).await?;
            if rect.encoding >= 0 {
                damage.push(Rect::new(rect.x as i32, rect.y as i32, rect.width as u32, rect.height as u32));
            }
        }
        Ok(damage)
    }
}
