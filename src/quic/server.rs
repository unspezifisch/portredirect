// PortRedirector-RS Common Server Code
//
// License: GPL-3.0-only
// Based on: Quinn example code (originally licensed under Apache-2.0/MIT)
// Original: https://github.com/quinn-rs/quinn/blob/204b14792b5e92eb2c43cdb1ff05426412ff4466/quinn/examples/server.rs

use anyhow::{Context, Error, Result};
use quinn::crypto::rustls::QuicServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use secrecy::SecretString;
use std::{fs, net::SocketAddr, path::PathBuf, sync::Arc, time::Instant};
use tracing::{debug, error, info, instrument, warn};

use crate::{get_config_dir, quic::ALPN_QUIC_PORTREDIRECT};

/// Configuration for the QUIC server.
///
/// This struct holds the necessary configuration parameters for setting up a QUIC server.
///
/// # Fields
///
/// * `cert_hostname` - The hostname for the certificate to use or generate.
/// * `cert_file` - The path to the certificate to use or generate.
/// * `key_file` - The path to the private key file to use or generate.
/// * `listen` - Bind address for the QUIC server.
/// * `stateless_retry` - Whether to enable stateless retry.
/// * `connection_limit` - Optional limit on the number of concurrently forwarded connections.
/// * `pr_psk` - Pre-shared key to authenticate the client to the server.
/// * `app_data` - Optionally, any application-specific data.
#[derive(Debug)]
#[allow(unused)]
pub struct ServerConfig<T> {
    pub cert_hostname: String,
    pub cert_file: PathBuf,
    pub key_file: PathBuf,
    pub listen: SocketAddr,
    pub stateless_retry: bool,
    pub connection_limit: Option<usize>,
    pub pr_psk: SecretString,
    pub app_data: T,
}

impl<T: Default> ServerConfig<T> {
    /// Creates a default server configuration.
    ///
    /// This function initializes a `ServerConfig` with default values, using the provided parameters or defaults.
    ///
    /// # Arguments
    ///
    /// * `config_dir` - The directory where the certificate and key files are located.
    /// * `cert_alt_name` - The Subject Alternae Name (SAN) for the QUIC server certificate.
    /// * `bind_socket` - Bind address for the QUIC server.
    /// * `psk` - Pre-shared key to authenticate the client to the server.
    /// * `app_data` - Optionally, any application-specific data.
    ///
    /// # Returns
    ///
    /// Returns a `ServerConfig` instance with the specified and/or default parameters.
    #[allow(unused)]
    pub fn create_default_config(
        config_dir: PathBuf,
        cert_alt_name: String,
        bind_socket: SocketAddr,
        psk: SecretString,
        app_data: Option<T>,
    ) -> Self {
        ServerConfig {
            cert_hostname: cert_alt_name,
            cert_file: config_dir.join("cert.der"),
            key_file: config_dir.join("key.der"),
            listen: bind_socket,
            stateless_retry: true,  // Be more secure by default
            connection_limit: None, // TODO add fn parameter for this
            pr_psk: psk,
            app_data: app_data.unwrap_or_default(),
        }
    }
}

/// Loads or generates a QUIC-compatible certificate and private key.
///
/// This function attempts to load a certificate and private key from the specified file paths.
/// If the files do not exist, it generates a self-signed certificate and saves it to the paths.
///
/// # Arguments
///
/// * `cert_alt_name` - The Subject Alternate Name (SAN) for the certificate.
/// * `key_path` - The path to the private key file to load, if it exists, or to save the generated key to if it does not.
/// * `cert_path` - The path to the certificate file, same applies.
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

/// Loads a QUIC-compatible certificate and private key from the specified file paths.
///
/// This function reads a private key and certificate chain from the provided file paths
/// and attempts to parse them into the required QUIC-compatible formats. It supports
/// both DER-encoded and PEM-encoded files. DER files must have the `.der` extension,
/// otherwise PEM is assumed.
///
/// # Arguments
///
/// * `key_path` - The path to the private key file.
/// * `cert_path` - The path to the certificate chain file.
///
/// # Returns
///
/// Returns a `Result` containing a tuple with the certificate chain and private key,
/// in a format suitable for quinn.
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

/// Generates a self-signed certificate and private key.
///
/// This function generates a self-signed certificate using the provided alternative
/// name for the certificate (e.g., a domain name or IP address). The generated files are saved
/// to the specified paths. The function then loads the certificate and private key into
/// QUIC-compatible formats.
///
/// # Arguments
///
/// * `cert_alt_name` - The alternative name for the certificate.
/// * `key_path` - The path to save the private key.
/// * `cert_path` - The path to save the certificate.
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
#[instrument()]
pub fn generate_quic_cert(
    cert_alt_name: String,
    key_path: PathBuf,
    cert_path: PathBuf,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    info!("generating self-signed certificate");
    let cert = rcgen::generate_simple_self_signed(vec![cert_alt_name]).unwrap();
    let key = PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der());

    // Create directories if they don't exist.
    let path = get_config_dir().unwrap();
    fs::create_dir_all(path).context("failed to create certificate directory")?;

    // Write certificate and private key to files.
    let cert = CertificateDer::from(cert.cert);
    fs::write(&cert_path, &cert).context("failed to write certificate")?;
    fs::write(&key_path, key.secret_pkcs8_der()).context("failed to write private key")?;

    Ok((vec![cert], key.into()))
}

/// Runs the QUIC server with the specified configuration and client handler.
///
/// This function sets up and runs a QUIC server using the provided configuration and
/// client connection handler. It handles incoming connections and spawns tasks to
/// process them.
///
/// # Arguments
///
/// * `config` - The server configuration.
/// * `handle_incoming_client` - A function to handle incoming client connections.
///
/// # Returns
///
/// Returns a `Result` indicating the success or failure of the server operation.
#[instrument(skip(config, handle_incoming_client))]
pub async fn run_quic_server<F, Fut, T>(
    config: ServerConfig<T>,
    handle_incoming_client: F,
) -> Result<()>
where
    F: Fn(Arc<ServerConfig<T>>, quinn::Connection) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<(), Error>> + Send + 'static,
{
    info!("Starting PR QUIC server setup");

    // Load or generate certificate.
    let (cert_chain, key_der) = load_or_generate_quic_cert(
        config.cert_hostname.clone(),
        config.key_file.clone(),
        config.cert_file.clone(),
    )
    .context("loading or generating cert")?;

    info!(
        "Configuring rustls server ({} certs, key: {:?})",
        cert_chain.len(),
        key_der
    );

    // Crypto setup.
    let mut server_crypto = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key_der)
        .context("rustls ServerConfig builder")?;
    server_crypto.alpn_protocols = ALPN_QUIC_PORTREDIRECT.iter().map(|&x| x.into()).collect();

    // QUIC server setup.
    let mut server_config =
        quinn::ServerConfig::with_crypto(Arc::new(QuicServerConfig::try_from(server_crypto)?));
    let transport_config = Arc::get_mut(&mut server_config.transport).unwrap();
    transport_config.max_concurrent_uni_streams(0_u8.into());
    transport_config.max_concurrent_bidi_streams(0_u8.into());

    // Start QUIC server listener.
    info!(listen_addr = %config.listen, "Binding QUIC endpoint");
    let endpoint = quinn::Endpoint::server(server_config, config.listen)?;

    // PR QUIC server side loop:
    // Handle incoming QUIC connections forever.
    let start = Instant::now();
    let config = Arc::from(config);
    info!("QUIC server is ready and accepting connections");
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
            let connection = conn
                .await
                .context("accepting incoming quic client connection")?;

            debug!(peer = %peer_info, "Accepting new QUIC client connection at {:?}", start.elapsed());

            let fut = handle_incoming_client(Arc::clone(&config), connection);
            tokio::spawn(async move {
                if let Err(e) = fut.await {
                    error!("connection failed: {reason}", reason = e.to_string())
                }
            });
        }
    }

    info!("PR QUIC server terminated after {:?}.", start.elapsed());

    Ok(())
}
