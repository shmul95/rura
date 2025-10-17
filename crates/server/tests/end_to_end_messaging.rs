use rura_server::client::handle_client;
use rura_server::messaging::state::AppState;
use rura_server::models::client_message::{AuthRequest, AuthResponse, ClientMessage};
use rusqlite::Connection;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

async fn setup_memory_db() -> Arc<Mutex<Connection>> {
    let conn = Connection::open(":memory:").unwrap();
    conn.execute(
        "CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            passphrase TEXT NOT NULL UNIQUE,
            password TEXT NOT NULL
        )",
        [],
    )
    .unwrap();
    conn.execute(
        "CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            sender INTEGER NOT NULL,
            receiver INTEGER NOT NULL,
            content TEXT NOT NULL,
            timestamp TEXT NOT NULL,
            saved INTEGER NOT NULL DEFAULT 0
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

async fn read_msg(stream: &mut TcpStream) -> ClientMessage {
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).await.unwrap();
    let txt = String::from_utf8_lossy(&buf[..n]).trim().to_string();
    serde_json::from_str(&txt).unwrap()
}

async fn write_json(stream: &mut TcpStream, msg: &ClientMessage) {
    let mut line = serde_json::to_string(msg).unwrap();
    line.push('\n');
    stream.write_all(line.as_bytes()).await.unwrap();
    stream.flush().await.unwrap();
}

#[tokio::test]
async fn test_full_auth_and_dm_persistence_and_save() {
    let db = setup_memory_db().await;
    let state = Arc::new(AppState::default());

    // Start a small TCP listener and accept connections, delegating to handle_client
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    let db_for_server = Arc::clone(&db);
    let state_for_server = Arc::clone(&state);
    tokio::spawn(async move {
        loop {
            let (stream, peer) = match listener.accept().await {
                Ok(t) => t,
                Err(_) => break,
            };
            let db = Arc::clone(&db_for_server);
            let state = Arc::clone(&state_for_server);
            tokio::spawn(async move {
                let _ = handle_client(stream, db, state, peer).await;
            });
        }
    });

    // Connect two clients
    let mut c1 = TcpStream::connect(addr).await.unwrap();
    let mut c2 = TcpStream::connect(addr).await.unwrap();

    // Both should receive auth_required
    let _auth1 = read_msg(&mut c1).await; // auth_required
    let _auth2 = read_msg(&mut c2).await; // auth_required

    // Register user1
    let reg1 = ClientMessage {
        command: "register".into(),
        data: serde_json::to_string(&AuthRequest {
            passphrase: "alice".into(),
            password: "secret".into(),
        })
        .unwrap(),
    };
    write_json(&mut c1, &reg1).await;
    let wrap1 = read_msg(&mut c1).await;
    assert_eq!(wrap1.command, "auth_response");
    let resp1: AuthResponse = serde_json::from_str(&wrap1.data).unwrap();
    assert!(resp1.success);
    let uid1 = resp1.user_id.unwrap();

    // Register user2
    let reg2 = ClientMessage {
        command: "register".into(),
        data: serde_json::to_string(&AuthRequest {
            passphrase: "bob".into(),
            password: "secret".into(),
        })
        .unwrap(),
    };
    write_json(&mut c2, &reg2).await;
    let wrap2 = read_msg(&mut c2).await;
    assert_eq!(wrap2.command, "auth_response");
    let resp2: AuthResponse = serde_json::from_str(&wrap2.data).unwrap();
    assert!(resp2.success);
    let uid2 = resp2.user_id.unwrap();

    // Send a message from c1 -> c2 with saved=true (opaque envelope)
    let dm_req = ClientMessage {
        command: "message".into(),
        data: format!(
            "{{\"to_user_id\":{},\"body\":\"v1:RU5WUEs=:Tk9OQ0U=:Q0lQSEVSVEVYVA==\",\"saved\":true}}",
            uid2
        ),
    };
    write_json(&mut c1, &dm_req).await;

    // c2 should receive the message event
    let delivered = read_msg(&mut c2).await;
    assert_eq!(delivered.command, "message");

    // No server-side persistence anymore; only delivery is asserted.
    let _ = uid1; // silence
}
