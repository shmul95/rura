use std::net::SocketAddr;
use tokio::io::AsyncWriteExt;

use crate::models::client_message::ClientMessage;

pub(super) async fn handle_client_message(
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

pub(super) async fn handle_message_success(
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

pub(super) async fn handle_message_parse_error(
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
