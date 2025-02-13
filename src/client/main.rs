// PortRedirector-RS Client - Main binary
//
// License: GPL-3.0-only

use anyhow::{Context, Result};
use clap::Parser;
use portredirect::app_data::ClientAppData;
use portredirect::client::metrics::start_metrics_server;
use portredirect::client::server_handler::handle_quic_server_connection;
use portredirect::get_config_dir;
use portredirect::quic::client::{run_quic_client, ClientConfig};
use secrecy::SecretString;
use std::net::ToSocketAddrs;
use std::sync::{Arc, Mutex};
use tracing::{info, span, Level};

/// Command-line arguments for the port redirector tool.
#[derive(Parser)]
struct Args {
    /// Destination host for data coming from QUIC connections.
    #[clap(long)]
    destination_host: String,

    /// Destination port (currently TCP only).
    #[clap(long)]
    destination_port: u16,

    /// QUIC connection remote host (server).
    #[clap(long)]
    quic_remote_host: String,

    /// QUIC connection remote port (server).
    #[clap(long)]
    quic_remote_port: u16,

    /// QUIC connection local host to bind to (client).
    #[clap(long, default_value = "0.0.0.0")]
    quic_local_host: String,

    /// QUIC connection local port to bind to (client).
    #[clap(long, default_value = "0")]
    quic_local_port: u16,

    /// Prometheus metrics host.
    #[clap(long)]
    provide_metrics: bool,

    /// QUIC remote hostname override for Subject Alt Name match in TLS cert.
    #[clap(long)]
    quic_remote_hostname_match: Option<String>,

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

    let root_span = span!(Level::INFO, "prclient_main");
    let _enter = root_span.enter();

    // Get or create config directory.
    let config_dir = get_config_dir()?;
    info!("Configuration directory: {:?}", config_dir);

    // Parse args.
    let args = Args::parse();
    // Target of "tunneled" TCP connections.
    let destination_addr = format!("{}:{}", args.destination_host, args.destination_port);
    // Local UDP bind address.
    let quic_local_addr = format!("{}:{}", args.quic_local_host, args.quic_local_port)
        .to_socket_addrs()
        .context("constructing QUIC local address")
        .expect("Invalid host or port")
        .next()
        .expect("Unable to resolve address");
    // Remote UDP server address.
    let quic_remote_addr = format!("{}:{}", args.quic_remote_host, args.quic_remote_port)
        .to_socket_addrs()
        .context("constructing QUIC remote address")
        .expect("Invalid host or port")
        .next()
        .expect("Unable to resolve address");

    info!(destination=%destination_addr, local=%quic_local_addr, remote=%quic_remote_addr, "Initializing QUIC Client");

    // Shared state for connection statistics.
    let stats = Arc::new(Mutex::new(ConnectionStats {
        connection_count: 0,
    }));

    // Spawn a task to periodically print stats.
    {
        let stats_clone = Arc::clone(&stats);
        tokio::spawn(async move {
            use tokio::time::{sleep, Duration};
            let mut printed_once = false;
            let mut previous_connection_count = 0;

            loop {
                sleep(Duration::from_secs(1)).await;
                let stats = stats_clone.lock().unwrap();
                if previous_connection_count != stats.connection_count || !printed_once {
                    info!("Active connections: {}", stats.connection_count);
                    previous_connection_count = stats.connection_count;
                    printed_once = true;
                }
            }
        });
    }

    // Start the metrics server.
    if args.provide_metrics {
        tokio::spawn(async { start_metrics_server(([0, 0, 0, 0], 9898)).await });
    }

    // Create QUIC client.
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    info!("QUIC connecting to {}", quic_remote_addr.clone());

    let forward_destination = destination_addr
        .to_socket_addrs()
        .context("resolving destination address")?
        .next()
        .context("resolving destination address")?;
    let app_config = ClientAppData::new(args.quic_psk, forward_destination);

    let quic_client_config = ClientConfig::create_default_config(
        config_dir,
        quic_local_addr,
        quic_remote_addr,
        args.quic_remote_hostname_match,
        None,
        app_config,
    );

    // Spawn the QUIC client
    run_quic_client(quic_client_config, handle_quic_server_connection)
        .await
        .context("QUIC client thread")?;

    Ok(())
}
