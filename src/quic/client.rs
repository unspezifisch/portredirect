// PortRedirector-RS Common Client Code
//
// License: GPL-3.0-only
// Based on: Quinn example code (originally licensed under Apache-2.0/MIT)
// Original: https://github.com/quinn-rs/quinn/blob/204b14792b5e92eb2c43cdb1ff05426412ff4466/quinn/examples/client.rs

// TODO import cleanup
use anyhow::{Error, Result};
use std::{net::SocketAddr, path::PathBuf};


#[derive(Debug)]
#[allow(unused)]
pub struct ClientConfig {
    pub cert_hostname_match: Option<String>,
    pub cert_file: PathBuf,

    pub listen: SocketAddr,
}

impl ClientConfig {
    #[allow(unused)]
    pub fn create_default_config(
        config_dir: PathBuf,
        cert_hostname_match: Option<String>,
        bind_socket: SocketAddr,
    ) -> Self {
        ClientConfig {
            cert_hostname_match,
            cert_file: config_dir.join("cert.der"),
            listen: bind_socket,
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
    /* HACK
    let mut roots = rustls::RootCertStore::empty();
    if let Some(ca_path) = options.ca {
        roots.add(CertificateDer::from(fs::read(ca_path)?))?;
    } else {
        let dirs = directories_next::ProjectDirs::from("org", "quinn", "quinn-examples").unwrap();
        match fs::read(dirs.data_local_dir().join("cert.der")) {
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
    let mut client_crypto = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();

    client_crypto.alpn_protocols = common::ALPN_QUIC_HTTP.iter().map(|&x| x.into()).collect();
    if options.keylog {
        client_crypto.key_log = Arc::new(rustls::KeyLogFile::new());
    }

    let client_config =
        quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(client_crypto)?));
    let mut endpoint = quinn::Endpoint::client(options.bind)?;
    endpoint.set_default_client_config(client_config);
     */

    Ok(())
}
