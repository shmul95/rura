use std::net::UdpSocket;

pub fn get_local_ip() -> Option<String> {
    // Connect to a public address (no traffic actually sent) to discover the outbound IP
    if let Ok(socket) = UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(local_addr) = socket.local_addr() {
                return Some(local_addr.ip().to_string());
            }
        }
    }
    None
}
