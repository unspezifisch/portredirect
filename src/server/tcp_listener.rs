// PortRedirect Server - Listener for external TCP connections
//
// License: GPL-3.0-only

use crate::bi_stream::BiStream;
use crate::server::tcp_forwarder::forward_tcp_to_quic_stream;
use crate::{app_data::ServerAppData, quic::server::ServerConfig};

use anyhow::{anyhow, Result};
use std::{sync::Arc, time::Instant};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::{debug, error, info, instrument, warn};

/// Accepts TCP connections and bridges them to QUIC.
#[instrument(skip(listener, config))]
pub async fn handle_tcp_listener(
    config: Arc<ServerConfig<ServerAppData>>,
    quic_conn: quinn::Connection,
    listener: TcpListener,
) -> Result<()> {
    info!("TCP listening on {}", listener.local_addr()?);

    loop {
        // Accept new external TCP connection.
        let (mut tcp_stream, peer_addr) = match listener.accept().await {
            Ok(conn) => conn,
            Err(e) => {
                error!("Failed to accept TCP connection: {}", e);
                continue;
            }
        };
        debug!("Accepted TCP connection from {:?}", peer_addr);

        // Open a bidirectional QUIC stream.
        let (send, recv) = match quic_conn.open_bi().await {
            Ok(stream) => stream,
            Err(e) => {
                error!("failed to open QUIC stream: {}", e);
                if let Err(e) = tcp_stream.shutdown().await {
                    error!("Failed to shutdown TCP stream: {:?}", e);
                }
                continue;
            }
        };

        let stream_id = recv.id(); // it's the same id for both directions
        let quic_stream = BiStream::new(recv.compat(), send.compat_write(), stream_id.to_string());
        debug!("Opened QUIC stream (id: {}) for TCP forwarding", stream_id);

        // Spawn a new task to handle forwarding between TCP and QUIC.
        tokio::spawn(async move {
            let start_time = Instant::now();

            if let Err(e) = forward_tcp_to_quic_stream(tcp_stream, quic_stream).await {
                warn!("TCP-to-QUIC stream terminated (id: {}): {:?}", stream_id, e);
            } else {
                debug!("TCP-to-QUIC stream (id {}) completed", stream_id);
            }

            debug!(
                "Stream (id {}) terminated after {:?}",
                stream_id,
                start_time.elapsed()
            );
        });
    }
}
