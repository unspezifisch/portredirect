// PortRedirector-RS Common Client Code
//
// License: GPL-3.0-only
// Based on: Quinn example code (originally licensed under Apache-2.0/MIT)
// Original: https://github.com/quinn-rs/quinn/blob/204b14792b5e92eb2c43cdb1ff05426412ff4466/quinn/examples/client.rs

use anyhow::{anyhow, Error, Result};
use quinn::crypto::rustls::QuicClientConfig;
use rustls::pki_types::CertificateDer;
use secrecy::SecretString;
use std::{fs, io, net::SocketAddr, path::PathBuf, sync::Arc, time::Instant};
use tracing::{debug, error, info, instrument, warn};

use super::ALPN_QUIC_PORTREDIRECT;

#[derive(Debug)]
#[allow(unused)]
pub struct ClientConfig {
    pub remote_hostname_match: Option<String>,
    pub ca_path: Option<PathBuf>,
    pub cert_file: PathBuf,

    pub local_socket: SocketAddr,
    pub remote_socket: SocketAddr,
    pub connection_limit: Option<usize>,

    pub pr_psk: SecretString,
}

impl ClientConfig {
    #[allow(unused)]
    pub fn create_default_config(
        config_dir: PathBuf,
        local_socket: SocketAddr,
        remote_socket: SocketAddr,
        remote_hostname_match: Option<String>,
        psk: SecretString,
    ) -> Self {
        ClientConfig {
            remote_hostname_match,
            ca_path: None,
            cert_file: config_dir.join("cert.der"),
            local_socket,
            remote_socket,
            connection_limit: None,
            pr_psk: psk,
        }
    }
}

#[instrument(skip(config, handle_incoming))]
pub async fn run_quic_client<F, Fut>(config: ClientConfig, handle_incoming: F) -> Result<(), Error>
where
    F: Fn(Arc<ClientConfig>, quinn::Connection) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<(), Error>> + Send + 'static,
{
    info!("Starting PR QUIC client setup");

    // Load CA chain, or if none is given, load cert file.
    let mut roots = rustls::RootCertStore::empty();
    if let Some(ca_path) = &config.ca_path {
        roots.add(CertificateDer::from(fs::read(&ca_path)?))?;
    } else {
        let cert_file_result = fs::read(&config.cert_file);

        match cert_file_result {
            Ok(cert) => {
                roots.add(CertificateDer::from(cert))?;
            }
            Err(ref e) if e.kind() == io::ErrorKind::NotFound => {
                info!("local server certificate not found");
            }
            Err(e) => {
                error!("failed to open local server certificate: {}", e);
            }
        }
    }

    // Crypto setup.
    let mut client_crypto = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    client_crypto.alpn_protocols = ALPN_QUIC_PORTREDIRECT.iter().map(|&x| x.into()).collect();

    let server_name_match = config
        .remote_hostname_match
        .clone()
        .unwrap_or_else(|| config.remote_socket.ip().to_string());

    // QUIC client setup.
    let client_config =
        quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(client_crypto)?));
    let mut endpoint = quinn::Endpoint::client(config.local_socket)?;
    endpoint.set_default_client_config(client_config);

    // Connect, or rather: establish tunnel.
    let start = Instant::now();
    info!(
        server_name_match,
        local = config.local_socket.to_string(),
        remote = config.remote_socket.to_string(),
        "Connecting to PR QUIC Server"
    );
    let conn = endpoint
        .connect(config.remote_socket, server_name_match.as_str())?
        .await
        .map_err(|e| anyhow!("failed to connect: {}", e))?;
    debug!("QUIC connected at {:?}", start.elapsed());

    // Open AUTH channel. It's where we prove to the server that we know the PSK and thus are to be trusted.
    // We already know we can trust the server because its TLS cert is signed by our CA.
    if false {
        let (mut send, mut recv) = conn
            .open_bi()
            .await
            .map_err(|e| anyhow!("failed to open stream: {}", e))?;

        // TODO do we need this? also it's not auth-specific.
        let rebind = false;
        if rebind {
            let socket = std::net::UdpSocket::bind("[::]:0")?;
            let addr = socket.local_addr()?;
            info!("rebinding to {addr}");
            endpoint.rebind(socket).expect("rebind failed");
        }

        // Auth request.
        let request = b"AUTH ME\n";
        send.write_all(request)
            .await
            .map_err(|e| anyhow!("failed to send request: {}", e))?;

        // HACK no auth checks at all
        warn!("TODO auth"); // TODO actually auth

        let response_start = Instant::now();
        debug!("AUTH request sent at {:?}", response_start - start);
        let resp = recv
            .read_to_end(64)
            .await
            .map_err(|e| anyhow!("failed to read response: {}", e))?;
        let duration = response_start.elapsed();
        debug!("AUTH response received in {:?}", duration);

        debug!("client data: {:?}", resp);
        if resp != b"AUTH OK\n" {
            return Err(anyhow!("server didn't send AUTH OK but {:?}", resp));
        }

        debug!("PR QUIC server reports client auth OK");
    }

    // PR QUIC client side loop:
    // Handle incoming streams forever.
    let config = Arc::from(config);
    info!("PR QUIC connection established in {:?}.", start.elapsed());
    while let Some(conn) = endpoint.accept().await {
        if config
            .connection_limit
            .is_some_and(|n| endpoint.open_connections() >= n)
        {
            warn!(
                "Refusing connection: open connection limit ({}) reached",
                config.connection_limit.unwrap()
            );
            conn.refuse();
        } else {
            let peer_info = format!(
                "server: {} (validated: {})",
                conn.remote_address(),
                conn.remote_address_validated()
            );
            debug!(peer = %peer_info, "Accepting new QUIC client connection at {:?}", start.elapsed());

            let connection = conn.await?;
            let fut = handle_incoming(Arc::clone(&config), connection);
            tokio::spawn(async move {
                if let Err(e) = fut.await {
                    error!("connection failed: {reason}", reason = e.to_string())
                }
            });
        }
    }

    info!("PR QUIC connection terminated after {:?}.", start.elapsed());

    Ok(())
}
