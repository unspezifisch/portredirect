// PortRedirector-RS Server - QUIC client handler
//
// License: GPL-3.0-only

use crate::app_data::ServerAppData;
use crate::protocol::auth::server_authenticate;
use crate::protocol::utils::SystemTimeProvider;
use crate::quic::server::{run_quic_server, ServerConfig};
use crate::quic::transport::QuinnAuthStream;
use crate::server::auth::handle_quic_client_auth;
use crate::{get_config_dir, ByteCount, PortRedirectProtocol};
use anyhow::{anyhow, Context, Result};
use clap::Parser;
use secrecy::SecretString;
use std::net::ToSocketAddrs;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio::time::sleep;
use tracing::{debug, error, info, instrument, span, Level};

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
    let mut auth_stream = handle_quic_client_auth(Arc::clone(&config), conn.clone())
        .await
        .with_context(|| {
            format!(
                "failed to authenticate PR QUIC client from {}",
                conn.remote_address()
            )
        })?;

    // TODO do the tasks still need this conn?
    {
        let mut quinn_conn = config.app_data.connection.lock().unwrap();
        *quinn_conn = Some(conn);
    }

    // Keep AUTH channel open, we might add some stats transmission later.
    let mut pong_count = 0usize;
    loop {
        sleep(PortRedirectProtocol::CONNECTION_KEEPALIVE_INTERVAL_SECONDS).await;

        // ping the client
        if let Err(e) = auth_stream.write_all(b"PING\n").await {
            error!("Failed to send PING: {:?}", e);
            break;
        }
        if let Err(e) = auth_stream.flush().await {
            error!("Failed to flush PING: {:?}", e);
            break;
        }

        // receive response from client
        let mut response_buf = [0u8; 5];
        match auth_stream.read(&mut response_buf).await {
            Ok(n) => {
                let response = std::str::from_utf8(&response_buf[..n])?;
                if response == "PONG\n" {
                    pong_count += 1;
                    info!("Received PONG, count: {}", pong_count);
                } else {
                    error!("Unexpected response to PING: {:?}", response);
                    break;
                }
            }
            Err(e) => {
                error!("Failed to read PONG: {:?}", e);
                break;
            }
        }
    }

    {
        let mut quinn_conn = config.app_data.connection.lock().unwrap();
        *quinn_conn = None;
    }

    Ok(())
}
