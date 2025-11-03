//! Counting reader wrapper for tracking byte consumption during decoding.
//!
//! This module provides utilities for debugging protocol framing issues by
//! tracking exactly how many bytes are read from a stream.

use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, ReadBuf};

/// A wrapper around an AsyncRead that counts the total number of bytes read.
///
/// This is useful for debugging protocol framing issues, as it allows tracking
/// how many bytes each decoder consumes from the stream.
///
/// # Examples
///
/// ```no_run
/// use rfb_protocol::io::counting::CountingReader;
/// use tokio::io::AsyncReadExt;
///
/// # async fn example() -> std::io::Result<()> {
/// let data = &b"Hello, world!"[..];
/// let mut counting = CountingReader::new(data);
///
/// let mut buf = [0u8; 5];
/// counting.read_exact(&mut buf).await?;
/// assert_eq!(counting.bytes_read(), 5);
///
/// counting.read_exact(&mut buf).await?;
/// assert_eq!(counting.bytes_read(), 10);
/// # Ok(())
/// # }
/// ```
pub struct CountingReader<R> {
    inner: R,
    bytes_read: Arc<AtomicU64>,
}

impl<R> CountingReader<R> {
    /// Create a new CountingReader wrapping the given reader.
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            bytes_read: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Get the total number of bytes read so far.
    pub fn bytes_read(&self) -> u64 {
        self.bytes_read.load(Ordering::Relaxed)
    }

    /// Reset the byte counter to zero.
    pub fn reset_counter(&self) {
        self.bytes_read.store(0, Ordering::Relaxed);
    }

    /// Get a clone of the counter handle that can be used to query byte count
    /// from other contexts.
    pub fn counter(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.bytes_read)
    }

    /// Unwrap the CountingReader and return the inner reader.
    pub fn into_inner(self) -> R {
        self.inner
    }

    /// Get a reference to the inner reader.
    pub fn get_ref(&self) -> &R {
        &self.inner
    }

    /// Get a mutable reference to the inner reader.
    pub fn get_mut(&mut self) -> &mut R {
        &mut self.inner
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for CountingReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        let result = Pin::new(&mut self.inner).poll_read(cx, buf);
        let after = buf.filled().len();
        let bytes_read = (after - before) as u64;
        self.bytes_read.fetch_add(bytes_read, Ordering::Relaxed);
        result
    }
}

/// Helper function to read exactly n bytes into a Vec.
///
/// This is useful for bounded decoding where you need to read a specific
/// number of bytes into memory before processing them.
///
/// # Examples
///
/// ```no_run
/// use rfb_protocol::io::counting::read_exact_to_vec;
/// use tokio::io::AsyncRead;
///
/// # async fn example<R: AsyncRead + Unpin>(reader: &mut R) -> std::io::Result<()> {
/// // Read 100 bytes into a Vec
/// let data = read_exact_to_vec(reader, 100).await?;
/// assert_eq!(data.len(), 100);
/// # Ok(())
/// # }
/// ```
pub async fn read_exact_to_vec<R: AsyncRead + Unpin>(
    reader: &mut R,
    len: usize,
) -> io::Result<Vec<u8>> {
    let mut buf = vec![0u8; len];
    tokio::io::AsyncReadExt::read_exact(reader, &mut buf).await?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn test_counting_reader_tracks_bytes() {
        let data = b"Hello, world!";
        let mut counting = CountingReader::new(&data[..]);

        // Read 5 bytes
        let mut buf = [0u8; 5];
        counting.read_exact(&mut buf).await.unwrap();
        assert_eq!(counting.bytes_read(), 5);
        assert_eq!(&buf, b"Hello");

        // Read 2 more bytes
        let mut buf2 = [0u8; 2];
        counting.read_exact(&mut buf2).await.unwrap();
        assert_eq!(counting.bytes_read(), 7);
        assert_eq!(&buf2, b", ");
    }

    #[tokio::test]
    async fn test_counting_reader_reset() {
        let data = b"Hello, world!";
        let mut counting = CountingReader::new(&data[..]);

        let mut buf = [0u8; 5];
        counting.read_exact(&mut buf).await.unwrap();
        assert_eq!(counting.bytes_read(), 5);

        counting.reset_counter();
        assert_eq!(counting.bytes_read(), 0);

        // Reads after reset continue to count from zero
        counting.read_exact(&mut buf).await.unwrap();
        assert_eq!(counting.bytes_read(), 5);
    }

    #[tokio::test]
    async fn test_read_exact_to_vec() {
        let data = b"Hello, world!";
        let mut reader = &data[..];

        let vec = read_exact_to_vec(&mut reader, 5).await.unwrap();
        assert_eq!(vec, b"Hello");
        assert_eq!(vec.len(), 5);

        let vec2 = read_exact_to_vec(&mut reader, 2).await.unwrap();
        assert_eq!(vec2, b", ");
    }

    #[tokio::test]
    async fn test_read_exact_to_vec_eof() {
        let data = b"Hi";
        let mut reader = &data[..];

        // Try to read more bytes than available
        let result = read_exact_to_vec(&mut reader, 10).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
    }

    #[tokio::test]
    async fn test_counter_handle() {
        let data = b"Hello, world!";
        let mut counting = CountingReader::new(&data[..]);

        // Get a handle to the counter
        let counter_handle = counting.counter();

        let mut buf = [0u8; 5];
        counting.read_exact(&mut buf).await.unwrap();

        // Counter can be read from the handle
        assert_eq!(counter_handle.load(Ordering::Relaxed), 5);
    }
}
