use rusqlite::Connection;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::io::AsyncWriteExt;

use crate::auth::handle_auth;
use crate::models::client_message::ClientMessage;

pub(super) async fn handle_unauthenticated_message(
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

pub(super) async fn handle_unauthenticated_parse_error(
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
