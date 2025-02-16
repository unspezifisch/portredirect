// PortRedirector-RS Server - Bridge TCP to QUIC stream
//
// License: GPL-3.0-only

use crate::forward::forward_bidirectional;
use crate::metrics_helper::DummyCounter;

use anyhow::Result;
use tokio::io::{AsyncRead, AsyncWrite};
use tracing::{debug, instrument};

/// Handles an incoming TCP connection and forwards it to a QUIC stream.
#[instrument(skip(tcp_stream, quic_stream))]
pub async fn forward_tcp_to_quic_stream<QuicStreamType>(
    mut tcp_stream: tokio::net::TcpStream,
    mut quic_stream: QuicStreamType,
) -> Result<()>
where
    QuicStreamType: AsyncRead + AsyncWrite + Unpin + std::fmt::Display,
{
    // HACK until the server gets metrics - Create dummy counters for both directions.
    let dummy_counter_a = DummyCounter::new();
    let dummy_counter_b = DummyCounter::new();

    let stream_name = format!("Server-A:TCP|B:QUIC({})", quic_stream.to_string());
    debug!(
        "Starting TCP<->QUIC stream handler, stream id {}",
        stream_name.clone()
    );

    forward_bidirectional(
        &mut tcp_stream,  // A
        &mut quic_stream, // B
        stream_name.clone(),
        &dummy_counter_a,
        &dummy_counter_b,
    )
    .await?;

    debug!("Closed TCP<->QUIC stream handler, stream id {}", stream_name);
    Ok(())
}
