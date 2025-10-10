//! Error types for the RFB client.

use std::io;
use thiserror::Error;

/// Errors that can occur during VNC client operation.
#[derive(Debug, Error)]
pub enum RfbClientError {
    /// Transport-level error (TCP, socket operations).
    #[error("Transport error: {0}")]
    Transport(#[from] io::Error),

    /// Connection failed (TCP connection establishment failed).
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    /// TLS/SSL error.
    #[error("TLS error: {0}")]
    TlsError(String),

    /// RFB handshake failed.
    #[error("Handshake failed: {0}")]
    Handshake(String),

    /// Security negotiation failed.
    #[error("Security negotiation failed: {0}")]
    Security(String),

    /// Authentication failed (wrong password, etc.).
    #[error("Authentication failed: {0}")]
    AuthFailed(String),

    /// Protocol error (malformed message, unexpected data).
    #[error("Protocol error: {0}")]
    Protocol(String),

    /// Encoding/decoding error.
    #[error("Encoding error: {0}")]
    Encoding(#[from] anyhow::Error),

    /// Unsupported encoding type.
    #[error("Unsupported encoding: {0}")]
    UnsupportedEncoding(i32),

    /// Connection timeout.
    #[error("Connection timeout after {0:?}")]
    Timeout(std::time::Duration),

    /// Unexpected message from server.
    #[error("Unexpected message: {0}")]
    UnexpectedMessage(String),

    /// Configuration error.
    #[error("Configuration error: {0}")]
    Config(String),

    /// Connection has been closed.
    #[error("Connection closed")]
    ConnectionClosed,

    /// Internal error (should not happen in normal operation).
    #[error("Internal error: {0}")]
    Internal(String),
}

impl RfbClientError {
    /// Returns true if this error is potentially retryable.
    ///
    /// Retryable errors are typically transient network issues that may
    /// succeed on retry. Non-retryable errors are fatal conditions like
    /// authentication failures or configuration errors.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Transport(_)
                | Self::Timeout(_)
                | Self::Handshake(_)
                | Self::TlsError(_)
                | Self::ConnectionFailed(_)
        )
    }

    /// Returns true if this is a fatal error that should not be retried.
    #[must_use]
    pub fn is_fatal(&self) -> bool {
        !self.is_retryable()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_categorization() {
        assert!(RfbClientError::Transport(io::Error::from(io::ErrorKind::ConnectionRefused))
            .is_retryable());
        assert!(RfbClientError::Timeout(std::time::Duration::from_secs(10)).is_retryable());
        assert!(RfbClientError::Handshake("test".to_string()).is_retryable());

        assert!(RfbClientError::AuthFailed("wrong password".to_string()).is_fatal());
        assert!(RfbClientError::Config("invalid host".to_string()).is_fatal());
        assert!(RfbClientError::UnsupportedEncoding(999).is_fatal());
    }

    #[test]
    fn test_error_display() {
        let err = RfbClientError::AuthFailed("wrong password".to_string());
        assert_eq!(err.to_string(), "Authentication failed: wrong password");

        let err = RfbClientError::Timeout(std::time::Duration::from_secs(5));
        assert!(err.to_string().contains("5s"));
    }
}
