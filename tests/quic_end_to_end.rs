// End-to-End Tests for the QUIC Client-Server Setup

use anyhow::Error;
use portredirect::quic::{client, server};
use secrecy::SecretString;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Notify;
use tracing::info;
use tokio::time::{timeout, Duration};

#[tokio::test]
async fn test_quic_connection() {
    // Initialize the tracing subscriber for logging
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_test_writer() // Ensures logs appear during `cargo test`
        .try_init();

    // Setup temporary config paths for certificates
    let config_dir = PathBuf::from(std::env::temp_dir());

    // Define server and client configuration
    let server_config: server::ServerConfig<()> = server::ServerConfig::create_default_config(
        config_dir.clone(),
        "localhost".to_string(),
        SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 8443),
        SecretString::new("test_psk".into()),
        None,
    );

    let client_config: client::ClientConfig<()> = client::ClientConfig::create_default_config(
        config_dir,
        SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 0),
        SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 8443),
        Some("localhost".to_string()),
        SecretString::new("test_psk".into()),
        None,
    );

    // Create a Notify instance to signal when a connection is established.
    let notify = Arc::new(Notify::new());
    let notify_clone = Arc::clone(&notify);

    let server_handle = tokio::spawn(async move {
        match server::run_quic_server(server_config, move |_, _conn| {
            let notify_inner = Arc::clone(&notify_clone);
            async move {
                info!("Server: New connection established");
                notify_inner.notify_one();
                Ok(())
            }
        }).await {
            Ok(_) => info!("Server finished successfully"),
            Err(e) => panic!("Server error: {:?}", e),
        }
    });
    
    let client_handle: tokio::task::JoinHandle<Result<(), Error>> = tokio::spawn(async move {
        match client::run_quic_client(client_config, |_, _conn| async move {
            info!("Client: Connection established");
            Ok(())
        }).await {
            Ok(result) => Ok(result),
            Err(e) => panic!("Client error: {:?}", e),
        }
    });

    let notify_result = timeout(Duration::from_secs(5), notify.notified()).await;
    assert!(notify_result.is_ok(), "Server did not signal within timeout");

    // Await the client result.
    let client_result = client_handle.await.unwrap();
    assert!(
        client_result.is_ok(),
        "Client failed to connect: {:#?}",
        client_result
    );

    // Optionally, wait for the server task to finish.
    server_handle.await.unwrap();
}
