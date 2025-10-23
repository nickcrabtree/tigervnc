//! ContentCache protocol message types.
//!
//! ContentCache provides 97-99% bandwidth reduction by sending only cache IDs
//! for repeated content instead of re-encoding pixels.
//!
//! # Protocol Flow
//!
//! 1. **First time content appears**: Server sends [`CachedRectInit`] with cache_id,
//!    actual encoding, and pixel data. Client decodes and stores in cache.
//!
//! 2. **Content repeats**: Server sends [`CachedRect`] with only cache_id (20 bytes).
//!    Client looks up cached pixels and blits them.
//!
//! 3. **Cache miss**: Client requests refresh, server re-sends with [`CachedRectInit`].
//!
//! # Example
//!
//! ```no_run
//! use rfb_protocol::messages::cache::{CachedRect, CachedRectInit};
//! use rfb_protocol::io::RfbInStream;
//! # async fn example<R: tokio::io::AsyncRead + Unpin>(stream: &mut RfbInStream<R>) -> std::io::Result<()> {
//!
//! // Server sends cache reference (only 20 bytes!)
//! let cached_rect = CachedRect::read_from(stream).await?;
//! println!("Cache ID: {}", cached_rect.cache_id);
//!
//! // Or server sends initial cached content
//! let cached_rect_init = CachedRectInit::read_from(stream).await?;
//! println!("Cache ID: {}, Encoding: {}", 
//!          cached_rect_init.cache_id,
//!          cached_rect_init.actual_encoding);
//! # Ok(())
//! # }
//! ```

use crate::io::{RfbInStream, RfbOutStream};
use tokio::io::{AsyncRead, AsyncWrite};

/// CachedRect - Reference to already-cached content.
///
/// The server sends this when it believes the client already has the pixel
/// data in its cache. This is only 8 bytes after the rectangle header,
/// containing just the cache_id.
///
/// # Wire Format (after 12-byte Rectangle header)
///
/// - 8 bytes: cache_id (u64, big-endian)
///
/// **Total**: 12 bytes (header) + 8 bytes = 20 bytes
///
/// Compare this to kilobytes for re-encoded content!
///
/// # Client Behavior
///
/// 1. Look up cache_id in local cache
/// 2. If **hit**: Blit cached pixels to framebuffer ✅
/// 3. If **miss**: Request refresh from server ⚠️
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CachedRect {
    /// Unique identifier for cached content.
    ///
    /// Server assigns this based on content hash.
    /// Must be non-zero (0 is reserved for errors).
    pub cache_id: u64,
}

impl CachedRect {
    /// Create a new CachedRect with the given cache ID.
    ///
    /// # Panics
    ///
    /// Panics in debug builds if cache_id is 0.
    pub fn new(cache_id: u64) -> Self {
        debug_assert_ne!(cache_id, 0, "Cache ID must be non-zero");
        Self { cache_id }
    }

    /// Read a CachedRect from an RFB input stream.
    ///
    /// **Note**: This only reads the 8-byte cache_id. The 12-byte Rectangle
    /// header must be read separately using [`Rectangle::read_from`].
    ///
    /// [`Rectangle::read_from`]: super::types::Rectangle::read_from
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - EOF is reached before reading 8 bytes
    /// - cache_id is 0 (invalid)
    pub async fn read_from<R: AsyncRead + Unpin>(
        stream: &mut RfbInStream<R>,
    ) -> std::io::Result<Self> {
        let cache_id = stream.read_u64().await?;

        // Validate cache_id is non-zero
        if cache_id == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "CachedRect cache_id must be non-zero",
            ));
        }

        Ok(Self { cache_id })
    }

    /// Write a CachedRect to an RFB output stream.
    ///
    /// **Note**: This only writes the 8-byte cache_id. The Rectangle header
    /// must be written separately.
    ///
    /// # Errors
    ///
    /// Returns an error if cache_id is 0.
    pub fn write_to<W: AsyncWrite + Unpin>(&self, stream: &mut RfbOutStream<W>) -> std::io::Result<()> {
        if self.cache_id == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "CachedRect cache_id must be non-zero",
            ));
        }

        stream.write_u64(self.cache_id);
        Ok(())
    }
}

/// CachedRectInit - Initial transmission with cache ID and encoded pixels.
///
/// The server sends this when transmitting content for the first time or when
/// the client has indicated a cache miss. It includes:
/// 1. cache_id to store under
/// 2. actual_encoding type for the pixel data
/// 3. Encoded pixel data (read separately by appropriate decoder)
///
/// # Wire Format (after 12-byte Rectangle header)
///
/// - 8 bytes: cache_id (u64, big-endian)
/// - 4 bytes: actual_encoding (i32, signed, big-endian)
/// - N bytes: encoded pixel data (depends on actual_encoding)
///
/// **Total**: 12 bytes (header) + 12 bytes + N bytes (encoded data)
///
/// # Client Behavior
///
/// 1. Read cache_id and actual_encoding
/// 2. Dispatch to appropriate decoder based on actual_encoding
/// 3. Decode pixel data to RGBA
/// 4. **Store** decoded pixels in cache under cache_id
/// 5. Blit to framebuffer
///
/// # Example
///
/// ```no_run
/// # use rfb_protocol::messages::cache::CachedRectInit;
/// # use rfb_protocol::messages::types::{Rectangle, ENCODING_TIGHT};
/// # use rfb_protocol::io::RfbInStream;
/// # async fn example<R: tokio::io::AsyncRead + Unpin>(stream: &mut RfbInStream<R>) -> std::io::Result<()> {
/// // Read rectangle header first
/// let rect = Rectangle::read_from(stream).await?;
/// assert_eq!(rect.encoding, rfb_protocol::messages::types::ENCODING_CACHED_RECT_INIT);
///
/// // Read CachedRectInit metadata
/// let init = CachedRectInit::read_from(stream).await?;
/// 
/// // Dispatch to appropriate decoder based on actual_encoding
/// match init.actual_encoding {
///     ENCODING_TIGHT => {
///         // decode_tight(stream, rect, init.cache_id).await?;
///     },
///     _ => {
///         // handle other encodings...
///     }
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CachedRectInit {
    /// Unique identifier to store decoded pixels under.
    ///
    /// Must be non-zero (0 is reserved for errors).
    pub cache_id: u64,

    /// Actual encoding type for the pixel data that follows.
    ///
    /// Can be any valid encoding:
    /// - [`ENCODING_RAW`] (0)
    /// - [`ENCODING_TIGHT`] (7)
    /// - [`ENCODING_ZRLE`] (16)
    /// - etc.
    ///
    /// **Note**: Must NOT be `ENCODING_CACHED_RECT` or `ENCODING_CACHED_RECT_INIT`
    /// (no recursive caching).
    ///
    /// [`ENCODING_RAW`]: super::types::ENCODING_RAW
    /// [`ENCODING_TIGHT`]: super::types::ENCODING_TIGHT
    /// [`ENCODING_ZRLE`]: super::types::ENCODING_ZRLE
    pub actual_encoding: i32,
}

impl CachedRectInit {
    /// Create a new CachedRectInit with the given cache ID and encoding.
    ///
    /// # Panics
    ///
    /// Panics in debug builds if:
    /// - cache_id is 0
    /// - actual_encoding is ENCODING_CACHED_RECT or ENCODING_CACHED_RECT_INIT
    pub fn new(cache_id: u64, actual_encoding: i32) -> Self {
        debug_assert_ne!(cache_id, 0, "Cache ID must be non-zero");
        debug_assert_ne!(
            actual_encoding,
            super::types::ENCODING_CACHED_RECT,
            "Cannot use CachedRect as actual_encoding"
        );
        debug_assert_ne!(
            actual_encoding,
            super::types::ENCODING_CACHED_RECT_INIT,
            "Cannot use CachedRectInit as actual_encoding"
        );
        Self {
            cache_id,
            actual_encoding,
        }
    }

    /// Read a CachedRectInit from an RFB input stream.
    ///
    /// **Note**: This only reads the 12-byte metadata (cache_id + actual_encoding).
    /// The encoded pixel data must be read separately by the appropriate decoder.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - EOF is reached before reading 12 bytes
    /// - cache_id is 0
    /// - actual_encoding is ENCODING_CACHED_RECT or ENCODING_CACHED_RECT_INIT
    pub async fn read_from<R: AsyncRead + Unpin>(
        stream: &mut RfbInStream<R>,
    ) -> std::io::Result<Self> {
        let cache_id = stream.read_u64().await?;
        let actual_encoding = stream.read_i32().await?;

        // Validate cache_id is non-zero
        if cache_id == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "CachedRectInit cache_id must be non-zero",
            ));
        }

        // Validate no recursive caching
        if actual_encoding == super::types::ENCODING_CACHED_RECT
            || actual_encoding == super::types::ENCODING_CACHED_RECT_INIT
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "CachedRectInit actual_encoding cannot be a cache encoding, got {}",
                    actual_encoding
                ),
            ));
        }

        Ok(Self {
            cache_id,
            actual_encoding,
        })
    }

    /// Write a CachedRectInit to an RFB output stream.
    ///
    /// **Note**: This only writes the 12-byte metadata. The encoded pixel data
    /// must be written separately.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - cache_id is 0
    /// - actual_encoding is ENCODING_CACHED_RECT or ENCODING_CACHED_RECT_INIT
    pub fn write_to<W: AsyncWrite + Unpin>(&self, stream: &mut RfbOutStream<W>) -> std::io::Result<()> {
        if self.cache_id == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "CachedRectInit cache_id must be non-zero",
            ));
        }

        if self.actual_encoding == super::types::ENCODING_CACHED_RECT
            || self.actual_encoding == super::types::ENCODING_CACHED_RECT_INIT
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "CachedRectInit actual_encoding cannot be a cache encoding, got {}",
                    self.actual_encoding
                ),
            ));
        }

        stream.write_u64(self.cache_id);
        stream.write_i32(self.actual_encoding);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[tokio::test]
    async fn test_cached_rect_round_trip() {
        let original = CachedRect::new(12345);

        // Write to buffer
        let mut buffer = Vec::new();
        let mut out_stream = RfbOutStream::new(&mut buffer);
        original.write_to(&mut out_stream).unwrap();
        out_stream.flush().await.unwrap();

        // Verify size
        assert_eq!(buffer.len(), 8);

        // Read back
        let mut in_stream = RfbInStream::new(Cursor::new(buffer));
        let read_back = CachedRect::read_from(&mut in_stream).await.unwrap();

        assert_eq!(original, read_back);
    }

    #[tokio::test]
    async fn test_cached_rect_zero_id_rejected() {
        let data = vec![0u8; 8]; // cache_id = 0
        let mut stream = RfbInStream::new(Cursor::new(data));
        let result = CachedRect::read_from(&mut stream).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("non-zero"));
    }

    #[tokio::test]
    async fn test_cached_rect_init_round_trip() {
        use super::super::types::ENCODING_TIGHT;
        let original = CachedRectInit::new(67890, ENCODING_TIGHT);

        // Write to buffer
        let mut buffer = Vec::new();
        let mut out_stream = RfbOutStream::new(&mut buffer);
        original.write_to(&mut out_stream).unwrap();
        out_stream.flush().await.unwrap();

        // Verify size
        assert_eq!(buffer.len(), 12); // 8 + 4

        // Read back
        let mut in_stream = RfbInStream::new(Cursor::new(buffer));
        let read_back = CachedRectInit::read_from(&mut in_stream).await.unwrap();

        assert_eq!(original, read_back);
    }

    #[tokio::test]
    async fn test_cached_rect_init_zero_id_rejected() {
        use super::super::types::ENCODING_RAW;
        let mut data = vec![0u8; 12];
        // cache_id = 0, encoding = RAW
        data[8..12].copy_from_slice(&ENCODING_RAW.to_be_bytes());

        let mut stream = RfbInStream::new(Cursor::new(data));
        let result = CachedRectInit::read_from(&mut stream).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("non-zero"));
    }

    #[tokio::test]
    async fn test_cached_rect_init_recursive_encoding_rejected() {
        use super::super::types::{ENCODING_CACHED_RECT, ENCODING_CACHED_RECT_INIT};

        // Test ENCODING_CACHED_RECT
        let mut data = vec![0u8; 12];
        let cache_id: u64 = 12345;
        data[0..8].copy_from_slice(&cache_id.to_be_bytes());
        data[8..12].copy_from_slice(&ENCODING_CACHED_RECT.to_be_bytes());

        let mut stream = RfbInStream::new(Cursor::new(data));
        let result = CachedRectInit::read_from(&mut stream).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("cannot be a cache encoding"));

        // Test ENCODING_CACHED_RECT_INIT
        let mut data = vec![0u8; 12];
        data[0..8].copy_from_slice(&cache_id.to_be_bytes());
        data[8..12].copy_from_slice(&ENCODING_CACHED_RECT_INIT.to_be_bytes());

        let mut stream = RfbInStream::new(Cursor::new(data));
        let result = CachedRectInit::read_from(&mut stream).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_cached_rect_init_all_standard_encodings() {
        use super::super::types::*;

        let encodings = vec![
            ENCODING_RAW,
            ENCODING_COPYRECT,
            ENCODING_RRE,
            ENCODING_HEXTILE,
            ENCODING_TIGHT,
            ENCODING_ZRLE,
        ];

        for encoding in encodings {
            let init = CachedRectInit::new(99999, encoding);

            // Write to buffer
            let mut buffer = Vec::new();
            let mut out_stream = RfbOutStream::new(&mut buffer);
            init.write_to(&mut out_stream).unwrap();
            out_stream.flush().await.unwrap();

            // Read back
            let mut in_stream = RfbInStream::new(Cursor::new(buffer));
            let read_back = CachedRectInit::read_from(&mut in_stream).await.unwrap();

            assert_eq!(init, read_back);
            assert_eq!(read_back.actual_encoding, encoding);
        }
    }

    #[tokio::test]
    async fn test_cached_rect_large_cache_id() {
        // Test with maximum u64 value
        let original = CachedRect::new(u64::MAX);

        let mut buffer = Vec::new();
        let mut out_stream = RfbOutStream::new(&mut buffer);
        original.write_to(&mut out_stream).unwrap();
        out_stream.flush().await.unwrap();

        let mut in_stream = RfbInStream::new(Cursor::new(buffer));
        let read_back = CachedRect::read_from(&mut in_stream).await.unwrap();

        assert_eq!(original, read_back);
    }
}
