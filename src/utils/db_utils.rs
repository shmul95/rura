use rusqlite::{Connection, Result as SqliteResult, params};
use sha2::{Digest, Sha256};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

pub fn init_db() -> SqliteResult<Connection> {
    let conn = Connection::open("rura.db")?;

    // Create users table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            passphrase TEXT NOT NULL UNIQUE,
            password TEXT NOT NULL
        )",
        [],
    )?;

    // Create messages table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            sender INTEGER NOT NULL,
            receiver INTEGER NOT NULL,
            content TEXT NOT NULL,
            timestamp TEXT NOT NULL,
            FOREIGN KEY(sender) REFERENCES users(id),
            FOREIGN KEY(receiver) REFERENCES users(id)
        )",
        [],
    )?;

    // Create connections table
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

pub async fn log_client_connection(
    conn: Arc<Mutex<Connection>>,
    client_addr: SocketAddr,
) -> SqliteResult<()> {
    let timestamp = chrono::Local::now().to_rfc3339();
    let conn = conn.lock().unwrap();
    conn.execute(
        "INSERT INTO connections (ip, timestamp) VALUES (?1, ?2)",
        params![client_addr.to_string(), timestamp],
    )?;
    println!("Logged connection from: {} at {}", client_addr, timestamp);
    Ok(())
}

fn hash_pass(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub async fn register_user(
    conn: Arc<Mutex<Connection>>,
    passphrase: &str,
    password: &str,
) -> SqliteResult<i64> {
    let hashed_passphrase = hash_pass(passphrase);
    let hashed_password = hash_pass(password);
    let conn = conn.lock().unwrap();

    // Check if user with this passphrase already exists
    let mut stmt = conn.prepare("SELECT id FROM users WHERE passphrase = ?1")?;
    let exists = stmt.exists(params![hashed_passphrase])?;

    if exists {
        return Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT),
            Some("User with this passphrase already exists".to_string()),
        ));
    }

    conn.execute(
        "INSERT INTO users (passphrase, password) VALUES (?1, ?2)",
        params![hashed_passphrase, hashed_password],
    )?;

    Ok(conn.last_insert_rowid())
}

pub async fn authenticate_user(
    conn: Arc<Mutex<Connection>>,
    passphrase: &str,
    password: &str,
) -> SqliteResult<Option<i64>> {
    let hashed_passphrase = hash_pass(passphrase);
    let hashed_password = hash_pass(password);
    let conn = conn.lock().unwrap();

    let mut stmt = conn.prepare("SELECT id FROM users WHERE passphrase = ?1 AND password = ?2")?;
    let user_id: Result<i64, _> = stmt
        .query_row(params![hashed_passphrase, hashed_password], |row| {
            row.get(0)
        });

    match user_id {
        Ok(id) => Ok(Some(id)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}
