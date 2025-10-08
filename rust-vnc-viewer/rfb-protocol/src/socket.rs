//! Socket abstractions for VNC connections.
//!
//! This module provides a unified interface for different socket types (TCP, Unix domain)
//! used in VNC client connections. All sockets implement the [`VncSocket`] trait which
//! provides common functionality like async I/O and peer address information.
//!
//! # Examples
//!
//! ```no_run
//! use rfb_protocol::socket::{TcpSocket, VncSocket};
//!
//! # async fn example() -> anyhow::Result<()> {
//! // Connect to a VNC server via TCP
//! let socket = TcpSocket::connect("localhost", 5900).await?;
//! println!("Connected to: {}", socket.peer_endpoint());
//! # Ok(())
//! # }
//! ```
//!
//! ```no_run
//! # #[cfg(unix)]
//! # async fn example() -> anyhow::Result<()> {
//! use rfb_protocol::socket::{UnixSocket, VncSocket};
//!
//! // Connect to a VNC server via Unix domain socket
//! let socket = UnixSocket::connect("/tmp/vnc.sock").await?;
//! println!("Connected to: {}", socket.peer_endpoint());
//! # Ok(())
//! # }
//! ```

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpStream, UnixStream};

/// Core trait for VNC socket connections.
///
/// This trait extends [`AsyncRead`] and [`AsyncWrite`] with VNC-specific functionality
/// for querying connection information. All socket types (TCP, Unix domain) implement
/// this trait, allowing them to be used interchangeably in the VNC protocol stack.
///
/// # Requirements
///
/// - `AsyncRead + AsyncWrite`: For async I/O operations
/// - `Send`: Can be moved across thread boundaries
/// - `Unpin`: Not bound by self-referential requirements
pub trait VncSocket: AsyncRead + AsyncWrite + Send + Unpin {
    /// Get the peer address as a human-readable string.
    ///
    /// For TCP sockets, this returns the IP address (e.g., "192.168.1.100").
    /// For Unix domain sockets, this returns the socket path.
    fn peer_address(&self) -> String;

    /// Get the peer endpoint including port/path information.
    ///
    /// For TCP sockets, this returns "address:port" (e.g., "192.168.1.100:5900").
    /// For Unix domain sockets, this returns "unix:path" (e.g., "unix:/tmp/vnc.sock").
    fn peer_endpoint(&self) -> String;

    /// Get the raw file descriptor for platform-specific operations.
    ///
    /// Returns `Some(fd)` on Unix-like systems, `None` on other platforms.
    /// This is useful for low-level operations like setting socket options.
    #[cfg(unix)]
    fn as_raw_fd(&self) -> Option<std::os::unix::io::RawFd>;
}

/// TCP socket wrapper for VNC connections.
///
/// Provides a thin wrapper around [`TcpStream`] with VNC-specific functionality.
/// The socket is automatically configured with `TCP_NODELAY` for low latency,
/// which is critical for interactive VNC sessions.
///
/// # Examples
///
/// ```no_run
/// use rfb_protocol::socket::TcpSocket;
/// use tokio::io::{AsyncReadExt, AsyncWriteExt};
///
/// # async fn example() -> anyhow::Result<()> {
/// let mut socket = TcpSocket::connect("localhost", 5900).await?;
///
/// // Write RFB version string
/// socket.write_all(b"RFB 003.008\n").await?;
///
/// // Read server version
/// let mut buf = [0u8; 12];
/// socket.read_exact(&mut buf).await?;
/// # Ok(())
/// # }
/// ```
pub struct TcpSocket {
    stream: TcpStream,
    peer_addr: SocketAddr,
}

impl TcpSocket {
    /// Connect to a VNC server via TCP.
    ///
    /// # Arguments
    ///
    /// * `host` - Hostname or IP address of the VNC server
    /// * `port` - Port number (typically 5900 + display number)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - DNS resolution fails
    /// - Connection is refused
    /// - Network is unreachable
    /// - Timeout occurs
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use rfb_protocol::socket::TcpSocket;
    ///
    /// # async fn example() -> anyhow::Result<()> {
    /// // Connect to display :0 (port 5900)
    /// let socket = TcpSocket::connect("192.168.1.100", 5900).await?;
    ///
    /// // Connect to display :1 (port 5901)
    /// let socket = TcpSocket::connect("localhost", 5901).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn connect(host: &str, port: u16) -> anyhow::Result<Self> {
        let addr = format!("{}:{}", host, port);
        let stream = TcpStream::connect(&addr).await?;
        let peer_addr = stream.peer_addr()?;

        // Disable Nagle's algorithm for low latency
        // This ensures small packets (like mouse movements) are sent immediately
        stream.set_nodelay(true)?;

        Ok(Self { stream, peer_addr })
    }

    /// Get the underlying TCP stream.
    ///
    /// This is useful for advanced operations like split read/write or
    /// accessing the raw socket for platform-specific configuration.
    pub fn into_inner(self) -> TcpStream {
        self.stream
    }
}

impl VncSocket for TcpSocket {
    fn peer_address(&self) -> String {
        self.peer_addr.ip().to_string()
    }

    fn peer_endpoint(&self) -> String {
        self.peer_addr.to_string()
    }

    #[cfg(unix)]
    fn as_raw_fd(&self) -> Option<std::os::unix::io::RawFd> {
        use std::os::unix::io::AsRawFd;
        Some(self.stream.as_raw_fd())
    }
}

impl AsyncRead for TcpSocket {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}

impl AsyncWrite for TcpSocket {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

/// Unix domain socket wrapper for VNC connections.
///
/// Provides a thin wrapper around [`UnixStream`] for local VNC connections.
/// Unix domain sockets offer better performance and security for local connections
/// compared to TCP sockets.
///
/// # Platform Support
///
/// Only available on Unix-like systems (Linux, macOS, BSD).
///
/// # Examples
///
/// ```no_run
/// # #[cfg(unix)]
/// # async fn example() -> anyhow::Result<()> {
/// use rfb_protocol::socket::UnixSocket;
/// use tokio::io::{AsyncReadExt, AsyncWriteExt};
///
/// let mut socket = UnixSocket::connect("/tmp/vnc.sock").await?;
/// socket.write_all(b"RFB 003.008\n").await?;
/// # Ok(())
/// # }
/// ```
#[cfg(unix)]
pub struct UnixSocket {
    stream: UnixStream,
    path: PathBuf,
}

#[cfg(unix)]
impl UnixSocket {
    /// Connect to a VNC server via Unix domain socket.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the Unix domain socket file
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Socket file doesn't exist
    /// - Permission denied
    /// - Connection refused
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # #[cfg(unix)]
    /// # async fn example() -> anyhow::Result<()> {
    /// use rfb_protocol::socket::UnixSocket;
    ///
    /// // Connect to a local VNC server via Unix socket
    /// let socket = UnixSocket::connect("/tmp/vnc-session-1.sock").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn connect(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path_ref = path.as_ref();
        let stream = UnixStream::connect(path_ref).await?;
        Ok(Self {
            stream,
            path: path_ref.to_path_buf(),
        })
    }

    /// Get the underlying Unix stream.
    ///
    /// This is useful for advanced operations or accessing the raw socket.
    pub fn into_inner(self) -> UnixStream {
        self.stream
    }
}

#[cfg(unix)]
impl VncSocket for UnixSocket {
    fn peer_address(&self) -> String {
        self.path.display().to_string()
    }

    fn peer_endpoint(&self) -> String {
        format!("unix:{}", self.path.display())
    }

    fn as_raw_fd(&self) -> Option<std::os::unix::io::RawFd> {
        use std::os::unix::io::AsRawFd;
        Some(self.stream.as_raw_fd())
    }
}

#[cfg(unix)]
impl AsyncRead for UnixSocket {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}

#[cfg(unix)]
impl AsyncWrite for UnixSocket {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn test_tcp_socket_connection() {
        // Start a test server
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Spawn server task
        tokio::spawn(async move {
            let (_socket, _addr) = listener.accept().await.unwrap();
            // Server accepts connection and immediately closes
        });

        // Connect client
        let socket = TcpSocket::connect("127.0.0.1", addr.port()).await.unwrap();

        // Verify peer address
        assert_eq!(socket.peer_address(), "127.0.0.1");
        assert!(socket.peer_endpoint().starts_with("127.0.0.1:"));
    }

    #[tokio::test]
    async fn test_tcp_socket_nodelay() {
        // Start a test server
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (_socket, _addr) = listener.accept().await.unwrap();
        });

        // Connect and verify TCP_NODELAY is set
        let socket = TcpSocket::connect("127.0.0.1", addr.port()).await.unwrap();
        let stream = socket.into_inner();
        assert!(stream.nodelay().unwrap());
    }

    #[tokio::test]
    async fn test_tcp_socket_connection_refused() {
        // Try to connect to a port that's not listening
        let result = TcpSocket::connect("127.0.0.1", 1).await;
        assert!(result.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_unix_socket_connection() {
        use tempfile::TempDir;
        use tokio::net::UnixListener;

        // Create temporary directory for socket
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("test.sock");

        // Start a test server
        let listener = UnixListener::bind(&socket_path).unwrap();
        let socket_path_clone = socket_path.clone();

        tokio::spawn(async move {
            let (_socket, _addr) = listener.accept().await.unwrap();
        });

        // Connect client
        let socket = UnixSocket::connect(&socket_path_clone).await.unwrap();

        // Verify peer address
        assert_eq!(
            socket.peer_address(),
            socket_path_clone.display().to_string()
        );
        assert_eq!(
            socket.peer_endpoint(),
            format!("unix:{}", socket_path_clone.display())
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_unix_socket_nonexistent() {
        // Try to connect to a socket that doesn't exist
        let result = UnixSocket::connect("/tmp/nonexistent-socket-12345.sock").await;
        assert!(result.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_raw_fd() {
        use tempfile::TempDir;
        use tokio::net::UnixListener;

        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("test.sock");

        let listener = UnixListener::bind(&socket_path).unwrap();
        let socket_path_clone = socket_path.clone();

        tokio::spawn(async move {
            let (_socket, _addr) = listener.accept().await.unwrap();
        });

        let socket = UnixSocket::connect(&socket_path_clone).await.unwrap();

        // Verify we can get a raw file descriptor
        let fd = socket.as_raw_fd();
        assert!(fd.is_some());
        assert!(fd.unwrap() > 0);
    }
}
