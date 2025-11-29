use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::Connection;
use tokio::net::TcpListener;

use rura_server::client::handle_client;
use rura_server::messaging::state::AppState;
use rura_server::utils::tls::make_tls_acceptor;
use rura_server::webrtc;

use rura_client::api::{
    accept_call, end_call, get_current_call_state, open_message_stream_cli, register_tls,
    start_call,
};
use rura_client::security;

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
    let mut ca_params = rcgen::CertificateParams::default();
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    ca_params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "rura test ca");
    let ca_cert = rcgen::Certificate::from_params(ca_params).expect("ca");

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

#[ignore = "requires local TCP bind permission; run manually when available"]
#[tokio::test]
async fn cli_call_flow_end_to_end() {
    // Generate TLS materials and write to temporary files
    let (cert_chain_pem, key_pem) = generate_tls_materials();
    let cert_file = tempfile::NamedTempFile::new().expect("cert tmp");
    let key_file = tempfile::NamedTempFile::new().expect("key tmp");
    std::fs::write(cert_file.path(), cert_chain_pem.as_bytes()).expect("write cert");
    std::fs::write(key_file.path(), key_pem.as_bytes()).expect("write key");

    // Prepare in-memory DB and server state (with WebRTC enabled)
    let db = create_test_db();
    let state = Arc::new(AppState::new(true));
    webrtc::register();
    let port = accept_n_connections(
        6,
        Arc::clone(&db),
        Arc::clone(&state),
        cert_file.path().to_str().unwrap(),
        key_file.path().to_str().unwrap(),
    )
    .await;

    // Extract CA PEM for the client from the chain (last certificate)
    let ca_pem = {
        let chain = std::fs::read_to_string(cert_file.path()).expect("read chain");
        chain
            .split("-----BEGIN CERTIFICATE-----")
            .filter(|s| !s.trim().is_empty())
            .map(|body| format!("-----BEGIN CERTIFICATE-----{}", body))
            .last()
            .unwrap()
    };

    // Two isolated client data directories representing two users
    let dir_a = tempfile::tempdir().expect("dir_a");
    let dir_b = tempfile::tempdir().expect("dir_b");

    // Register user A
    unsafe {
        std::env::set_var("RURA_CLIENT_DATA_DIR", dir_a.path());
    }
    let reg_a = tokio::task::spawn_blocking({
        let ca = ca_pem.clone();
        move || {
            let _ = security::generate_and_store_identity();
            register_tls(
                "localhost".to_string(),
                port,
                ca,
                "".to_string(),
                "secret".to_string(),
            )
        }
    })
    .await
    .expect("spawn")
    .expect("register a ok");
    let user_a = reg_a.user_id.expect("user_a id");

    // Register user B
    unsafe {
        std::env::set_var("RURA_CLIENT_DATA_DIR", dir_b.path());
    }
    let reg_b = tokio::task::spawn_blocking({
        let ca = ca_pem.clone();
        move || {
            let _ = security::generate_and_store_identity();
            register_tls(
                "localhost".to_string(),
                port,
                ca,
                "".to_string(),
                "secret".to_string(),
            )
        }
    })
    .await
    .expect("spawn")
    .expect("register b ok");
    let user_b = reg_b.user_id.expect("user_b id");

    // Open message streams for both users (CLI-style)
    unsafe {
        std::env::set_var("RURA_CLIENT_DATA_DIR", dir_a.path());
    }
    let (stream_a_uid, _rx_a) = tokio::task::spawn_blocking({
        let ca = ca_pem.clone();
        move || {
            open_message_stream_cli(
                "localhost".to_string(),
                port,
                ca,
                "".to_string(),
                "secret".to_string(),
            )
        }
    })
    .await
    .expect("spawn")
    .expect("open stream a");
    assert_eq!(stream_a_uid, user_a);

    unsafe {
        std::env::set_var("RURA_CLIENT_DATA_DIR", dir_b.path());
    }
    let (stream_b_uid, _rx_b) = tokio::task::spawn_blocking({
        let ca = ca_pem.clone();
        move || {
            open_message_stream_cli(
                "localhost".to_string(),
                port,
                ca,
                "".to_string(),
                "secret".to_string(),
            )
        }
    })
    .await
    .expect("spawn")
    .expect("open stream b");
    assert_eq!(stream_b_uid, user_b);

    // User A starts an audio-only call to user B.
    unsafe {
        std::env::set_var("RURA_CLIENT_DATA_DIR", dir_a.path());
    }
    let state_a = tokio::task::spawn_blocking(move || start_call(user_a, user_b, false))
        .await
        .expect("spawn")
        .expect("start_call a->b");
    let call_id = state_a.call_id.clone();

    // Allow signaling to propagate and B to record incoming call state.
    tokio::time::sleep(Duration::from_millis(500)).await;

    // User B should see an incoming ringing call.
    unsafe {
        std::env::set_var("RURA_CLIENT_DATA_DIR", dir_b.path());
    }
    let state_b = tokio::task::spawn_blocking(|| get_current_call_state())
        .await
        .expect("spawn")
        .expect("get_current_call_state b")
        .expect("b has active call");
    assert_eq!(state_b.call_id, call_id);
    assert_eq!(state_b.remote_user_id, user_a);

    // B accepts the call, then ends it.
    let call_id_clone = call_id.clone();
    let state_b_connected = tokio::task::spawn_blocking(move || {
        accept_call(user_b, call_id_clone.clone(), false)
    })
    .await
    .expect("spawn")
    .expect("accept_call b");
    assert_eq!(state_b_connected.call_id, call_id);

    tokio::time::sleep(Duration::from_millis(300)).await;

    tokio::task::spawn_blocking({
        let cid = call_id.clone();
        move || end_call(user_b, cid)
    })
    .await
    .expect("spawn")
    .expect("end_call b");

    // Give time for hangup propagation and local call state cleanup.
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Both sides should now report no active call.
    unsafe {
        std::env::set_var("RURA_CLIENT_DATA_DIR", dir_a.path());
    }
    let a_state_after = tokio::task::spawn_blocking(|| get_current_call_state())
        .await
        .expect("spawn")
        .expect("get_current_call_state a after");
    assert!(
        a_state_after.is_none(),
        "caller should not remain in a call after callee hangup"
    );

    unsafe {
        std::env::set_var("RURA_CLIENT_DATA_DIR", dir_b.path());
    }
    let b_state_after = tokio::task::spawn_blocking(|| get_current_call_state())
        .await
        .expect("spawn")
        .expect("get_current_call_state b after");
    assert!(b_state_after.is_none(), "callee should see call cleared");
}
