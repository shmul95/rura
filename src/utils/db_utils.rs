use rusqlite::{params, Connection, Result as SqliteResult};
use std::net::{SocketAddr};
use std::sync::{Arc, Mutex};

pub fn init_db() -> SqliteResult<Connection> {
    let conn = Connection::open("clients.db")?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS connections (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ip TEXT NOT NULL,
            timestamp TEXT NOT NULL
        )",
        [],
    )?;
    Ok(conn)
}

pub async fn log_client_connection(conn: Arc<Mutex<Connection>>, client_addr: SocketAddr) -> SqliteResult<()> {
    let timestamp = chrono::Local::now().to_rfc3339();
    let conn = conn.lock().unwrap();
    conn.execute(
        "INSERT INTO connections (ip, timestamp) VALUES (?1, ?2)",
        params![client_addr.to_string(), timestamp],
    )?;
    println!("Logged connection from: {} at {}", client_addr, timestamp);
    Ok(())
}
