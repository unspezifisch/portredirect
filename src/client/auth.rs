// PortRedirector-RS Client
//
// License: GPL-3.0-only

use crate::app_data::ClientAppData;
use crate::protocol::auth::client_authenticate;
use crate::quic::client::{run_quic_client, ClientConfig};
use crate::quic::transport::QuinnAuthStream;
use crate::{get_config_dir, PortRedirectProtocol};
use anyhow::{anyhow, Context, Error, Result};
use clap::Parser;
use secrecy::SecretString;
use std::net::ToSocketAddrs;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::task::JoinHandle;
use tracing::{debug, info, instrument, span, warn, Level};

// Handles our custom authentication stream.
#[instrument[skip(config, connection)]]
pub async fn handle_quic_auth(
    config: Arc<ClientConfig<ClientAppData>>,
    connection: quinn::Connection,
) -> Result<QuinnAuthStream> {
    // Accept the first QUIC stream, which is for authenticating us to the server.
    debug!("Accepting server-initiated QUIC stream.");

    // Client auth loop. Runs until server is happy.
    if let Ok((send, recv)) = connection.accept_bi().await {
        let mut stream = QuinnAuthStream::new(send, recv);
        debug!("opened bidi channel for AUTH with stream {}", stream);

        match client_authenticate(&mut stream, config.app_data.connection_auth_psk.clone()).await {
            Ok(()) => {
                info!("Authentication successful");
            }
            Err(e) => {
                warn!("Authentication failed: {}", e);
                return Err(anyhow!("failed to authenticate against PR QUIC server"));
            }
        }

        Ok(stream)
    } else {
        Err(anyhow!("failed to accept bidi AUTH connection"))
    }
}
