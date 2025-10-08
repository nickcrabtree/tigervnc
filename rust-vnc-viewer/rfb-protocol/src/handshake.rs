//! RFB protocol handshake implementation.
//!
//! This module implements the three-phase RFB (Remote Framebuffer) protocol handshake
//! according to the RFB Protocol 3.8 specification:
//!
//! 1. **Protocol Version Negotiation** - Client and server agree on RFB version (3.3 or 3.8)
//! 2. **Security Handshake** - Negotiate and execute security/authentication type
//! 3. **Initialization** - Exchange ClientInit/ServerInit messages
//!
//! # Supported Protocol Versions
//!
//! - **RFB 3.8** - Full support (preferred)
//! - **RFB 3.3** - Basic compatibility support
//!
//! The client always advertises RFB 3.8 but will negotiate down to 3.3 if the server
//! only supports 3.3-3.6. This maintains compatibility while preferring modern features.
//!
//! # Security Types
//!
//! **Current Implementation**: Only `SecurityType::None` (value 1) is supported.
//! This is suitable for connections over SSH tunnels or trusted networks.
//!
//! **Future**: VNC Authentication (type 2), VeNCrypt/TLS, and other security types
//! may be added in later phases.
//!
//! # Wire Format
//!
//! All multi-byte integers use **big-endian** (network byte order) per RFB specification.
//!
//! # Error Handling
//!
//! This module follows the project's **fail-fast** policy:
//! - Invalid protocol versions are rejected immediately
//! - Unsupported security types cause connection failure
//! - Malformed messages result in clear error messages
//! - No defensive fallbacks or silent degradation
//!
//! # References
//!
//! - [RFB Protocol 3.8 Specification](https://github.com/rfbproto/rfbproto/blob/master/rfbproto.rst)
//! - TigerVNC CConnection.cxx implementation (C++ reference)

use crate::io::{RfbInStream, RfbOutStream};
use crate::messages;
use tokio::io::{AsyncRead, AsyncWrite};

/// RFB protocol version string sent by client.
///
/// We always send version 3.8 as our preferred version, but will negotiate
/// down to 3.3 if the server reports 3.3-3.6.
const CLIENT_VERSION_BYTES: &[u8; 12] = b"RFB 003.008\n";

/// Security type constant for no authentication (connection over trusted network).
const SECURITY_TYPE_NONE: u8 = 1;

/// Negotiated RFB protocol version after handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NegotiatedVersion {
    /// RFB 3.3 - Original protocol with limited security options
    V3_3,
    /// RFB 3.8 - Modern protocol with improved security negotiation
    V3_8,
}

/// Negotiate RFB protocol version with the server.
pub async fn negotiate_version<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    instream: &mut RfbInStream<R>,
    outstream: &mut RfbOutStream<W>,
) -> std::io::Result<NegotiatedVersion> {
    // Read server version string (exactly 12 bytes)
    let mut version_buf = [0u8; 12];
    instream.read_bytes(&mut version_buf).await?;

    // Validate format: "RFB xxx.yyy\n"
    if &version_buf[0..4] != b"RFB " || version_buf[11] != b'\n' || version_buf[7] != b'.' {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "invalid RFB version string: expected 'RFB xxx.yyy\\n', got {:?}",
                String::from_utf8_lossy(&version_buf)
            ),
        ));
    }

    // Parse major and minor version numbers
    let major_str = std::str::from_utf8(&version_buf[4..7]).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid major version digits: {}", e),
        )
    })?;

    let minor_str = std::str::from_utf8(&version_buf[8..11]).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid minor version digits: {}", e),
        )
    })?;

    let major: u32 = major_str.parse().map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid major version number: {}", e),
        )
    })?;

    let minor: u32 = minor_str.parse().map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid minor version number: {}", e),
        )
    })?;

    // Validate server version is supported (>= 3.3)
    if major < 3 || (major == 3 && minor < 3) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            format!(
                "unsupported RFB version {}.{} (< 003.003)",
                major, minor
            ),
        ));
    }

    // Determine negotiated version
    let negotiated = if major == 3 && minor < 7 {
        NegotiatedVersion::V3_3
    } else {
        NegotiatedVersion::V3_8
    };

    // Always send RFB 3.8 as client version
    outstream.write_bytes(CLIENT_VERSION_BYTES);
    outstream.flush().await?;

    Ok(negotiated)
}

/// Negotiate security type with the server.
pub async fn negotiate_security<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    instream: &mut RfbInStream<R>,
    outstream: &mut RfbOutStream<W>,
    negotiated: NegotiatedVersion,
) -> std::io::Result<()> {
    match negotiated {
        NegotiatedVersion::V3_8 => negotiate_security_3_8(instream, outstream).await,
        NegotiatedVersion::V3_3 => negotiate_security_3_3(instream).await,
    }
}

async fn negotiate_security_3_8<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    instream: &mut RfbInStream<R>,
    outstream: &mut RfbOutStream<W>,
) -> std::io::Result<()> {
    let count = instream.read_u8().await?;
    
    if count == 0 {
        let reason_len = instream.read_u32().await? as usize;
        let mut reason_buf = vec![0u8; reason_len];
        instream.read_bytes(&mut reason_buf).await?;
        let reason = String::from_utf8_lossy(&reason_buf);
        return Err(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            format!("server offered no security types: {}", reason),
        ));
    }

    let mut types = vec![0u8; count as usize];
    instream.read_bytes(&mut types).await?;

    if !types.contains(&SECURITY_TYPE_NONE) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            format!(
                "no supported security types offered by server (got {:?}, only None=1 supported)",
                types
            ),
        ));
    }

    outstream.write_u8(SECURITY_TYPE_NONE);
    outstream.flush().await?;

    let result = instream.read_u32().await?;
    match result {
        0 => Ok(()),
        1 => {
            let reason_len = instream.read_u32().await? as usize;
            let mut reason_buf = vec![0u8; reason_len];
            instream.read_bytes(&mut reason_buf).await?;
            let reason = String::from_utf8_lossy(&reason_buf);
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("security handshake failed: {}", reason),
            ))
        }
        other => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid security result value: {} (expected 0 or 1)", other),
        )),
    }
}

async fn negotiate_security_3_3<R: AsyncRead + Unpin>(
    instream: &mut RfbInStream<R>,
) -> std::io::Result<()> {
    let security_type = instream.read_u32().await?;

    match security_type {
        0 => {
            let reason_len = instream.read_u32().await? as usize;
            let mut reason_buf = vec![0u8; reason_len];
            instream.read_bytes(&mut reason_buf).await?;
            let reason = String::from_utf8_lossy(&reason_buf);
            Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                format!("server rejected connection: {}", reason),
            ))
        }
        1 => Ok(()),
        other => Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            format!(
                "unsupported security type for RFB 3.3: {} (only None=1 supported)",
                other
            ),
        )),
    }
}

/// Send ClientInit message to the server.
pub async fn send_client_init<W: AsyncWrite + Unpin>(
    outstream: &mut RfbOutStream<W>,
    shared: bool,
) -> std::io::Result<()> {
    let client_init = messages::ClientInit { shared };
    client_init.write_to(outstream);
    outstream.flush().await?;
    Ok(())
}

/// Receive ServerInit message from the server.
pub async fn recv_server_init<R: AsyncRead + Unpin>(
    instream: &mut RfbInStream<R>,
) -> std::io::Result<messages::ServerInit> {
    messages::ServerInit::read_from(instream).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::types::PixelFormat;

    fn create_duplex_pair() -> (
        (RfbInStream<tokio::io::DuplexStream>, RfbOutStream<tokio::io::DuplexStream>),
        (RfbInStream<tokio::io::DuplexStream>, RfbOutStream<tokio::io::DuplexStream>),
    ) {
        let (client_read, server_write) = tokio::io::duplex(1024);
        let (server_read, client_write) = tokio::io::duplex(1024);
        (
            (RfbInStream::new(client_read), RfbOutStream::new(client_write)),
            (RfbInStream::new(server_read), RfbOutStream::new(server_write)),
        )
    }

    #[tokio::test]
    async fn test_version_negotiation_3_8() {
        let ((mut client_in, mut client_out), (mut server_in, mut server_out)) = create_duplex_pair();

        server_out.write_bytes(b"RFB 003.008\n");
        server_out.flush().await.unwrap();

        let negotiated = negotiate_version(&mut client_in, &mut client_out).await.unwrap();
        assert_eq!(negotiated, NegotiatedVersion::V3_8);

        let mut buf = [0u8; 12];
        server_in.read_bytes(&mut buf).await.unwrap();
        assert_eq!(&buf, b"RFB 003.008\n");
    }

    #[tokio::test]
    async fn test_version_negotiation_3_3() {
        let ((mut client_in, mut client_out), (mut server_in, mut server_out)) = create_duplex_pair();

        server_out.write_bytes(b"RFB 003.003\n");
        server_out.flush().await.unwrap();

        let negotiated = negotiate_version(&mut client_in, &mut client_out).await.unwrap();
        assert_eq!(negotiated, NegotiatedVersion::V3_3);

        let mut buf = [0u8; 12];
        server_in.read_bytes(&mut buf).await.unwrap();
        assert_eq!(&buf, b"RFB 003.008\n");
    }

    #[tokio::test]
    async fn test_unsupported_version() {
        let ((mut client_in, mut client_out), (_, mut server_out)) = create_duplex_pair();

        server_out.write_bytes(b"RFB 002.002\n");
        server_out.flush().await.unwrap();

        let result = negotiate_version(&mut client_in, &mut client_out).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("unsupported") && err_msg.contains("2.2"));
    }

    #[tokio::test]
    async fn test_security_none_3_8() {
        let ((mut client_in, mut client_out), (mut server_in, mut server_out)) = create_duplex_pair();

        server_out.write_u8(1);
        server_out.write_u8(SECURITY_TYPE_NONE);
        server_out.flush().await.unwrap();

        tokio::spawn(async move {
            let _ = server_in.read_u8().await.unwrap();
            server_out.write_u32(0);
            server_out.flush().await.unwrap();
        });

        negotiate_security(&mut client_in, &mut client_out, NegotiatedVersion::V3_8).await.unwrap();
    }

    #[tokio::test]
    async fn test_security_none_3_3() {
        let ((mut client_in, mut client_out), (_, mut server_out)) = create_duplex_pair();

        server_out.write_u32(1);
        server_out.flush().await.unwrap();

        negotiate_security(&mut client_in, &mut client_out, NegotiatedVersion::V3_3).await.unwrap();
    }

    #[tokio::test]
    async fn test_client_init_sent_shared_true() {
        let ((_, mut client_out), (mut server_in, _)) = create_duplex_pair();

        send_client_init(&mut client_out, true).await.unwrap();

        let shared_byte = server_in.read_u8().await.unwrap();
        assert_eq!(shared_byte, 1);
    }

    #[tokio::test]
    async fn test_server_init_parsing() {
        let ((mut client_in, _), (_, mut server_out)) = create_duplex_pair();

        server_out.write_u16(1920);
        server_out.write_u16(1080);

        let pf = PixelFormat {
            bits_per_pixel: 32,
            depth: 24,
            big_endian: 0,
            true_color: 1,
            red_max: 255,
            green_max: 255,
            blue_max: 255,
            red_shift: 16,
            green_shift: 8,
            blue_shift: 0,
        };
        pf.write_to(&mut server_out).unwrap();

        let name = b"Test Desktop";
        server_out.write_u32(name.len() as u32);
        server_out.write_bytes(name);
        server_out.flush().await.unwrap();

        let server_init = recv_server_init(&mut client_in).await.unwrap();
        assert_eq!(server_init.framebuffer_width, 1920);
        assert_eq!(server_init.framebuffer_height, 1080);
        assert_eq!(server_init.pixel_format, pf);
        assert_eq!(server_init.name, "Test Desktop");
    }
}
