// PortRedirector-RS Server - Authenticate client to server
//
// License: GPL-3.0-only

use crate::app_data::ServerAppData;
use crate::protocol::auth::server_authenticate;
use crate::protocol::utils::SystemTimeProvider;
use crate::quic::server::ServerConfig;
use crate::quic::transport::GenericQuicStream;
use anyhow::{anyhow, Result};
use std::sync::Arc;
use tracing::{debug, error, info, instrument};

// Authenticates the PR QUIC client to us, the server.
// Called by handle_quic_client_connection.
#[instrument(skip(config, conn))]
pub async fn handle_quic_client_auth(
    config: Arc<ServerConfig<Arc<ServerAppData>>>,
    conn: quinn::Connection,
) -> Result<GenericQuicStream> {
    debug!("Authenticating PR QUIC client");

    // An unknown client just connected, they need to authenticate or get kicked.
    let (send, recv) = conn
        .open_bi()
        .await
        .map_err(|e| anyhow!("failed to open AUTH stream: {}", e))?;
    let mut stream = GenericQuicStream::new(send, recv);
    debug!("opened bidi channel for AUTH with stream id {}", stream);

    match server_authenticate(
        &mut stream,
        config.app_data.connection_auth_psk.to_owned(),
        &SystemTimeProvider,
    )
    .await
    {
        Ok(()) => {
            info!("Authenticated PR QUIC client OK");
        }
        Err(e) => {
            error!("Failed to authenticate PR QUIC client: {:?}", e);
            return Err(e);
        }
    }

    Ok(stream)
}
