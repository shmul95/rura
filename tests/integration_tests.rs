use conduit::auth::handlers::handle_auth;
use conduit::models::client_message::{ClientMessage, AuthRequest, AuthResponse};
use conduit::utils::db_utils::register_user;
use std::sync::{Arc, Mutex};
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{timeout, Duration};

async fn create_test_db() -> Arc<Mutex<rusqlite::Connection>> {
    let conn = rusqlite::Connection::open(":memory:").unwrap();
    
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

async fn setup_test_server() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    (listener, addr)
}

async fn read_response(stream: &mut TcpStream) -> Result<String, Box<dyn std::error::Error>> {
    let mut buffer = [0; 1024];
    let n = timeout(Duration::from_secs(5), stream.read(&mut buffer)).await??;
    Ok(String::from_utf8_lossy(&buffer[..n]).trim().to_string())
}

#[tokio::test]
async fn test_complete_client_server_auth_flow() {
    // This test follows the exact interaction flow provided by the user:
    // S {"command":"auth_required","data":"Please authenticate first"}
    // C {"command":"greetings","data":"Hello!"}  
    // S {"command":"error","data":"Authentication required..."}
    // C {"command":"register","data":"{\"passphrase\":\"alice\",\"password\":\"secret123\"}"}
    // S {"success":true,"message":"Registration successful","user_id":1}
    // C {"command":"login","data":"{\"passphrase\":\"alice\",\"password\":\"secret123\"}"}
    // S {"success":true,"message":"Authentication successful","user_id":1}

    let (listener, _addr) = setup_test_server().await;
    let db = create_test_db().await;
    
    // Simulate client connection
    let client_addr = "127.0.0.1:0".parse().unwrap();
    
    // Accept a connection (simulate)
    let (mut server_stream, mut client_stream) = {
        let listener_addr = listener.local_addr().unwrap();
        
        let client_stream = TcpStream::connect(listener_addr).await.unwrap();
        let (server_stream, _) = listener.accept().await.unwrap();
        
        (server_stream, client_stream)
    };

    // Step 1: Client sends invalid command (greetings)
    let greetings_msg = ClientMessage {
        command: "greetings".to_string(),
        data: "Hello!".to_string(),
    };

    // Process the invalid command through handle_auth
    let auth_result = handle_auth(&mut server_stream, Arc::clone(&db), client_addr, &greetings_msg).await.unwrap();
    assert_eq!(auth_result, None); // Should not authenticate

    // Read the error response that handle_auth should send
    let error_response = read_response(&mut client_stream).await.unwrap();
    let error_msg: ClientMessage = serde_json::from_str(&error_response).unwrap();
    assert_eq!(error_msg.command, "error");
    assert!(error_msg.data.contains("Authentication required"));

    // Step 2: Client registers
    let register_msg = ClientMessage {
        command: "register".to_string(),
        data: serde_json::to_string(&AuthRequest {
            passphrase: "alice".to_string(),
            password: "secret123".to_string(),
        }).unwrap(),
    };

    let register_result = handle_auth(&mut server_stream, Arc::clone(&db), client_addr, &register_msg).await.unwrap();
    assert!(register_result.is_some()); // Should return user ID

    // Read the registration success response
    let register_response = read_response(&mut client_stream).await.unwrap();
    let register_wrapper: ClientMessage = serde_json::from_str(&register_response).unwrap();
    assert_eq!(register_wrapper.command, "auth_response");
    
    let register_resp: AuthResponse = serde_json::from_str(&register_wrapper.data).unwrap();
    assert!(register_resp.success);
    assert_eq!(register_resp.message, "Registration successful");
    assert!(register_resp.user_id.is_some());
    let user_id = register_resp.user_id.unwrap();

    // Step 3: Client logs in with same credentials
    let login_msg = ClientMessage {
        command: "login".to_string(),
        data: serde_json::to_string(&AuthRequest {
            passphrase: "alice".to_string(),
            password: "secret123".to_string(),
        }).unwrap(),
    };

    let login_result = handle_auth(&mut server_stream, Arc::clone(&db), client_addr, &login_msg).await.unwrap();
    assert_eq!(login_result, Some(user_id)); // Should return same user ID

    // Read the login success response
    let login_response = read_response(&mut client_stream).await.unwrap();
    let login_wrapper: ClientMessage = serde_json::from_str(&login_response).unwrap();
    assert_eq!(login_wrapper.command, "auth_response");
    
    let login_resp: AuthResponse = serde_json::from_str(&login_wrapper.data).unwrap();
    assert!(login_resp.success);
    assert_eq!(login_resp.message, "Authentication successful");
    assert_eq!(login_resp.user_id, Some(user_id));

    // At this point, the client is authenticated and can communicate
    // The handle_auth function should not be called for regular messages
    // This demonstrates the complete auth flow works as expected
}

#[tokio::test]
async fn test_registration_then_login_different_sessions() {
    // Test that a user can register in one session and login in another
    
    let db = create_test_db().await;
    let client_addr = "127.0.0.1:0".parse().unwrap();

    // Session 1: Registration
    let (mut server_stream1, mut client_stream1) = {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        
        let client_stream = TcpStream::connect(addr).await.unwrap();
        let (server_stream, _) = listener.accept().await.unwrap();
        
        (server_stream, client_stream)
    };

    let register_msg = ClientMessage {
        command: "register".to_string(),
        data: serde_json::to_string(&AuthRequest {
            passphrase: "bob".to_string(),
            password: "mypassword".to_string(),
        }).unwrap(),
    };

    let register_result = handle_auth(&mut server_stream1, Arc::clone(&db), client_addr, &register_msg).await.unwrap();
    assert!(register_result.is_some());

    let register_response = read_response(&mut client_stream1).await.unwrap();
    let register_wrapper: ClientMessage = serde_json::from_str(&register_response).unwrap();
    let register_resp: AuthResponse = serde_json::from_str(&register_wrapper.data).unwrap();
    assert!(register_resp.success);
    let user_id = register_resp.user_id.unwrap();

    // Session 2: Login (new connection)
    let (mut server_stream2, mut client_stream2) = {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        
        let client_stream = TcpStream::connect(addr).await.unwrap();
        let (server_stream, _) = listener.accept().await.unwrap();
        
        (server_stream, client_stream)
    };

    let login_msg = ClientMessage {
        command: "login".to_string(),
        data: serde_json::to_string(&AuthRequest {
            passphrase: "bob".to_string(),
            password: "mypassword".to_string(),
        }).unwrap(),
    };

    let login_result = handle_auth(&mut server_stream2, Arc::clone(&db), client_addr, &login_msg).await.unwrap();
    assert_eq!(login_result, Some(user_id));

    let login_response = read_response(&mut client_stream2).await.unwrap();
    let login_wrapper: ClientMessage = serde_json::from_str(&login_response).unwrap();
    let login_resp: AuthResponse = serde_json::from_str(&login_wrapper.data).unwrap();
    assert!(login_resp.success);
    assert_eq!(login_resp.user_id, Some(user_id));
}

#[tokio::test]
async fn test_multiple_failed_login_attempts() {
    let db = create_test_db().await;
    let client_addr = "127.0.0.1:0".parse().unwrap();

    // First register a user
    register_user(Arc::clone(&db), "charlie", "correctpass").await.unwrap();

    // Test multiple failed login attempts
    for wrong_password in &["wrong1", "wrong2", "wrong3"] {
        let (mut server_stream, mut client_stream) = {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            
            let client_stream = TcpStream::connect(addr).await.unwrap();
            let (server_stream, _) = listener.accept().await.unwrap();
            
            (server_stream, client_stream)
        };

        let login_msg = ClientMessage {
            command: "login".to_string(),
            data: serde_json::to_string(&AuthRequest {
                passphrase: "charlie".to_string(),
                password: wrong_password.to_string(),
            }).unwrap(),
        };

        let login_result = handle_auth(&mut server_stream, Arc::clone(&db), client_addr, &login_msg).await.unwrap();
        assert_eq!(login_result, None); // Should fail

        let login_response = read_response(&mut client_stream).await.unwrap();
        let login_wrapper: ClientMessage = serde_json::from_str(&login_response).unwrap();
        let login_resp: AuthResponse = serde_json::from_str(&login_wrapper.data).unwrap();
        assert!(!login_resp.success);
        assert!(login_resp.message.contains("Invalid"));
    }

    // Finally, test correct login works
    let (mut server_stream, mut client_stream) = {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        
        let client_stream = TcpStream::connect(addr).await.unwrap();
        let (server_stream, _) = listener.accept().await.unwrap();
        
        (server_stream, client_stream)
    };

    let correct_login_msg = ClientMessage {
        command: "login".to_string(),
        data: serde_json::to_string(&AuthRequest {
            passphrase: "charlie".to_string(),
            password: "correctpass".to_string(),
        }).unwrap(),
    };

    let correct_result = handle_auth(&mut server_stream, Arc::clone(&db), client_addr, &correct_login_msg).await.unwrap();
    assert!(correct_result.is_some());

    let correct_response = read_response(&mut client_stream).await.unwrap();
    let correct_wrapper: ClientMessage = serde_json::from_str(&correct_response).unwrap();
    let correct_resp: AuthResponse = serde_json::from_str(&correct_wrapper.data).unwrap();
    assert!(correct_resp.success);
}
