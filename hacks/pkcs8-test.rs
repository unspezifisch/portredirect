#!/usr/bin/env rust-script
//! Dependencies can be specified in the script file itself as follows:
//!
//! ```cargo
//! [dependencies]
//! rcgen = { version = "*", features = ["pem", "crypto"] }
//! pkcs8 = "*"
//! pem = "*"
//! ```

use rcgen::{Certificate, CertificateParams, KeyPair};
use pkcs8::PrivateKeyInfo;
use std::fs::File;
use pem::Pem;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Step 1: Generate a key pair and certificate using rcgen
    let params = CertificateParams::new(vec!["example.com".to_string()]);
    let cert = Certificate::try_from(params)?;

    // The private key is part of the `cert` object
    let private_key: &KeyPair = &cert.serialize_private_key();

    // Step 2: Convert the private key to PKCS#8 format using pkcs8
    let pkcs8_private_key = PrivateKeyInfo::from(private_key);
    let pkcs8_bytes = pkcs8_private_key.to_der()?;

    // Step 3: Save the private key in PEM format
    let pem = Pem {
        tag: "PRIVATE KEY".to_string(),
        contents: pkcs8_bytes,
    };

    let mut file = File::create("private_key.pem")?;
    pem::encode(&pem, &mut file)?;

    // Also, print the certificate in PEM format
    let cert_pem = cert.serialize_pem()?;
    println!("Generated Certificate PEM:\n{}", cert_pem);

    println!("Private key saved to private_key.pem");

    Ok(())
}
