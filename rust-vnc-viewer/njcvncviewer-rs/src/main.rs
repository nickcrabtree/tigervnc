use anyhow::{Context, Result};
use clap::Parser;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::info;

mod app;
mod display;
mod fullscreen;
mod ui;

#[derive(Parser, Debug)]
#[command(name = "njcvncviewer-rs")]
#[command(author, version, about, long_about = None)]
struct Args {
    /// VNC server address (host:display or host::port format)
    /// Examples: localhost:1, 192.168.1.100::5900
    #[arg(value_name = "SERVER")]
    server: Option<String>,

    /// Request shared session (allow other clients to connect)
    #[arg(short, long, default_value_t = false)]
    shared: bool,

    /// Verbose logging level (repeat for more verbosity: -v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Full screen mode
    #[arg(short, long)]
    fullscreen: bool,

    /// Window width (ignored in fullscreen mode)
    #[arg(long, default_value_t = 1024)]
    width: u32,

    /// Window height (ignored in fullscreen mode)
    #[arg(long, default_value_t = 768)]
    height: u32,

    /// Monitor selector for fullscreen: "primary", index (0,1,2...), or name substring
    #[arg(long)]
    monitor: Option<String>,

    /// Configuration file path
    #[arg(long)]
    config: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Default connection settings
    pub connection: ConnectionConfig,

    /// Display settings
    pub display: DisplayConfig,

    /// Input settings  
    pub input: InputConfig,

    /// UI settings
    pub ui: UiConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    /// Default server address
    pub default_server: Option<String>,

    /// Default shared session setting
    pub shared: bool,

    /// Reconnection settings
    pub max_retries: u32,
    pub retry_delay_ms: u64,

    /// TLS settings
    pub verify_certificates: bool,
    pub allow_self_signed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayConfig {
    /// Default scaling mode
    pub scale_mode: String, // "native", "fit", "fill"

    /// Window dimensions  
    pub window_width: u32,
    pub window_height: u32,

    /// Fullscreen on connect
    pub fullscreen: bool,

    /// Cursor mode
    pub cursor_mode: String, // "local", "remote", "dot"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputConfig {
    /// Middle button emulation
    pub middle_button_emulation: bool,
    pub middle_button_timeout_ms: u64,

    /// Mouse throttling
    pub mouse_throttle_ms: u64,
    pub mouse_distance_threshold: f32,

    /// Keyboard settings
    pub key_repeat_throttle_ms: u64,

    /// Gesture settings
    pub gestures_enabled: bool,
    pub scroll_momentum_decay: f32,
    pub zoom_sensitivity: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    /// Show status bar
    pub show_status_bar: bool,

    /// Show menu bar
    pub show_menu_bar: bool,

    /// Theme
    pub dark_mode: bool,

    /// Statistics refresh rate
    pub stats_refresh_ms: u64,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            connection: ConnectionConfig {
                default_server: None,
                shared: false,
                max_retries: 3,
                retry_delay_ms: 1000,
                verify_certificates: true,
                allow_self_signed: false,
            },
            display: DisplayConfig {
                scale_mode: "fit".to_string(),
                window_width: 1024,
                window_height: 768,
                fullscreen: false,
                cursor_mode: "local".to_string(),
            },
            input: InputConfig {
                middle_button_emulation: true,
                middle_button_timeout_ms: 200,
                mouse_throttle_ms: 16, // ~60fps
                mouse_distance_threshold: 5.0,
                key_repeat_throttle_ms: 50, // 20 keys/sec
                gestures_enabled: true,
                scroll_momentum_decay: 0.95,
                zoom_sensitivity: 0.1,
            },
            ui: UiConfig {
                show_status_bar: true,
                show_menu_bar: true,
                dark_mode: false,
                stats_refresh_ms: 1000,
            },
        }
    }
}

fn parse_server_address(server: &str) -> Result<(String, u16)> {
    if let Some((host, port_or_display)) = server.split_once("::") {
        // host::port format
        let port: u16 = port_or_display.parse().context("Invalid port number")?;
        Ok((host.to_string(), port))
    } else if let Some((host, display)) = server.split_once(':') {
        // host:display format (display number adds to 5900)
        let display_num: u16 = display.parse().context("Invalid display number")?;
        Ok((host.to_string(), 5900 + display_num))
    } else {
        // Just hostname, assume display :0
        Ok((server.to_string(), 5900))
    }
}

fn init_logging(level: u8) {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};

    let filter = match level {
        0 => EnvFilter::new("info"),
        1 => EnvFilter::new("debug"),
        2 => EnvFilter::new("trace"),
        _ => EnvFilter::new("trace"),
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer())
        .init();
}

fn load_config(config_path: Option<PathBuf>) -> Result<AppConfig> {
    let config_file = if let Some(path) = config_path {
        path
    } else {
        // Use default config directory
        if let Some(proj_dirs) = ProjectDirs::from("org", "tigervnc", "njcvncviewer-rs") {
            let config_dir = proj_dirs.config_dir();
            std::fs::create_dir_all(config_dir).context("Failed to create config directory")?;
            config_dir.join("config.toml")
        } else {
            return Ok(AppConfig::default());
        }
    };

    if config_file.exists() {
        let config_str =
            std::fs::read_to_string(&config_file).context("Failed to read config file")?;
        let config: AppConfig =
            toml::from_str(&config_str).context("Failed to parse config file")?;
        info!("Loaded config from: {}", config_file.display());
        Ok(config)
    } else {
        // Create default config file
        let default_config = AppConfig::default();
        let config_str = toml::to_string_pretty(&default_config)
            .context("Failed to serialize default config")?;
        std::fs::write(&config_file, config_str).context("Failed to write default config file")?;
        info!("Created default config at: {}", config_file.display());
        Ok(default_config)
    }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let args = Args::parse();

    init_logging(args.verbose);

    // Load configuration
    let mut config = load_config(args.config).context("Failed to load configuration")?;

    // Override config with command line arguments
    if args.shared {
        config.connection.shared = true;
    }

    if args.fullscreen {
        config.display.fullscreen = true;
    }

    if args.width != 1024 {
        config.display.window_width = args.width;
    }

    if args.height != 768 {
        config.display.window_height = args.height;
    }

    let initial_server = args.server.or(config.connection.default_server.clone());

    info!("Starting TigerVNC Rust Viewer");
    if let Some(ref server) = initial_server {
        let (host, port) =
            parse_server_address(server).context("Failed to parse server address")?;
        info!("Initial server: {}:{}", host, port);
    }

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([
                config.display.window_width as f32,
                config.display.window_height as f32,
            ])
            .with_title("TigerVNC Rust Viewer")
            .with_fullscreen(config.display.fullscreen),
        ..Default::default()
    };

    eframe::run_native(
        "TigerVNC Rust Viewer",
        native_options,
        Box::new(move |cc| {
            Box::new(app::VncViewerApp::new(
                cc,
                config,
                initial_server,
                args.monitor.clone(),
            ))
        }),
    )
    .map_err(|e| anyhow::anyhow!("GUI error: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_server_display() {
        assert_eq!(
            parse_server_address("localhost:1").unwrap(),
            ("localhost".to_string(), 5901)
        );
        assert_eq!(
            parse_server_address("192.168.1.100:0").unwrap(),
            ("192.168.1.100".to_string(), 5900)
        );
    }

    #[test]
    fn test_parse_server_port() {
        assert_eq!(
            parse_server_address("localhost::5902").unwrap(),
            ("localhost".to_string(), 5902)
        );
        assert_eq!(
            parse_server_address("vnc.example.com::5900").unwrap(),
            ("vnc.example.com".to_string(), 5900)
        );
    }

    #[test]
    fn test_parse_server_hostname_only() {
        assert_eq!(
            parse_server_address("localhost").unwrap(),
            ("localhost".to_string(), 5900)
        );
    }

    #[test]
    fn test_parse_server_invalid_port() {
        assert!(parse_server_address("localhost::invalid").is_err());
    }

    #[test]
    fn test_parse_server_invalid_display() {
        assert!(parse_server_address("localhost:abc").is_err());
    }

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert!(!config.connection.shared);
        assert_eq!(config.display.scale_mode, "fit");
        assert!(config.input.middle_button_emulation);
        assert!(config.ui.show_status_bar);
    }
}
