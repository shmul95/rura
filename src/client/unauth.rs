use rusqlite::Connection;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncWrite, AsyncWriteExt};

use crate::auth::handle_auth;
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
    if let Some(user_id) = handle_auth(stream, Arc::clone(&conn), client_addr, &msg).await? {
        *authenticated_user_id = Some(user_id);
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
        data: "Invalid JSON format".to_string(),
    };
    let response = serde_json::to_string(&error_msg)? + "\n";
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn test_unauth_invalid_json_sends_error() {
        // Create a loopback TCP pair
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Accept in background
        let accept_fut = tokio::spawn(async move { listener.accept().await.unwrap().0 });

        // Connect client side
        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();

        // Get server side stream
        let mut server_stream = accept_fut.await.unwrap();

        // Invoke the error handler
        let parse_err =
            serde_json::from_str::<crate::models::client_message::ClientMessage>("not json")
                .unwrap_err();
        let client_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080);
        handle_unauthenticated_parse_error(&mut server_stream, client_addr, parse_err)
            .await
            .unwrap();

        // Read from client side
        let mut buf = [0u8; 1024];
        let n = client.read(&mut buf).await.unwrap();
        let raw = String::from_utf8_lossy(&buf[..n]).trim().to_string();
        let msg: crate::models::client_message::ClientMessage = serde_json::from_str(&raw).unwrap();
        assert_eq!(msg.command, "error");
        assert_eq!(msg.data, "Invalid JSON format");
    }
}
