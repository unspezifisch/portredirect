// PortRedirector-RS Client
//
// License: GPL-3.0-only

use crate::app_data::ClientAppData;
use crate::protocol::auth::client_authenticate;
use crate::quic::client::{run_quic_client, ClientConfig};
use crate::quic::transport::QuinnAuthStream;
use crate::{get_config_dir, PortRedirectProtocol};
use anyhow::{anyhow, Context, Error, Result};
use clap::Parser;
use secrecy::SecretString;
use std::net::ToSocketAddrs;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::task::JoinHandle;
use tracing::{debug, info, instrument, span, warn, Level};

// Handles individual QUIC streams.
// TODO consolidate with server/main.rs
#[instrument[skip(config, quic_send, quic_recv)]]
pub async fn handle_tcp_forwarding(
    config: Arc<ClientConfig<ClientAppData>>,
    mut quic_send: quinn::SendStream,
    mut quic_recv: quinn::RecvStream,
) -> Result<(), Error> {
    let tcp_stream =  // Create TCP connection to remote destination
        tokio::net::TcpStream::connect(&config.app_data.forward_destination)
            .await
            .map_err(|e| anyhow!("failed to connect to destination: {}", e))?;

    let stream_id = quic_recv.id();
    debug!("Starting QUIC->TCP stream handler, stream id {}", stream_id);

    let (mut tcp_read_half, mut tcp_write_half) = tcp_stream.into_split();

    // Forward TCP -> QUIC
    let tcp_to_quic: JoinHandle<Result<()>> = tokio::spawn(async move {
        let mut buf = [0; PortRedirectProtocol::TCP_QUIC_FORWARDING_BUFFER_SIZE];
        while let Ok(bytes_read) = tcp_read_half.read(&mut buf).await {
            if bytes_read == 0 {
                break; // End of stream
            }
            quic_send.write_all(&buf[..bytes_read]).await?;
        }
        quic_send.finish()?; // Signal end of stream
        _ = quic_send.stopped().await; // Wait for the stream to be closed
        Ok(())
    });

    // Forward QUIC -> TCP
    let quic_to_tcp: JoinHandle<Result<()>> = tokio::spawn(async move {
        let mut buf = [0; PortRedirectProtocol::TCP_QUIC_FORWARDING_BUFFER_SIZE];
        while let Ok(Some(bytes_read)) = quic_recv.read(&mut buf).await {
            if bytes_read == 0 {
                break; // End of stream
            }
            tcp_write_half.write_all(&buf[..bytes_read]).await?;
        }
        Ok(())
    });

    // Wait for both directions to complete.
    let result = tokio::try_join!(tcp_to_quic, quic_to_tcp);
    if let Err(e) = result {
        return Err(anyhow!(
            "Error in receive side of QUIC tunnel for TCP forwarding: {:?}",
            e
        ));
    }

    debug!("Closed QUIC->TCP stream handler, stream id {}", stream_id);
    Ok(())
}
