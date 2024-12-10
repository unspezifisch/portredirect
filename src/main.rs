use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use portredirect::quic::setup_quic;
use quinn::Connection;
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

mod quic;

/// Modes of operation for the port redirector.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
enum Mode {
    Quic,
    DirectForwarding,
    UnreachableTestHACK,
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

/// Returns the path to the configuration directory, creating it if necessary.
fn get_config_dir() -> Result<PathBuf> {
    let mut config_dir =
        dirs::config_dir().context("Failed to find your platform's config directory")?;
    config_dir.push("portredirect");

    // Create the directory if it doesn't exist
    std::fs::create_dir_all(&config_dir).context("create config dir")?;

    Ok(config_dir)
}

#[tokio::main]
async fn main() -> Result<()> {
    // Get or create config directory.
    let config_dir = get_config_dir()?;
    println!("Configuration directory: {:?}", config_dir);

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
                eprintln!("Error: --remote-host and --remote-port must be specified in DirectForwarding mode.");
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
                eprintln!(
                    "Error: --quic-server-port and --quic-psk must be specified in Quic mode."
                );
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
                println!(
                    "\rActive connections: {}\tTotal data: {} bytes",
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
    println!("TCP listening on {}", listener.local_addr()?);

    // Create QUIC server if needed.
    if args.mode == Mode::Quic {
        rustls::crypto::ring::default_provider()
            .install_default()
            .expect("Failed to install rustls crypto provider");

        println!("QUIC listening on {}", quic_bind_addr.clone());

        let config = portredirect::quic::QuicConfig::create_default_config(config_dir, quic_bind_addr);

        // Spawn the QUIC server
        tokio::spawn(async {
            if let Err(e) = setup_quic(config).await {
                eprintln!("QUIC setup error: {:?}", e);
            }
        });
    }

    // Accept incoming connections.
    loop {
        let (local_socket, _) = match listener.accept().await {
            Ok(listener) => listener,
            Err(e) => {
                eprintln!("Failed to accept connection: {}", e);
                continue;
            }
        };
        println!("New TCP connection from: {:?}", local_socket.peer_addr());

        if args.mode == Mode::DirectForwarding {
            let stats = stats.clone();
            let remote_addr = remote_addr.clone();

            tokio::spawn(async move {
                // Handle the connection.
                if let Err(e) =
                    handle_tcp_connection_redirect(local_socket, remote_addr, stats).await
                {
                    eprintln!("Connection error: {}", e);
                }
            });
        } else if args.mode == Mode::Quic {
            /*
            let quic_connection = quic_connection.clone();

            tokio::spawn(async move {
                if let Err(e) = handle_tcp_to_quic(tcp_stream, quic_connection).await {
                    eprintln!("Error handling TCP connection: {:?}", e);
                }
            })*/
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
    // Increment connection count.
    {
        let mut stats = stats.lock().unwrap();
        stats.connection_count += 1;
    }

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

    // Decrement connection count.
    {
        let mut stats = stats.lock().unwrap();
        stats.connection_count -= 1;
    }

    Ok(())
}

async fn handle_tcp_to_quic(
    mut tcp_stream: tokio::net::TcpStream,
    quic_connection: Connection,
) -> Result<(), Box<dyn std::error::Error>> {
    // Open a new QUIC stream
    let (mut quic_send, mut quic_recv) = quic_connection.open_bi().await?;
    println!("Opened QUIC stream for TCP forwarding");

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
    println!("Closed QUIC stream for TCP connection");
    Ok(())
}
