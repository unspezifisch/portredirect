// PortRedirector-RS Server
//
// License: GPL-3.0-only

use anyhow::{anyhow, Context, Result};
use clap::{Parser, ValueEnum};
use portredirect::quic::server::{run_quic_server, ServerConfig};
use portredirect::{get_config_dir, ByteCount, PortRedirectProtocol};
use rand::rngs::OsRng;
use rand::RngCore;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio::time::sleep;
use tracing::{debug, error, info, instrument, span, warn, Level};
use portredirect::PRAppData;

#[derive(Debug)]
struct WorkerBundle {
    quic_recv: quinn::RecvStream,
    quic_send: quinn::SendStream,
}

/// Command-line arguments for the port redirector tool.
#[derive(Parser)]
struct Args {
    /// Local host to bind the listener.
    #[clap(long)]
    local_host: String,

    /// Local port to bind the listener.
    #[clap(long)]
    local_port: u16,

    /// Remote host to forward traffic to.
    #[clap(long)]
    remote_host: Option<String>,

    /// Remote port to forward traffic to.
    #[clap(long)]
    remote_port: Option<u16>,

    /// QUIC server listener host.
    #[clap(long, default_value = "127.0.0.1")]
    quic_server_host: String,

    /// QUIC server listener port.
    #[clap(long)]
    quic_server_port: Option<u16>,

    /// QUIC server certificate Subject Alt Name.
    #[clap(long, default_value = "localhost")]
    quic_cert_hostname: String,

    /// Pre-shared key for authentication over QUIC.
    #[clap(long)]
    quic_psk: Option<SecretString>,
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

    let root_span = span!(Level::INFO, "prserver_main");
    let _enter = root_span.enter();

    // Get or create config directory.
    let config_dir = get_config_dir()?;
    info!("Configuration directory: {:?}", config_dir);

    // Parse args.
    let args = Args::parse();
    let mut remote_addr = String::new();
    let mut quic_bind_addr: SocketAddr = "127.0.0.1:4433".parse().expect("Failed to parse address");
    let local_addr = format!("{}:{}", args.local_host, args.local_port);

    match (args.quic_server_host, args.quic_server_port, &args.quic_psk) {
        (quic_server_host, Some(quic_server_port), Some(_)) => {
            // Parameters are complete.
            quic_bind_addr = format!("{}:{}", quic_server_host, quic_server_port)
                .to_socket_addrs()
                .expect("Invalid host or port")
                .next()
                .expect("Unable to resolve address");
        }
        _ => {
            error!("Error: --quic-server-port and --quic-psk must be specified in Quic mode.");
            std::process::exit(1);
        }
    }

    // Shared state for connection statistics.
    let stats = Arc::new(Mutex::new(ConnectionStats {
        connection_count: 0,
    }));

    // Spawn a task to periodically print stats.
    let stats_clone = stats.clone();
    tokio::spawn(async move {
        use tokio::time::{sleep, Duration};
        let mut printed_once = false;
        let mut previous_connection_count = 0;

        loop {
            sleep(Duration::from_millis(100)).await;
            let stats = stats_clone.lock().unwrap();
            if previous_connection_count != stats.connection_count || !printed_once {
                info!("Active connections: {}", stats.connection_count);
                previous_connection_count = stats.connection_count;
                printed_once = true;
            }
        }
    });

    // Create local listener.
    let listener = TcpListener::bind(local_addr.clone()).await.map_err(|e| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("Failed to bind to {}: {}", local_addr, e),
        )
    })?;
    info!("TCP listening on {}", listener.local_addr()?);

    // Create QUIC server.
    let quic_psk = args
        .quic_psk
        .expect("PSK is required for QUIC PR operation");
    let app_config = Arc::new(PRAppData::new(quic_psk));

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    info!("QUIC listening on {}", quic_bind_addr.clone());

    let config = ServerConfig::create_default_config(
        config_dir,
        args.quic_cert_hostname,
        quic_bind_addr,
        None,
        Some(Arc::clone(&app_config)),
    );

    // Spawn the QUIC server
    tokio::spawn(async {
        if let Err(e) = run_quic_server(config, handle_quic_client_connection).await {
            error!(error = %e, "QUIC thread error");
        }
    });

    // Handle incoming TCP connections forever.
    loop {
        let (local_socket, _) = match listener.accept().await {
            Ok(listener) => listener,
            Err(e) => {
                error!("Failed to accept connection: {}", e);
                continue;
            }
        };
        debug!("New TCP connection from: {:?}", local_socket.peer_addr());

        let stats_clone = Arc::clone(&stats);

        let quinn_conn = {
            let quinn_conn = app_config.quinn_connection.lock().unwrap();
            quinn_conn.clone()
        };
        let quinn_conn = match quinn_conn {
            Some(quinn_conn) => quinn_conn,
            None => {
                error!("Can't accept new TCP connection because we have no QUIC connection");
                // TODO do we need to close this connection?
                continue;
            }
        };

        let (quic_send, quic_recv) = quinn_conn
            .open_bi()
            .await
            .map_err(|e| anyhow!("failed to open QUIC stream: {}", e))?;
        let stream_id = quic_send.id();
        debug!(
            "Opened bidi QUIC stream for TCP forwarding, stream id {}",
            stream_id
        );

        let worker_bundle = WorkerBundle {
            quic_recv,
            quic_send,
        };

        tokio::spawn(async move {
            // Increment connection count.
            {
                let mut stats = stats_clone.lock().unwrap();
                stats.connection_count += 1;
            }

            // Bridge data through QUIC connection to PR client, who bridges it to an outgoing TCP connection.
            let start = Instant::now();
            if let Err(e) = handle_tcp_to_quic_stream(local_socket, worker_bundle).await {
                error!("Error handling QUIC/TCP stream: {:?}", e);
            }
            debug!(
                "QUIC/TCP stream terminated after {:?}, stream id {}",
                start.elapsed(),
                stream_id
            );

            // Decrement connection count.
            {
                let mut stats = stats_clone.lock().unwrap();
                stats.connection_count -= 1;
            }
        });
    }
}

/// Handles a single connection by forwarding traffic between the local and remote sockets.
///
/// # Arguments
/// * `local_socket` - The accepted local socket.
/// * `remote_addr` - The address of the destination.
async fn handle_tcp_to_tcp(local_socket: TcpStream, remote_addr: String) -> Result<()> {
    let remote_socket = TcpStream::connect(remote_addr).await?;

    // Split the sockets into read and write halves
    let (mut local_read, mut local_write) = local_socket.into_split();
    let (mut remote_read, mut remote_write) = remote_socket.into_split();

    // Forward data from local to remote.
    let local_to_remote_task: JoinHandle<Result<()>> = tokio::spawn(async move {
        let mut buffer = [0u8; PortRedirectProtocol::TCP_DIRECT_FORWARDING_BUFFER_SIZE];
        while let Ok(bytes_read) = local_read.read(&mut buffer).await {
            if bytes_read == 0 {
                break;
            }
            remote_write.write_all(&buffer[..bytes_read]).await?;
        }
        Ok(())
    });

    // Forward data from remote to local.
    let remote_to_local_task: JoinHandle<Result<()>> = tokio::spawn(async move {
        let mut buffer = [0u8; PortRedirectProtocol::TCP_DIRECT_FORWARDING_BUFFER_SIZE];
        while let Ok(bytes_read) = remote_read.read(&mut buffer).await {
            if bytes_read == 0 {
                break;
            }
            local_write.write_all(&buffer[..bytes_read]).await?;
        }
        Ok(())
    });

    // Wait for both tasks to complete.
    let result = tokio::try_join!(local_to_remote_task, remote_to_local_task);
    if let Err(e) = result {
        error!("Error in TCP forwarding: {:?}", e);
    }

    Ok(())
}

// Handles incoming TCP connections, forwards them to a QUIC stream.
// TODO consolidate with client/main.rs
#[allow(unused)]
async fn handle_tcp_to_quic_stream(
    mut tcp_stream: tokio::net::TcpStream,
    mut bundle: WorkerBundle,
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

// Handles one PR QUIC client connection.
// Called by run_quic_server.
#[instrument(skip(config, conn))]
async fn handle_quic_client_auth(
    config: Arc<ServerConfig<Arc<PRAppData>>>,
    conn: quinn::Connection,
) -> Result<(quinn::SendStream, quinn::RecvStream)> {
    debug!("Authenticating PR QUIC client");

    // An unknown client just connected, they need to authenticate or get kicked.
    let (mut send, mut recv) = conn
        .open_bi()
        .await
        .map_err(|e| anyhow!("failed to open AUTH stream: {}", e))?;
    debug!("opened bidi channel for AUTH");

    // Step 1: Generate a challenge
    // Add a (coarse) timestamp to guarantee unique challenge.
    let coarse_unix_time = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() / 60;

    // Generate 32 random bytes
    let mut random_bytes = [0u8; 32];
    OsRng.fill_bytes(&mut random_bytes);
    let random_bytes_hex = hex::encode(random_bytes);

    let challenge = format!(
        "this-is-the-challenge-{}-at-{}-pr-v1",
        random_bytes_hex, coarse_unix_time
    );
    debug!(
        challenge_len = challenge.len(),
        challenge = challenge,
        "AUTH: sending challenge"
    );

    // Step 2: Send the challenge
    let auth_start = Instant::now();
    let request = format!("WHO THE HECK ARE YOU?\n{}\n", challenge);
    assert!(
        request.len() <= PortRedirectProtocol::CHALLENGE_REQUEST_BUFFER_LENGTH,
        "Challenge string exceeds maximum length"
    );
    send.write_all(request.as_bytes())
        .await
        .map_err(|e| anyhow!("failed to send AUTH request: {}", e))?;
    send.flush().await?;

    // Step 3: Wait for the client's response
    // SHA-256 is 32 bytes, so hex-encoded length is 64.
    // Add 1 byte for LF ("\n").
    let mut buffer = [0u8; 65];
    let n = recv
        .read(&mut buffer)
        .await
        .map_err(|e| anyhow!("failed to read AUTH response: {}", e))?;
    debug!(
        response_len = n,
        "AUTH: got response in {:?}",
        auth_start.elapsed()
    );

    // Validate length of response.
    let n = n.unwrap_or(0);
    if n != buffer.len() {
        return Err(anyhow!(
            "wrong length AUTH response n={} (want {})",
            n,
            buffer.len()
        ));
    }

    // Decode to string.
    let client_response =
        std::str::from_utf8(&buffer[..n]).map_err(|e| anyhow!("invalid UTF-8: {}", e))?;
    if client_response.len() != buffer.len() {
        return Err(anyhow!(
            "wrong length AUTH response n_decoded={} (want {})",
            client_response.len(),
            buffer.len()
        ));
    }
    if !client_response.ends_with("\n") {
        return Err(anyhow!("wrong line terminator"));
    }

    // Step 4: Compute expected response
    let mut hasher = Sha256::new();
    hasher.update(challenge);
    hasher.update(config.app_data.connection_auth_psk.expose_secret());
    let expected_response = hex::encode(hasher.finalize()) + "\n";

    // Step 5: Validate the client's response
    if client_response == expected_response {
        // Send good result.
        send.write_all(b"HAPPY\n")
            .await
            .map_err(|e| anyhow!("failed to send HAPPY response: {}", e))?;
        send.flush().await?;
    } else {
        // Send bad result.
        send.write_all(b"BAD\n")
            .await
            .map_err(|e| anyhow!("failed to send BAD response: {}", e))?;
        send.flush().await?;
        send.finish()?;
        return Err(anyhow!("Authentication failed, response mismatch"));
    }
    debug!("Authenticated PR QUIC client OK");

    Ok((send, recv))
}

// Handles one PR QUIC client connection.
// Called by run_quic_server.
#[instrument(skip(config, conn))]
async fn handle_quic_client_connection(
    config: Arc<ServerConfig<Arc<PRAppData>>>,
    conn: quinn::Connection,
) -> Result<()> {
    debug!(
        "Handling potential PR QUIC client connection from {}",
        conn.remote_address()
    );

    // First, ensure the client is authenticated.
    let (mut auth_stream_send, mut auth_stream_recv) =
        handle_quic_client_auth(Arc::clone(&config), conn.clone())
            .await
            .with_context(|| {
                format!(
                    "failed to authenticate PR QUIC client from {}",
                    conn.remote_address()
                )
            })?;

    // TODO do the tasks still need this conn?
    {
        let mut quinn_conn = config.app_data.quinn_connection.lock().unwrap();
        *quinn_conn = Some(conn);
    }

    // Keep AUTH channel open, we might add some stats transmission later.
    let mut pong_count = 0usize;
    loop {
        sleep(PortRedirectProtocol::CONNECTION_KEEPALIVE_INTERVAL_SECONDS).await;

        // ping the client
        if let Err(e) = auth_stream_send.write_all(b"PING\n").await {
            error!("Failed to send PING: {:?}", e);
            break;
        }
        if let Err(e) = auth_stream_send.flush().await {
            error!("Failed to flush PING: {:?}", e);
            break;
        }

        // receive response from client
        let mut response_buf = [0u8; 5];
        match auth_stream_recv.read(&mut response_buf).await {
            Ok(Some(n)) => {
                let response = std::str::from_utf8(&response_buf[..n])?;
                if response == "PONG\n" {
                    pong_count += 1;
                    info!("Received PONG, count: {}", pong_count);
                } else {
                    error!("Unexpected response to PING: {:?}", response);
                    break;
                }
            }
            Ok(None) => {
                warn!("Stream is finished");
                break;
            }
            Err(e) => {
                error!("Failed to read PONG: {:?}", e);
                break;
            }
        }
    }

    {
        let mut quinn_conn = config.app_data.quinn_connection.lock().unwrap();
        *quinn_conn = None;
    }

    Ok(())
}
