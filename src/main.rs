use clap::Parser;
use std::sync::{Arc, Mutex};
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

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
}

/// Data structure to hold connection statistics.
struct ConnectionStats {
    connection_count: usize,
    total_bytes: u64,
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let args = Args::parse();
    let local_addr = format!("{}:{}", args.local_host, args.local_port);
    let remote_addr = format!("{}:{}", args.remote_host, args.remote_port);

    let listener = TcpListener::bind(local_addr).await?;
    println!("Listening on {}", listener.local_addr()?);

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

    // Accept incoming connections.
    loop {
        let (local_socket, _) = match listener.accept().await {
            Ok(listener) => listener,
            Err(e) => {
                eprintln!("Failed to accept connection: {}", e);
                continue;
            }
        };
        let remote_addr = remote_addr.clone();
        let stats = stats.clone();

        tokio::spawn(async move {
            // Increment connection count.
            {
                let mut stats = stats.lock().unwrap();
                stats.connection_count += 1;
            }

            // Handle the connection.
            if let Err(e) = handle_connection(local_socket, remote_addr, stats.clone()).await {
                eprintln!("Connection error: {}", e);
            }

            // Decrement connection count.
            {
                let mut stats = stats.lock().unwrap();
                stats.connection_count -= 1;
            }
        });
    }
}

/// Handles a single connection by forwarding traffic between the local and remote sockets.
///
/// # Arguments
/// * `local_socket` - The accepted local socket.
/// * `remote_addr` - The address of the remote server.
/// * `stats` - Shared statistics for connection tracking.
async fn handle_connection(
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

    // Forward data from remote to local.
    let remote_to_local_task = tokio::spawn(async move {
        let mut buffer = [0u8; 1024];
        while let Ok(bytes_read) = remote_read.read(&mut buffer).await {
            if bytes_read == 0 {
                break;
            }
            local_write.write_all(&buffer[..bytes_read]).await?;

            let mut stats = stats.lock().unwrap();
            stats.total_bytes += bytes_read as u64;
        }
        Ok::<(), io::Error>(())
    });

    // Wait for both tasks to complete.
    let _ = tokio::try_join!(local_to_remote_task, remote_to_local_task)?;

    Ok(())
}
