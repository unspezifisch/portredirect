// based on https://github.com/quinn-rs/quinn/blob/204b14792b5e92eb2c43cdb1ff05426412ff4466/quinn/examples/server.rs
use anyhow::{anyhow, Context, Result};
use quinn::crypto::rustls::QuicServerConfig;
use rcgen::{generate_simple_self_signed, CertifiedKey};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use std::{ascii, fs, net::SocketAddr, path::PathBuf, str, sync::Arc};
use tracing::{debug, error, info, instrument, span, warn, Instrument, Level, Span};

#[derive(Debug)]
#[allow(unused)]
pub struct QuicConfig {
    pub cert_hostname: String,
    pub cert_file: PathBuf,
    pub key_file: PathBuf,

    pub listen: SocketAddr,
    pub stateless_retry: bool,
    pub connection_limit: Option<usize>,
}

impl QuicConfig {
    #[allow(unused)]
    pub fn create_default_config(config_dir: PathBuf, bind_socket: SocketAddr) -> Self {
        QuicConfig {
            cert_hostname: "localhost".to_string(),
            cert_file: config_dir.join("cert.pem"),
            key_file: config_dir.join("key.pem"),
            listen: bind_socket,
            stateless_retry: false,
            connection_limit: None,
        }
    }
}

#[allow(unused)]
pub const ALPN_QUIC_HTTP: &[&[u8]] = &[b"hq-29"]; // HACK this should be our own protocol ID

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
pub fn try_load_quic_cert(
    key_path: PathBuf,
    cert_path: PathBuf,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    // Load private key
    let key = fs::read(key_path.clone()).context("failed to read private key")?;

    // Try to load the key as DER first
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.clone()));

    // Load cert chain
    let cert_chain = fs::read(cert_path.clone()).context("failed to read certificate chain")?;
    let cert_chain = if cert_path.extension().is_some_and(|x| x == "der") {
        // DER format
        vec![CertificateDer::from(cert_chain)]
    } else {
        // PEM format
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
pub fn generate_quic_cert(
    cert_alt_name: String,
    key_path: PathBuf,
    cert_path: PathBuf,
) -> Result<()> {
    info!("Generating self-signed certificate");
    let CertifiedKey { cert, key_pair } = generate_simple_self_signed(vec![cert_alt_name.into()])?;
    let key = PrivatePkcs8KeyDer::from(key_pair.serialize_der());
    let cert: rcgen::Certificate = cert.into();

    fs::write(&cert_path, cert.der())
        .with_context(|| format!("failed to write certificate to: {}", cert_path.display()))?;
    fs::write(&key_path, key.secret_pkcs8_der())
        .with_context(|| format!("failed to write private key to: {}", key_path.display()))?;
    Ok(())
}

#[allow(unused)]
#[instrument(skip(config), fields(hostname = %config.cert_hostname))]
pub async fn setup_and_run_quic_server(config: QuicConfig) -> Result<()> {
    info!("Starting QUIC server setup");

    let (cert_chain, key_der) = match try_load_quic_cert(config.key_file.clone(), config.cert_file.clone()) {
        Ok(ret) => {
            info!("Successfully loaded QUIC certificate");
            ret
        }
        Err(e) => {
            warn!(error = %e, "Failed to load QUIC certificate, generating new one");
            generate_quic_cert(
                config.cert_hostname.clone(),
                config.key_file.clone(),
                config.cert_file.clone(),
            )
            .with_context(|| {
                format!(
                    "Generating QUIC certificate (because we couldn't load it earlier: {})",
                    e
                )
            })?;
            info!("Generated new QUIC certificate, attempting to load it");
            try_load_quic_cert(config.key_file.clone(), config.cert_file.clone())
                .context("loading after generating")?
        }
    };

    info!("Configuring rustls server");
    let mut server_crypto = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key_der)
        .context("rustls config")?;
    server_crypto.alpn_protocols = ALPN_QUIC_HTTP.iter().map(|&x| x.into()).collect();

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
            debug!("Accepting QUIC connection from {}", peer_info);

            let fut = handle_connection_quic(conn).instrument(conn_span.clone());
            tokio::spawn(async move {
                if let Err(e) = fut.await {
                    error!("Error during QUIC connection from {}: {}", peer_info, e);
                }
            });
        }
    }

    Ok(())
}

#[allow(unused)]
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
    debug!(escaped=%escaped, "hrq");

    // Execute the request
    let resp = vec![0x41, 0x42, 0x43];
    // Write the response
    send.write_all(&resp)
        .await
        .map_err(|e| anyhow!("failed to send response: {}", e))?;
    // Gracefully terminate the stream
    send.finish().unwrap();
    debug!("complete");
    Ok(())
}
