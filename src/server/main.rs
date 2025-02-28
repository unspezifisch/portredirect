// PortRedirect Server
//
// License: GPL-3.0-only

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use portredirect::app_data::ServerAppData;
use portredirect::get_config_dir;
use portredirect::quic::server::{run_quic_server, ServerConfig};
use portredirect::server::client_handler::handle_quic_client_connection;
use secrecy::SecretString;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::time::{sleep, Duration};
use tracing::{error, info, instrument, span, Level};

/// Command-line arguments for the server side.
#[derive(Parser, Debug)]
struct Args {
    /// Full path to configuration directory.
    #[clap(long)]
    config_dir: Option<String>,

    /// TCP listener host for external connections.
    #[clap(long)]
    local_host: String,

    /// TCP listener port for external connections.
    #[clap(long)]
    local_port: u16,

    /// Allow clients to create additional TCP listeners for external connections.
    #[clap(long)]
    clients_additional_listeners: bool,

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

    // Install the default crypto provider for rustls.
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    // Parse command-line arguments.
    let args = Args::parse();

    // Retrieve (or create) the configuration directory.
    let config_dir =
        get_config_dir(args.config_dir).context("Failed to get configuration directory")?;
    info!("Configuration directory: {:?}", config_dir);

    // Parse QUIC server listener address.
    let local_addr = resolve_socket_addr(&format!("{}:{}", args.local_host, args.local_port));
    let quic_addr = resolve_socket_addr(&format!(
        "{}:{}",
        args.quic_server_host, args.quic_server_port
    ))
    .context("Failed to resolve QUIC bind address")?;

    // Set up QUIC server configuration.
    let app_data = Arc::new(ServerAppData::new(args.quic_psk, local_addr));
    info!("QUIC will listen on {}", quic_addr);

    let quic_config = ServerConfig::create_default_config(
        config_dir,
        args.quic_cert_hostname,
        quic_addr,
        None,
        app_data.clone(),
    );

    // Start QUIC server.
    run_quic_server(quic_config, handle_quic_client_connection)
        .await
        .with_context(|| "PortRedirect Server Error")?;

    info!("PortRedirect Server exited cleanly");
    Ok(())
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
