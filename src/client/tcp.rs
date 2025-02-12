// PortRedirector-RS Client - Bridge QUIC stream to TCP
//
// License: GPL-3.0-only

use crate::app_data::ClientAppData;
use crate::forward::forward_bidirectional;
use crate::quic::client::ClientConfig;
use crate::quic::transport::GenericQuicStream;
use anyhow::{anyhow, Error, Result};
use std::sync::Arc;
use tracing::{debug, instrument};

// Handles individual QUIC streams.
#[instrument[skip(config, quic_stream)]]
pub async fn handle_tcp_forwarding(
    config: Arc<ClientConfig<ClientAppData>>,
    mut quic_stream: GenericQuicStream,
) -> Result<(), Error> {
    let mut tcp_stream =  // Create TCP connection to remote destination
        tokio::net::TcpStream::connect(&config.app_data.forward_destination)
            .await
            .map_err(|e| anyhow!("failed to connect to destination: {}", e))?;

    let stream_id = quic_stream.recv.id();
    debug!("Starting QUIC->TCP stream handler, stream id {}", stream_id);

    forward_bidirectional(&mut tcp_stream, &mut quic_stream, stream_id, stream_id).await?;

    debug!("Closed QUIC->TCP stream handler, stream id {}", stream_id);
    Ok(())
}
