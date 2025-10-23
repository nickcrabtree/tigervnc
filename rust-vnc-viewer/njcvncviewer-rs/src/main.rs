use anyhow::{Context, Result};
use clap::Parser;
use tracing::info;

mod app;
mod connection;

#[derive(Parser, Debug)]
#[command(name = "njcvncviewer-rs")]
#[command(author, version, about, long_about = None)]
struct Args {
    /// VNC server address (host:display or host::port format)
    /// Examples: localhost:1, 192.168.1.100::5900
    #[arg(value_name = "SERVER")]
    server: String,

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
}

fn parse_server_address(server: &str) -> Result<(String, u16)> {
    if let Some((host, port_or_display)) = server.split_once("::") {
        // host::port format
        let port: u16 = port_or_display
            .parse()
            .context("Invalid port number")?;
        Ok((host.to_string(), port))
    } else if let Some((host, display)) = server.split_once(':') {
        // host:display format (display number adds to 5900)
        let display_num: u16 = display
            .parse()
            .context("Invalid display number")?;
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

fn main() -> Result<()> {
    let args = Args::parse();

    init_logging(args.verbose);

    let (host, port) = parse_server_address(&args.server)
        .context("Failed to parse server address")?;

    info!("Connecting to {}:{}", host, port);
    info!("Shared session: {}", args.shared);

    let shared = args.shared;
    let width = args.width;
    let height = args.height;
    let fullscreen = args.fullscreen;

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([width as f32, height as f32])
            .with_title("TigerVNC Rust Viewer")
            .with_fullscreen(fullscreen),
        ..Default::default()
    };

    eframe::run_native(
        "TigerVNC Rust Viewer",
        native_options,
        Box::new(move |cc| Box::new(app::VncViewerApp::new(cc, host, port, shared))),
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
}
