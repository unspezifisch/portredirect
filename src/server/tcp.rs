// PortRedirector-RS Server - Incoming TCP connection handler
//
// License: GPL-3.0-only

use crate::app_data::ServerAppData;
use crate::protocol::auth::server_authenticate;
use crate::protocol::utils::SystemTimeProvider;
use crate::quic::server::{run_quic_server, ServerConfig};
use crate::quic::transport::QuinnAuthStream;
use crate::server::auth::handle_quic_client_auth;
use crate::{get_config_dir, ByteCount, PortRedirectProtocol};
use anyhow::{anyhow, Context, Result};
use clap::Parser;
use secrecy::SecretString;
use std::net::ToSocketAddrs;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio::time::sleep;
use tracing::{debug, error, info, instrument, span, Level};

use super::utils::QuinnWorkerBundle;

// Handles incoming TCP connections, forwards them to a QUIC stream.
// TODO consolidate with client/main.rs
#[allow(unused)]
pub async fn handle_tcp_to_quic_stream(
    mut tcp_stream: tokio::net::TcpStream,
    mut bundle: QuinnWorkerBundle,
) -> Result<()> {
    let stream_id = bundle.quic_send.id();
    let (mut tcp_read_half, mut tcp_write_half) = tcp_stream.into_split();

    // Forward TCP -> QUIC
    let tcp_to_quic: JoinHandle<Result<ByteCount>> = tokio::spawn(async move {
        let mut byte_count = 0 as ByteCount;
        let mut buf = [0; PortRedirectProtocol::TCP_QUIC_FORWARDING_BUFFER_SIZE];
        while let Ok(bytes_read) = tcp_read_half.read(&mut buf).await {
            if bytes_read == 0 {
                break; // End of stream
            }
            byte_count += bytes_read as ByteCount;
            bundle.quic_send.write_all(&buf[..bytes_read]).await?;
        }
        bundle.quic_send.finish()?; // Signal end of stream
        _ = bundle.quic_send.stopped().await; // Wait for the stream to be closed
        Ok(byte_count)
    });

    // Forward QUIC -> TCP
    let quic_to_tcp: JoinHandle<Result<ByteCount>> = tokio::spawn(async move {
        let mut byte_count = 0 as ByteCount;
        let mut buf = [0; PortRedirectProtocol::TCP_QUIC_FORWARDING_BUFFER_SIZE];
        while let Ok(Some(bytes_read)) = bundle.quic_recv.read(&mut buf).await {
            if bytes_read == 0 {
                break; // End of stream
            }
            byte_count += bytes_read as ByteCount;
            tcp_write_half.write_all(&buf[..bytes_read]).await?;
        }
        tcp_write_half.shutdown().await?;
        Ok(byte_count)
    });

    // Wait for both directions to complete.
    let result = tokio::try_join!(tcp_to_quic, quic_to_tcp);
    match result {
        Ok((Ok(quic_tx_bytes), Ok(quic_rx_bytes))) => {
            info!(
                "TCP to QUIC byte count: {:?}, QUIC to TCP byte count: {:?}, stream id {}",
                quic_tx_bytes, quic_rx_bytes, stream_id
            );
        }
        Ok((Err(e), _)) | Ok((_, Err(e))) => {
            return Err(anyhow!(
                "Error in one side of QUIC tunnel for TCP forwarding: {:?}, stream id {}",
                e,
                stream_id
            ));
        }
        Err(e) => {
            return Err(anyhow!(
                "Join error in TCP forwarding: {:?}, stream id {}",
                e,
                stream_id
            ));
        }
    }

    debug!("Closed QUIC stream for TCP connection");
    Ok(())
}
