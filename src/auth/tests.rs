use crate::auth::handlers::*;
use crate::models::client_message::{ClientMessage, AuthRequest, AuthResponse};
use crate::utils::db_utils::register_user;
use rusqlite::Connection;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream};

fn test_socket_addr() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080)
}

async fn create_test_db() -> Arc<Mutex<Connection>> {
    let conn = Connection::open(":memory:").unwrap();
    
    // Create tables
    conn.execute(
        "CREATE TABLE users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            passphrase TEXT NOT NULL UNIQUE,
            password TEXT NOT NULL
        )",
        [],
    ).unwrap();
    
    Arc::new(Mutex::new(conn))
}

async fn create_mock_stream_pair() -> (tokio::net::TcpStream, tokio::net::TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    
    let client_stream = TcpStream::connect(addr).await.unwrap();
    let (server_stream, _) = listener.accept().await.unwrap();
    
    (server_stream, client_stream)
}

async fn read_response(stream: &mut TcpStream) -> String {
    let mut buffer = [0; 1024];
    let n = stream.read(&mut buffer).await.unwrap();
    String::from_utf8_lossy(&buffer[..n]).trim().to_string()
}

#[tokio::test]
async fn test_invalid_command_returns_error() {
    let (mut server_stream, mut client_stream) = create_mock_stream_pair().await;
    let conn = create_test_db().await;
    let client_addr = test_socket_addr();

    let invalid_message = ClientMessage {
        command: "greetings".to_string(),
        data: "Hello!".to_string(),
    };

    // Process with handle_auth (this will send response to server_stream)
    let result = handle_auth(&mut server_stream, conn, client_addr, &invalid_message).await.unwrap();
    assert_eq!(result, None); // Should return None (not authenticated)

    // Read and verify the error response from client_stream
    let response = read_response(&mut client_stream).await;
    let response_msg: ClientMessage = serde_json::from_str(&response).unwrap();
    assert_eq!(response_msg.command, "error");
    assert_eq!(response_msg.data, "Authentication required. Please send 'login' or 'register' command first");
}

#[tokio::test]
async fn test_register_new_user_success() {
    let (mut server_stream, mut client_stream) = create_mock_stream_pair().await;
    let conn = create_test_db().await;
    let client_addr = test_socket_addr();

    let register_message = ClientMessage {
        command: "register".to_string(),
        data: serde_json::to_string(&AuthRequest {
            passphrase: "testuser".to_string(),
            password: "testpass".to_string(),
        }).unwrap(),
    };

    // Process registration
    let result = handle_auth(&mut server_stream, conn, client_addr, &register_message).await.unwrap();
    assert!(result.is_some()); // Should return user ID

    // Read and verify the success response
    let response = read_response(&mut client_stream).await;
    let response_wrapper: ClientMessage = serde_json::from_str(&response).unwrap();
    assert_eq!(response_wrapper.command, "auth_response");
    
    let response_msg: AuthResponse = serde_json::from_str(&response_wrapper.data).unwrap();
    assert!(response_msg.success);
    assert_eq!(response_msg.message, "Registration successful");
    assert!(response_msg.user_id.is_some());
}

#[tokio::test]
async fn test_login_valid_user_success() {
    let (mut server_stream, mut client_stream) = create_mock_stream_pair().await;
    let conn = create_test_db().await;
    let client_addr = test_socket_addr();

    // First register a user
    let user_id = register_user(
        Arc::clone(&conn), 
        "testuser", 
        "testpass"
    ).await.unwrap();

    let login_message = ClientMessage {
        command: "login".to_string(),
        data: serde_json::to_string(&AuthRequest {
            passphrase: "testuser".to_string(),
            password: "testpass".to_string(),
        }).unwrap(),
    };

    // Process login
    let result = handle_auth(&mut server_stream, conn, client_addr, &login_message).await.unwrap();
    assert_eq!(result, Some(user_id)); // Should return the correct user ID

    // Read and verify the success response
    let response = read_response(&mut client_stream).await;
    let response_wrapper: ClientMessage = serde_json::from_str(&response).unwrap();
    assert_eq!(response_wrapper.command, "auth_response");
    
    let response_msg: AuthResponse = serde_json::from_str(&response_wrapper.data).unwrap();
    assert!(response_msg.success);
    assert_eq!(response_msg.message, "Authentication successful");
    assert_eq!(response_msg.user_id, Some(user_id));
}

#[tokio::test]
async fn test_login_invalid_credentials_error() {
    let (mut server_stream, mut client_stream) = create_mock_stream_pair().await;
    let conn = create_test_db().await;
    let client_addr = test_socket_addr();

    // First register a user
    register_user(
        Arc::clone(&conn), 
        "testuser", 
        "testpass"
    ).await.unwrap();

    let login_message = ClientMessage {
        command: "login".to_string(),
        data: serde_json::to_string(&AuthRequest {
            passphrase: "testuser".to_string(),
            password: "wrongpass".to_string(),
        }).unwrap(),
    };

    // Process login with wrong password
    let result = handle_auth(&mut server_stream, conn, client_addr, &login_message).await.unwrap();
    assert_eq!(result, None); // Should return None (failed)

    // Read and verify the error response
    let response = read_response(&mut client_stream).await;
    let response_wrapper: ClientMessage = serde_json::from_str(&response).unwrap();
    assert_eq!(response_wrapper.command, "auth_response");
    
    let response_msg: AuthResponse = serde_json::from_str(&response_wrapper.data).unwrap();
    assert!(!response_msg.success);
    assert!(response_msg.message.contains("Invalid"));
    assert_eq!(response_msg.user_id, None);
}
