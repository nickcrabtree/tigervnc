//! Core decoding interfaces for RFB (VNC) encodings.
//!
//! This crate defines the [`Decoder`] trait that all encoding implementations must implement.
//! A decoder is responsible for reading a single framebuffer update rectangle (as sent by the
//! server using a specific encoding) from the network stream, transforming it into the
//! client's pixel format, and writing pixels into a [`MutablePixelBuffer`].
//!
//! # Key Concepts
//!
//! - **Async decoding**: Decoders read from a tokio [`AsyncRead`]-backed [`RfbInStream`]
//! - **Rectangle-based**: Decoders operate on a single rectangle at a time
//! - **Fail-fast policy**: Decoders must not perform defensive fallbacks; fail with clear errors
//! - **Encoding types**: Each decoder handles one RFB encoding type (i32 identifier from spec)
//!
//! # Example
//!
//! ```no_run
//! use anyhow::Result;
//! use rfb_encodings::{Decoder, ENCODING_RAW, RfbInStream};
//! use rfb_encodings::{PixelFormat, Rectangle, MutablePixelBuffer};
//! use tokio::io::AsyncRead;
//!
//! struct NoopDecoder;
//!
//! impl Decoder for NoopDecoder {
//!     fn encoding_type(&self) -> i32 {
//!         ENCODING_RAW
//!     }
//!
//!     async fn decode<R: AsyncRead + Unpin>(
//!         &self,
//!         _stream: &mut RfbInStream<R>,
//!         _rect: &Rectangle,
//!         _pixel_format: &PixelFormat,
//!         _buffer: &mut dyn MutablePixelBuffer,
//!     ) -> Result<()> {
//!         // A real implementation would read from stream and write into buffer
//!         Ok(())
//!     }
//! }
//! ```
//!
//! # Encoding Types
//!
//! The RFB protocol defines several standard encodings for transmitting screen updates:
//!
//! - [`ENCODING_RAW`] (0): Uncompressed pixel data
//! - [`ENCODING_COPY_RECT`] (1): Copy from another screen region
//! - [`ENCODING_RRE`] (2): Rise-and-Run-length Encoding
//! - [`ENCODING_HEXTILE`] (5): Tiled encoding with sub-rectangles
//! - [`ENCODING_TIGHT`] (7): JPEG or zlib compressed
//! - [`ENCODING_ZRLE`] (16): Zlib-compressed RLE
//!
//! Pseudo-encodings (negative values) indicate special operations:
//!
//! - [`ENCODING_LAST_RECT`] (-224): Last rectangle in update
//! - [`ENCODING_DESKTOP_SIZE`] (-223): Desktop resolution change

use anyhow::Result;
use tokio::io::AsyncRead;

// Re-export types from rfb-protocol and rfb-pixelbuffer used by decoders
pub use rfb_pixelbuffer::MutablePixelBuffer;
pub use rfb_protocol::io::RfbInStream;
pub use rfb_protocol::messages::types::{PixelFormat, Rectangle};

// Encoding implementations
pub mod raw;
pub use raw::RawDecoder;

pub mod copyrect;
pub use copyrect::CopyRectDecoder;

pub mod rre;
pub use rre::RREDecoder;

pub mod hextile;
pub use hextile::HextileDecoder;

pub mod tight;
pub use tight::TightDecoder;

pub mod zrle;
pub use zrle::ZRLEDecoder;

// ContentCache for caching decoded pixel data and decoders
pub mod content_cache;
pub use content_cache::{CacheStats, CachedPixels, ContentCache};

pub mod cached_rect;
pub use cached_rect::CachedRectDecoder;

pub mod cached_rect_init;
pub use cached_rect_init::CachedRectInitDecoder;

// PersistentCache
pub mod persistent_cache;
pub use persistent_cache::{PersistentCachedPixels, PersistentClientCache};

pub mod persistent_cached_rect;
pub use persistent_cached_rect::PersistentCachedRectDecoder;

pub mod persistent_cached_rect_init;
pub use persistent_cached_rect_init::PersistentCachedRectInitDecoder;

// PersistentCache encodings
/// PersistentCachedRect encoding: reference by 16-byte content hash
pub const ENCODING_PERSISTENT_CACHED_RECT: i32 = 102;
/// PersistentCachedRectInit encoding: include hash + actual encoding + data
pub const ENCODING_PERSISTENT_CACHED_RECT_INIT: i32 = 103;

// Standard VNC encodings
/// Raw encoding: uncompressed pixel data (simplest encoding).
pub const ENCODING_RAW: i32 = 0;

/// CopyRect encoding: copy rectangle from another screen location.
pub const ENCODING_COPY_RECT: i32 = 1;

/// RRE (Rise-and-Run-length Encoding): background color + sub-rectangles.
pub const ENCODING_RRE: i32 = 2;

/// Hextile encoding: 16x16 tiles with multiple sub-encodings.
pub const ENCODING_HEXTILE: i32 = 5;

/// Zlib encoding: zlib-compressed raw pixels.
pub const ENCODING_ZLIB: i32 = 6;

/// Tight encoding: JPEG or zlib compression with palette mode.
pub const ENCODING_TIGHT: i32 = 7;

/// TRLE (Tiled Run-Length Encoding): 16x16 tiles with RLE.
pub const ENCODING_TRLE: i32 = 15;

/// ZRLE (Zlib Run-Length Encoding): zlib + RLE in 64x64 tiles.
pub const ENCODING_ZRLE: i32 = 16;

// Pseudo-encodings (negative values indicate special operations)
/// Pseudo-encoding: last rectangle marker in framebuffer update.
pub const ENCODING_LAST_RECT: i32 = -224;

/// Pseudo-encoding: desktop size change notification.
pub const ENCODING_DESKTOP_SIZE: i32 = -223;

// ContentCache encoding types (TigerVNC extension)
/// CachedRect encoding: reference to cached content (cache hit).
pub const ENCODING_CACHED_RECT: i32 = 100;

/// CachedRectInit encoding: initial transmission with cache ID (cache miss).
pub const ENCODING_CACHED_RECT_INIT: i32 = 101;

/// Core trait for all RFB encoding/decoding implementations.
///
/// Implementations read encoded rectangle data from the network stream,
/// convert pixels to the client's pixel format, and write them to the buffer.
///
/// # Contract
///
/// Implementors must:
/// - Read exactly the bytes for the rectangle as defined by their encoding
/// - Handle pixel format conversions correctly
/// - Write pixels to the buffer using appropriate methods
/// - Fail fast with clear error messages (no defensive fallbacks)
/// - Avoid unnecessary allocations where possible
///
/// # Example
///
/// ```no_run
/// use anyhow::Result;
/// use rfb_encodings::{Decoder, ENCODING_RAW, RfbInStream};
/// use rfb_encodings::{PixelFormat, Rectangle, MutablePixelBuffer};
/// use tokio::io::AsyncRead;
///
/// struct RawDecoder;
///
/// impl Decoder for RawDecoder {
///     fn encoding_type(&self) -> i32 {
///         ENCODING_RAW
///     }
///
///     async fn decode<R: AsyncRead + Unpin>(
///         &self,
///         stream: &mut RfbInStream<R>,
///         rect: &Rectangle,
///         pixel_format: &PixelFormat,
///         buffer: &mut dyn MutablePixelBuffer,
///     ) -> Result<()> {
///         // Read width * height * bytes_per_pixel bytes from stream
///         // Convert to buffer's pixel format and write to buffer
///         // (Actual implementation would go here)
///         Ok(())
///     }
/// }
/// ```
#[allow(async_fn_in_trait)]
pub trait Decoder {
    /// Returns the RFB encoding type this decoder handles.
    ///
    /// This should be one of the `ENCODING_*` constants defined in this crate.
    ///
    /// # Example
    ///
    /// ```
    /// use rfb_encodings::{Decoder, ENCODING_RAW};
    /// # struct MyDecoder;
    /// # impl Decoder for MyDecoder {
    /// #     fn encoding_type(&self) -> i32 { ENCODING_RAW }
    /// #     async fn decode<R: tokio::io::AsyncRead + Unpin>(
    /// #         &self, _: &mut rfb_encodings::RfbInStream<R>, _: &rfb_encodings::Rectangle,
    /// #         _: &rfb_encodings::PixelFormat, _: &mut dyn rfb_encodings::MutablePixelBuffer,
    /// #     ) -> anyhow::Result<()> { Ok(()) }
    /// # }
    ///
    /// let decoder = MyDecoder;
    /// assert_eq!(decoder.encoding_type(), ENCODING_RAW);
    /// ```
    fn encoding_type(&self) -> i32;

    /// Decode a single rectangle from the input stream into the pixel buffer.
    ///
    /// # Parameters
    ///
    /// - `stream`: Network input stream with helpers for reading RFB types
    /// - `rect`: The rectangle bounds and encoding to decode
    /// - `pixel_format`: The target pixel format (from ServerInit)
    /// - `buffer`: The destination buffer to write decoded pixels into
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The input bytes are malformed or insufficient (EOF)
    /// - Pixel format conversion fails
    /// - Writing to the buffer fails (out of bounds, etc.)
    /// - The encoding-specific data is invalid
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use anyhow::Result;
    /// # use rfb_encodings::{Decoder, ENCODING_RAW, RfbInStream};
    /// # use rfb_encodings::{PixelFormat, Rectangle, MutablePixelBuffer};
    /// # use tokio::io::AsyncRead;
    /// # struct MyDecoder;
    /// # impl Decoder for MyDecoder {
    /// #     fn encoding_type(&self) -> i32 { ENCODING_RAW }
    /// async fn decode<R: AsyncRead + Unpin>(
    ///     &self,
    ///     stream: &mut RfbInStream<R>,
    ///     rect: &Rectangle,
    ///     pixel_format: &PixelFormat,
    ///     buffer: &mut dyn MutablePixelBuffer,
    /// ) -> Result<()> {
    ///     // Decoder implementation reads from stream and writes to buffer
    ///     # Ok(())
    /// }
    /// # }
    /// ```
    async fn decode<R: AsyncRead + Unpin>(
        &self,
        stream: &mut RfbInStream<R>,
        rect: &Rectangle,
        pixel_format: &PixelFormat,
        buffer: &mut dyn MutablePixelBuffer,
    ) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoopDecoder;

    impl Decoder for NoopDecoder {
        fn encoding_type(&self) -> i32 {
            ENCODING_RAW
        }

        async fn decode<R: AsyncRead + Unpin>(
            &self,
            _stream: &mut RfbInStream<R>,
            _rect: &Rectangle,
            _pixel_format: &PixelFormat,
            _buffer: &mut dyn MutablePixelBuffer,
        ) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_trait_can_be_implemented() {
        let decoder = NoopDecoder;
        assert_eq!(decoder.encoding_type(), ENCODING_RAW);
    }

    #[test]
    fn test_encoding_constants() {
        assert_eq!(ENCODING_RAW, 0);
        assert_eq!(ENCODING_COPY_RECT, 1);
        assert_eq!(ENCODING_RRE, 2);
        assert_eq!(ENCODING_HEXTILE, 5);
        assert_eq!(ENCODING_ZLIB, 6);
        assert_eq!(ENCODING_TIGHT, 7);
        assert_eq!(ENCODING_TRLE, 15);
        assert_eq!(ENCODING_ZRLE, 16);
        assert_eq!(ENCODING_LAST_RECT, -224);
        assert_eq!(ENCODING_DESKTOP_SIZE, -223);
    }
}
