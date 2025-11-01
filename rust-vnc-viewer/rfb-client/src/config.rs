//! Configuration types for the VNC client.

use crate::errors::RfbClientError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

/// Complete VNC client configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Connection settings.
    pub connection: ConnectionConfig,
    /// Display settings.
    pub display: DisplayConfig,
    /// Security settings.
    pub security: SecurityConfig,
    /// Input settings.
    pub input: InputConfig,
    /// Reconnection settings.
    pub reconnect: ReconnectConfig,
    /// ContentCache settings.
    pub content_cache: ContentCacheConfig,
    /// PersistentCache settings.
    #[serde(default)]
    pub persistent_cache: PersistentCacheConfig,
}

/// Connection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    /// Server hostname or IP address.
    pub host: String,
    /// Server port (typically 5900 + display number).
    pub port: u16,
    /// VNC password (if required).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// Connection timeout in milliseconds.
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_timeout_ms() -> u64 {
    10_000
}

/// Display configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayConfig {
    /// Preferred encodings in priority order.
    #[serde(default = "default_encodings")]
    pub encodings: Vec<i32>,
    /// JPEG quality (0-9), if applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<u8>,
    /// Compression level (0-9), if applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compression: Option<u8>,
}

fn default_encodings() -> Vec<i32> {
    // Minimal, known-good baseline encodings: Raw(0), CopyRect(1), ZRLE(16)
    vec![
        rfb_encodings::ENCODING_RAW,
        rfb_encodings::ENCODING_COPY_RECT,
        rfb_encodings::ENCODING_ZRLE,
    ]
}

/// Security configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// TLS configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tls: Option<TlsConfig>,
    /// View-only mode (no input sent to server).
    #[serde(default)]
    pub view_only: bool,
}

/// TLS configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsConfig {
    /// Enable TLS encryption.
    pub enabled: bool,
    /// Server name for certificate validation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
    /// Path to CA certificate file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ca_file: Option<PathBuf>,
    /// Skip certificate validation (DANGEROUS - use only for testing).
    #[serde(default)]
    pub danger_accept_invalid_certs: bool,
}

/// Input configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputConfig {
    /// Pointer event rate limit in Hz.
    #[serde(default = "default_pointer_rate_hz")]
    pub pointer_rate_hz: u32,
    /// Enable pointer event throttling.
    #[serde(default = "default_true")]
    pub pointer_throttle: bool,
}

fn default_pointer_rate_hz() -> u32 {
    60
}

fn default_true() -> bool {
    true
}

/// Reconnection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconnectConfig {
    /// Enable automatic reconnection.
    #[serde(default)]
    pub enabled: bool,
    /// Maximum number of retry attempts (0 = infinite).
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    /// Initial backoff duration in milliseconds.
    #[serde(default = "default_backoff_ms")]
    pub backoff_ms: u64,
    /// Maximum backoff duration in milliseconds.
    #[serde(default = "default_max_backoff_ms")]
    pub max_backoff_ms: u64,
    /// Jitter factor (0.0-1.0) for backoff randomization.
    #[serde(default = "default_jitter")]
    pub jitter: f32,
}

fn default_max_retries() -> u32 {
    5
}

fn default_backoff_ms() -> u64 {
    1_000
}

fn default_max_backoff_ms() -> u64 {
    30_000
}

fn default_jitter() -> f32 {
    0.1
}

/// ContentCache configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentCacheConfig {
    /// Enable ContentCache protocol for bandwidth reduction.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Maximum cache size in megabytes.
    #[serde(default = "default_cache_size_mb")]
    pub size_mb: usize,
    /// Cache entry lifetime in seconds (0 = no expiration).
    #[serde(default = "default_max_age_seconds")]
    pub max_age_seconds: u64,
    /// Minimum rectangle size to cache (in pixels).
    #[serde(default = "default_min_rect_size")]
    pub min_rect_size: u32,
    /// Cache utilization threshold for proactive cleanup (0.0-1.0).
    #[serde(default = "default_cleanup_threshold")]
    pub cleanup_threshold: f64,
}

fn default_cache_size_mb() -> usize {
    2048 // 2GB default
}

fn default_max_age_seconds() -> u64 {
    300 // 5 minutes
}

fn default_min_rect_size() -> u32 {
    4096 // 64x64 pixels minimum
}

fn default_cleanup_threshold() -> f64 {
    0.8 // Start cleanup at 80% utilization
}

fn default_false() -> bool {
    false
}

/// PersistentCache configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PersistentCacheConfig {
    /// Enable PersistentCache protocol (-321) with disk-backed storage.
    #[serde(default)]
    pub enabled: bool,
    /// Maximum cache size in megabytes (on disk and in memory accounting).
    #[serde(default = "default_persistent_cache_size_mb")]
    pub size_mb: usize,
}

fn default_persistent_cache_size_mb() -> usize {
    2048
}

impl Default for Config {
    fn default() -> Self {
        Self {
            connection: ConnectionConfig {
                host: String::new(),
                port: 5900,
                password: None,
                timeout_ms: default_timeout_ms(),
            },
            display: DisplayConfig {
                encodings: default_encodings(),
                quality: None,
                compression: None,
            },
            security: SecurityConfig {
                tls: None,
                view_only: false,
            },
            input: InputConfig {
                pointer_rate_hz: default_pointer_rate_hz(),
                pointer_throttle: default_true(),
            },
            reconnect: ReconnectConfig {
                enabled: false,
                max_retries: default_max_retries(),
                backoff_ms: default_backoff_ms(),
                max_backoff_ms: default_max_backoff_ms(),
                jitter: default_jitter(),
            },
            content_cache: ContentCacheConfig {
                enabled: default_true(),
                size_mb: default_cache_size_mb(),
                max_age_seconds: default_max_age_seconds(),
                min_rect_size: default_min_rect_size(),
                cleanup_threshold: default_cleanup_threshold(),
            },
            persistent_cache: PersistentCacheConfig {
                enabled: false,
                size_mb: default_persistent_cache_size_mb(),
            },
        }
    }
}

impl Config {
    /// Creates a new configuration builder.
    #[must_use]
    pub fn builder() -> ConfigBuilder {
        ConfigBuilder::default()
    }

    /// Validates the configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if any configuration values are invalid.
    pub fn validate(&self) -> Result<(), RfbClientError> {
        // Validate host
        if self.connection.host.is_empty() {
            return Err(RfbClientError::Config("Host cannot be empty".to_string()));
        }

        // Validate port
        if self.connection.port == 0 {
            return Err(RfbClientError::Config("Port cannot be 0".to_string()));
        }

        // Validate encodings
        if self.display.encodings.is_empty() {
            return Err(RfbClientError::Config(
                "At least one encoding must be specified".to_string(),
            ));
        }

        // Validate jitter
        if !(0.0..=1.0).contains(&self.reconnect.jitter) {
            return Err(RfbClientError::Config(
                "Jitter must be between 0.0 and 1.0".to_string(),
            ));
        }

        // Validate ContentCache config
        if self.content_cache.size_mb == 0 && self.content_cache.enabled {
            return Err(RfbClientError::Config(
                "ContentCache size cannot be 0 when enabled".to_string(),
            ));
        }

        if !(0.0..=1.0).contains(&self.content_cache.cleanup_threshold) {
            return Err(RfbClientError::Config(
                "ContentCache cleanup threshold must be between 0.0 and 1.0".to_string(),
            ));
        }

        Ok(())
    }

    /// Returns the connection timeout duration.
    #[must_use]
    pub fn timeout(&self) -> Duration {
        Duration::from_millis(self.connection.timeout_ms)
    }

    /// Returns the complete encodings list including cache protocols if enabled.
    ///
    /// NOTE: Encodings are listed in PREFERENCE ORDER. The server's EncodeManager
    /// selects encodings based on the order they appear in this list.
    /// Pseudo-encodings (negative values) should generally come BEFORE real encodings.
    #[must_use]
    pub fn effective_encodings(&self) -> Vec<i32> {
        let mut encodings = Vec::new();

        // Pseudo-encodings FIRST (server prefers encodings listed earlier)

        // Add pseudo-encodings for protocol features
        encodings.push(-312); // pseudoEncodingFence
        encodings.push(-313); // pseudoEncodingContinuousUpdates

        // Add ContentCache encodings if enabled
        if self.content_cache.enabled {
            encodings.push(-320); // pseudoEncodingContentCache - tells server we support cache protocol
            encodings.push(rfb_encodings::ENCODING_CACHED_RECT); // 100 - cache hit (reference)
            encodings.push(rfb_encodings::ENCODING_CACHED_RECT_INIT); // 101 - cache miss (data + store)
        }

        // Add PersistentCache encodings if enabled
        if self.persistent_cache.enabled {
            encodings.push(-321); // pseudoEncodingPersistentCache - tells server we support persistent cache
            encodings.push(rfb_encodings::ENCODING_PERSISTENT_CACHED_RECT); // 102
            encodings.push(rfb_encodings::ENCODING_PERSISTENT_CACHED_RECT_INIT);
            // 103
        }

        // Real encodings AFTER pseudo-encodings
        encodings.push(rfb_encodings::ENCODING_RAW);
        encodings.push(rfb_encodings::ENCODING_COPY_RECT);
        encodings.push(rfb_encodings::ENCODING_ZRLE);

        encodings
    }
}

/// Builder for creating a `Config`.
#[derive(Default)]
pub struct ConfigBuilder {
    config: Config,
}

impl ConfigBuilder {
    /// Sets the server hostname or IP address.
    #[must_use]
    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.config.connection.host = host.into();
        self
    }

    /// Sets the server port.
    #[must_use]
    pub fn port(mut self, port: u16) -> Self {
        self.config.connection.port = port;
        self
    }

    /// Sets the VNC password.
    #[must_use]
    pub fn password(mut self, password: impl Into<String>) -> Self {
        self.config.connection.password = Some(password.into());
        self
    }

    /// Builds the configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the configuration is invalid.
    pub fn build(self) -> Result<Config, RfbClientError> {
        self.config.validate()?;
        Ok(self.config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_builder() {
        let config = Config::builder()
            .host("localhost")
            .port(5900)
            .build()
            .unwrap();

        assert_eq!(config.connection.host, "localhost");
        assert_eq!(config.connection.port, 5900);
    }

    #[test]
    fn test_config_validation_empty_host() {
        let config = Config::default();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_zero_port() {
        let mut config = Config::default();
        config.connection.host = "localhost".to_string();
        config.connection.port = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_invalid_jitter() {
        let mut config = Config::default();
        config.connection.host = "localhost".to_string();
        config.reconnect.jitter = 1.5;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_effective_encodings_with_caches_enabled() {
        // Default now enables ContentCache (but PersistentCache disabled); verify ordering
        // CRITICAL: Pseudo-encodings MUST come BEFORE real encodings
        // (server selects encodings based on list order)
        let config = Config::default();
        let encodings = config.effective_encodings();
        // 2 proto pseudo + 3 ContentCache + 3 base = 8 (PersistentCache disabled by default)
        assert_eq!(encodings.len(), 8);
        // Pseudo-encodings first
        assert_eq!(encodings[0], -312); // Fence
        assert_eq!(encodings[1], -313); // ContinuousUpdates
        assert_eq!(encodings[2], -320); // ContentCache pseudo
        assert_eq!(encodings[3], rfb_encodings::ENCODING_CACHED_RECT); // 100
        assert_eq!(encodings[4], rfb_encodings::ENCODING_CACHED_RECT_INIT); // 101
        // Real encodings last
        assert_eq!(encodings[5], rfb_encodings::ENCODING_RAW);
        assert_eq!(encodings[6], rfb_encodings::ENCODING_COPY_RECT);
        assert_eq!(encodings[7], rfb_encodings::ENCODING_ZRLE);
    }

    #[test]
    fn test_effective_encodings_no_caches() {
        // When caches are disabled, we still advertise Fence and CU pseudo-encodings
        // Pseudo-encodings come FIRST, then real encodings
        let mut config = Config::default();
        config.content_cache.enabled = false;
        config.persistent_cache.enabled = false;
        let encodings = config.effective_encodings();
        assert_eq!(
            encodings,
            vec![
                -312, // Fence
                -313, // ContinuousUpdates
                rfb_encodings::ENCODING_RAW,
                rfb_encodings::ENCODING_COPY_RECT,
                rfb_encodings::ENCODING_ZRLE,
            ]
        );
    }
}
