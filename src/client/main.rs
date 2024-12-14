use anyhow::Result;
use clap::{Parser, ValueEnum};
use portredirect::get_config_dir;
use portredirect::quic::{run_quic_server, QuicConfig};
use quinn::Connection;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, error, info, span, Level};
use tracing_subscriber;

/// Command-line arguments for the port redirector tool.
#[derive(Parser)]
struct Args {
    /// Destination host for data coming from QUIC connections.
    #[clap(long)]
    destination_host: String,

    /// Destination port.
    #[clap(long)]
    destination_port: u16,

    /// QUIC server listener host.
    #[clap(long, default_value = "127.0.0.1")]
    quic_server_host: String,

    /// QUIC server listener port.
    #[clap(long, default_value = "4433")]
    quic_server_port: u16,

    /// Pre-shared key for authentication over QUIC.
    #[clap(long)]
    quic_psk: Option<String>,
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

    let root_span = span!(Level::INFO, "prclient_main");
    let _enter = root_span.enter();

    // Get or create config directory.
    let config_dir = get_config_dir()?;
    info!("Configuration directory: {:?}", config_dir);

    // Parse args.
    let args = Args::parse();
    let mut remote_addr = String::new();
    let mut quic_bind_addr: SocketAddr = "127.0.0.1:4433".parse().expect("Failed to parse address");
    let local_addr = format!("{}:{}", args.local_host, args.local_port);

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

    // HACK Create QUIC server if needed.
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    info!("QUIC listening on {}", quic_bind_addr.clone());

    let config =
        QuicConfig::create_default_config(config_dir, args.quic_cert_hostname, quic_bind_addr);

    // Spawn the QUIC server
    tokio::spawn(async {
        if let Err(e) = run_quic_server(config).await {
            error!(error = %e, "QUIC thread error");
        }
    });

    // Accept incoming connections.
    loop {
        let (local_socket, _) = match listener.accept().await {
            Ok(listener) => listener,
            Err(e) => {
                eprintln!("Failed to accept connection: {}", e);
                continue;
            }
        };
        debug!("New TCP connection from: {:?}", local_socket.peer_addr());

        // Increment connection count.
        {
            let mut stats = stats.lock().unwrap();
            stats.connection_count += 1;
        }

        /* TODO client glue
        let quic_connection = quic_connection.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_tcp_to_quic(tcp_stream, quic_connection).await {
                eprintln!("Error handling TCP connection: {:?}", e);
            }
        })*/

        // Decrement connection count.
        {
            let mut stats = stats.lock().unwrap();
            stats.connection_count -= 1;
        }
    }
}

#[allow(unused)]
async fn handle_tcp_to_quic(
    mut tcp_stream: tokio::net::TcpStream,
    quic_connection: Connection,
) -> Result<(), Box<dyn std::error::Error>> {
    // Open a new QUIC stream
    let (mut quic_send, mut quic_recv) = quic_connection.open_bi().await?;
    debug!("Opened QUIC stream for TCP forwarding");

    /*
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
