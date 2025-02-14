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

use super::metrics::{BYTES_TRANSMITTED_A, BYTES_TRANSMITTED_B};

// Handles individual QUIC streams.
// TODO resolve design differences vs. server/tcp_forwarder.rs
#[instrument[skip(config, quic_stream)]]
pub async fn forward_tcp_to_quic_stream(
    config: Arc<ClientConfig<ClientAppData>>,
    mut quic_stream: GenericQuicStream,
) -> Result<(), Error> {
    let mut tcp_stream =  // Create TCP connection to remote destination
        tokio::net::TcpStream::connect(&config.app_data.forward_destination)
            .await
            .map_err(|e| anyhow!("failed to connect to destination: {}", e))?;

    let stream_id = quic_stream.recv.get_ref().id();
    debug!("Starting QUIC->TCP stream handler, stream id {}", stream_id);

    forward_bidirectional(
        &mut tcp_stream,
        &mut quic_stream,
        stream_id,
        // Force dereferencing here because the counter is a LazyStatic.
        &*BYTES_TRANSMITTED_A,
        &*BYTES_TRANSMITTED_B,
    )
    .await?;

    debug!("Closed QUIC->TCP stream handler, stream id {}", stream_id);
    Ok(())
}
