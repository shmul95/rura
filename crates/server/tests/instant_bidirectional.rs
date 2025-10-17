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
async fn instant_bidirectional_delivery() {
    let db = setup_memory_db().await;
    let state = Arc::new(AppState::default());

    // Spawn server loop on localhost
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
    let mut a = TcpStream::connect(addr).await.unwrap();
    let mut b = TcpStream::connect(addr).await.unwrap();

    // Initial auth_required prompts
    let _ = read_msg(&mut a).await;
    let _ = read_msg(&mut b).await;

    // Register Alice
    write_json(
        &mut a,
        &ClientMessage {
            command: "register".into(),
            data: serde_json::to_string(&AuthRequest {
                passphrase: "alice".into(),
                password: "secret".into(),
            })
            .unwrap(),
        },
    )
    .await;
    let wrap1 = read_msg(&mut a).await;
    let resp1: AuthResponse = serde_json::from_str(&wrap1.data).unwrap();
    assert!(resp1.success);
    let alice = resp1.user_id.unwrap();

    // Register Bob
    write_json(
        &mut b,
        &ClientMessage {
            command: "register".into(),
            data: serde_json::to_string(&AuthRequest {
                passphrase: "bob".into(),
                password: "secret".into(),
            })
            .unwrap(),
        },
    )
    .await;
    let wrap2 = read_msg(&mut b).await;
    let resp2: AuthResponse = serde_json::from_str(&wrap2.data).unwrap();
    assert!(resp2.success);
    let bob = resp2.user_id.unwrap();

    // A -> B instant
    write_json(
        &mut a,
        &ClientMessage {
            command: "message".into(),
            data: format!("{{\"to_user_id\":{bob},\"body\":\"hi bob\"}}"),
        },
    )
    .await;
    let delivered_to_b = read_msg(&mut b).await;
    assert_eq!(delivered_to_b.command, "message");

    // B -> A instant
    write_json(
        &mut b,
        &ClientMessage {
            command: "message".into(),
            data: format!("{{\"to_user_id\":{alice},\"body\":\"hi alice\"}}"),
        },
    )
    .await;
    let delivered_to_a = read_msg(&mut a).await;
    assert_eq!(delivered_to_a.command, "message");
}

#[tokio::test]
async fn relogin_overwrites_online_route_latest_wins() {
    let db = setup_memory_db().await;
    let state = Arc::new(AppState::default());
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

    // Two sockets for the same user (alice_old and alice_new)
    let mut alice_old = TcpStream::connect(addr).await.unwrap();
    let mut alice_new = TcpStream::connect(addr).await.unwrap();
    let mut bob = TcpStream::connect(addr).await.unwrap();
    let _ = read_msg(&mut alice_old).await;
    let _ = read_msg(&mut alice_new).await;
    let _ = read_msg(&mut bob).await;

    // Register alice on first socket
    write_json(
        &mut alice_old,
        &ClientMessage {
            command: "register".into(),
            data: serde_json::to_string(&AuthRequest {
                passphrase: "alice".into(),
                password: "secret".into(),
            })
            .unwrap(),
        },
    )
    .await;
    let wrap1 = read_msg(&mut alice_old).await;
    let resp1: AuthResponse = serde_json::from_str(&wrap1.data).unwrap();
    let alice_id = resp1.user_id.unwrap();

    // Register bob
    write_json(
        &mut bob,
        &ClientMessage {
            command: "register".into(),
            data: serde_json::to_string(&AuthRequest {
                passphrase: "bob".into(),
                password: "secret".into(),
            })
            .unwrap(),
        },
    )
    .await;
    let _ = read_msg(&mut bob).await; // auth_response

    // Login alice again on second socket (this overwrites outbound route)
    write_json(
        &mut alice_new,
        &ClientMessage {
            command: "login".into(),
            data: serde_json::to_string(&AuthRequest {
                passphrase: "alice".into(),
                password: "secret".into(),
            })
            .unwrap(),
        },
    )
    .await;
    let _ = read_msg(&mut alice_new).await; // auth_response

    // Bob sends a message to alice: only the latest session should receive it
    write_json(
        &mut bob,
        &ClientMessage {
            command: "message".into(),
            data: format!("{{\"to_user_id\":{alice_id},\"body\":\"hi alice again\"}}"),
        },
    )
    .await;

    // Expect delivery on alice_new, not on alice_old
    let delivered_new = read_msg(&mut alice_new).await;
    assert_eq!(delivered_new.command, "message");
}
