// PortRedirector-RS Client
//
// License: GPL-3.0-only

use anyhow::{anyhow, Context, Error, Result};
use clap::Parser;
use portredirect::app_data::ClientAppData;
use portredirect::quic::client::{run_quic_client, ClientConfig};
use portredirect::{get_config_dir, PortRedirectProtocol};
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use std::net::ToSocketAddrs;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::task::JoinHandle;
use tracing::{debug, info, instrument, span, warn, Level};

/// Command-line arguments for the port redirector tool.
#[derive(Parser)]
struct Args {
    /// Destination host for data coming from QUIC connections.
    #[clap(long)]
    destination_host: String,

    /// Destination port (currently TCP only).
    #[clap(long)]
    destination_port: u16,

    /// QUIC connection remote host (server).
    #[clap(long)]
    quic_remote_host: String,

    /// QUIC connection remote port (server).
    #[clap(long)]
    quic_remote_port: u16,

    /// QUIC connection local host to bind to (client).
    #[clap(long, default_value = "0.0.0.0")]
    quic_local_host: String,

    /// QUIC connection local port to bind to (client).
    #[clap(long, default_value = "0")]
    quic_local_port: u16,

    /// QUIC remote hostname override for Subject Alt Name match in TLS cert.
    #[clap(long)]
    quic_remote_hostname_match: Option<String>,

    /// Pre-shared key for authentication over QUIC.
    #[clap(long)]
    quic_psk: SecretString,
}

/// Data structure to hold connection statistics.
struct ConnectionStats {
    connection_count: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .with_target(true)
        .with_line_number(true)
        .init();

    let root_span = span!(Level::INFO, "prclient_main");
    let _enter = root_span.enter();

    // Get or create config directory.
    let config_dir = get_config_dir()?;
    info!("Configuration directory: {:?}", config_dir);

    // Parse args.
    let args = Args::parse();
    // Target of "tunneled" TCP connections.
    let destination_addr = format!("{}:{}", args.destination_host, args.destination_port);
    // Local UDP bind address.
    let quic_local_addr = format!("{}:{}", args.quic_local_host, args.quic_local_port)
        .to_socket_addrs()
        .context("constructing QUIC local address")
        .expect("Invalid host or port")
        .next()
        .expect("Unable to resolve address");
    // Remote UDP server address.
    let quic_remote_addr = format!("{}:{}", args.quic_remote_host, args.quic_remote_port)
        .to_socket_addrs()
        .context("constructing QUIC remote address")
        .expect("Invalid host or port")
        .next()
        .expect("Unable to resolve address");

    info!(destination=%destination_addr, local=%quic_local_addr, remote=%quic_remote_addr, "Initializing QUIC Client");

    // Shared state for connection statistics.
    let stats = Arc::new(Mutex::new(ConnectionStats {
        connection_count: 0,
    }));

    // Spawn a task to periodically print stats.
    {
        let stats_clone = Arc::clone(&stats);
        tokio::spawn(async move {
            use tokio::time::{sleep, Duration};
            let mut printed_once = false;
            let mut previous_connection_count = 0;

            loop {
                sleep(Duration::from_secs(1)).await;
                let stats = stats_clone.lock().unwrap();
                if previous_connection_count != stats.connection_count || !printed_once {
                    info!("Active connections: {}", stats.connection_count);
                    previous_connection_count = stats.connection_count;
                    printed_once = true;
                }
            }
        });
    }

    // Create QUIC client.
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    info!("QUIC connecting to {}", quic_remote_addr.clone());

    let forward_destination = destination_addr
        .to_socket_addrs()
        .context("resolving destination address")?
        .next()
        .context("resolving destination address")?;
    let app_config = ClientAppData::new(args.quic_psk, forward_destination);

    let quic_client_config = ClientConfig::create_default_config(
        config_dir,
        quic_local_addr,
        quic_remote_addr,
        args.quic_remote_hostname_match,
        None,
        app_config,
    );

    // Spawn the QUIC client
    run_quic_client(quic_client_config, handle_quic_to_tcp)
        .await
        .context("QUIC client thread")?;

    Ok(())
}

// Handles our custom authentication stream.
#[instrument[skip(config, connection)]]
async fn handle_quic_auth(
    config: Arc<ClientConfig<ClientAppData>>,
    connection: quinn::Connection,
) -> Result<(quinn::SendStream, quinn::RecvStream)> {
    // Accept the first QUIC stream, which is for authenticating us to the server.
    debug!("Accepting server-initiated QUIC stream.");

    // Client auth loop. Runs until server is happy.
    if let Ok((mut send, mut recv)) = connection.accept_bi().await {
        debug!("opened bidi channel for AUTH");

        // Read challenge from the QUIC stream
        let mut buffer = [0u8; PortRedirectProtocol::CHALLENGE_REQUEST_BUFFER_LENGTH];
        recv.read(&mut buffer)
            .await
            .map_err(|e| anyhow!("failed to read from QUIC stream: {}", e))?;

        // Convert the buffer to a string
        let received =
            std::str::from_utf8(&buffer).map_err(|e| anyhow!("received invalid UTF-8: {}", e))?;

        // Split into lines
        let mut lines = received.lines();

        // Validate the first line
        let first_line = lines
            .next()
            .ok_or_else(|| anyhow!("missing first line in authentication message"))?;
        if first_line != "WHO THE HECK ARE YOU?" {
            return Err(anyhow!("unexpected first line: {}", first_line));
        }

        // Validate the second line (challenge request)
        let second_line = lines
            .next()
            .ok_or_else(|| anyhow!("missing second line in authentication message"))?;
        debug!("Received challenge request: {}", second_line);

        // Compute the SHA-256 hash and hex-encode it
        let mut hasher = Sha256::new();
        hasher.update(second_line);
        hasher.update(config.app_data.connection_auth_psk.expose_secret());
        let response_hex = hex::encode(hasher.finalize());
        debug!("Responding with SHA-256 hex: {}", response_hex);

        // Send the response to the server
        let response = response_hex + "\n";
        send.write_all(response.as_bytes())
            .await
            .map_err(|e| anyhow!("failed to send response: {}", e))?;
        send.flush().await?;

        // We expect a line with HAPPY or BAD.
        let mut buffer = [0u8; 16];
        recv.read(&mut buffer)
            .await
            .map_err(|e| anyhow!("failed to read from QUIC stream (final): {}", e))?;

        if let Ok(buffer_str) = std::str::from_utf8(&buffer) {
            if buffer_str[.."HAPPY".len()] == *"HAPPY" {
                // cool
                info!("Authentication successful.")
            } else {
                // not cool
                return Err(anyhow!(
                    "Server was unhappy with our response: {}.",
                    buffer_str.trim()
                ));
            }
        } else {
            // not cool either
            return Err(anyhow!(
                "Server was so unhappy with our response that it sent garbage."
            ));
        }

        debug!("Authentication routine done.");
        Ok((send, recv))
    } else {
        Err(anyhow!("failed to accept bidi AUTH connection"))
    }
}

// Handles incoming QUIC streams, forwards them to their destination.
// Called directly by run_quic_client.
#[instrument[skip(config, conn)]]
async fn handle_quic_to_tcp(
    config: Arc<ClientConfig<ClientAppData>>,
    conn: quinn::Connection,
) -> Result<()> {
    // First, ensure the client is authenticated.
    let (mut auth_stream_send, mut auth_stream_recv) =
        handle_quic_auth(Arc::clone(&config), conn.clone())
            .await
            .context("failed to authenticate against PR QUIC server")?;
    tokio::spawn(async move {
        let mut ping_count = 0usize;
        loop {
            let mut buf = [0u8; 16];
            match auth_stream_recv.read(&mut buf).await {
                Ok(Some(_)) => {
                    if let Ok(text) = std::str::from_utf8(&buf) {
                        if text.starts_with("PING") {
                            ping_count += 1;
                            info!("Received PING, count: {}", ping_count);

                            match auth_stream_send.write_all(b"PONG\n").await {
                                Ok(()) => (),
                                _ => {
                                    warn!("Failed to send PONG");
                                    break;
                                }
                            }
                        } else {
                            info!("Received data (text): {}", text.trim());
                        }
                    } else {
                        info!("Received data (buf): {:?}", buf);
                    }
                }
                Ok(None) => {
                    warn!("Stream is finished");
                    break;
                }
                Err(e) => {
                    warn!("Error reading from auth stream: {}", e);
                    break;
                }
            }
        }
    });

    while let Ok((send, recv)) = conn.accept_bi().await {
        info!("Opened QUIC stream for new forwarded connection");

        let config = Arc::clone(&config);
        tokio::spawn(async move {
            if let Err(e) = handle_quic_stream(config, send, recv).await {
                warn!("Error handling QUIC stream: {}", e);
            }
        });
    }

    debug!("Closed QUIC connection handler");
    Ok(())
}

// Handles individual QUIC streams.
// TODO consolidate with server/main.rs
#[instrument[skip(config, quic_send, quic_recv)]]
async fn handle_quic_stream(
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
