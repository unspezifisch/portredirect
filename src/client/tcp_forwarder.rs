// PortRedirect Client - Bridge QUIC stream to TCP
//
// License: GPL-3.0-only

use crate::app_data::ClientAppData;
use crate::forward::forward_bidirectional;
use crate::quic::client::ClientConfig;

use super::metrics_counter::{BYTES_TRANSMITTED_A, BYTES_TRANSMITTED_B};

use anyhow::{anyhow, Error, Result};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tracing::{debug, instrument};

// Handles individual QUIC streams.
#[instrument[skip(config, quic_stream)]]
pub async fn forward_tcp_to_quic_stream<QuicStreamType>(
    config: Arc<ClientConfig<ClientAppData>>,
    mut quic_stream: QuicStreamType,
) -> Result<(), Error>
where
    QuicStreamType: AsyncRead + AsyncWrite + Unpin + std::fmt::Display,
{
    // Create TCP connection to remote destination
    let mut tcp_stream = tokio::net::TcpStream::connect(&config.app_data.forward_destination)
        .await
        .map_err(|e| anyhow!("failed to connect to destination: {}", e))?;

    let stream_name = format!("Client-A:TCP|B:QUIC({})", quic_stream);
    debug!(
        "Starting TCP<->QUIC stream handler, stream id {}",
        stream_name.clone()
    );

    forward_bidirectional(
        &mut tcp_stream,  // A
        &mut quic_stream, // B
        stream_name.clone(),
        // Force dereferencing here because the counter is a LazyStatic.
        &*BYTES_TRANSMITTED_A,
        &*BYTES_TRANSMITTED_B,
    )
    .await?;

    debug!(
        "Closed TCP<->QUIC stream handler, stream id {}",
        stream_name
    );
    Ok(())
}
