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

        // Try to get the active QUIC connection.
        let quic_conn = match config.app_data.connection.as_ref() {
            Some(conn) => conn,
            None => {
                error!("No active QUIC connection available to handle TCP traffic");
                if let Err(e) = tcp_stream.shutdown().await { // TODO what's the worst case duration of this?
                    error!("Failed to shutdown TCP stream: {:?}", e);
                }
                continue;
            }
        };

        // Open a bidirectional QUIC stream.
        let (send, recv) = quic_conn
            .open_bi()
            .await
            .map_err(|e| anyhow!("failed to open AUTH stream: {}", e))?;

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
