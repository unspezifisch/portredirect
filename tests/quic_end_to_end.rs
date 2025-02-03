// End-to-End Tests for the QUIC Client-Server Setup

use portredirect::quic::{client, server};
use secrecy::SecretString;
use tokio::sync::Notify;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

#[tokio::test]
async fn test_quic_connection() {
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

    // Create a Notify instance to signal when the server is ready.
    let notify = Arc::new(Notify::new());
    let notify_clone = Arc::clone(&notify);

    // Start server in a separate task.
    tokio::spawn(async move {
        let result = server::run_quic_server(server_config, move |_, _conn| {
            // Clone notify into the callback.
            let notify_inner = Arc::clone(&notify_clone);
            async move {
                info!("Server: New connection established");
                notify_inner.notify_one();
                Ok(())
            }
        })
        .await;

        assert!(result.is_ok());
    });

    // Wait for the notification that the server is ready.
    notify.notified().await;

    // Run client and test connection.
    let result = client::run_quic_client(client_config, |_, _conn| async move {
        info!("Client: Connection established");
        Ok(())
    })
    .await;

    assert!(result.is_ok(), "Client failed to connect: {:#?}", result);
}
