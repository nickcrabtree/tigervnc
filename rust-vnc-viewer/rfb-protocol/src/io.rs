//! Buffered I/O streams for RFB protocol communication.
//!
//! This module provides efficient buffered reading and writing for the RFB protocol,
//! with type-safe methods for reading/writing primitive types in network byte order.
//!
//! # Examples
//!
//! ```no_run
//! use rfb_protocol::io::{RfbInStream, RfbOutStream};
//! use rfb_protocol::TcpSocket;
//! use tokio::io::AsyncWriteExt;
//!
//! # async fn example() -> std::io::Result<()> {
//! let socket = TcpSocket::connect("localhost", 5900).await.unwrap();
//! let (reader, writer) = tokio::io::split(socket);
//!
//! // Reading from RFB stream
//! let mut input = RfbInStream::new(reader);
//! let message_type = input.read_u8().await?;
//! let width = input.read_u16().await?;
//! let height = input.read_u16().await?;
//!
//! // Writing to RFB stream
//! let mut output = RfbOutStream::new(writer);
//! output.write_u8(1); // SetPixelFormat
//! output.write_u16(1024); // width
//! output.write_u16(768); // height
//! output.flush().await?;
//! # Ok(())
//! # }
//! ```

use bytes::{Buf, BufMut, BytesMut};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Buffered input stream for reading RFB protocol data.
///
/// This stream provides efficient buffered reading with methods for reading
/// primitive types in network byte order (big-endian). Data is buffered
/// internally to minimize system calls.
///
/// # Buffer Management
///
/// The stream maintains an internal buffer (default 8KB) that is filled
/// on-demand. Methods like `read_u16()` and `read_u32()` read from this
/// buffer when possible, only performing I/O when the buffer needs refilling.
///
/// # Examples
///
/// ```no_run
/// use rfb_protocol::io::RfbInStream;
/// # async fn example<R: tokio::io::AsyncRead + Unpin>(reader: R) -> std::io::Result<()> {
/// let mut stream = RfbInStream::new(reader);
///
/// // Read RFB version string (12 bytes)
/// let mut version = [0u8; 12];
/// stream.read_bytes(&mut version).await?;
///
/// // Read security type
/// let security_type = stream.read_u8().await?;
///
/// // Skip padding
/// stream.skip(3).await?;
/// # Ok(())
/// # }
/// ```
pub struct RfbInStream<R> {
    reader: R,
    buffer: BytesMut,
}

impl<R: AsyncRead + Unpin> RfbInStream<R> {
    /// Create a new input stream with default buffer size (8KB).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use rfb_protocol::io::RfbInStream;
    /// # async fn example<R: tokio::io::AsyncRead + Unpin>(reader: R) {
    /// let stream = RfbInStream::new(reader);
    /// # }
    /// ```
    pub fn new(reader: R) -> Self {
        Self::with_capacity(reader, 8192)
    }

    /// Create a new input stream with specified buffer capacity.
    ///
    /// # Arguments
    ///
    /// * `reader` - The underlying async reader
    /// * `capacity` - Initial buffer capacity in bytes
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use rfb_protocol::io::RfbInStream;
    /// # async fn example<R: tokio::io::AsyncRead + Unpin>(reader: R) {
    /// // Use larger buffer for high-bandwidth connections
    /// let stream = RfbInStream::with_capacity(reader, 16384);
    /// # }
    /// ```
    pub fn with_capacity(reader: R, capacity: usize) -> Self {
        Self {
            reader,
            buffer: BytesMut::with_capacity(capacity),
        }
    }

    /// Ensure at least `n` bytes are available in the buffer.
    ///
    /// Reads from the underlying reader until the buffer contains at least
    /// `n` bytes. Returns an error if EOF is reached before `n` bytes are
    /// available.
    async fn ensure_bytes(&mut self, n: usize) -> std::io::Result<()> {
        while self.buffer.len() < n {
            let bytes_read = self.reader.read_buf(&mut self.buffer).await?;
            if bytes_read == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    format!("expected {} bytes, got {}", n, self.buffer.len()),
                ));
            }
        }
        Ok(())
    }

    /// Read a single byte (u8).
    ///
    /// # Errors
    ///
    /// Returns an error if EOF is reached or an I/O error occurs.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use rfb_protocol::io::RfbInStream;
    /// # async fn example<R: tokio::io::AsyncRead + Unpin>(mut stream: RfbInStream<R>) -> std::io::Result<()> {
    /// let message_type = stream.read_u8().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn read_u8(&mut self) -> std::io::Result<u8> {
        self.ensure_bytes(1).await?;
        Ok(self.buffer.get_u8())
    }

    /// Read a 16-bit unsigned integer in network byte order (big-endian).
    ///
    /// # Errors
    ///
    /// Returns an error if EOF is reached or an I/O error occurs.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use rfb_protocol::io::RfbInStream;
    /// # async fn example<R: tokio::io::AsyncRead + Unpin>(mut stream: RfbInStream<R>) -> std::io::Result<()> {
    /// let width = stream.read_u16().await?;
    /// let height = stream.read_u16().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn read_u16(&mut self) -> std::io::Result<u16> {
        self.ensure_bytes(2).await?;
        Ok(self.buffer.get_u16())
    }

    /// Read a 32-bit unsigned integer in network byte order (big-endian).
    ///
    /// # Errors
    ///
    /// Returns an error if EOF is reached or an I/O error occurs.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use rfb_protocol::io::RfbInStream;
    /// # async fn example<R: tokio::io::AsyncRead + Unpin>(mut stream: RfbInStream<R>) -> std::io::Result<()> {
    /// let encoding = stream.read_u32().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn read_u32(&mut self) -> std::io::Result<u32> {
        self.ensure_bytes(4).await?;
        Ok(self.buffer.get_u32())
    }

    /// Read a 32-bit signed integer in network byte order (big-endian).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use rfb_protocol::io::RfbInStream;
    /// # async fn example<R: tokio::io::AsyncRead + Unpin>(mut stream: RfbInStream<R>) -> std::io::Result<()> {
    /// let encoding_type = stream.read_i32().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn read_i32(&mut self) -> std::io::Result<i32> {
        self.ensure_bytes(4).await?;
        Ok(self.buffer.get_i32())
    }

    /// Read exactly `buf.len()` bytes into the provided buffer.
    ///
    /// # Errors
    ///
    /// Returns an error if EOF is reached before the buffer is filled,
    /// or if an I/O error occurs.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use rfb_protocol::io::RfbInStream;
    /// # async fn example<R: tokio::io::AsyncRead + Unpin>(mut stream: RfbInStream<R>) -> std::io::Result<()> {
    /// let mut pixel_data = vec![0u8; 1024];
    /// stream.read_bytes(&mut pixel_data).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn read_bytes(&mut self, buf: &mut [u8]) -> std::io::Result<()> {
        self.ensure_bytes(buf.len()).await?;
        self.buffer.copy_to_slice(buf);
        Ok(())
    }

    /// Skip `n` bytes in the stream.
    ///
    /// This is more efficient than reading and discarding data.
    ///
    /// # Errors
    ///
    /// Returns an error if EOF is reached before `n` bytes are skipped,
    /// or if an I/O error occurs.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use rfb_protocol::io::RfbInStream;
    /// # async fn example<R: tokio::io::AsyncRead + Unpin>(mut stream: RfbInStream<R>) -> std::io::Result<()> {
    /// // Skip 3 bytes of padding
    /// stream.skip(3).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn skip(&mut self, n: usize) -> std::io::Result<()> {
        self.ensure_bytes(n).await?;
        self.buffer.advance(n);
        Ok(())
    }

    /// Get the number of bytes currently available in the buffer.
    ///
    /// This indicates how many bytes can be read without performing I/O.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use rfb_protocol::io::RfbInStream;
    /// # fn example<R: tokio::io::AsyncRead + Unpin>(stream: &RfbInStream<R>) {
    /// let buffered = stream.available();
    /// println!("Can read {} bytes without I/O", buffered);
    /// # }
    /// ```
    pub fn available(&self) -> usize {
        self.buffer.len()
    }

    /// Get a reference to the underlying reader.
    pub fn get_ref(&self) -> &R {
        &self.reader
    }

    /// Get a mutable reference to the underlying reader.
    pub fn get_mut(&mut self) -> &mut R {
        &mut self.reader
    }

    /// Consume the stream and return the underlying reader.
    pub fn into_inner(self) -> R {
        self.reader
    }
}

/// Buffered output stream for writing RFB protocol data.
///
/// This stream provides efficient buffered writing with methods for writing
/// primitive types in network byte order (big-endian). Data is buffered
/// internally and only written when [`flush()`](Self::flush) is called.
///
/// # Important: Flushing
///
/// You **must** call [`flush()`](Self::flush) to ensure buffered data is
/// actually sent over the network. Dropping the stream without flushing
/// will lose any buffered data.
///
/// # Examples
///
/// ```no_run
/// use rfb_protocol::io::RfbOutStream;
/// # async fn example<W: tokio::io::AsyncWrite + Unpin>(writer: W) -> std::io::Result<()> {
/// let mut stream = RfbOutStream::new(writer);
///
/// // Buffer writes
/// stream.write_u8(0); // SetPixelFormat message
/// stream.write_u8(0); // padding
/// stream.write_u8(0); // padding
/// stream.write_u8(0); // padding
///
/// // Send all buffered data
/// stream.flush().await?;
/// # Ok(())
/// # }
/// ```
pub struct RfbOutStream<W> {
    writer: W,
    buffer: BytesMut,
}

impl<W: AsyncWrite + Unpin> RfbOutStream<W> {
    /// Create a new output stream with default buffer size (8KB).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use rfb_protocol::io::RfbOutStream;
    /// # fn example<W: tokio::io::AsyncWrite + Unpin>(writer: W) {
    /// let stream = RfbOutStream::new(writer);
    /// # }
    /// ```
    pub fn new(writer: W) -> Self {
        Self::with_capacity(writer, 8192)
    }

    /// Create a new output stream with specified buffer capacity.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use rfb_protocol::io::RfbOutStream;
    /// # fn example<W: tokio::io::AsyncWrite + Unpin>(writer: W) {
    /// let stream = RfbOutStream::with_capacity(writer, 16384);
    /// # }
    /// ```
    pub fn with_capacity(writer: W, capacity: usize) -> Self {
        Self {
            writer,
            buffer: BytesMut::with_capacity(capacity),
        }
    }

    /// Write a single byte (u8).
    ///
    /// The byte is buffered and not sent until [`flush()`](Self::flush) is called.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use rfb_protocol::io::RfbOutStream;
    /// # fn example<W: tokio::io::AsyncWrite + Unpin>(mut stream: RfbOutStream<W>) {
    /// stream.write_u8(42);
    /// # }
    /// ```
    pub fn write_u8(&mut self, value: u8) {
        self.buffer.put_u8(value);
    }

    /// Write a 16-bit unsigned integer in network byte order (big-endian).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use rfb_protocol::io::RfbOutStream;
    /// # fn example<W: tokio::io::AsyncWrite + Unpin>(mut stream: RfbOutStream<W>) {
    /// stream.write_u16(1920); // width
    /// stream.write_u16(1080); // height
    /// # }
    /// ```
    pub fn write_u16(&mut self, value: u16) {
        self.buffer.put_u16(value);
    }

    /// Write a 32-bit unsigned integer in network byte order (big-endian).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use rfb_protocol::io::RfbOutStream;
    /// # fn example<W: tokio::io::AsyncWrite + Unpin>(mut stream: RfbOutStream<W>) {
    /// stream.write_u32(0x12345678);
    /// # }
    /// ```
    pub fn write_u32(&mut self, value: u32) {
        self.buffer.put_u32(value);
    }

    /// Write a 32-bit signed integer in network byte order (big-endian).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use rfb_protocol::io::RfbOutStream;
    /// # fn example<W: tokio::io::AsyncWrite + Unpin>(mut stream: RfbOutStream<W>) {
    /// stream.write_i32(-42);
    /// # }
    /// ```
    pub fn write_i32(&mut self, value: i32) {
        self.buffer.put_i32(value);
    }

    /// Write a byte slice to the buffer.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use rfb_protocol::io::RfbOutStream;
    /// # fn example<W: tokio::io::AsyncWrite + Unpin>(mut stream: RfbOutStream<W>) {
    /// stream.write_bytes(b"RFB 003.008\n");
    /// # }
    /// ```
    pub fn write_bytes(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
    }

    /// Flush all buffered data to the underlying writer.
    ///
    /// This writes all buffered data to the writer and ensures it is sent
    /// (by calling the writer's `flush()` method).
    ///
    /// # Errors
    ///
    /// Returns an error if writing fails or if the underlying writer's
    /// `flush()` method returns an error.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use rfb_protocol::io::RfbOutStream;
    /// # async fn example<W: tokio::io::AsyncWrite + Unpin>(mut stream: RfbOutStream<W>) -> std::io::Result<()> {
    /// stream.write_u8(1);
    /// stream.write_u16(100);
    /// stream.flush().await?; // Send buffered data
    /// # Ok(())
    /// # }
    /// ```
    pub async fn flush(&mut self) -> std::io::Result<()> {
        if !self.buffer.is_empty() {
            self.writer.write_all(&self.buffer).await?;
            self.buffer.clear();
        }
        self.writer.flush().await
    }

    /// Get the number of bytes currently buffered.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use rfb_protocol::io::RfbOutStream;
    /// # fn example<W: tokio::io::AsyncWrite + Unpin>(stream: &RfbOutStream<W>) {
    /// let buffered = stream.buffered();
    /// println!("{} bytes waiting to be flushed", buffered);
    /// # }
    /// ```
    pub fn buffered(&self) -> usize {
        self.buffer.len()
    }

    /// Get a reference to the underlying writer.
    pub fn get_ref(&self) -> &W {
        &self.writer
    }

    /// Get a mutable reference to the underlying writer.
    pub fn get_mut(&mut self) -> &mut W {
        &mut self.writer
    }

    /// Consume the stream and return the underlying writer.
    ///
    /// **Warning:** Any buffered data will be lost. Call [`flush()`](Self::flush)
    /// first if you need to send buffered data.
    pub fn into_inner(self) -> W {
        self.writer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[tokio::test]
    async fn test_read_u8() {
        let data = vec![42u8, 100, 255];
        let mut stream = RfbInStream::new(Cursor::new(data));

        assert_eq!(stream.read_u8().await.unwrap(), 42);
        assert_eq!(stream.read_u8().await.unwrap(), 100);
        assert_eq!(stream.read_u8().await.unwrap(), 255);
    }

    #[tokio::test]
    async fn test_read_u16() {
        let data = vec![0x12, 0x34, 0xAB, 0xCD];
        let mut stream = RfbInStream::new(Cursor::new(data));

        assert_eq!(stream.read_u16().await.unwrap(), 0x1234);
        assert_eq!(stream.read_u16().await.unwrap(), 0xABCD);
    }

    #[tokio::test]
    async fn test_read_u32() {
        let data = vec![0x12, 0x34, 0x56, 0x78];
        let mut stream = RfbInStream::new(Cursor::new(data));

        assert_eq!(stream.read_u32().await.unwrap(), 0x12345678);
    }

    #[tokio::test]
    async fn test_read_i32() {
        let data = vec![0xFF, 0xFF, 0xFF, 0xFE]; // -2 in two's complement
        let mut stream = RfbInStream::new(Cursor::new(data));

        assert_eq!(stream.read_i32().await.unwrap(), -2);
    }

    #[tokio::test]
    async fn test_read_bytes() {
        let data = vec![1, 2, 3, 4, 5];
        let mut stream = RfbInStream::new(Cursor::new(data));

        let mut buf = [0u8; 3];
        stream.read_bytes(&mut buf).await.unwrap();
        assert_eq!(buf, [1, 2, 3]);

        let mut buf = [0u8; 2];
        stream.read_bytes(&mut buf).await.unwrap();
        assert_eq!(buf, [4, 5]);
    }

    #[tokio::test]
    async fn test_skip() {
        let data = vec![1, 2, 3, 4, 5];
        let mut stream = RfbInStream::new(Cursor::new(data));

        stream.skip(2).await.unwrap();
        assert_eq!(stream.read_u8().await.unwrap(), 3);
        stream.skip(1).await.unwrap();
        assert_eq!(stream.read_u8().await.unwrap(), 5);
    }

    #[tokio::test]
    async fn test_available() {
        let data = vec![1, 2, 3, 4, 5];
        let mut stream = RfbInStream::new(Cursor::new(data));

        // Initially no data buffered
        assert_eq!(stream.available(), 0);

        // Reading buffers all available data
        stream.read_u8().await.unwrap();
        assert!(stream.available() > 0);
    }

    #[tokio::test]
    async fn test_read_eof() {
        let data = vec![1, 2];
        let mut stream = RfbInStream::new(Cursor::new(data));

        stream.read_u8().await.unwrap();
        stream.read_u8().await.unwrap();

        // Should fail with UnexpectedEof
        let result = stream.read_u8().await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::UnexpectedEof);
    }

    #[tokio::test]
    async fn test_write_u8() {
        let mut buffer = Vec::new();
        let mut stream = RfbOutStream::new(&mut buffer);

        stream.write_u8(42);
        stream.write_u8(100);
        stream.flush().await.unwrap();

        assert_eq!(buffer, vec![42, 100]);
    }

    #[tokio::test]
    async fn test_write_u16() {
        let mut buffer = Vec::new();
        let mut stream = RfbOutStream::new(&mut buffer);

        stream.write_u16(0x1234);
        stream.write_u16(0xABCD);
        stream.flush().await.unwrap();

        assert_eq!(buffer, vec![0x12, 0x34, 0xAB, 0xCD]);
    }

    #[tokio::test]
    async fn test_write_u32() {
        let mut buffer = Vec::new();
        let mut stream = RfbOutStream::new(&mut buffer);

        stream.write_u32(0x12345678);
        stream.flush().await.unwrap();

        assert_eq!(buffer, vec![0x12, 0x34, 0x56, 0x78]);
    }

    #[tokio::test]
    async fn test_write_i32() {
        let mut buffer = Vec::new();
        let mut stream = RfbOutStream::new(&mut buffer);

        stream.write_i32(-2);
        stream.flush().await.unwrap();

        assert_eq!(buffer, vec![0xFF, 0xFF, 0xFF, 0xFE]);
    }

    #[tokio::test]
    async fn test_write_bytes() {
        let mut buffer = Vec::new();
        let mut stream = RfbOutStream::new(&mut buffer);

        stream.write_bytes(b"Hello");
        stream.flush().await.unwrap();

        assert_eq!(buffer, b"Hello");
    }

    #[tokio::test]
    async fn test_buffered() {
        let mut buffer = Vec::new();
        let mut stream = RfbOutStream::new(&mut buffer);

        assert_eq!(stream.buffered(), 0);

        stream.write_u8(1);
        assert_eq!(stream.buffered(), 1);

        stream.write_u16(0x1234);
        assert_eq!(stream.buffered(), 3);

        stream.flush().await.unwrap();
        assert_eq!(stream.buffered(), 0);
    }

    #[tokio::test]
    async fn test_round_trip() {
        let mut buffer = Vec::new();

        // Write data
        {
            let mut out = RfbOutStream::new(&mut buffer);
            out.write_u8(42);
            out.write_u16(0x1234);
            out.write_u32(0xDEADBEEF);
            out.write_bytes(b"test");
            out.flush().await.unwrap();
        }

        // Read it back
        {
            let mut inp = RfbInStream::new(Cursor::new(&buffer));
            assert_eq!(inp.read_u8().await.unwrap(), 42);
            assert_eq!(inp.read_u16().await.unwrap(), 0x1234);
            assert_eq!(inp.read_u32().await.unwrap(), 0xDEADBEEF);
            let mut buf = [0u8; 4];
            inp.read_bytes(&mut buf).await.unwrap();
            assert_eq!(&buf, b"test");
        }
    }
}
