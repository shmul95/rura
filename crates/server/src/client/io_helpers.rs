use std::net::SocketAddr;

pub(super) async fn handle_connection_closed(client_addr: SocketAddr) {
    println!("Connection closed by {}", client_addr);
}

pub(super) async fn handle_read_error(client_addr: SocketAddr, e: std::io::Error) {
    eprintln!("Error reading from {}: {}", client_addr, e);
}

