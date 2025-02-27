// PortRedirect Server - Listener for TCP connections
//
// License: GPL-3.0-only

use crate::app_data::ServerAppData;
use crate::bi_stream::BiStream;
use crate::server::tcp_forwarder::forward_tcp_to_quic_stream;

use anyhow::{anyhow, Result};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpListener;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::{debug, error, instrument, warn};

/// Accepts TCP connections and bridges them to QUIC.
#[instrument(skip(listener, app_data))]
pub async fn handle_tcp_listener(
    listener: TcpListener,
    app_data: Arc<ServerAppData>,
    active_connections: Arc<AtomicUsize>,
) -> Result<()> {
    loop {
        let (tcp_stream, peer_addr) = match listener.accept().await {
            Ok(conn) => conn,
            Err(e) => {
                error!("Failed to accept TCP connection: {}", e);
                continue;
            }
        };
        debug!("Accepted TCP connection from {:?}", peer_addr);

        // Try to get the active QUIC connection.
        let quic_conn = {
            // Note: If the lock fails, the application will panic.
            app_data.connection.lock().unwrap().clone()
        };

        let quic_conn = match quic_conn {
            Some(conn) => conn,
            None => {
                error!("No active QUIC connection available to handle TCP traffic");
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

        let connections = active_connections.clone();

        // Spawn a new task to handle forwarding between TCP and QUIC.
        tokio::spawn(async move {
            // Increment active connections.
            connections.fetch_add(1, Ordering::SeqCst);
            let start_time = Instant::now();

            if let Err(e) = forward_tcp_to_quic_stream(tcp_stream, quic_stream).await {
                warn!(
                    "TCP-to-QUIC stream terminated (id: {}): {:?}",
                    stream_id, e
                );
            } else {
                debug!("TCP-to-QUIC stream (id {}) completed", stream_id);
            }

            debug!(
                "Stream (id {}) terminated after {:?}",
                stream_id,
                start_time.elapsed()
            );
            // Decrement active connections.
            connections.fetch_sub(1, Ordering::SeqCst);
        });
    }
}
