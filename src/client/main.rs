// PortRedirector-RS Client
//
// License: GPL-3.0-only

// TODO import cleanup 2
use anyhow::{Context, Error, Result};
use clap::Parser;
use portredirect::get_config_dir;
use portredirect::quic::client::{run_quic_client, ClientConfig};
use std::net::ToSocketAddrs;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, error, info, instrument, span, warn, Level};

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

    /// QUIC remote hostname override for Subject Alt Name match in TLS cert.
    #[clap(long)]
    quic_remote_hostname_match: Option<String>,

    /// Pre-shared key for authentication over QUIC.
    #[clap(long)]
    quic_psk: Option<String>,
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

    // Create QUIC client.
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    info!("QUIC connecting to {}", quic_remote_addr.clone());

    let config = ClientConfig::create_default_config(
        config_dir,
        quic_local_addr,
        quic_remote_addr,
        args.quic_remote_hostname_match,
    );

    // Spawn the QUIC client
    if let Err(e) = run_quic_client(config, handle_quic_to_tcp).await {
        error!(error = %e, "QUIC client thread error");
    }

    Ok(())
}

// Handles incoming QUIC streams, forwards them to their destination.
#[allow(unused)]
#[instrument[skip(conn)]]
async fn handle_quic_to_tcp(mut conn: quinn::Incoming) -> Result<(), Error> {
    // Open a new QUIC stream
    debug!("Accepting server-initiated QUIC stream.");
    let stream = conn.accept();
    let stream = match stream {
        Err(quinn::ConnectionError::ApplicationClosed { .. }) => {
            warn!("stream closed");
            return Ok(());
        }
        Err(e) => {
            error!(error = %e, "stream error");
            return Err(anyhow::Error::from(e));
        }
        Ok(s) => s,
    };

    loop {
        debug!("handle_quic_to_tcp");
        sleep(Duration::from_secs(10)).await;
    }
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
