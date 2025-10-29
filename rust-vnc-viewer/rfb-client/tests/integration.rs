//! Integration tests for rfb-client.
//!
//! These tests require a running VNC server. Set the environment variable
//! VNC_TEST_SERVER to specify the server address (default: localhost:5900).
//!
//! Example:
//!   VNC_TEST_SERVER=localhost:5901 cargo test --test integration -- --nocapture

use rfb_client::{ClientBuilder, ClientCommand, Config, ServerEvent};
use std::env;
use std::time::Duration;
use tokio::time::{sleep, timeout};

/// Get VNC server address from environment or use default
fn get_test_server() -> (String, u16) {
    let server = env::var("VNC_TEST_SERVER").unwrap_or_else(|_| "localhost:5900".to_string());

    if let Some((host, port_str)) = server.split_once(':') {
        let port = port_str.parse::<u16>().expect("Invalid port");
        (host.to_string(), port)
    } else {
        (server, 5900)
    }
}

/// Test basic connection and handshake
#[tokio::test]
#[ignore] // Requires running VNC server
async fn test_basic_connection() -> anyhow::Result<()> {
    let (host, port) = get_test_server();

    let config = Config::builder().host(&host).port(port).build()?;

    let client = ClientBuilder::new(config).build().await?;
    let handle = client.handle();

    // Wait for connected event
    let event = timeout(Duration::from_secs(5), handle.events().recv_async()).await??;

    match event {
        ServerEvent::Connected {
            width,
            height,
            name,
            ..
        } => {
            println!("Connected to: {} ({}x{})", name, width, height);
            assert!(width > 0);
            assert!(height > 0);
        }
        _ => panic!("Expected Connected event, got: {:?}", event),
    }

    // Clean shutdown
    handle.send(ClientCommand::Close)?;

    Ok(())
}

/// Test framebuffer updates
#[tokio::test]
#[ignore] // Requires running VNC server
async fn test_framebuffer_updates() -> anyhow::Result<()> {
    let (host, port) = get_test_server();

    let config = Config::builder().host(&host).port(port).build()?;

    let client = ClientBuilder::new(config).build().await?;
    let handle = client.handle();

    // Wait for connected event
    let event = timeout(Duration::from_secs(5), handle.events().recv_async()).await??;
    assert!(matches!(event, ServerEvent::Connected { .. }));

    // Request a full framebuffer update
    handle.send(ClientCommand::RequestUpdate {
        incremental: false,
        rect: None,
    })?;

    // Wait for framebuffer update (may take multiple events)
    let mut received_update = false;
    for _ in 0..10 {
        match timeout(Duration::from_secs(2), handle.events().recv_async()).await {
            Ok(Ok(ServerEvent::FramebufferUpdated { damage })) => {
                println!("Received framebuffer update with {} damage regions", damage.len());
                received_update = true;
                break;
            }
            Ok(Ok(other)) => {
                println!("Received event: {:?}", other);
            }
            Ok(Err(e)) => return Err(e.into()),
            Err(_) => break, // Timeout
        }
    }

    assert!(received_update, "Did not receive framebuffer update");

    // Clean shutdown
    handle.send(ClientCommand::Close)?;

    Ok(())
}

/// Test pointer events
#[tokio::test]
#[ignore] // Requires running VNC server
async fn test_pointer_events() -> anyhow::Result<()> {
    let (host, port) = get_test_server();

    let config = Config::builder().host(&host).port(port).build()?;

    let client = ClientBuilder::new(config).build().await?;
    let handle = client.handle();

    // Wait for connected event
    let event = timeout(Duration::from_secs(5), handle.events().recv_async()).await??;
    assert!(matches!(event, ServerEvent::Connected { .. }));

    // Send pointer event
    handle.send(ClientCommand::Pointer {
        x: 100,
        y: 100,
        buttons: 0x01, // Left button
    })?;

    // Small delay to ensure event is processed
    sleep(Duration::from_millis(50)).await;

    // Release button
    handle.send(ClientCommand::Pointer {
        x: 100,
        y: 100,
        buttons: 0x00,
    })?;

    sleep(Duration::from_millis(50)).await;

    // Clean shutdown
    handle.send(ClientCommand::Close)?;

    Ok(())
}

/// Test keyboard events
#[tokio::test]
#[ignore] // Requires running VNC server
async fn test_key_events() -> anyhow::Result<()> {
    let (host, port) = get_test_server();

    let config = Config::builder().host(&host).port(port).build()?;

    let client = ClientBuilder::new(config).build().await?;
    let handle = client.handle();

    // Wait for connected event
    let event = timeout(Duration::from_secs(5), handle.events().recv_async()).await??;
    assert!(matches!(event, ServerEvent::Connected { .. }));

    // Send key down event (Latin small letter 'a')
    handle.send(ClientCommand::Key {
        key: 0x0061,
        down: true,
    })?;

    sleep(Duration::from_millis(50)).await;

    // Send key up event
    handle.send(ClientCommand::Key {
        key: 0x0061,
        down: false,
    })?;

    sleep(Duration::from_millis(50)).await;

    // Clean shutdown
    handle.send(ClientCommand::Close)?;

    Ok(())
}

/// Test configuration validation
#[tokio::test]
async fn test_config_validation() {
    // Invalid host (empty)
    let result = Config::builder().host("").port(5900).build();
    assert!(result.is_err());

    // Invalid port (0)
    let result = Config::builder().host("localhost").port(0).build();
    assert!(result.is_err());

    // Valid configuration
    let result = Config::builder().host("localhost").port(5900).build();
    assert!(result.is_ok());
}
