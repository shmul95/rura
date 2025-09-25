use std::net::SocketAddr;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

use crate::models::client_message::ClientMessage;

pub(super) async fn handle_connection_closed(client_addr: SocketAddr) {
    println!("Connection closed by {}", client_addr);
}

pub(super) async fn handle_read_error(client_addr: SocketAddr, e: std::io::Error) {
    eprintln!("Error reading from {}: {}", client_addr, e);
}

pub(super) async fn writer_task(
    mut write_stream: tokio::net::TcpStream,
    mut rx: mpsc::UnboundedReceiver<ClientMessage>,
) {
    while let Some(msg) = rx.recv().await {
        if let Ok(mut json) = serde_json::to_string(&msg) {
            json.push('\n');
            if write_stream.write_all(json.as_bytes()).await.is_err() {
                break;
            }
            let _ = write_stream.flush().await;
        }
    }
}
