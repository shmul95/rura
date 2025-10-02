use clap::Parser;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

use rura::client::handle_client;
use rura::messaging::state::AppState;
use rura::models::args::Args;
use rura::utils::db_utils::init_db;
use rura::utils::get_local_ip::get_local_ip;
use rura::utils::tls::make_tls_acceptor;

#[tokio::main]
async fn main() -> tokio::io::Result<()> {
    // Parse command-line arguments
    let args = Args::parse();
    let bind_addr = format!("0.0.0.0:{}", args.port);

    // Get and display local IP
    let local_ip = get_local_ip().unwrap_or_else(|| "Unknown".to_string());
    println!("Server's local IP address: {}", local_ip);

    // Initialize SQLite database
    let conn = Arc::new(Mutex::new(init_db().expect("Failed to init the db")));

    // Initialize shared in-memory state (online users)
    let state = Arc::new(AppState::default());

    // Build TLS acceptor (TLS-only server)
    let tls_acceptor: TlsAcceptor = make_tls_acceptor(&args.tls_cert, &args.tls_key)
        .expect("Failed to initialize TLS (check --tls-cert/--tls-key)");

    // Start TCP listener
    let listener = TcpListener::bind(&bind_addr).await?;
    println!(
        "Server listening on {}:{} (accessible from other devices)",
        local_ip, args.port
    );

    // Accept connections
    loop {
        let (stream, client_addr) = listener.accept().await?;
        let conn = Arc::clone(&conn);
        let state = Arc::clone(&state);
        let acceptor = tls_acceptor.clone();

        tokio::spawn(async move {
            match acceptor.accept(stream).await {
                Ok(tls_stream) => {
                    if let Err(e) = handle_client(tls_stream, conn, state, client_addr).await {
                        eprintln!("Error handling TLS client {}: {}", client_addr, e);
                    }
                }
                Err(e) => {
                    eprintln!("TLS handshake failed with {}: {}", client_addr, e);
                }
            }
        });
    }
}

