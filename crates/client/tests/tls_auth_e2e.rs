use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use tokio::net::TcpListener;

// Reuse the actual server acceptor and handlers
use rura_server::client::handle_client;
use rura_server::messaging::state::AppState;
// message persistence removed
use rura_server::utils::tls::make_tls_acceptor;

// The client functions under test
// use rura_client::api::login_and_fetch_history_tls; // server history removed
use rura_client::api::{login_tls, register_tls};

fn create_test_db() -> Arc<Mutex<Connection>> {
    let conn = Connection::open(":memory:").expect("open in-memory db");
    conn.execute(
        "CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            passphrase TEXT NOT NULL UNIQUE,
            password TEXT NOT NULL
        )",
        [],
    )
    .expect("create users");

    conn.execute(
        "CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            sender INTEGER NOT NULL,
            receiver INTEGER NOT NULL,
            content TEXT NOT NULL,
            timestamp TEXT NOT NULL,
            saved INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY(sender) REFERENCES users(id),
            FOREIGN KEY(receiver) REFERENCES users(id)
        )",
        [],
    )
    .expect("create messages");

    conn.execute(
        "CREATE TABLE IF NOT EXISTS connections (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ip TEXT NOT NULL,
            timestamp TEXT NOT NULL
        )",
        [],
    )
    .expect("create connections");

    Arc::new(Mutex::new(conn))
}

fn generate_tls_materials() -> (String, String) {
    // Create a CA certificate
    let mut ca_params = rcgen::CertificateParams::default();
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    ca_params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "rura test ca");
    let ca_cert = rcgen::Certificate::from_params(ca_params).expect("ca");

    // Create a server cert signed by the CA for localhost
    let mut srv_params = rcgen::CertificateParams::new(vec!["localhost".into()]);
    srv_params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "rura server");
    srv_params
        .extended_key_usages
        .push(rcgen::ExtendedKeyUsagePurpose::ServerAuth);
    let srv_cert = rcgen::Certificate::from_params(srv_params).expect("server");
    let srv_pem = srv_cert
        .serialize_pem_with_signer(&ca_cert)
        .expect("srv pem");
    let srv_key_pem = srv_cert.serialize_private_key_pem();

    // Concatenate chain: leaf + CA so server presents full chain
    let ca_pem = ca_cert.serialize_pem().expect("ca pem");
    let chain_pem = format!("{}{}", srv_pem, ca_pem);
    (chain_pem, srv_key_pem)
}

async fn accept_n_connections(
    n: usize,
    db: Arc<Mutex<Connection>>,
    state: Arc<AppState>,
    cert_pem_path: &str,
    key_pem_path: &str,
) -> u16 {
    let listener = TcpListener::bind(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)))
        .await
        .expect("bind");
    let local_addr = listener.local_addr().expect("local addr");
    let port = local_addr.port();
    let acceptor = make_tls_acceptor(cert_pem_path, key_pem_path).expect("acceptor");

    tokio::spawn(async move {
        for _ in 0..n {
            let (stream, addr) = listener.accept().await.expect("accept");
            let db = Arc::clone(&db);
            let state = Arc::clone(&state);
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                match acceptor.accept(stream).await {
                    Ok(tls_stream) => {
                        let _ = handle_client(tls_stream, db, state, addr).await;
                    }
                    Err(e) => eprintln!("TLS accept error: {}", e),
                }
            });
        }
    });

    port
}

#[tokio::test]
async fn tls_register_then_login_end_to_end() {
    // Generate TLS materials and write to temporary files for server acceptor
    let (cert_chain_pem, key_pem) = generate_tls_materials();
    let cert_file = tempfile::NamedTempFile::new().expect("cert tmp");
    let key_file = tempfile::NamedTempFile::new().expect("key tmp");
    std::fs::write(cert_file.path(), cert_chain_pem.as_bytes()).expect("write cert");
    std::fs::write(key_file.path(), key_pem.as_bytes()).expect("write key");

    // Prepare server state
    let db = create_test_db();
    let state = Arc::new(AppState::default());
    let port = accept_n_connections(
        3,
        Arc::clone(&db),
        Arc::clone(&state),
        cert_file.path().to_str().unwrap(),
        key_file.path().to_str().unwrap(),
    )
    .await;

    // Client CA is the CA part from our chain (the second cert)
    let ca_pem = {
        // Extract CA by regenerating it deterministically for the test call; simpler: reuse tail of chain
        // Since we concatenated srv+ca, split by lines and take last cert block
        let chain = std::fs::read_to_string(cert_file.path()).expect("read chain");
        chain
            .split("-----BEGIN CERTIFICATE-----")
            .filter(|s| !s.trim().is_empty())
            .map(|body| format!("-----BEGIN CERTIFICATE-----{}", body))
            .last()
            .unwrap()
    };

    // Register
    let ca_pem_for_reg = ca_pem.clone();
    let reg = tokio::task::spawn_blocking(move || {
        register_tls(
            "localhost".to_string(),
            port,
            ca_pem_for_reg,
            "alice".to_string(),
            "secret".to_string(),
        )
    })
    .await
    .expect("spawn")
    .expect("register ok");
    assert!(reg.success, "registration should succeed: {}", reg.message);
    let uid = reg.user_id.expect("user_id assigned");

    // Login
    let ca_pem2 = ca_pem;
    let login = tokio::task::spawn_blocking(move || {
        login_tls(
            "localhost".to_string(),
            port,
            ca_pem2,
            "alice".to_string(),
            "secret".to_string(),
        )
    })
    .await
    .expect("spawn")
    .expect("login ok");
    assert!(login.success, "login should succeed: {}", login.message);
    assert_eq!(login.user_id, Some(uid));

    // History flow removed; test ends after successful login
}
