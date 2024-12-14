// PortRedirector-RS Common Server Code
//
// License: GPL-3.0-only
// Based on: Quinn example code (originally licensed under Apache-2.0/MIT)
// Original: https://github.com/quinn-rs/quinn/blob/204b14792b5e92eb2c43cdb1ff05426412ff4466/quinn/examples/server.rs

use anyhow::{anyhow, bail, Context, Result};
use quinn::crypto::rustls::QuicServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use std::{ascii, fs, io, net::SocketAddr, path::PathBuf, str, sync::Arc};
use tracing::{debug, error, info, instrument, warn, Span};

use crate::{get_config_dir, quic::ALPN_QUIC_PORTREDIRECT};

#[derive(Debug)]
#[allow(unused)]
pub struct ServerConfig {
    pub cert_hostname: String,
    pub cert_file: PathBuf,
    pub key_file: PathBuf,

    pub listen: SocketAddr,
    pub stateless_retry: bool,
    pub connection_limit: Option<usize>,
}

impl ServerConfig {
    #[allow(unused)]
    pub fn create_default_config(
        config_dir: PathBuf,
        cert_alt_name: String,
        bind_socket: SocketAddr,
    ) -> Self {
        ServerConfig {
            cert_hostname: cert_alt_name,
            cert_file: config_dir.join("cert.der"),
            key_file: config_dir.join("key.der"),
            listen: bind_socket,
            stateless_retry: false,
            connection_limit: None,
        }
    }
}

/// Attempts to load a QUIC-compatible certificate and private key from the specified file paths.
///
/// This function reads a private key and certificate chain from the provided file paths
/// and attempts to parse them into the required QUIC-compatible formats. It supports
/// both DER-encoded and PEM-encoded files.
///
/// # Arguments
///
/// * `key_path` - A `PathBuf` specifying the location of the private key file.
/// * `cert_path` - A `PathBuf` specifying the location of the certificate chain file.
///
/// # Returns
///
/// Returns a `Result` containing a tuple:
/// * `Vec<CertificateDer<'static>>` - The parsed certificate chain as DER-encoded certificates.
/// * `PrivateKeyDer<'static>` - The parsed private key in a QUIC-compatible format.
///
/// On success, the tuple contains the certificate chain and private key. On failure,
/// it returns an `anyhow::Error` describing the issue encountered during file
/// reading or parsing.
///
/// # Examples
///
/// ```rust
/// use std::path::PathBuf;
/// use anyhow::Result;
/// use portredirect::quic::try_load_quic_cert;
///
/// fn main() -> Result<()> {
///     let key_path = PathBuf::from("TEST-key-NONEXISTENT.pem");
///     let cert_path = PathBuf::from("TEST-cert-NONEXISTENT.pem");
///
///     let result = try_load_quic_cert(key_path, cert_path);
///
///     // Assert that an error is returned
///     assert!(result.is_err(), "Expected an error for nonexistent files");
///
///     Ok(())
/// }
/// ```
///
/// Note: Ensure that the file paths provided are accessible and have the correct permissions.
#[allow(unused)]
#[instrument()]
pub fn load_or_generate_quic_cert(
    cert_alt_name: String,
    key_path: PathBuf,
    cert_path: PathBuf,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    if key_path.exists() && cert_path.exists() {
        load_quic_cert(key_path, cert_path)
    } else {
        generate_quic_cert(cert_alt_name, key_path, cert_path)
    }
}

#[allow(unused)]
#[instrument()]
pub fn load_quic_cert(
    key_path: PathBuf,
    cert_path: PathBuf,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    // Try loading
    let key = fs::read(key_path.clone()).context("failed to read private key")?;
    let key = if key_path.extension().is_some_and(|x| x == "der") {
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key))
    } else {
        rustls_pemfile::private_key(&mut &*key)
            .context("malformed PKCS #1 private key")?
            .ok_or_else(|| anyhow::Error::msg("no private keys found"))?
    };
    let cert_chain = fs::read(cert_path.clone()).context("failed to read certificate chain")?;
    let cert_chain = if cert_path.extension().is_some_and(|x| x == "der") {
        vec![CertificateDer::from(cert_chain)]
    } else {
        rustls_pemfile::certs(&mut &*cert_chain)
            .collect::<Result<_, _>>()
            .context("invalid PEM-encoded certificate")?
    };

    Ok((cert_chain, key))
}

/// Generates a self-signed certificate and private key, or loads existing ones if they exist.
///
/// This function checks for the presence of the certificate and private key at the specified paths.
/// If either is missing, it generates a self-signed certificate using the provided alternative
/// name for the certificate (e.g., a domain name or IP address). The generated files are saved
/// to the specified paths. The function then loads the certificate and private key into
/// QUIC-compatible formats.
///
/// # Arguments
///
/// * `cert_alt_name` - A `String` specifying the subject alternative name for the self-signed certificate.
/// * `key_path` - A `PathBuf` specifying the location to save or load the private key.
/// * `cert_path` - A `PathBuf` specifying the location to save or load the certificate.
///
/// # Returns
///
/// Returns a `Result` containing a tuple:
/// * `Vec<CertificateDer<'static>>` - The parsed certificate chain as DER-encoded certificates.
/// * `PrivateKeyDer<'static>` - The parsed private key in a QUIC-compatible format.
///
/// On success, the tuple contains the certificate chain and private key. On failure,
/// it returns an `anyhow::Error` describing the issue encountered during file reading,
/// writing, or parsing.
///
/// # Examples
///
/// ```rust
/// use std::fs;
/// use std::path::PathBuf;
/// use tempfile::NamedTempFile;
/// use anyhow::{ensure, Context, Result};
/// use rcgen::{generate_simple_self_signed, KeyPair, CertifiedKey};
/// use portredirect::quic::{generate_quic_cert, try_load_quic_cert};
///
/// fn main() -> Result<()> {
///     // Create temporary file paths for the certificate and key.
///     let cert_temp = NamedTempFile::new()?;
///     let key_temp = NamedTempFile::new()?;
///
///     let cert_path = cert_temp.path().to_path_buf();
///     let key_path = key_temp.path().to_path_buf();
///     println!("Test files: cert {:?}, key {:?}", cert_path, key_path);
///
///     // Generate the self-signed certificate and private key.
///     generate_quic_cert("localhost".into(), key_path.clone(), cert_path.clone())?;
///
///     // Check that the certificate and key files exist.
///     ensure!(
///         cert_path.exists(),
///         "Certificate file was not created at {:?}",
///         cert_path
///     );
///     ensure!(
///         key_path.exists(),
///         "Private key file was not created at {:?}",
///         key_path
///     );
///
///     // Validate the written certificate and key.
///     let cert_pem = fs::read_to_string(&cert_path).context("failed to read certificate")?;
///     let key_pem = fs::read_to_string(&key_path).context("failed to read private key")?;
///
///     // Parse the key pair and ensure it matches the certificate.
///     let parsed_key_pair = KeyPair::from_pem(&key_pem).context("failed to parse private key")?;
///     ensure!(
///         parsed_key_pair.compatible_algs().next().is_some(),
///         "The public key in the certificate does not match the private key"
///     );
///
///     println!("Certificate and key generated! Trying to load them...");
///
///     // Attempt to load the generated certificate and key.
///     let result = try_load_quic_cert(key_path.clone(), cert_path.clone())?;
///
///     // Validate that the loading succeeded.
///     assert!(result.0.len() > 0, "Expected at least one certificate in the chain");
///
///     println!("Certificate and key loading succeeded!");
///
///     Ok(())
/// }
/// ```
///
/// Note: This function is suitable for development and testing purposes. For production,
/// use a trusted certificate authority to issue certificates.
#[allow(unused)]
#[instrument()]
pub fn generate_quic_cert(
    cert_alt_name: String,
    key_path: PathBuf,
    cert_path: PathBuf,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    let path = get_config_dir().unwrap();
    let (cert, key) = match fs::read(&cert_path).and_then(|x| Ok((x, fs::read(&key_path)?))) {
        Ok((cert, key)) => (
            CertificateDer::from(cert),
            PrivateKeyDer::try_from(key).map_err(anyhow::Error::msg)?,
        ),
        Err(ref e) if e.kind() == io::ErrorKind::NotFound => {
            info!("generating self-signed certificate");
            let cert = rcgen::generate_simple_self_signed(vec![cert_alt_name]).unwrap();
            let key = PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der());
            let cert = cert.cert.into();
            fs::create_dir_all(path).context("failed to create certificate directory")?;
            fs::write(&cert_path, &cert).context("failed to write certificate")?;
            fs::write(&key_path, key.secret_pkcs8_der()).context("failed to write private key")?;
            (cert, key.into())
        }
        Err(e) => {
            bail!("failed to read certificate: {}", e);
        }
    };

    Ok((vec![cert], key))
}

#[allow(unused)]
#[instrument(skip(config))]
pub async fn run_quic_server(config: ServerConfig) -> Result<()> {
    info!("Starting QUIC server setup");

    let (cert_chain, key_der) = load_or_generate_quic_cert(
        config.cert_hostname,
        config.key_file.clone(),
        config.cert_file.clone(),
    )
    .context("loading or generating cert")?;

    info!("Configuring rustls server ({} certs, key: {:?})", cert_chain.len(), key_der);

    let mut server_crypto = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key_der)
        .context("rustls ServerConfig builder")?;
    server_crypto.alpn_protocols = ALPN_QUIC_PORTREDIRECT.iter().map(|&x| x.into()).collect();

    let mut server_config =
        quinn::ServerConfig::with_crypto(Arc::new(QuicServerConfig::try_from(server_crypto)?));
    let transport_config = Arc::get_mut(&mut server_config.transport).unwrap();
    transport_config.max_concurrent_uni_streams(0_u8.into());

    info!(listen_addr = %config.listen, "Binding QUIC endpoint");
    let endpoint = quinn::Endpoint::server(server_config, config.listen)?;

    info!("QUIC server is ready and accepting connections");
    while let Some(conn) = endpoint.accept().await {
        let conn_span = Span::current();

        if config
            .connection_limit
            .is_some_and(|n| endpoint.open_connections() >= n)
        {
            warn!(
                "Refusing connection: open connection limit ({}) reached",
                config.connection_limit.unwrap()
            );
            conn.refuse();
        } else if config.stateless_retry && !conn.remote_address_validated() {
            warn!(
                "Requiring connection from {} to validate its address",
                conn.remote_address()
            );
            conn.retry().unwrap();
        } else {
            let peer_info = format!(
                "client: {} (validated: {})",
                conn.remote_address(),
                conn.remote_address_validated()
            );
            debug!(peer = %peer_info, "Accepting new QUIC connection");

            tokio::spawn(async move {
                if let Err(e) = handle_connection_quic(conn).await {
                    error!(error = %e, peer = %peer_info, "Error during QUIC connection");
                }
            });
        }
    }

    Ok(())
}

#[allow(unused)]
#[instrument(skip(conn))]
async fn handle_connection_quic(conn: quinn::Incoming) -> Result<()> {
    let connection = conn.await?;
    async {
        debug!("QUIC connection established");

        // Each stream initiated by the client constitutes a new request.
        loop {
            let stream = connection.accept_bi().await;
            let stream = match stream {
                Err(quinn::ConnectionError::ApplicationClosed { .. }) => {
                    debug!("QUIC connection closed");
                    return Ok(());
                }
                Err(e) => {
                    return Err(e);
                }
                Ok(s) => s,
            };
            let fut = handle_request_quic(stream);
            tokio::spawn(async move {
                if let Err(e) = fut.await {
                    error!("failed: {reason}", reason = e.to_string());
                }
            });
        }
    }
    .await?;
    Ok(())
}

#[allow(unused)]
#[instrument(skip(send, recv))]
async fn handle_request_quic(
    (mut send, mut recv): (quinn::SendStream, quinn::RecvStream),
) -> Result<()> {
    let req = recv
        .read_to_end(64 * 1024)
        .await
        .map_err(|e| anyhow!("failed reading request: {}", e))?;
    let mut escaped = String::new();
    for &x in &req[..] {
        let part = ascii::escape_default(x).collect::<Vec<_>>();
        escaped.push_str(str::from_utf8(&part).unwrap());
    }
    debug!(escaped=%escaped);

    // Execute the request
    let resp = b"HELLO I AM PRSERVER, WHO ARE YOU?".to_vec();
    // Write the response
    send.write_all(&resp)
        .await
        .map_err(|e| anyhow!("failed to send response: {}", e))?;
    // Gracefully terminate the stream
    send.finish().unwrap();
    debug!("complete");
    Ok(())
}
