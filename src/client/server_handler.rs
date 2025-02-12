// PortRedirector-RS Client
//
// License: GPL-3.0-only

use crate::app_data::ClientAppData;
use crate::client::auth::handle_quic_auth;
use crate::client::tcp::handle_tcp_forwarding;
use crate::quic::client::ClientConfig;
use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{debug, info, instrument, warn};

// Handles incoming QUIC streams, forwards them to their destination.
// Called directly by run_quic_client.
#[instrument[skip(config, conn)]]
pub async fn handle_quic_server_connection(
    config: Arc<ClientConfig<ClientAppData>>,
    conn: quinn::Connection,
) -> Result<()> {
    // First, ensure the client is authenticated.
    let mut auth_stream = handle_quic_auth(Arc::clone(&config), conn.clone())
        .await
        .context("failed to authenticate against PR QUIC server")?;
    tokio::spawn(async move {
        let mut ping_count = 0usize;
        loop {
            let mut buf = [0u8; 16];
            match auth_stream.read(&mut buf).await {
                Ok(_) => {
                    if let Ok(text) = std::str::from_utf8(&buf) {
                        if text.starts_with("PING") {
                            ping_count += 1;
                            info!("Received PING, count: {}", ping_count);

                            match auth_stream.write_all(b"PONG\n").await {
                                Ok(()) => (),
                                _ => {
                                    warn!("Failed to send PONG");
                                    break;
                                }
                            }
                        } else {
                            info!("Received data (text): {}", text.trim());
                        }
                    } else {
                        info!("Received data (buf): {:?}", buf);
                    }
                }
                Err(e) => {
                    warn!("Error reading from auth stream: {}", e);
                    break;
                }
            }
        }
    });

    while let Ok((send, recv)) = conn.accept_bi().await {
        info!("Opened QUIC stream for new forwarded connection");

        let config = Arc::clone(&config);
        tokio::spawn(async move {
            if let Err(e) = handle_tcp_forwarding(config, send, recv).await {
                warn!("Error handling QUIC stream: {}", e);
            }
        });
    }

    debug!("Closed QUIC connection handler");
    Ok(())
}
