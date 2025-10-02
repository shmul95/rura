use std::fs::File;
use std::io::{self, BufReader};
use std::sync::Arc;

use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::rustls::{self, ServerConfig};

fn load_certs(path: &str) -> io::Result<Vec<CertificateDer<'static>>> {
    let mut reader = BufReader::new(File::open(path)?);
    let certs: Vec<CertificateDer<'static>> =
        rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;
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
    let mut reader = BufReader::new(File::open(path)?);
    let mut pkcs8 =
        rustls_pemfile::pkcs8_private_keys(&mut reader).collect::<Result<Vec<_>, _>>()?;
    if let Some(key) = pkcs8.pop() {
        return Ok(PrivateKeyDer::from(key));
    }

    // Fallback to RSA (PKCS#1)
    let mut reader = BufReader::new(File::open(path)?);
    let mut rsa = rustls_pemfile::rsa_private_keys(&mut reader).collect::<Result<Vec<_>, _>>()?;
    if let Some(key) = rsa.pop() {
        return Ok(PrivateKeyDer::from(key));
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
        .map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid cert/key: {e}"),
            )
        })?;

    Ok(TlsAcceptor::from(Arc::new(config)))
}

