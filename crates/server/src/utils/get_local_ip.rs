use std::net::UdpSocket;

pub fn get_local_ip() -> Option<String> {
    // Connect to a public address (no traffic actually sent) to discover the outbound IP
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let local_addr = socket.local_addr().ok()?;
    Some(local_addr.ip().to_string())
}
