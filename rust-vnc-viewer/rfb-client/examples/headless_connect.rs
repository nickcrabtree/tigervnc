//! Headless VNC client example - connect and log server events.
//!
//! Usage:
//!   cargo run --example headless_connect -- localhost:5900
//!
//! This example demonstrates:
//! - Creating a client configuration
//! - Connecting to a VNC server
//! - Processing server events
//! - Requesting framebuffer updates
//! - Graceful shutdown

use rfb_client::{ClientBuilder, ClientCommand, Config, ServerEvent};
use std::env;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, error, debug};
use tracing_subscriber;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // Parse command line arguments
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <host>:<port>", args[0]);
        eprintln!("Example: {} localhost:5900", args[0]);
        std::process::exit(1);
    }

    let server = &args[1];
    let (host, port) = parse_server_address(server)?;

    info!("Connecting to {}:{}", host, port);

    // Create configuration
    let config = Config::builder()
        .host(&host)
        .port(port)
        .build()?;

    // Build and connect client
    let client = match ClientBuilder::new(config).build().await {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to connect: {}", e);
            return Err(e.into());
        }
    };

    let handle = client.handle();

    // Spawn task to request periodic updates
    let cmd_handle = handle.clone();
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_millis(100)).await;
            if cmd_handle
                .send(ClientCommand::RequestUpdate {
                    incremental: true,
                    rect: None,
                })
                .is_err()
            {
                break;
            }
        }
    });

    // Process events
    let mut update_count = 0u64;
    while let Ok(event) = handle.events().recv_async().await {
        match event {
            ServerEvent::Connected {
                width,
                height,
                name,
                ..
            } => {
                info!("✓ Connected to server");
                info!("  Desktop: {} ({}x{})", name, width, height);
            }
            ServerEvent::FramebufferUpdated { damage } => {
                update_count += 1;
                if update_count % 10 == 0 {
                    debug!("Received {} updates (damage regions: {})", update_count, damage.len());
                }
            }
            ServerEvent::DesktopResized { width, height } => {
                info!("Desktop resized to {}x{}", width, height);
            }
            ServerEvent::Bell => {
                debug!("Bell");
            }
            ServerEvent::ServerCutText { text } => {
                debug!("Server clipboard: {} bytes", text.len());
            }
            ServerEvent::Error { message } => {
                error!("Server error: {}", message);
            }
            ServerEvent::ConnectionClosed => {
                info!("Connection closed");
                break;
            }
        }
    }

    info!("Received {} framebuffer updates total", update_count);
    info!("Shutting down...");

    Ok(())
}

fn parse_server_address(server: &str) -> anyhow::Result<(String, u16)> {
    if let Some((host, port_str)) = server.split_once(':') {
        let port = port_str.parse::<u16>()?;
        Ok((host.to_string(), port))
    } else {
        // Default VNC port
        Ok((server.to_string(), 5900))
    }
}
