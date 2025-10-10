//! High-level connection management and handshake.
//!
//! Establishes a transport (TCP or TLS), performs the RFB version and security
//! handshakes, sends ClientInit, and reads ServerInit. Returns buffered RFB
//! input/output streams ready for normal operation.

use crate::{
    config::{Config, SecurityConfig},
    errors::RfbClientError,
    protocol,
    transport::{self, Transport},
};
use rfb_protocol::handshake::{negotiate_security, negotiate_version, NegotiatedVersion};
use rfb_protocol::io::{RfbInStream, RfbOutStream};
use rfb_protocol::messages::ServerInit;
use tokio::io::{AsyncRead, AsyncWrite};

/// Connected RFB session components.
pub struct Connection<R, W> {
    /// Buffered input stream for reading RFB data.
    pub input: RfbInStream<R>,
    /// Buffered output stream for writing RFB data.
    pub output: RfbOutStream<W>,
    /// Negotiated protocol version.
    pub version: NegotiatedVersion,
    /// Initial server parameters (framebuffer size, pixel format, name).
    pub server_init: ServerInit,
}

impl<R: AsyncRead + Unpin, W: AsyncWrite + Unpin> Connection<R, W> {
    /// Returns the negotiated framebuffer width and height.
    #[must_use]
    pub fn size(&self) -> (u16, u16) {
        (
            self.server_init.framebuffer_width,
            self.server_init.framebuffer_height,
        )
    }
}

/// Establish a new RFB connection using the given configuration.
///
/// Steps:
/// 1) Create transport (TCP or TLS)
/// 2) Split into read/write halves and wrap with RfbInStream/RfbOutStream
/// 3) Negotiate version (send client version)
/// 4) Negotiate security (currently supports None only per rfb-protocol)
/// 5) Send ClientInit (shared session)
/// 6) Read ServerInit (framebuffer params)
pub async fn establish(config: &Config) -> Result<Connection<impl AsyncRead + Unpin, impl AsyncWrite + Unpin>, RfbClientError> {
    // 1) Transport
    let host = &config.connection.host;
    let port = config.connection.port;

    let transport = if use_tls(&config.security) {
        let tls_cfg = to_transport_tls_config(&config.security);
        Transport::connect_tls(host, port, tls_cfg).await?
    } else {
        Transport::connect_tcp(host, port).await?
    };

    // 2) Streams
let (mut input, mut output) = transport.split();

    // 3) Version negotiation
    let version = negotiate_version(&mut input, &mut output)
        .await
        .map_err(|e| RfbClientError::Handshake(format!("version negotiation failed: {}", e)))?;

    // 4) Security negotiation (None only as of rfb-protocol impl)
    negotiate_security(&mut input, &mut output, version)
        .await
        .map_err(|e| RfbClientError::Security(format!("security negotiation failed: {}", e)))?;

    // 5) ClientInit (shared = true)
    protocol::write_client_init(&mut output, true).await?;

    // 6) ServerInit
    let server_init = ServerInit::read_from(&mut input)
        .await
        .map_err(|e| RfbClientError::Protocol(format!("failed to read ServerInit: {}", e)))?;

    Ok(Connection {
        input,
        output,
        version,
        server_init,
    })
}

fn use_tls(security: &SecurityConfig) -> bool {
    match &security.tls {
        Some(t) => t.enabled,
        None => false,
    }
}

fn to_transport_tls_config(_security: &SecurityConfig) -> transport::TlsConfig {
    // Minimum viable mapping for now:
    // - honor danger_accept_invalid_certs by disabling verification
    // - system roots otherwise
    let mut cfg = transport::TlsConfig::new();
    if let Some(tls) = &_security.tls {
        if tls.danger_accept_invalid_certs {
            cfg = cfg.disable_verification();
        }
        // Future: load custom roots from tls.ca_file if provided
    }
    cfg
}
