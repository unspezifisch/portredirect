use crate::forward::forward_bidirectional;
use crate::quic::transport::GenericQuicStream;
use anyhow::Result;
use tracing::debug;

/// Handles an incoming TCP connection and forwards it to a QUIC stream.
pub async fn handle_tcp_to_quic_stream(
    mut tcp_stream: tokio::net::TcpStream,
    mut quic_stream: GenericQuicStream,
) -> Result<()> {
    // Render the display strings before calling forward_bidirectional.
    let stream_id_tcp = format!("{}", tcp_stream.peer_addr()?);
    let stream_id_quic = format!("{}", quic_stream);

    // Now pass the strings. The mutable borrow of `quic_stream` is separate from the owned strings.
    forward_bidirectional(
        &mut tcp_stream,
        &mut quic_stream,
        stream_id_tcp,
        stream_id_quic.clone(),
    )
    .await?;

    debug!(
        "Closed QUIC stream for TCP connection (stream id {})",
        stream_id_quic
    );
    Ok(())
}
