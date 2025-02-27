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
    config: Arc<ServerConfig<Arc<ServerAppData>>>,
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

    // Store the connection in shared state.
    // (Assuming that app_data.connection is now a tokio::sync::Mutex<Option<quinn::Connection>>)
    {
        let mut quinn_conn = config.app_data.connection.lock().unwrap();
        *quinn_conn = Some(conn.clone());
    }

    // Run the keepalive (PING/PONG) loop.
    let keepalive_result = run_keepalive_server_loop(auth_stream).await;

    // Clear the stored connection.
    {
        let mut quinn_conn = config.app_data.connection.lock().unwrap();
        *quinn_conn = None;
    }

    // Optionally, gracefully close the connection if supported:
    conn.close(0u32.into(), b"normal shutdown");

    keepalive_result
}
