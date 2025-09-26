use rusqlite::Connection;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};

use crate::messaging::state::AppState;
use crate::models::client_message::ClientMessage;
use crate::utils::db_utils::log_client_connection;

mod authed;
mod dispatch;
mod io_helpers;
mod loop_task;
mod unauth;

pub async fn handle_client_with_addr<S>(
    mut stream: S,
    conn: Arc<Mutex<Connection>>,
    state: Arc<AppState>,
    client_addr: SocketAddr,
) -> tokio::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
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
    loop_task::handle_client_loop(&mut stream, Arc::clone(&conn), state, client_addr).await?;
    Ok(())
}

// Back-compat wrapper for call sites expecting a TcpStream and deriving the address internally
pub async fn handle_client(
    stream: tokio::net::TcpStream,
    conn: Arc<Mutex<Connection>>,
    state: Arc<AppState>,
) -> tokio::io::Result<()> {
    let client_addr = stream.peer_addr()?;
    handle_client_with_addr(stream, conn, state, client_addr).await
}
