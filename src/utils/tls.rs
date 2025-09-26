use std::fs::File;
use std::io::{self, BufReader};
use std::sync::Arc;

use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig;
use tokio_rustls::TlsAcceptor;

fn load_certs(path: &str) -> io::Result<Vec<CertificateDer<'static>>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let certs = rustls_pemfile::certs(&mut reader)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("failed to read certs: {e}")))?;
    if certs.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "no certificates found in cert file",
        ));
    }
    Ok(certs)
}

fn load_private_key(path: &str) -> io::Result<PrivateKeyDer<'static>> {
    // Try PKCS#8 first
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut keys = rustls_pemfile::pkcs8_private_keys(&mut reader)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("failed to read pkcs8 key: {e}")))?;
    if let Some(key) = keys.pop() {
        return Ok(key);
    }

    // Fallback to RSA (PKCS#1)
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut keys = rustls_pemfile::rsa_private_keys(&mut reader)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("failed to read rsa key: {e}")))?;
    if let Some(key) = keys.pop() {
        return Ok(key);
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "no supported private key found (expecting PKCS#8 or RSA)",
    ))
}

pub fn make_tls_acceptor(cert_path: &str, key_path: &str) -> io::Result<TlsAcceptor> {
    let certs = load_certs(cert_path)?;
    let key = load_private_key(key_path)?;

    let config: ServerConfig = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, format!("invalid cert/key: {e}")))?;

    Ok(TlsAcceptor::from(Arc::new(config)))
}

