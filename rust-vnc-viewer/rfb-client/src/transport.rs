//! Transport layer (TCP and TLS) for VNC connections.
//!
//! This module provides the low-level transport abstractions for RFB connections,
//! supporting both plain TCP and TLS-encrypted connections.
//!
//! # Examples
//!
//! ## Plain TCP Connection
//!
//! ```no_run
//! use rfb_client::transport::Transport;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let transport = Transport::connect_tcp("localhost", 5900).await?;
//! // Use transport for RFB communication
//! # Ok(())
//! # }
//! ```
//!
//! ## TLS Connection
//!
//! ```no_run
//! use rfb_client::transport::{Transport, TlsConfig};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let tls_config = TlsConfig::default();
//! let transport = Transport::connect_tls("localhost", 5900, tls_config).await?;
//! // Use transport for secure RFB communication
//! # Ok(())
//! # }
//! ```

use crate::errors::RfbClientError;
use rfb_protocol::io::{RfbInStream, RfbOutStream};
use rustls::pki_types::ServerName;
use rustls::RootCertStore;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite, ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

/// TLS configuration for secure VNC connections.
///
/// This struct configures the TLS client, including certificate validation
/// and supported protocol versions.
///
/// # Examples
///
/// ```
/// use rfb_client::transport::TlsConfig;
///
/// // Use system root certificates (recommended)
/// let config = TlsConfig::default();
///
/// // Disable certificate verification (insecure, for testing only)
/// let insecure = TlsConfig::new().disable_verification();
/// ```
#[derive(Clone, Debug)]
pub struct TlsConfig {
    /// Verify server certificates (should always be true in production)
    pub verify_certificates: bool,
    /// Optional custom root certificates (in addition to system roots)
    pub custom_roots: Vec<Vec<u8>>,
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl TlsConfig {
    /// Create a new TLS configuration with secure defaults.
    ///
    /// Certificate verification is enabled by default.
    pub fn new() -> Self {
        Self {
            verify_certificates: true,
            custom_roots: Vec::new(),
        }
    }

    /// Disable certificate verification.
    ///
    /// # Security Warning
    ///
    /// This is **insecure** and should only be used for testing or development.
    /// Disabling verification makes the connection vulnerable to man-in-the-middle attacks.
    pub fn disable_verification(mut self) -> Self {
        self.verify_certificates = false;
        self
    }

    /// Add a custom root certificate (PEM or DER encoded).
    ///
    /// This certificate will be trusted in addition to the system root certificates.
    pub fn add_root_certificate(mut self, cert: Vec<u8>) -> Self {
        self.custom_roots.push(cert);
        self
    }
}

/// Transport layer for VNC connections.
///
/// This enum represents either a plain TCP connection or a TLS-encrypted connection.
/// It provides unified access to the underlying streams via `RfbInStream` and `RfbOutStream`.
///
/// # Connection Types
///
/// - **Plain**: Unencrypted TCP connection
/// - **Tls**: TLS-encrypted connection
///
/// # Examples
///
/// ```no_run
/// use rfb_client::transport::Transport;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // Connect via plain TCP
/// let transport = Transport::connect_tcp("10.0.0.5", 5901).await?;
///
/// // Split into input and output streams
/// let (input, output) = transport.split();
/// # Ok(())
/// # }
/// ```
pub enum Transport {
    /// Plain TCP connection (unencrypted)
    Plain(PlainTransport),
    /// TLS-encrypted connection
    Tls(TlsTransport),
}

/// Plain TCP transport.
pub struct PlainTransport {
    stream: TcpStream,
}

/// TLS-encrypted transport.
pub struct TlsTransport {
    stream: tokio_rustls::client::TlsStream<TcpStream>,
}

impl Transport {
    /// Connect to a VNC server via plain TCP.
    ///
    /// This establishes an unencrypted TCP connection to the specified host and port.
    /// TCP_NODELAY is enabled for low-latency communication.
    ///
    /// # Arguments
    ///
    /// * `host` - Server hostname or IP address
    /// * `port` - Server port (typically 5900-5999)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - DNS resolution fails
    /// - Connection is refused
    /// - Network is unreachable
    /// - TCP_NODELAY cannot be set
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use rfb_client::transport::Transport;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let transport = Transport::connect_tcp("localhost", 5900).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn connect_tcp(host: &str, port: u16) -> Result<Self, RfbClientError> {
        let addr = format!("{}:{}", host, port);
        let stream = TcpStream::connect(&addr).await.map_err(|e| {
            RfbClientError::ConnectionFailed(format!("Failed to connect to {}: {}", addr, e))
        })?;

        // Enable TCP_NODELAY for low-latency VNC protocol
        stream.set_nodelay(true).map_err(|e| {
            RfbClientError::ConnectionFailed(format!("Failed to set TCP_NODELAY: {}", e))
        })?;

        // Log local and remote addresses for correlation with server logs
        if let (Ok(local), Ok(peer)) = (stream.local_addr(), stream.peer_addr()) {
            tracing::info!("Connected via TCP: local={} -> remote={}", local, peer);
        } else {
            tracing::info!("Connected to {} via plain TCP", addr);
        }
        Ok(Transport::Plain(PlainTransport { stream }))
    }

    /// Connect to a VNC server via TLS.
    ///
    /// This establishes a TLS-encrypted connection to the specified host and port.
    /// The underlying TCP connection has TCP_NODELAY enabled.
    ///
    /// # Arguments
    ///
    /// * `host` - Server hostname or IP address (used for SNI and certificate validation)
    /// * `port` - Server port (typically 5900-5999)
    /// * `tls_config` - TLS configuration including certificate validation settings
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - TCP connection fails
    /// - TLS handshake fails
    /// - Certificate verification fails (when enabled)
    /// - Invalid hostname for SNI
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use rfb_client::transport::{Transport, TlsConfig};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let tls_config = TlsConfig::default();
    /// let transport = Transport::connect_tls("secure.example.com", 5900, tls_config).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn connect_tls(
        host: &str,
        port: u16,
        tls_config: TlsConfig,
    ) -> Result<Self, RfbClientError> {
        let addr = format!("{}:{}", host, port);

        // Establish TCP connection first
        let stream = TcpStream::connect(&addr).await.map_err(|e| {
            RfbClientError::ConnectionFailed(format!("Failed to connect to {}: {}", addr, e))
        })?;

        stream.set_nodelay(true).map_err(|e| {
            RfbClientError::ConnectionFailed(format!("Failed to set TCP_NODELAY: {}", e))
        })?;

        // Build TLS client configuration
        let config = if tls_config.verify_certificates {
            // Secure configuration with certificate verification
            let mut root_store = RootCertStore::empty();

            // Load system root certificates
            let native_certs = rustls_native_certs::load_native_certs().map_err(|e| {
                RfbClientError::TlsError(format!("Failed to load system certificates: {}", e))
            })?;

            for cert in native_certs {
                root_store.add(cert).map_err(|e| {
                    RfbClientError::TlsError(format!("Invalid system certificate: {}", e))
                })?;
            }

            // Add custom roots if provided
            for cert_bytes in &tls_config.custom_roots {
                let cert = rustls::pki_types::CertificateDer::from(cert_bytes.clone());
                root_store.add(cert).map_err(|e| {
                    RfbClientError::TlsError(format!("Invalid custom certificate: {}", e))
                })?;
            }

            rustls::ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_no_client_auth()
        } else {
            // Insecure configuration without certificate verification
            tracing::warn!("TLS certificate verification is DISABLED - insecure!");

            rustls::ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(NoCertificateVerification))
                .with_no_client_auth()
        };

        let connector = TlsConnector::from(Arc::new(config));

        // Parse server name for SNI
        let server_name = ServerName::try_from(host.to_string()).map_err(|e| {
            RfbClientError::TlsError(format!("Invalid hostname '{}': {}", host, e))
        })?;

        // Perform TLS handshake
        let tls_stream = connector.connect(server_name, stream).await.map_err(|e| {
            RfbClientError::TlsError(format!("TLS handshake failed: {}", e))
        })?;

        if let (Ok(local), Ok(peer)) = (tls_stream.get_ref().0.local_addr(), tls_stream.get_ref().0.peer_addr()) {
            tracing::info!("Connected via TLS: local={} -> remote={}", local, peer);
        } else {
            tracing::info!("Connected to {} via TLS", addr);
        }
        Ok(Transport::Tls(TlsTransport { stream: tls_stream }))
    }

    /// Split the transport into separate input and output streams.
    ///
    /// This allows simultaneous reading and writing on different tasks.
    ///
    /// # Returns
    ///
    /// A tuple of `(RfbInStream, RfbOutStream)` for reading and writing RFB protocol data.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use rfb_client::transport::Transport;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let transport = Transport::connect_tcp("localhost", 5900).await?;
    /// let (input, output) = transport.split();
    ///
    /// // Use input and output streams in separate tasks
    /// tokio::spawn(async move {
    ///     // Read from input stream
    /// });
    ///
    /// tokio::spawn(async move {
    ///     // Write to output stream
    /// });
    /// # Ok(())
    /// # }
    /// ```
    pub fn split(self) -> (RfbInStream<TransportRead>, RfbOutStream<TransportWrite>) {
        match self {
            Transport::Plain(plain) => {
                let (read, write) = tokio::io::split(plain.stream);
                (
                    RfbInStream::new(TransportRead::Plain(read)),
                    RfbOutStream::new(TransportWrite::Plain(write)),
                )
            }
            Transport::Tls(tls) => {
                let (read, write) = tokio::io::split(tls.stream);
                (
                    RfbInStream::new(TransportRead::Tls(read)),
                    RfbOutStream::new(TransportWrite::Tls(write)),
                )
            }
        }
    }
}

/// Read half of a transport (plain TCP or TLS).
///
/// This enum wraps the read half of either a TCP stream or TLS stream,
/// providing a unified `AsyncRead` implementation.
pub enum TransportRead {
    /// Plain TCP read stream
    Plain(ReadHalf<TcpStream>),
    /// TLS read stream
    Tls(ReadHalf<tokio_rustls::client::TlsStream<TcpStream>>),
}

impl AsyncRead for TransportRead {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match &mut *self {
            TransportRead::Plain(stream) => std::pin::Pin::new(stream).poll_read(cx, buf),
            TransportRead::Tls(stream) => std::pin::Pin::new(stream).poll_read(cx, buf),
        }
    }
}

/// Write half of a transport (plain TCP or TLS).
///
/// This enum wraps the write half of either a TCP stream or TLS stream,
/// providing a unified `AsyncWrite` implementation.
pub enum TransportWrite {
    /// Plain TCP write stream
    Plain(WriteHalf<TcpStream>),
    /// TLS write stream
    Tls(WriteHalf<tokio_rustls::client::TlsStream<TcpStream>>),
}

impl AsyncWrite for TransportWrite {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match &mut *self {
            TransportWrite::Plain(stream) => std::pin::Pin::new(stream).poll_write(cx, buf),
            TransportWrite::Tls(stream) => std::pin::Pin::new(stream).poll_write(cx, buf),
        }
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match &mut *self {
            TransportWrite::Plain(stream) => std::pin::Pin::new(stream).poll_flush(cx),
            TransportWrite::Tls(stream) => std::pin::Pin::new(stream).poll_flush(cx),
        }
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match &mut *self {
            TransportWrite::Plain(stream) => std::pin::Pin::new(stream).poll_shutdown(cx),
            TransportWrite::Tls(stream) => std::pin::Pin::new(stream).poll_shutdown(cx),
        }
    }
}

/// Certificate verifier that accepts all certificates (INSECURE!).
///
/// This is only used when certificate verification is explicitly disabled.
/// Should never be used in production.
#[derive(Debug)]
struct NoCertificateVerification;

impl rustls::client::danger::ServerCertVerifier for NoCertificateVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        // Accept all certificates without verification (INSECURE!)
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tls_config_defaults() {
        let config = TlsConfig::default();
        assert!(config.verify_certificates);
        assert!(config.custom_roots.is_empty());
    }

    #[test]
    fn test_tls_config_disable_verification() {
        let config = TlsConfig::new().disable_verification();
        assert!(!config.verify_certificates);
    }

    #[test]
    fn test_tls_config_add_root_certificate() {
        let cert = vec![0x30, 0x82]; // Fake cert bytes
        let config = TlsConfig::new().add_root_certificate(cert.clone());
        assert_eq!(config.custom_roots.len(), 1);
        assert_eq!(config.custom_roots[0], cert);
    }

    // Note: Full connection tests require a running VNC server
    // and are better suited for integration tests
}
