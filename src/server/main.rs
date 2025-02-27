// PortRedirect Server
//
// License: GPL-3.0-only

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use portredirect::app_data::ServerAppData;
use portredirect::quic::server::{run_quic_server, ServerConfig};
use portredirect::server::client_handler::handle_quic_client_connection;
use portredirect::get_config_dir;
use portredirect::server::tcp_listener::handle_tcp_listener;
use secrecy::SecretString;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::net::TcpListener;
use tokio::time::{sleep, Duration};
use tracing::{error, info, instrument, span, Level};

/// Command-line arguments for the port redirector tool.
#[derive(Parser, Debug)]
struct Args {
    /// Local host to bind the TCP listener.
    #[clap(long)]
    local_host: String,

    /// Local port to bind the TCP listener.
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
    #[clap(long, default_value = "127.0.0.1")]
    quic_cert_hostname: String,

    /// Pre-shared key for authentication over QUIC.
    #[clap(long)]
    quic_psk: SecretString,
}

/// Program entry point.
#[tokio::main]
async fn main() -> Result<()> {
    setup_tracing();

    // Create a root span for logging.
    let _root_span = span!(Level::INFO, "prserver_main").entered();

    // Retrieve (or create) the configuration directory.
    let config_dir = get_config_dir().context("Failed to get configuration directory")?;
    info!("Configuration directory: {:?}", config_dir);

    // Parse command-line arguments.
    let args = Args::parse();
    let local_addr = format!("{}:{}", args.local_host, args.local_port);
    let quic_addr = resolve_socket_addr(&format!(
        "{}:{}",
        args.quic_server_host, args.quic_server_port
    ))
    .context("Failed to resolve QUIC bind address")?;

    // Use an atomic counter for connection statistics.
    let active_connections = Arc::new(AtomicUsize::new(0));
    tokio::spawn(report_active_connections(active_connections.clone()));

    // Create the TCP listener.
    let listener = TcpListener::bind(&local_addr)
        .await
        .with_context(|| format!("Failed to bind TCP listener to {}", local_addr))?;
    info!("TCP listening on {}", listener.local_addr()?);

    // Set up QUIC server configuration.
    let app_data = Arc::new(ServerAppData::new(args.quic_psk));
    // Install the default crypto provider for rustls.
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");
    info!("QUIC will listen on {}", quic_addr);

    let quic_config = ServerConfig::create_default_config(
        config_dir,
        args.quic_cert_hostname,
        quic_addr,
        None,
        app_data.clone(),
    );

    // Spawn the QUIC server task.
    tokio::spawn(run_quic_server_task(quic_config));

    // Start accepting and handling TCP connections.
    handle_tcp_listener(listener, app_data, active_connections).await
}

/// Sets up tracing for logging.
fn setup_tracing() {
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .with_target(true)
        .with_line_number(true)
        .init();
}

/// Resolves a socket address from a string.
fn resolve_socket_addr(addr: &str) -> Result<SocketAddr> {
    addr.to_socket_addrs()?
        .next()
        .ok_or_else(|| anyhow!("Unable to resolve address: {}", addr))
}

/// Periodically reports the number of active connections.
async fn report_active_connections(active_connections: Arc<AtomicUsize>) {
    let mut previous = 0;
    loop {
        sleep(Duration::from_millis(100)).await;
        let current = active_connections.load(Ordering::Relaxed);
        if current != previous {
            info!("Active connections: {}", current);
            previous = current;
        }
    }
}

/// Runs the QUIC server in its own asynchronous task.
#[instrument(skip(quic_config))]
async fn run_quic_server_task(quic_config: ServerConfig<Arc<ServerAppData>>) {
    if let Err(e) = run_quic_server(quic_config, handle_quic_client_connection).await {
        error!(error = %e, "QUIC server encountered an error");
    }
}
