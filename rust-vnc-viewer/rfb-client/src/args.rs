//! Command-line argument parsing for VNC client applications.
//!
//! This module is only available when the `cli` feature is enabled.
//! It provides a structured way to parse command-line arguments and
//! convert them into a `Config` object.
//!
//! # Examples
//!
//! ```no_run
//! use rfb_client::args::Args;
//! use rfb_client::Config;
//!
//! let args = Args::parse();
//! let config = Config::from_args(args)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use crate::config::{Config, ConfigBuilder};
use clap::Parser;

/// VNC client command-line arguments.
#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// VNC server address (host:port or host:display)
    ///
    /// Examples:
    ///   - localhost:5900
    ///   - 192.168.1.100:0 (display :0 = port 5900)
    ///   - vnc.example.com:1 (display :1 = port 5901)
    #[arg(value_name = "SERVER")]
    pub server: String,

    /// Server port (overrides port in SERVER if specified)
    #[arg(short = 'p', long, value_name = "PORT")]
    pub port: Option<u16>,

    /// Password for authentication
    #[arg(short = 'P', long, value_name = "PASSWORD", env = "VNC_PASSWORD")]
    pub password: Option<String>,

    /// Enable TLS encryption
    #[arg(long)]
    pub tls: bool,

    /// Path to TLS certificate file (PEM format)
    #[arg(long, value_name = "FILE", requires = "tls")]
    pub tls_cert: Option<String>,

    /// Disable TLS certificate verification (insecure)
    #[arg(long, requires = "tls")]
    pub tls_insecure: bool,

    /// Preferred encodings (comma-separated)
    ///
    /// Available: raw, copyrect, rre, hextile, tight, zrle
    #[arg(short = 'e', long, value_name = "ENCODINGS", value_delimiter = ',')]
    pub encodings: Option<Vec<String>>,

    /// Request only changed regions (incremental updates)
    #[arg(short = 'i', long)]
    pub incremental: bool,

    /// View-only mode (no input events sent)
    #[arg(long)]
    pub view_only: bool,

    /// Shared session (allow multiple clients)
    #[arg(short = 's', long)]
    pub shared: bool,

    /// Configuration file path (TOML format)
    #[arg(short = 'c', long, value_name = "FILE")]
    pub config: Option<String>,

    /// Enable verbose logging
    #[arg(short = 'v', long, action = clap::ArgAction::Count)]
    pub verbose: u8,
}

impl Args {
    /// Parse command-line arguments.
    #[must_use]
    pub fn parse() -> Self {
        <Self as Parser>::parse()
    }

    /// Parse arguments from an iterator.
    ///
    /// # Errors
    ///
    /// Returns an error if the arguments are invalid.
    pub fn try_parse_from<I, T>(iter: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        <Self as Parser>::try_parse_from(iter)
    }
}

impl Config {
    /// Create a configuration from command-line arguments.
    ///
    /// If a config file is specified in the arguments, it will be loaded
    /// first, then overridden by explicit command-line arguments.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The config file cannot be read or parsed
    /// - The server address is invalid
    /// - The configuration validation fails
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use rfb_client::args::Args;
    /// use rfb_client::Config;
    ///
    /// let args = Args::parse();
    /// let config = Config::from_args(args)?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn from_args(args: Args) -> Result<Self, crate::errors::RfbClientError> {
        // Start with config file if provided
        let mut builder = if let Some(config_path) = &args.config {
            let config_str = std::fs::read_to_string(config_path).map_err(|e| {
                crate::errors::RfbClientError::Config(format!(
                    "Failed to read config file '{}': {}",
                    config_path, e
                ))
            })?;
            let config: Config = toml::from_str(&config_str).map_err(|e| {
                crate::errors::RfbClientError::Config(format!(
                    "Failed to parse config file '{}': {}",
                    config_path, e
                ))
            })?;
            // Start with loaded config
            Config::builder()
                .host(&config.connection.host)
                .port(config.connection.port)
        } else {
            Config::builder()
        };

        // Parse server address (host:port or host:display)
        let (host, port) = parse_server_address(&args.server)?;

        // Override with command-line arguments
        builder = builder.host(&host);

        // Use explicit port if provided, otherwise use parsed port
        if let Some(p) = args.port {
            builder = builder.port(p);
        } else {
            builder = builder.port(port);
        }

        // Security settings
        if let Some(password) = args.password {
            builder = builder.password(password);
        }

        // TODO: TLS and other advanced settings will be added when those builder methods exist
        // For now, just build with basic connection settings

        builder.build()
    }
}

/// Parse server address in the format "host:port" or "host:display".
///
/// VNC display numbers (0-99) are converted to port numbers (5900-5999).
fn parse_server_address(server: &str) -> Result<(String, u16), crate::errors::RfbClientError> {
    if let Some((host, port_or_display)) = server.split_once(':') {
        let num = port_or_display.parse::<u16>().map_err(|_| {
            crate::errors::RfbClientError::Config(format!(
                "Invalid port or display number: {}",
                port_or_display
            ))
        })?;

        let port = if num < 100 {
            // Display number: :0 = 5900, :1 = 5901, etc.
            5900 + num
        } else {
            // Direct port number
            num
        };

        Ok((host.to_string(), port))
    } else {
        // No port specified, use default VNC port
        Ok((server.to_string(), 5900))
    }
}

/// Parse encoding names to encoding IDs.
fn parse_encodings(names: &[String]) -> Result<Vec<i32>, crate::errors::RfbClientError> {
    use rfb_protocol::messages::types::{
        ENCODING_COPYRECT, ENCODING_HEXTILE, ENCODING_RAW, ENCODING_RRE, ENCODING_TIGHT,
        ENCODING_ZRLE,
    };

    let mut encodings = Vec::new();
    for name in names {
        let encoding = match name.to_lowercase().as_str() {
            "raw" => ENCODING_RAW,
            "copyrect" | "copy-rect" => ENCODING_COPYRECT,
            "rre" => ENCODING_RRE,
            "hextile" => ENCODING_HEXTILE,
            "tight" => ENCODING_TIGHT,
            "zrle" => ENCODING_ZRLE,
            _ => {
                return Err(crate::errors::RfbClientError::Config(format!(
                    "Unknown encoding: {}",
                    name
                )))
            }
        };
        encodings.push(encoding);
    }

    Ok(encodings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_server_address_with_port() {
        let (host, port) = parse_server_address("localhost:5900").unwrap();
        assert_eq!(host, "localhost");
        assert_eq!(port, 5900);
    }

    #[test]
    fn test_parse_server_address_with_display() {
        let (host, port) = parse_server_address("localhost:0").unwrap();
        assert_eq!(host, "localhost");
        assert_eq!(port, 5900);

        let (host, port) = parse_server_address("192.168.1.100:1").unwrap();
        assert_eq!(host, "192.168.1.100");
        assert_eq!(port, 5901);
    }

    #[test]
    fn test_parse_server_address_no_port() {
        let (host, port) = parse_server_address("localhost").unwrap();
        assert_eq!(host, "localhost");
        assert_eq!(port, 5900);
    }

    #[test]
    fn test_parse_encodings() {
        let names = vec!["raw".to_string(), "tight".to_string(), "zrle".to_string()];
        let encodings = parse_encodings(&names).unwrap();
        assert_eq!(encodings.len(), 3);
    }

    #[test]
    fn test_parse_encodings_invalid() {
        let names = vec!["invalid".to_string()];
        assert!(parse_encodings(&names).is_err());
    }

    #[test]
    fn test_args_minimal() {
        let args = Args::try_parse_from(["test", "localhost:5900"]).unwrap();
        assert_eq!(args.server, "localhost:5900");
        assert_eq!(args.port, None);
        assert!(!args.tls);
    }

    #[test]
    fn test_args_with_options() {
        let args = Args::try_parse_from([
            "test",
            "localhost:5900",
            "--tls",
            "--shared",
            "--encodings",
            "tight,zrle",
        ])
        .unwrap();
        assert!(args.tls);
        assert!(args.shared);
        assert_eq!(args.encodings.as_ref().unwrap().len(), 2);
    }
}
