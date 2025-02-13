// PortRedirector-RS Server - Bridge TCP to QUIC stream
//
// License: GPL-3.0-only

use crate::forward::forward_bidirectional;
use crate::metrics_helper::DummyCounter;
use crate::quic::transport::GenericQuicStream;

use anyhow::Result;
use tracing::{debug, instrument};

/// Handles an incoming TCP connection and forwards it to a QUIC stream.
#[instrument(skip(tcp_stream, quic_stream))]
pub async fn forward_tcp_to_quic_stream(
    mut tcp_stream: tokio::net::TcpStream,
    mut quic_stream: GenericQuicStream,
) -> Result<()> {
    // Render the display strings before calling forward_bidirectional.
    let stream_id_quic = format!("{}", quic_stream);

    // HACK until the server gets metrics - Create dummy counters for both directions.
    let dummy_counter_a = DummyCounter::new();
    let dummy_counter_b = DummyCounter::new();

    // Now pass the strings. The mutable borrow of `quic_stream` is separate from the owned strings.
    forward_bidirectional(
        &mut tcp_stream,
        &mut quic_stream,
        stream_id_quic.clone(),
        &dummy_counter_a,
        &dummy_counter_b,
    )
    .await?;

    debug!(
        "Closed QUIC stream for TCP connection (stream id {})",
        stream_id_quic
    );
    Ok(())
}
