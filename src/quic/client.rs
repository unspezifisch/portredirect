// PortRedirector-RS Common Client Code
//
// License: GPL-3.0-only
// Based on: Quinn example code (originally licensed under Apache-2.0/MIT)
// Original: https://github.com/quinn-rs/quinn/blob/204b14792b5e92eb2c43cdb1ff05426412ff4466/quinn/examples/client.rs

use anyhow::{anyhow, Error, Result};
use quinn::crypto::rustls::QuicClientConfig;
use rustls::pki_types::CertificateDer;
use std::{
    fs,
    io::{self, Write},
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tracing::{debug, error, info};

use super::ALPN_QUIC_PORTREDIRECT;

#[derive(Debug)]
#[allow(unused)]
pub struct ClientConfig {
    pub remote_hostname_match: Option<String>,
    pub ca_path: Option<PathBuf>,
    pub cert_file: PathBuf,

    pub local_socket: SocketAddr,
    pub remote_socket: SocketAddr,
}

impl ClientConfig {
    #[allow(unused)]
    pub fn create_default_config(
        config_dir: PathBuf,
        local_socket: SocketAddr,
        remote_socket: SocketAddr,
        remote_hostname_match: Option<String>,
    ) -> Self {
        ClientConfig {
            remote_hostname_match,
            ca_path: None,
            cert_file: config_dir.join("cert.der"),
            local_socket,
            remote_socket,
        }
    }
}

pub async fn run_quic_client<F, Fut>(
    config: ClientConfig,
    callback: F,
) -> Result<(), Box<dyn std::error::Error>>
where
    F: Fn(tokio::net::TcpStream, quinn::Connection) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<(), Error>> + Send,
{
    // Load CA chain, or if none is given, load cert file.
    let mut roots = rustls::RootCertStore::empty();
    if let Some(ca_path) = config.ca_path {
        roots.add(CertificateDer::from(fs::read(ca_path)?))?;
    } else {
        match fs::read(config.cert_file) {
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

    // QUIC client setup.
    let client_config =
        quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(client_crypto)?));
    let mut endpoint = quinn::Endpoint::client(config.local_socket)?;
    endpoint.set_default_client_config(client_config);

    // Initial request.
    let request = format!("HELLO I AM PRCLIENT\r\n");

    // Connect.
    let start = Instant::now();
    info!(cert_hostname_match=config.remote_hostname_match, remote=config.remote_socket.to_string(), "Connecting to PR QUIC Server");
    let conn = endpoint
        .connect(config.remote_socket, config.remote_hostname_match.unwrap().as_str())?
        .await
        .map_err(|e| anyhow!("failed to connect: {}", e))?;
    debug!("connected at {:?}", start.elapsed());
    let (mut send, mut recv) = conn
        .open_bi()
        .await
        .map_err(|e| anyhow!("failed to open stream: {}", e))?;

    // TODO do we need this?
    let rebind = false;
    if rebind {
        let socket = std::net::UdpSocket::bind("[::]:0").unwrap();
        let addr = socket.local_addr().unwrap();
        info!("rebinding to {addr}");
        endpoint.rebind(socket).expect("rebind failed");
    }

    send.write_all(request.as_bytes())
        .await
        .map_err(|e| anyhow!("failed to send request: {}", e))?;
    send.finish().unwrap();

    let response_start = Instant::now();
    eprintln!("request sent at {:?}", response_start - start);
    let resp = recv
        .read_to_end(usize::MAX)
        .await
        .map_err(|e| anyhow!("failed to read response: {}", e))?;
    let duration = response_start.elapsed();
    eprintln!(
        "response received in {:?} - {} KiB/s",
        duration,
        resp.len() as f32 / (duration_secs(&duration) * 1024.0)
    );
    io::stdout().write_all(&resp).unwrap();
    io::stdout().flush().unwrap();
    conn.close(0u32.into(), b"done");

    // Give the server a fair chance to receive the close packet
    endpoint.wait_idle().await;

    // TODO callback etc.

    Ok(())
}

fn duration_secs(x: &Duration) -> f32 {
    x.as_secs() as f32 + x.subsec_nanos() as f32 * 1e-9
}
