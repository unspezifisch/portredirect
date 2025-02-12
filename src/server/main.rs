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
use tokio::io::{self};
use tokio::net::TcpListener;
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
    let local_addr = format!("{}:{}", args.local_host, args.local_port);
    let quic_bind_addr = format!("{}:{}", args.quic_server_host, args.quic_server_port)
        .to_socket_addrs()
        .expect("Invalid host or port")
        .next()
        .expect("Unable to resolve address");

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
    let quic_psk = args.quic_psk;
    let app_config = Arc::new(ServerAppData::new(quic_psk));

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    info!("QUIC listening on {}", quic_bind_addr.clone());

    let config = ServerConfig::create_default_config(
        config_dir,
        args.quic_cert_hostname,
        quic_bind_addr,
        None,
        Arc::clone(&app_config),
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
            let quinn_conn = app_config.connection.lock().unwrap();
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

        let worker_bundle = QuinnWorkerBundle {
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
