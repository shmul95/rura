use rusqlite::Connection;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::auth::handle_auth;
use crate::models::client_message::ClientMessage;
use crate::utils::db_utils::log_client_connection;

async fn handle_message_success(
    stream: &mut tokio::net::TcpStream,
    client_addr: SocketAddr,
    user_id: i64,
    msg: ClientMessage,
) -> tokio::io::Result<()> {
    println!(
        "Received from authenticated user {} ({}): {:?}",
        user_id, client_addr, msg
    );
    // Echo back the same message (you can modify this to handle different commands)
    let response = serde_json::to_string(&msg)? + "\n";
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

async fn handle_message_parse_error(
    stream: &mut tokio::net::TcpStream,
    client_addr: SocketAddr,
    e: serde_json::Error,
) -> tokio::io::Result<()> {
    eprintln!("Invalid JSON from {}: {}", client_addr, e);
    let error_msg = ClientMessage {
        command: "error".to_string(),
        data: "Invalid JSON".to_string(),
    };
    let response = serde_json::to_string(&error_msg)? + "\n";
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

async fn handle_connection_closed(client_addr: SocketAddr) {
    println!("Connection closed by {}", client_addr);
}

async fn handle_unauthenticated_message(
    stream: &mut tokio::net::TcpStream,
    conn: Arc<Mutex<Connection>>,
    client_addr: SocketAddr,
    msg: ClientMessage,
    authenticated_user_id: &mut Option<i64>,
) -> tokio::io::Result<()> {
    if let Some(user_id) = handle_auth(stream, Arc::clone(&conn), client_addr, &msg).await? {
        *authenticated_user_id = Some(user_id);
    }
    Ok(())
}

async fn handle_unauthenticated_parse_error(
    stream: &mut tokio::net::TcpStream,
    client_addr: SocketAddr,
    e: serde_json::Error,
) -> tokio::io::Result<()> {
    eprintln!("Invalid JSON from {}: {}", client_addr, e);
    let error_msg = ClientMessage {
        command: "error".to_string(),
        data: "Invalid JSON format".to_string(),
    };
    let response = serde_json::to_string(&error_msg)? + "\n";
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

async fn handle_read_error(client_addr: SocketAddr, e: std::io::Error) {
    eprintln!("Error reading from {}: {}", client_addr, e);
}

async fn handle_client_message(
    stream: &mut tokio::net::TcpStream,
    client_addr: SocketAddr,
    user_id: i64,
    buffer: &[u8],
) -> tokio::io::Result<()> {
    let received = String::from_utf8_lossy(buffer).to_string();
    // Try to parse incoming data as JSON
    match serde_json::from_str::<ClientMessage>(&received) {
        Ok(msg) => handle_message_success(stream, client_addr, user_id, msg).await,
        Err(e) => handle_message_parse_error(stream, client_addr, e).await,
    }
}

async fn handle_client_loop(
    stream: &mut tokio::net::TcpStream,
    conn: Arc<Mutex<Connection>>,
    client_addr: SocketAddr,
) -> tokio::io::Result<()> {
    let mut buffer = [0; 1024];
    let mut authenticated_user_id: Option<i64> = None;

    loop {
        match stream.read(&mut buffer).await {
            Ok(0) => {
                handle_connection_closed(client_addr).await;
                break;
            }
            Ok(n) => {
                if authenticated_user_id.is_none() {
                    // User not authenticated, only allow auth commands
                    let received = String::from_utf8_lossy(&buffer[..n]).to_string();
                    match serde_json::from_str::<ClientMessage>(&received) {
                        Ok(msg) => {
                            handle_unauthenticated_message(
                                stream,
                                Arc::clone(&conn),
                                client_addr,
                                msg,
                                &mut authenticated_user_id,
                            )
                            .await?
                        }
                        Err(e) => {
                            handle_unauthenticated_parse_error(stream, client_addr, e).await?
                        }
                    }
                } else {
                    // User is authenticated, allow normal communication
                    handle_client_message(
                        stream,
                        client_addr,
                        authenticated_user_id.unwrap(),
                        &buffer[..n],
                    )
                    .await?;
                }
            }
            Err(e) => {
                handle_read_error(client_addr, e).await;
                break;
            }
        }
    }
    Ok(())
}

pub async fn handle_client(
    mut stream: tokio::net::TcpStream,
    conn: Arc<Mutex<Connection>>,
) -> tokio::io::Result<()> {
    let client_addr = stream.peer_addr()?;

    // Log client connection to SQLite
    log_client_connection(Arc::clone(&conn), client_addr)
        .await
        .unwrap_or_else(|e| {
            eprintln!("Failed to log connection: {}", e);
        });

    // Send initial authentication request
    let auth_prompt = ClientMessage {
        command: "auth_required".to_string(),
        data: "Please authenticate by sending 'login' or 'register' command with your credentials"
            .to_string(),
    };
    let response = serde_json::to_string(&auth_prompt)? + "\n";
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;

    // Handle client authentication and subsequent messages
    handle_client_loop(&mut stream, Arc::clone(&conn), client_addr).await?;
    Ok(())
}
