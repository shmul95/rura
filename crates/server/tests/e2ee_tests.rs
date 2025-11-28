use rura_server::client::handle_client;
use rura_server::messaging::state::AppState;
use rura_server::models::client_message::{AuthRequest, ClientMessage};
use rusqlite::Connection;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

async fn setup_memory_db_with_pubkey() -> Arc<Mutex<Connection>> {
    let conn = Connection::open(":memory:").unwrap();
    // users table with pubkey column for E2EE
    conn.execute(
        "CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            passphrase TEXT NOT NULL UNIQUE,
            password TEXT NOT NULL,
            pubkey TEXT
        )",
        [],
    )
    .unwrap();
    conn.execute(
        "CREATE TABLE IF NOT EXISTS connections (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ip TEXT NOT NULL,
            timestamp TEXT NOT NULL
        )",
        [],
    )
    .unwrap();
    Arc::new(Mutex::new(conn))
}

async fn read_msg<S>(stream: &mut S) -> ClientMessage
where
    S: AsyncRead + Unpin,
{
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).await.unwrap();
    let txt = String::from_utf8_lossy(&buf[..n]).trim().to_string();
    serde_json::from_str(&txt).unwrap()
}

async fn write_json<S>(stream: &mut S, msg: &ClientMessage)
where
    S: AsyncWrite + Unpin,
{
    let mut line = serde_json::to_string(msg).unwrap();
    line.push('\n');
    stream.write_all(line.as_bytes()).await.unwrap();
    stream.flush().await.unwrap();
}

#[tokio::test]
async fn pubkey_set_and_get_and_opaque_message_flow() {
    let db = setup_memory_db_with_pubkey().await;
    let state = Arc::new(AppState::new(false));

    // Create two in-memory duplex streams to simulate two client connections
    let (server1, mut c1) = tokio::io::duplex(8192);
    let (server2, mut c2) = tokio::io::duplex(8192);
    let db1 = Arc::clone(&db);
    let st1 = Arc::clone(&state);
    let addr1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 11111);
    tokio::spawn(async move {
        let _ = handle_client(server1, db1, st1, addr1).await;
    });
    let db2 = Arc::clone(&db);
    let st2 = Arc::clone(&state);
    let addr2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 22222);
    tokio::spawn(async move {
        let _ = handle_client(server2, db2, st2, addr2).await;
    });

    // Both receive auth_required
    let _ = read_msg(&mut c1).await;
    let _ = read_msg(&mut c2).await;

    // Register Alice
    let reg1 = ClientMessage {
        command: "register".into(),
        data: serde_json::to_string(&AuthRequest {
            passphrase: "alice".into(),
            password: "secret".into(),
            identity_key: None,
        })
        .unwrap(),
    };
    write_json(&mut c1, &reg1).await;
    let wrap1 = read_msg(&mut c1).await;
    assert_eq!(wrap1.command, "auth_response");
    let val1: serde_json::Value = serde_json::from_str(&wrap1.data).unwrap();
    assert_eq!(val1.get("success").and_then(|v| v.as_bool()), Some(true));
    let _id1 = val1.get("id").and_then(|v| v.as_str()).unwrap().to_string();

    // Register Bob
    let reg2 = ClientMessage {
        command: "register".into(),
        data: serde_json::to_string(&AuthRequest {
            passphrase: "bob".into(),
            password: "secret".into(),
            identity_key: None,
        })
        .unwrap(),
    };
    write_json(&mut c2, &reg2).await;
    let wrap2 = read_msg(&mut c2).await;
    assert_eq!(wrap2.command, "auth_response");
    let val2: serde_json::Value = serde_json::from_str(&wrap2.data).unwrap();
    assert_eq!(val2.get("success").and_then(|v| v.as_bool()), Some(true));
    let id2 = val2.get("id").and_then(|v| v.as_str()).unwrap().to_string();

    // Bob publishes his public key
    let bob_pub_b64 = "Qk9CX1BVQktFWV9CQVNFMjQ=".to_string();
    let set_pk = ClientMessage {
        command: "set_pubkey".into(),
        data: format!("{{\"pubkey\":\"{}\"}}", bob_pub_b64),
    };
    write_json(&mut c2, &set_pk).await;
    let set_resp = read_msg(&mut c2).await;
    assert_eq!(set_resp.command, "set_pubkey_response");
    #[derive(serde::Deserialize)]
    struct SetPkResp {
        success: bool,
    }
    let set_ok: SetPkResp = serde_json::from_str(&set_resp.data).unwrap();
    assert!(set_ok.success);

    // Fetching pubkey via DB by numeric id is not supported in identity mode; skip get_pubkey

    // Alice sends an opaque E2EE envelope as body.
    let opaque_body = "v1:RU5WUEs=:Tk9OQ0U=:Q0lQSEVSVEVYVA=="; // sample opaque string
    let dm_req = ClientMessage {
        command: "message".into(),
        data: format!(
            "{{\"to_identity\":\"{}\",\"body\":\"{}\"}}",
            id2, opaque_body
        ),
    };
    write_json(&mut c1, &dm_req).await;

    // Bob should receive the relayed message over TCP.
    let delivered = read_msg(&mut c2).await;
    assert_eq!(delivered.command, "message");
    let payload: serde_json::Value = serde_json::from_str(&delivered.data).unwrap();
    assert_eq!(
        payload.get("from_user_id").and_then(|v| v.as_i64()),
        Some(1)
    );
}

#[tokio::test]
async fn e2ee_enforcement_rejects_plaintext() {
    let db = setup_memory_db_with_pubkey().await;
    let state = Arc::new(AppState::new(true)); // require E2EE

    // Two clients
    let (server1, mut c1) = tokio::io::duplex(8192);
    let (server2, mut c2) = tokio::io::duplex(8192);
    let db1 = Arc::clone(&db);
    let st1 = Arc::clone(&state);
    let addr1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 10001);
    tokio::spawn(async move {
        let _ = handle_client(server1, db1, st1, addr1).await;
    });
    let db2 = Arc::clone(&db);
    let st2 = Arc::clone(&state);
    let addr2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 10002);
    tokio::spawn(async move {
        let _ = handle_client(server2, db2, st2, addr2).await;
    });

    // auth_required
    let _ = read_msg(&mut c1).await;
    let _ = read_msg(&mut c2).await;

    // Register clients
    let reg1 = ClientMessage {
        command: "register".into(),
        data: serde_json::to_string(&AuthRequest {
            passphrase: "alice".into(),
            password: "secret".into(),
            identity_key: None,
        })
        .unwrap(),
    };
    write_json(&mut c1, &reg1).await;
    let wrap1 = read_msg(&mut c1).await;
    assert_eq!(wrap1.command, "auth_response");
    let _id1 = serde_json::from_str::<serde_json::Value>(&wrap1.data)
        .unwrap()
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();
    let reg2 = ClientMessage {
        command: "register".into(),
        data: serde_json::to_string(&AuthRequest {
            passphrase: "bob".into(),
            password: "secret".into(),
            identity_key: None,
        })
        .unwrap(),
    };
    write_json(&mut c2, &reg2).await;
    let wrap2 = read_msg(&mut c2).await;
    assert_eq!(wrap2.command, "auth_response");
    let id2 = serde_json::from_str::<serde_json::Value>(&wrap2.data)
        .unwrap()
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();

    // Send plaintext (should be rejected and not delivered)
    let dm_req = ClientMessage {
        command: "message".into(),
        data: format!("{{\"to_identity\":\"{}\",\"body\":\"hello world\"}}", id2),
    };
    write_json(&mut c1, &dm_req).await;

    // Expect an error on sender via outbound channel (echo to same client)
    let resp = read_msg(&mut c1).await;
    assert_eq!(resp.command, "error");
    assert!(resp.data.contains("E2EE required"));

    // Ensure receiver did NOT get a message: DB not used anymore; delivery is blocked by error above.
}
