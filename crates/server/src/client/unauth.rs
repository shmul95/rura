use rusqlite::Connection;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncWrite, AsyncWriteExt};

use crate::auth::{handle_auth_command_error, handle_auth_login, handle_auth_register};
use crate::models::client_message::ClientMessage;

pub(super) async fn handle_unauthenticated_message<W>(
    stream: &mut W,
    conn: Arc<Mutex<Connection>>,
    client_addr: SocketAddr,
    msg: ClientMessage,
    authenticated_user_id: &mut Option<i64>,
) -> tokio::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    match msg.command.as_str() {
        "login" => {
            *authenticated_user_id = handle_auth_login(stream, conn, client_addr, &msg).await?;
        }
        "register" => {
            *authenticated_user_id = handle_auth_register(stream, conn, client_addr, &msg).await?;
        }
        _ => {
            *authenticated_user_id = handle_auth_command_error(stream).await?;
        }
    }
    Ok(())
}

pub(super) async fn handle_unauthenticated_parse_error<W>(
    stream: &mut W,
    client_addr: SocketAddr,
    e: serde_json::Error,
) -> tokio::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    eprintln!("Invalid JSON from {}: {}", client_addr, e);
    let error_msg = ClientMessage {
        command: "error".to_string(),
        data: "Invalid JSON".to_string(),
    };
    let response = serde_json::to_string(&error_msg)? + "\n";
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await
}
