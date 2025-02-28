// PortRedirect Server - QUIC client handler
//
// License: GPL-3.0-only

use crate::app_data::ServerAppData;
use crate::protocol::keepalive::run_keepalive_server_loop;
use crate::quic::server::ServerConfig;

use super::auth::handle_quic_client_auth;

use anyhow::{Context, Result};
use std::sync::Arc;
use tracing::{debug, instrument};

// Handles one PR QUIC client connection.
// Called by run_quic_server.
#[instrument(skip(config, conn))]
pub async fn handle_quic_client_connection(
    config: Arc<ServerConfig<ServerAppData>>,
    conn: quinn::Connection,
) -> Result<()> {
    debug!(
        "Handling potential PR QUIC client connection from {}",
        conn.remote_address()
    );

    // First, ensure the client is authenticated.
    let auth_stream = handle_quic_client_auth(Arc::clone(&config), conn.clone())
        .await
        .with_context(|| {
            format!(
                "failed to authenticate PR QUIC client from {}",
                conn.remote_address()
            )
        })?;

    // Run the keepalive (PING/PONG) loop.
    let keepalive_result = run_keepalive_server_loop(auth_stream).await;

    // Create the TCP listener.
    let listener = TcpListener::bind(&local_addr)
        .await
        .with_context(|| format!("Failed to bind TCP listener to {}", local_addr))?;
    info!("TCP listening on {}", listener.local_addr()?);
    
    // Start accepting and handling TCP connections.
    handle_tcp_listener(listener, app_data).await;

    conn.close(0u32.into(), b"normal shutdown");

    keepalive_result
}
