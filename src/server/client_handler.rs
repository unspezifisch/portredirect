// PortRedirector-RS Server - QUIC client handler
//
// License: GPL-3.0-only

use crate::app_data::ServerAppData;
use crate::quic::server::ServerConfig;
use crate::server::auth::handle_quic_client_auth;
use crate::PortRedirectProtocol;
use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::time::{interval, timeout};
use tracing::{debug, error, info, instrument};

// Constants for the protocol
const KEEP_ALIVE_INTERVAL: Duration = PortRedirectProtocol::CONNECTION_KEEPALIVE_INTERVAL_SECONDS;
const PING_MESSAGE: &[u8] = b"PING\n";
const EXPECTED_PONG: &str = "PONG\n";
const READ_TIMEOUT: Duration = Duration::from_secs(10);

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
    let keepalive_result = run_keepalive_loop(auth_stream).await;

    // Clear the stored connection.
    {
        let mut quinn_conn = config.app_data.connection.lock().unwrap();
        *quinn_conn = None;
    }

    // Optionally, gracefully close the connection if supported:
    conn.close(0u32.into(), b"normal shutdown");

    keepalive_result
}

/// Runs the keepalive loop that periodically sends a PING and expects a PONG response.
async fn run_keepalive_loop<T>(mut auth_stream: T) -> Result<()>
where
    T: AsyncReadExt + AsyncWriteExt + Unpin,
{
    let mut tick_interval = interval(KEEP_ALIVE_INTERVAL);
    let mut pong_count = 0usize;

    loop {
        tick_interval.tick().await;

        // Send PING message.
        if let Err(e) = auth_stream.write_all(PING_MESSAGE).await {
            error!("Failed to send PING: {:?}", e);
            break;
        }
        if let Err(e) = auth_stream.flush().await {
            error!("Failed to flush PING: {:?}", e);
            break;
        }
        debug!("PING sent to client");

        // Read response with a timeout.
        let mut response_buf = Vec::with_capacity(16);
        // We use a BufReader to read until a newline. Alternatively, if the protocol
        // guarantees a fixed-size response you could use read_exact.
        let mut reader = BufReader::new(&mut auth_stream);
        match timeout(READ_TIMEOUT, reader.read_until(b'\n', &mut response_buf)).await {
            Ok(Ok(0)) => {
                error!("Client closed the connection");
                break;
            }
            Ok(Ok(_)) => {
                let response = std::str::from_utf8(&response_buf)
                    .context("Received invalid UTF-8 response")?;
                if response == EXPECTED_PONG {
                    pong_count += 1;
                    info!("Received PONG, count: {}", pong_count);
                } else {
                    error!("Unexpected response to PING: {:?}", response);
                    break;
                }
            }
            Ok(Err(e)) => {
                error!("Failed to read PONG: {:?}", e);
                break;
            }
            Err(_) => {
                error!("Timed out waiting for PONG response");
                break;
            }
        }
    }

    Ok(())
}
