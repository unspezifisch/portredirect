// from https://github.com/quinn-rs/quinn/blob/204b14792b5e92eb2c43cdb1ff05426412ff4466/quinn/examples/server.rs
use std::{
    ascii, fs, io, net::SocketAddr, path::{self, Path, PathBuf}, str, sync::Arc
};

use anyhow::{anyhow, bail, Context, Error, Result};
use quinn::crypto::rustls::QuicServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

#[derive(Debug)]
struct QuicConfig {
    cert_hostname: String,
    cert_file: PathBuf,
    key_file: PathBuf,

    listen: SocketAddr,
    stateless_retry: bool,
    connection_limit: Option<usize>,
}

fn try_load_quic_cert(key_path: PathBuf, cert_path: PathBuf) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    let key = fs::read(key_path).context("failed to read private key")?;
    let key = if key_path.extension().is_some_and(|x| x == "der") {
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key))
    } else {
        rustls_pemfile::private_key(&mut &*key)
            .context("malformed PKCS #1 private key")?
            .ok_or_else(|| anyhow::Error::msg("no private keys found"))?
    };
    let cert_chain = fs::read(cert_path).context("failed to read certificate chain")?;
    let cert_chain = if cert_path.extension().is_some_and(|x| x == "der") {
        vec![CertificateDer::from(cert_chain)]
    } else {
        rustls_pemfile::certs(&mut &*cert_chain)
            .collect::<Result<_, _>>()
            .context("invalid PEM-encoded certificate")?
    };

    Ok((cert_chain, key))
}

fn generate_quic_cert(key_path: PathBuf, cert_path: PathBuf) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>), Error> {
    let (cert, key) = match fs::read(&cert_path).and_then(|x| Ok((x, fs::read(&key_path)?))) {
        Ok((cert, key)) => (
            CertificateDer::from(cert),
            PrivateKeyDer::try_from(key).map_err(anyhow::Error::msg)?,
        ),
        Err(ref e) if e.kind() == io::ErrorKind::NotFound => {
            println!("generating self-signed certificate");
            let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
            let key = PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der());
            let cert = cert.cert.into();
            fs::write(&cert_path, &cert).context("failed to write certificate")?;
            fs::write(&key_path, key.secret_pkcs8_der())
                .context("failed to write private key")?;
            (cert, key.into())
        }
        Err(e) => {
            bail!("failed to read certificate: {}", e);
        }
    };

    Ok((vec![cert], key))
}

#[tokio::main]
async fn setup_quic(config: QuicConfig) -> Result<()> {
    let (certs, key) = match try_load_quic_cert(config.key_file, config.cert_file) {
        Ok(ret) => ret,
        Err(_) => generate_quic_cert(config.key_file, config.cert_file).context("generating QUIC certificate")?,
    };

    let mut server_crypto = rustls::ServerConfig::builder()
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
            tokio::spawn(
                async move {
                    if let Err(e) = fut.await {
                        eprintln!("failed: {reason}", reason = e.to_string());
                    }
                }
            );
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
