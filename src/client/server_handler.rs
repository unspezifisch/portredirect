// PortRedirector-RS Client - Handle connection to QUIC/PRRS server
//
// License: GPL-3.0-only

use crate::client::metrics::{
    CONNECTIONS_ACCEPTED, KEEPALIVE_ERRORS, SERVER_CONNECTIONS_GRACEFULLY_CLOSED_TOTAL,
    SERVER_CONNECTIONS_OPENED_TOTAL, TCP_FORWARDING_ERRORS,
};
use crate::protocol::keepalive::run_keepalive_client_loop;
use crate::quic::client::ClientConfig;
use crate::{app_data::ClientAppData, quic::transport::GenericQuicStream};
use anyhow::{Context, Result};
use std::sync::Arc;
use tracing::{debug, info, instrument, warn};

use super::auth::handle_quic_auth_client_side;
use super::tcp::forward_tcp_to_quic_stream;

/// Handles the connection to the QUIC server, authenticates and keeps it alive.
/// Called directly by run_quic_client.
#[instrument(skip(config, conn))]
pub async fn handle_quic_server_connection(
    config: Arc<ClientConfig<ClientAppData>>,
    conn: quinn::Connection,
) -> Result<()> {
    // We have just connected to the QUIC server.
    SERVER_CONNECTIONS_OPENED_TOTAL.inc();

    // We need to prove we know the PSK to authenticate.
    let auth_stream = handle_quic_auth_client_side(Arc::clone(&config), conn.clone())
        .await
        .context("failed to authenticate against PR QUIC server")?;

    // Start the keepalive loop to maintain the QUIC connection.
    // This loop periodically sends a PING and expects a PONG response.
    tokio::spawn(async move {
        if let Err(e) = run_keepalive_client_loop(auth_stream).await {
            KEEPALIVE_ERRORS.inc();
            warn!("Keepalive loop terminated with error: {}", e);
        }
    });

    // Accept bidirectional QUIC streams for new forwarded connections.
    while let Ok((send, recv)) = conn.accept_bi().await {
        info!("Opened QUIC stream for new forwarded connection");
        CONNECTIONS_ACCEPTED.inc();

        let quic_stream = GenericQuicStream::new(send, recv);
        let config = Arc::clone(&config);
        tokio::spawn(async move {
            if let Err(e) = forward_tcp_to_quic_stream(config, quic_stream).await {
                TCP_FORWARDING_ERRORS.inc();
                warn!("Error handling QUIC stream: {}", e);
            }
        });
    }

    debug!("Closed QUIC connection handler");
    SERVER_CONNECTIONS_GRACEFULLY_CLOSED_TOTAL.inc();
    Ok(())
}
