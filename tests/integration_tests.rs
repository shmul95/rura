use conduit::auth::handlers::handle_auth;
use conduit::models::client_message::{AuthRequest, AuthResponse, ClientMessage};
use conduit::utils::db_utils::register_user;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, DuplexStream, duplex};

fn test_socket_addr() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080)
}

async fn create_test_db() -> Arc<Mutex<rusqlite::Connection>> {
    let conn = rusqlite::Connection::open(":memory:").unwrap();

    conn.execute(
        "CREATE TABLE users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            passphrase TEXT NOT NULL UNIQUE,
            password TEXT NOT NULL
        )",
        [],
    )
    .unwrap();

    Arc::new(Mutex::new(conn))
}

fn stream_pair() -> (DuplexStream, DuplexStream) {
    duplex(2048)
}

async fn read_message(stream: &mut DuplexStream) -> ClientMessage {
    let mut buffer = [0; 1024];
    let n = stream.read(&mut buffer).await.unwrap();
    let raw = String::from_utf8_lossy(&buffer[..n]).trim().to_string();
    serde_json::from_str(&raw).unwrap()
}

#[tokio::test]
async fn test_complete_client_server_auth_flow() {
    let (mut server_stream, mut client_stream) = stream_pair();
    let db = create_test_db().await;
    let client_addr = test_socket_addr();

    let greetings_msg = ClientMessage {
        command: "greetings".to_string(),
        data: "Hello!".to_string(),
    };
    let result = handle_auth(
        &mut server_stream,
        Arc::clone(&db),
        client_addr,
        &greetings_msg,
    )
    .await
    .unwrap();
    assert!(result.is_none());
    let error_msg: ClientMessage = read_message(&mut client_stream).await;
    assert_eq!(error_msg.command, "error");

    let register_msg = ClientMessage {
        command: "register".to_string(),
        data: serde_json::to_string(&AuthRequest {
            passphrase: "alice".to_string(),
            password: "secret123".to_string(),
        })
        .unwrap(),
    };
    let register_result = handle_auth(
        &mut server_stream,
        Arc::clone(&db),
        client_addr,
        &register_msg,
    )
    .await
    .unwrap();
    assert!(register_result.is_some());
    let register_wrapper: ClientMessage = read_message(&mut client_stream).await;
    let register_resp: AuthResponse = serde_json::from_str(&register_wrapper.data).unwrap();
    assert!(register_resp.success);
    let user_id = register_resp.user_id.unwrap();

    let login_msg = ClientMessage {
        command: "login".to_string(),
        data: serde_json::to_string(&AuthRequest {
            passphrase: "alice".to_string(),
            password: "secret123".to_string(),
        })
        .unwrap(),
    };
    let login_result = handle_auth(&mut server_stream, Arc::clone(&db), client_addr, &login_msg)
        .await
        .unwrap();
    assert_eq!(login_result, Some(user_id));
    let login_wrapper: ClientMessage = read_message(&mut client_stream).await;
    let login_resp: AuthResponse = serde_json::from_str(&login_wrapper.data).unwrap();
    assert!(login_resp.success);
    assert_eq!(login_resp.user_id, Some(user_id));
}

#[tokio::test]
async fn test_registration_then_login_different_sessions() {
    let db = create_test_db().await;
    let client_addr = test_socket_addr();

    let (mut registration_stream, mut registration_client) = stream_pair();
    let register_msg = ClientMessage {
        command: "register".to_string(),
        data: serde_json::to_string(&AuthRequest {
            passphrase: "bob".to_string(),
            password: "mypassword".to_string(),
        })
        .unwrap(),
    };
    let register_result = handle_auth(
        &mut registration_stream,
        Arc::clone(&db),
        client_addr,
        &register_msg,
    )
    .await
    .unwrap();
    assert!(register_result.is_some());
    let register_wrapper: ClientMessage = read_message(&mut registration_client).await;
    let register_resp: AuthResponse = serde_json::from_str(&register_wrapper.data).unwrap();
    let user_id = register_resp.user_id.unwrap();

    let (mut login_stream, mut login_client) = stream_pair();
    let login_msg = ClientMessage {
        command: "login".to_string(),
        data: serde_json::to_string(&AuthRequest {
            passphrase: "bob".to_string(),
            password: "mypassword".to_string(),
        })
        .unwrap(),
    };
    let login_result = handle_auth(&mut login_stream, Arc::clone(&db), client_addr, &login_msg)
        .await
        .unwrap();
    assert_eq!(login_result, Some(user_id));
    let login_wrapper: ClientMessage = read_message(&mut login_client).await;
    let login_resp: AuthResponse = serde_json::from_str(&login_wrapper.data).unwrap();
    assert!(login_resp.success);
    assert_eq!(login_resp.user_id, Some(user_id));
}

#[tokio::test]
async fn test_multiple_failed_login_attempts() {
    let db = create_test_db().await;
    let client_addr = test_socket_addr();

    register_user(Arc::clone(&db), "charlie", "supersecret")
        .await
        .unwrap();

    let (mut stream, mut client_side) = stream_pair();
    for _ in 0..3 {
        let login_msg = ClientMessage {
            command: "login".to_string(),
            data: serde_json::to_string(&AuthRequest {
                passphrase: "charlie".to_string(),
                password: "wrong".to_string(),
            })
            .unwrap(),
        };
        let result = handle_auth(&mut stream, Arc::clone(&db), client_addr, &login_msg)
            .await
            .unwrap();
        assert!(result.is_none());
        let wrapper: ClientMessage = read_message(&mut client_side).await;
        let resp: AuthResponse = serde_json::from_str(&wrapper.data).unwrap();
        assert!(!resp.success);
    }
}
