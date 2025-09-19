use rusqlite::{Connection};
use std::net::{SocketAddr};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::models::client_message::ClientMessage;
use crate::utils::db_utils::log_client_connection;

async fn handle_client_message(
    stream: &mut tokio::net::TcpStream,
    client_addr: SocketAddr,
    buffer: &[u8]
) -> tokio::io::Result<()> {
    let received = String::from_utf8_lossy(&buffer).to_string();
    // Try to parse incoming data as JSON
    match serde_json::from_str::<ClientMessage>(&received) {
        Ok(msg) => {
            println!("Received from {}: {:?}", client_addr, msg);
            // Echo back the same message
            let response = serde_json::to_string(&msg)? + "\n";
            stream.write_all(response.as_bytes()).await?;
            stream.flush().await?;
        }
        Err(e) => {
            eprintln!("Invalid JSON from {}: {}", client_addr, e);
            let error_msg = ClientMessage {
                command: "error".to_string(),
                data: "Invalid JSON".to_string(),
            };
            let response = serde_json::to_string(&error_msg)? + "\n";
            stream.write_all(response.as_bytes()).await?;
            stream.flush().await?;
        }
    }
    Ok(())
}

async fn handle_client_loop(
    stream: &mut tokio::net::TcpStream,
    client_addr: SocketAddr
) -> tokio::io::Result<()> {
    let mut buffer = [0; 1024];

    loop {
        match stream.read(&mut buffer).await {
            Ok(0) => {
                println!("Connection closed by {}", client_addr);
                break;
            }
            Ok(n) => {
                handle_client_message(stream, client_addr, &buffer[..n]).await?;
            }
            Err(e) => {
                eprintln!("Error reading from {}: {}", client_addr, e);
                break;
            }
        }
    }
    Ok(())
}

pub async fn handle_client(
    mut stream: tokio::net::TcpStream,
    conn: Arc<Mutex<Connection>>
) -> tokio::io::Result<()> {
    let client_addr = stream.peer_addr()?;

    // Log client connection to SQLite
    log_client_connection(Arc::clone(&conn), client_addr).await.unwrap_or_else(|e| {
        eprintln!("Failed to log connection: {}", e);
    });

    // Send initial JSON greeting
    let greeting = ClientMessage {
        command: "greeting".to_string(),
        data: "Hello, World!".to_string(),
    };
    let response = serde_json::to_string(&greeting)? + "\n";
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;

    // Read client messages (JSON)
    handle_client_loop(&mut stream, client_addr).await?;
    Ok(())
}
