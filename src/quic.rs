// based on https://github.com/quinn-rs/quinn/blob/204b14792b5e92eb2c43cdb1ff05426412ff4466/quinn/examples/server.rs
use std::{ascii, fs, net::SocketAddr, path::PathBuf, str, sync::Arc};

use anyhow::{anyhow, Context, Error, Result};
use quinn::crypto::rustls::QuicServerConfig;
use rcgen::{generate_simple_self_signed, CertifiedKey};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

#[derive(Debug)]
pub struct QuicConfig {
    cert_hostname: String,
    cert_file: PathBuf,
    key_file: PathBuf,

    listen: SocketAddr,
    stateless_retry: bool,
    connection_limit: Option<usize>,
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
pub fn try_load_quic_cert(
    key_path: PathBuf,
    cert_path: PathBuf,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    // Load private key
    let key = fs::read(key_path.clone()).context("failed to read private key")?;
    let key = if key_path.extension().is_some_and(|x| x == "der") {
        // DER format
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key))
    } else {
        // PEM format
        rustls_pemfile::private_key(&mut &*key)
            .context("malformed PKCS #1 private key")?
            .ok_or_else(|| anyhow::Error::msg("no private keys found"))?
    };

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
///     // Attempt to load the generated certificate and key.
///     //let result = try_load_quic_cert(key_path.clone(), cert_path.clone())?;
///
///     // Validate that the loading succeeded.
///     //assert!(result.0.len() > 0, "Expected at least one certificate in the chain");
///
///     // Validate the written certificate and key.
///     let cert_pem = fs::read_to_string(&cert_path).context("failed to read certificate")?;
///     let key_pem = fs::read_to_string(&key_path).context("failed to read private key")?;
///
///     // Parse the key pair and ensure it matches the certificate.
///     let parsed_key_pair = KeyPair::from_pem(&key_pem).context("failed to parse private key")?;
///     //let parsed_cert = rcgen::Certificate::from .context("failed to parse certificate")?;
///     ensure!(
///         parsed_key_pair.compatible_algs().next().is_some(),
///         "The public key in the certificate does not match the private key"
///     );
///
///     println!("Certificate and key validation succeeded!");
///
///     Ok(())
/// }
/// ```
///
/// Note: This function is suitable for development and testing purposes. For production,
/// use a trusted certificate authority to issue certificates.
pub fn generate_quic_cert(
    cert_alt_name: String,
    key_path: PathBuf,
    cert_path: PathBuf,
) -> Result<()> {
    println!("Generating self-signed certificate");
    let CertifiedKey { cert, key_pair } = generate_simple_self_signed(vec![cert_alt_name.into()])?;
    fs::write(&cert_path, cert.pem()).context("failed to write certificate")?;
    fs::write(&key_path, key_pair.serialize_pem()).context("failed to write private key")?;
    Ok(())
}

#[tokio::main]
pub async fn setup_quic(config: QuicConfig) -> Result<()> {
    let (certs, key) = match try_load_quic_cert(config.key_file.clone(), config.cert_file.clone()) {
        Ok(ret) => ret,
        Err(_) => {
            generate_quic_cert(config.cert_hostname, config.key_file.clone(), config.cert_file.clone())
                .context("generating QUIC certificate")?;
            try_load_quic_cert(config.key_file.clone(), config.cert_file.clone())?
        }
    };

    let server_crypto = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;

    let mut server_config =
        quinn::ServerConfig::with_crypto(Arc::new(QuicServerConfig::try_from(server_crypto)?));
    let transport_config = Arc::get_mut(&mut server_config.transport).unwrap();
    transport_config.max_concurrent_uni_streams(0_u8.into());

    let endpoint = quinn::Endpoint::server(server_config, config.listen)?;
    eprintln!("listening on {}", endpoint.local_addr()?);

    while let Some(conn) = endpoint.accept().await {
        if config
            .connection_limit
            .is_some_and(|n| endpoint.open_connections() >= n)
        {
            println!("refusing due to open connection limit");
            conn.refuse();
        } else if config.stateless_retry && !conn.remote_address_validated() {
            println!("requiring connection to validate its address");
            conn.retry().unwrap();
        } else {
            println!("accepting connection");
            let fut = handle_connection_quic(conn);
            tokio::spawn(async move {
                if let Err(e) = fut.await {
                    eprintln!("connection failed: {reason}", reason = e.to_string())
                }
            });
        }
    }

    Ok(())
}

async fn handle_connection_quic(conn: quinn::Incoming) -> Result<()> {
    let connection = conn.await?;
    async {
        println!("QUIC connection established");

        // Each stream initiated by the client constitutes a new request.
        loop {
            let stream = connection.accept_bi().await;
            let stream = match stream {
                Err(quinn::ConnectionError::ApplicationClosed { .. }) => {
                    println!("QUIC connection closed");
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
                    eprintln!("failed: {reason}", reason = e.to_string());
                }
            });
        }
    }
    .await?;
    Ok(())
}

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
    println!("{}", escaped);

    // Execute the request
    let resp = vec![0x41, 0x42, 0x43];
    // Write the response
    send.write_all(&resp)
        .await
        .map_err(|e| anyhow!("failed to send response: {}", e))?;
    // Gracefully terminate the stream
    send.finish().unwrap();
    println!("complete");
    Ok(())
}
