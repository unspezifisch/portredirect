// PortRedirector-RS Server
//
// License: GPL-3.0-only

use anyhow::{anyhow, Result};
use clap::Parser;
use portredirect::app_data::ServerAppData;
use portredirect::quic::server::{run_quic_server, ServerConfig};
use portredirect::server::client_handler::handle_quic_client_connection;
use portredirect::server::tcp::handle_tcp_to_quic_stream;
use portredirect::server::utils::QuinnWorkerBundle;
use portredirect::get_config_dir;
use secrecy::SecretString;
use std::net::ToSocketAddrs;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::io;
use tokio::net::TcpListener;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, span, Level};

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
    #[clap(long, default_value = "4433")]
    quic_server_port: u16,

    /// QUIC server certificate Subject Alt Name.
    #[clap(long, default_value = "localhost")]
    quic_cert_hostname: String,

    /// Pre-shared key for authentication over QUIC.
    #[clap(long)]
    quic_psk: SecretString,
}

/// Holds connection statistics.
struct ConnectionStats {
    connection_count: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize the tracing subscriber.
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .with_target(true)
        .with_line_number(true)
        .init();

    let _root_span = span!(Level::INFO, "prserver_main").entered();

    // Get or create the configuration directory.
    let config_dir = get_config_dir()?;
    info!("Configuration directory: {:?}", config_dir);

    // Parse command-line arguments.
    let args = Args::parse();
    let local_addr = format!("{}:{}", args.local_host, args.local_port);
    let quic_bind_addr = format!("{}:{}", args.quic_server_host, args.quic_server_port)
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| anyhow!("Unable to resolve QUIC server address"))?;

    // Shared state for connection statistics.
    let stats = Arc::new(Mutex::new(ConnectionStats { connection_count: 0 }));

    // Spawn a task to periodically log connection stats.
    let stats_clone = Arc::clone(&stats);
    tokio::spawn(async move {
        let mut printed_once = false;
        let mut previous_connection_count = 0;
        loop {
            sleep(Duration::from_millis(100)).await;
            let current_count = stats_clone.lock().unwrap().connection_count;
            if current_count != previous_connection_count || !printed_once {
                info!("Active connections: {}", current_count);
                previous_connection_count = current_count;
                printed_once = true;
            }
        }
    });

    // Create the TCP listener.
    let listener = TcpListener::bind(&local_addr).await.map_err(|e| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("Failed to bind to {}: {}", local_addr, e),
        )
    })?;
    info!("TCP listening on {}", listener.local_addr()?);

    // Prepare the QUIC server configuration.
    let app_config = Arc::new(ServerAppData::new(args.quic_psk));
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");
    info!("QUIC listening on {}", quic_bind_addr);

    let quic_config = ServerConfig::create_default_config(
        config_dir,
        args.quic_cert_hostname,
        quic_bind_addr,
        None,
        Arc::clone(&app_config),
    );

    // Spawn the QUIC server.
    tokio::spawn(async move {
        if let Err(e) = run_quic_server(quic_config, handle_quic_client_connection).await {
            error!(error = %e, "QUIC server encountered an error");
        }
    });

    // Handle incoming TCP connections indefinitely.
    loop {
        let (local_socket, _) = match listener.accept().await {
            Ok(conn) => conn,
            Err(e) => {
                error!("Failed to accept TCP connection: {}", e);
                continue;
            }
        };
        debug!(
            "New TCP connection from: {:?}",
            local_socket.peer_addr().ok()
        );

        // Try to obtain a QUIC connection from the shared configuration.
        let quinn_conn = { app_config.connection.lock().unwrap().clone() };
        let quinn_conn = match quinn_conn {
            Some(conn) => conn,
            None => {
                error!("No QUIC connection available to handle TCP connection");
                continue;
            }
        };

        // Open a bidirectional QUIC stream.
        let (quic_send, quic_recv) = match quinn_conn.open_bi().await {
            Ok(stream) => stream,
            Err(e) => {
                error!("Failed to open QUIC stream: {}", e);
                continue;
            }
        };
        let stream_id = quic_send.id();
        debug!(
            "Opened bidirectional QUIC stream (ID: {}) for TCP forwarding",
            stream_id
        );

        let worker_bundle = QuinnWorkerBundle { quic_recv, quic_send };
        let stats_clone = Arc::clone(&stats);

        // Spawn a task to bridge data between the TCP connection and the QUIC stream.
        tokio::spawn(async move {
            {
                // Increment connection count.
                let mut stats = stats_clone.lock().unwrap();
                stats.connection_count += 1;
            }

            let start = Instant::now();
            if let Err(e) = handle_tcp_to_quic_stream(local_socket, worker_bundle).await {
                error!("Error in QUIC/TCP stream handling: {:?}", e);
            }
            debug!(
                "Closed QUIC/TCP stream (ID: {}) after {:?}",
                stream_id,
                start.elapsed()
            );

            {
                // Decrement connection count.
                let mut stats = stats_clone.lock().unwrap();
                stats.connection_count -= 1;
            }
        });
    }
}
