use clap::Parser;
use portredirect::quic::setup_quic;
use quinn::Connection;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

mod quic;

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
    remote_host: String,

    /// Remote port to forward traffic to.
    #[clap(long)]
    remote_port: u16,

    /// Pre-shared key for authentication over QUIC.
    #[clap(long)]
    psk: Option<String>,
}

/// Data structure to hold connection statistics.
struct ConnectionStats {
    connection_count: usize,
    total_bytes: u64,
}

fn get_config_dir() -> PathBuf {
    let mut config_dir = dirs::config_dir().expect("Failed to find config directory");
    config_dir.push("portredirect");
    config_dir
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let config_dir = get_config_dir();

    let args = Args::parse();
    let local_addr = format!("{}:{}", args.local_host, args.local_port);
    let remote_addr = format!("{}:{}", args.remote_host, args.remote_port);
    let do_tcp_redirect = args.psk.is_none();
    let do_quic = !do_tcp_redirect;

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
    let listener = TcpListener::bind(local_addr).await?;
    println!("Listening on {}", listener.local_addr()?);

    // Create QUIC server if needed.
    if do_quic {
        use std::net::SocketAddr;

        let cert_file = config_dir.join("cert.pem");
        let key_file = config_dir.join("key.pem");

        let config = portredirect::quic::QuicConfig {
            cert_hostname: String::from("example.com"),
            cert_file,
            key_file,
            listen: "0.0.0.0:44333".parse::<SocketAddr>().unwrap(),
            stateless_retry: false,
            connection_limit: None,
        };

        println!("{:?}", config);

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

        if do_tcp_redirect {
            let remote_addr = remote_addr.clone();
            let stats = stats.clone();
            println!("New TCP connection from: {}", remote_addr);

            tokio::spawn(async move {
                // Handle the connection.
                if let Err(e) =
                    handle_tcp_connection_redirect(local_socket, remote_addr, stats.clone()).await
                {
                    eprintln!("Connection error: {}", e);
                }
            });
        } else if do_quic {
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
