// PortRedirect Server - QUIC client handler
//
// License: GPL-3.0-only

use crate::protocol::keepalive::run_control_channel_loop;
use crate::quic::server::ServerConfig;
use crate::{app_data::ServerAppData, server::tcp_listener::handle_tcp_listener};

use super::auth::authenticate_quic_client;

use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::{debug, info, instrument};

// Handles one PR QUIC client connection.
// Called by run_quic_server.
#[instrument(skip(config, quic_conn))]
pub async fn handle_quic_client_connection(
    config: Arc<ServerConfig<ServerAppData>>,
    quic_conn: quinn::Connection,
) -> Result<()> {
    debug!(
        "Handling QUIC client connection from {}",
        quic_conn.remote_address()
    );

    // First, ensure the client is authenticated.
    // TODO add timeout for auth
    let control_stream = match authenticate_quic_client(Arc::clone(&config), quic_conn.clone()).await {
        Ok(stream) => stream,
        Err(err) => {
            // Terminate the connection upon authentication failure.
            quic_conn.close(0u32.into(), b"failed authentication");
            return Err(err).context(format!(
                "failed to authenticate PR QUIC client from {}",
                quic_conn.remote_address()
            ));
        }
    };

    // TODO: handle config over control stream (eg. TCP port)

    // Create the TCP listener.
    let tcp_handle = if !config.app_data.clients_additional_listeners {
        let tcp_addr = config.app_data.default_tcp_listener;
        let listener = match TcpListener::bind(tcp_addr).await {
            Ok(listener) => listener,
            Err(err) => {
                // Terminate the connection upon failure to bind the TCP listener.
                quic_conn.close(0u32.into(), b"failed binding tcp listener");
                return Err(err).context(format!("Failed to bind TCP listener to {}", tcp_addr));
            }
        };

        // Spawn the TCP listener in its own task.
        let tcp_config = Arc::clone(&config);
        let quic_conn_clone = quic_conn.clone();
        let tcp_handle =
            tokio::spawn(async move { handle_tcp_listener(tcp_config, quic_conn_clone, listener).await });

        tcp_handle
    } else {
        info!("Additional TCP listeners not implemented");
        tokio::spawn(async { Ok(()) })
    };

    // Run the keepalive (PING/PONG) loop concurrently.
    let control_channel_result = run_control_channel_loop(control_stream).await;

    // Close the QUIC connection after the keepalive loop completes.
    quic_conn.close(0u32.into(), b"normal shutdown");

    // TODO add possibility for client to initiate teardown of all TCP connections and the listener, over the control stream
    // TODO close all TCP connections
    // TODO tell TCP listener task to quit
    // Await the TCP listener task.
    tcp_handle.await??;

    control_channel_result
}
