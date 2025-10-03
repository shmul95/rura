use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use rura_server::messaging::state::AppState;
use rura_server::models::client_message::{AuthRequest, ClientMessage};
use rura_server::utils::db_utils::store_message;

#[tokio::test]
async fn history_returns_persisted_messages_for_user() {
    // In-memory DB schema
    let conn = Arc::new(Mutex::new(rusqlite::Connection::open(":memory:").unwrap()));
    {
        let c = conn.lock().unwrap();
        c.execute(
            "CREATE TABLE users (id INTEGER PRIMARY KEY AUTOINCREMENT, passphrase TEXT NOT NULL UNIQUE, password TEXT NOT NULL)",
            [],
        ).unwrap();
        c.execute(
            "CREATE TABLE messages (
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
        .unwrap();
        c.execute(
            "CREATE TABLE connections (id INTEGER PRIMARY KEY AUTOINCREMENT, ip TEXT NOT NULL, timestamp TEXT NOT NULL)",
            [],
        ).unwrap();
    }

    let state = Arc::new(AppState::default());

    // Use a duplex stream to run the full client handler
    let (mut server_stream, mut client_stream) = tokio::io::duplex(4096);
    let client_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 12345);
    let h = tokio::spawn(rura_server::client::handle_client(
        server_stream,
        Arc::clone(&conn),
        Arc::clone(&state),
        client_addr,
    ));

    // Read auth_required
    let mut buf = [0u8; 2048];
    let n = client_stream.read(&mut buf).await.unwrap();
    let _ = String::from_utf8_lossy(&buf[..n]).to_string();

    // Register user 1
    let reg = ClientMessage {
        command: "register".to_string(),
        data: serde_json::to_string(&AuthRequest {
            passphrase: "alice".into(),
            password: "secret".into(),
        })
        .unwrap(),
    };
    let mut line = serde_json::to_string(&reg).unwrap();
    line.push('\n');
    client_stream.write_all(line.as_bytes()).await.unwrap();
    client_stream.flush().await.unwrap();

    // Read auth_response
    let n = client_stream.read(&mut buf).await.unwrap();
    let raw = String::from_utf8_lossy(&buf[..n]).trim().to_string();
    let wrap: ClientMessage = serde_json::from_str(&raw).unwrap();
    assert_eq!(wrap.command, "auth_response");

    // Insert a message for user 1 -> 1
    let _ = store_message(Arc::clone(&conn), 1, 1, "hello history", false)
        .await
        .unwrap();

    // Request history
    let req = rura_server::messaging::models::HistoryRequest { limit: Some(50) };
    let msg = ClientMessage {
        command: "history".into(),
        data: serde_json::to_string(&req).unwrap(),
    };
    let mut line = serde_json::to_string(&msg).unwrap();
    line.push('\n');
    client_stream.write_all(line.as_bytes()).await.unwrap();
    client_stream.flush().await.unwrap();

    // Read history_response
    let n = client_stream.read(&mut buf).await.unwrap();
    let raw = String::from_utf8_lossy(&buf[..n]).trim().to_string();
    let wrap: ClientMessage = serde_json::from_str(&raw).unwrap();
    assert_eq!(wrap.command, "history_response");
    let parsed: rura_server::messaging::models::HistoryResponse =
        serde_json::from_str(&wrap.data).unwrap();
    assert!(parsed.success);
    assert!(parsed.messages.iter().any(|m| m.body == "hello history"));

    drop(client_stream);
    let _ = h.await;
}
