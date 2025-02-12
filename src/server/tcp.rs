use crate::{ByteCount, PortRedirectProtocol};
use anyhow::{Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::task::JoinHandle;
use tracing::{debug, info};

use super::utils::QuinnWorkerBundle;

/// Handles an incoming TCP connection and forwards it to a QUIC stream.
pub async fn handle_tcp_to_quic_stream(
    tcp_stream: tokio::net::TcpStream,
    mut bundle: QuinnWorkerBundle,
) -> Result<()> {
    let stream_id = bundle.quic_send.id();
    let (mut tcp_reader, mut tcp_writer) = tcp_stream.into_split();

    // Task to forward data from TCP to QUIC.
    let tcp_to_quic: JoinHandle<Result<ByteCount>> = tokio::spawn(async move {
        let mut total_bytes: ByteCount = 0;
        let mut buf = [0u8; PortRedirectProtocol::TCP_QUIC_FORWARDING_BUFFER_SIZE];
        loop {
            let n = tcp_reader
                .read(&mut buf)
                .await
                .context("Error reading from TCP stream")?;
            if n == 0 {
                break; // End of stream.
            }
            total_bytes += n as ByteCount;
            bundle
                .quic_send
                .write_all(&buf[..n])
                .await
                .context("Error writing to QUIC stream")?;
        }
        // Signal end of stream and wait for a proper shutdown.
        bundle
            .quic_send
            .finish()
            .context("Error finishing QUIC send stream")?;
        if let Err(e) = bundle.quic_send.stopped().await {
            tracing::warn!("QUIC stream stopped with error: {:?}", e);
        }
        Ok(total_bytes)
    });

    // Task to forward data from QUIC to TCP.
    let quic_to_tcp: JoinHandle<Result<ByteCount>> = tokio::spawn(async move {
        let mut total_bytes: ByteCount = 0;
        let mut buf = [0u8; PortRedirectProtocol::TCP_QUIC_FORWARDING_BUFFER_SIZE];
        loop {
            // The QUIC read returns an Option: `None` or `Some(bytes_read)`.
            let bytes_opt = bundle
                .quic_recv
                .read(&mut buf)
                .await
                .context("Error reading from QUIC stream")?;
            match bytes_opt {
                Some(0) | None => break, // End of stream.
                Some(n) => {
                    total_bytes += n as ByteCount;
                    tcp_writer
                        .write_all(&buf[..n])
                        .await
                        .context("Error writing to TCP stream")?;
                }
            }
        }
        tcp_writer
            .shutdown()
            .await
            .context("Error shutting down TCP writer")?;
        Ok(total_bytes)
    });

    // Wait for both directions to complete.
    let (tcp_to_quic_result, quic_to_tcp_result) =
        tokio::try_join!(tcp_to_quic, quic_to_tcp)
            .context("One of the forwarding tasks failed")?;

    // Now check the results.
    match (tcp_to_quic_result, quic_to_tcp_result) {
        (Ok(n1), Ok(n2)) => {
            info!(
                "TCP→QUIC forwarded {} bytes, QUIC→TCP forwarded {} bytes (stream id {})",
                n1, n2, stream_id
            );
        }
        (Err(e), _) | (_, Err(e)) => {
            return Err(anyhow::anyhow!(
                "Error in one side of the QUIC tunnel for TCP forwarding (stream id {}): {:?}",
                stream_id,
                e
            ));
        }
    }

    debug!("Closed QUIC stream for TCP connection (stream id {})", stream_id);
    Ok(())
}
