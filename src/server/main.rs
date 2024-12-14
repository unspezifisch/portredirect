// PortRedirector-RS Server
//
// License: GPL-3.0-only

use anyhow::Result;
use clap::{Parser, ValueEnum};
use portredirect::get_config_dir;
use portredirect::quic::server::{run_quic_server, ServerConfig};
use quinn::Connection;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, error, info, span, Level};

/// Modes of operation for the port redirector.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
enum Mode {
    Quic,
    DirectForwarding,
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
    quic_psk: Option<String>,

    /// Mode of operation: quic or direct-forwarding.
    #[clap(long, value_enum)]
    mode: Mode,
}

/// Data structure to hold connection statistics.
struct ConnectionStats {
    connection_count: usize,
    total_bytes: u64,
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

    if args.mode == Mode::DirectForwarding {
        // Ensure required arguments are provided
        match (args.remote_host.as_ref(), args.remote_port) {
            (Some(host), Some(port)) => {
                remote_addr = format!("{}:{}", host, port);
            }
            _ => {
                error!("Error: --remote-host and --remote-port must be specified in DirectForwarding mode.");
                std::process::exit(1);
            }
        }
    } else if args.mode == Mode::Quic {
        match (args.quic_server_host, args.quic_server_port, args.quic_psk) {
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
    } else {
        unreachable!();
    }

    // Shared state for connection statistics.
    let stats = Arc::new(Mutex::new(ConnectionStats {
        connection_count: 0,
        total_bytes: 0,
    }));

    // Spawn a task to periodically print stats.
    let stats_clone = stats.clone();
    tokio::spawn(async move {
        use tokio::time::{sleep, Duration};
        let mut printed_once = false;
        let mut previous_total_bytes = 0u64;

        loop {
            sleep(Duration::from_secs(1)).await;
            let stats = stats_clone.lock().unwrap();
            if stats.total_bytes != previous_total_bytes || !printed_once {
                info!(
                    "Active connections: {}, Total data: {} bytes",
                    stats.connection_count, stats.total_bytes
                );
                previous_total_bytes = stats.total_bytes;
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

    // Create QUIC server if needed.
    if args.mode == Mode::Quic {
        rustls::crypto::ring::default_provider()
            .install_default()
            .expect("Failed to install rustls crypto provider");

        info!("QUIC listening on {}", quic_bind_addr.clone());

        let config = ServerConfig::create_default_config(
            config_dir,
            args.quic_cert_hostname,
            quic_bind_addr,
        );

        // Spawn the QUIC server
        tokio::spawn(async {
            if let Err(e) = run_quic_server(config, handl).await {
                error!(error = %e, "QUIC thread error");
            }
        });
    }

    // Handle incoming TCP connections forever.
    loop {
        let (local_socket, _) = match listener.accept().await {
            Ok(listener) => listener,
            Err(e) => {
                eprintln!("Failed to accept connection: {}", e);
                continue;
            }
        };
        debug!("New TCP connection from: {:?}", local_socket.peer_addr());

        if args.mode == Mode::DirectForwarding {
            let stats = stats.clone();
            let remote_addr = remote_addr.clone();

            tokio::spawn(async move {
                // Handle the connection.
                if let Err(e) =
                    handle_tcp_connection_redirect(local_socket, remote_addr, stats).await
                {
                    error!("Connection error: {}", e);
                }
            });
        } else if args.mode == Mode::Quic {
            tokio::spawn(async move {
                // Increment connection count.
                {
                    let mut stats = stats.lock().unwrap();
                    stats.connection_count += 1;
                }

                let start = Instant::now();
                if let Err(e) = handle_tcp_to_quic(tcp_stream).await {
                    eprintln!("Error handling TCP connection: {:?}", e);
                }
                debug!("TCP connection terminated after {:?}", start.elapsed());

                // Decrement connection count.
                {
                    let mut stats = stats.lock().unwrap();
                    stats.connection_count -= 1;
                }
            });
        } else {
            unreachable!();
        }
    }
}

/// Handles a single connection by forwarding traffic between the local and remote sockets.
///
/// # Arguments
/// * `local_socket` - The accepted local socket.
/// * `remote_addr` - The address of the remote server.
/// * `stats` - Shared statistics for connection tracking.
async fn handle_tcp_connection_redirect(
    local_socket: TcpStream,
    remote_addr: String,
    stats: Arc<Mutex<ConnectionStats>>,
) -> io::Result<()> {
    let remote_socket = TcpStream::connect(remote_addr).await?;

    // Split the sockets into read and write halves
    let (mut local_read, mut local_write) = local_socket.into_split();
    let (mut remote_read, mut remote_write) = remote_socket.into_split();

    let stats_clone = stats.clone();

    // Forward data from local to remote.
    let local_to_remote_task = tokio::spawn(async move {
        let mut buffer = [0u8; 1024];
        while let Ok(bytes_read) = local_read.read(&mut buffer).await {
            if bytes_read == 0 {
                break;
            }
            remote_write.write_all(&buffer[..bytes_read]).await?;

            let mut stats = stats_clone.lock().unwrap();
            stats.total_bytes += bytes_read as u64;
        }
        Ok::<(), io::Error>(())
    });

    let stats_clone = stats.clone();
    // Forward data from remote to local.
    let remote_to_local_task = tokio::spawn(async move {
        let mut buffer = [0u8; 1024];
        while let Ok(bytes_read) = remote_read.read(&mut buffer).await {
            if bytes_read == 0 {
                break;
            }
            local_write.write_all(&buffer[..bytes_read]).await?;

            let mut stats = stats_clone.lock().unwrap();
            stats.total_bytes += bytes_read as u64;
        }
        Ok::<(), io::Error>(())
    });

    // Wait for both tasks to complete.
    let _ = tokio::try_join!(local_to_remote_task, remote_to_local_task)?;

    Ok(())
}

// Handles incoming TCP connections.
#[allow(unused)]
async fn handle_tcp_to_quic(
    mut tcp_stream: tokio::net::TcpStream,
) -> Result<(), Box<dyn std::error::Error>> {
    // Open a new QUIC stream
    //let (mut quic_send, mut quic_recv) = quic_connection.open_bi().await?;
    debug!("Opened QUIC stream for TCP forwarding");

    loop {
        debug!("handle_tcp_to_quic");
        sleep(Duration::from_secs(10)).await;
    }

    /* TODO implement forwarding
      // Forward TCP -> QUIC
      let tcp_to_quic = tokio::spawn(async move {
          let mut buf = [0; 1024];
          while let Ok(bytes_read) = tcp_stream.read(&mut buf).await {
              if bytes_read == 0 {
                  break; // End of stream
              }
              quic_send.write_all(&buf[..bytes_read]).await?;
          }
          quic_send.finish().await?; // Signal end of stream
          Ok::<(), Box<dyn std::error::Error>>(())
      });

      // Forward QUIC -> TCP
      let quic_to_tcp = tokio::spawn(async move {
          let mut buf = [0; 1024];
          while let Ok(bytes_read) = quic_recv.read(&mut buf).await {
              if bytes_read == 0 {
                  break; // End of stream
              }
              tcp_stream.write_all(&buf[..bytes_read]).await?;
          }
          Ok::<(), Box<dyn std::error::Error>>(())
      });

      // Wait for both directions to complete
      tokio::try_join!(tcp_to_quic, quic_to_tcp)?;
    */
    debug!("Closed QUIC stream for TCP connection");
    Ok(())
}

// Handles
#[instrument(skip(conn))]
async fn handle_connection_quic(conn: quinn::Incoming) -> Result<()> {
    let connection = conn.await?;
    async {
        debug!("QUIC connection established");

        // Each stream initiated by the client constitutes a new request.
        loop {
            let stream = connection.accept_bi().await;
            let stream = match stream {
                Err(quinn::ConnectionError::ApplicationClosed { .. }) => {
                    debug!("QUIC connection closed");
                    return Ok(());
                }
                Err(e) => {
                    return Err(e);
                }
                Ok(s) => s,
            };
            let fut = handle_request_quic(stream);
            tokio::spawn(async move {
                if let Err(e) = fut.await {
                    error!("failed: {reason}", reason = e.to_string());
                }
            });
        }
    }
    .await?;
    Ok(())
}

#[allow(unused)]
#[instrument(skip(send, recv))]
async fn handle_request_quic(
    (mut send, mut recv): (quinn::SendStream, quinn::RecvStream),
) -> Result<()> {
    let req = recv
        .read_to_end(64 * 1024)
        .await
        .map_err(|e| anyhow!("failed reading request: {}", e))?;
    let mut escaped = String::new();
    for &x in &req[..] {
        let part = ascii::escape_default(x).collect::<Vec<_>>();
        escaped.push_str(str::from_utf8(&part).unwrap());
    }
    debug!(escaped=%escaped);

    // Execute the request
    let resp = b"HELLO I AM PRSERVER, WHO ARE YOU?".to_vec();
    // Write the response
    send.write_all(&resp)
        .await
        .map_err(|e| anyhow!("failed to send response: {}", e))?;
 
    Ok(())
}
