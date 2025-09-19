use argon2::password_hash::{Error as PasswordHashError, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use rusqlite::{params, Connection, Result as SqliteResult, ffi};
use rand_core::OsRng;
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

fn map_password_error(err: PasswordHashError) -> rusqlite::Error {
    rusqlite::Error::SqliteFailure(
        ffi::Error::new(ffi::SQLITE_ERROR),
        Some(format!("password hashing error: {err}")),
    )
}

fn hash_password(password: &str) -> Result<String, rusqlite::Error> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(map_password_error)
}

fn password_matches(hash: &str, password: &str) -> Result<bool, rusqlite::Error> {
    let parsed_hash = PasswordHash::new(hash).map_err(map_password_error)?;
    match Argon2::default().verify_password(password.as_bytes(), &parsed_hash) {
        Ok(_) => Ok(true),
        Err(PasswordHashError::Password) => Ok(false),
        Err(e) => Err(map_password_error(e)),
    }
}

pub async fn register_user(
    conn: Arc<Mutex<Connection>>,
    passphrase: &str,
    password: &str,
) -> SqliteResult<i64> {
    let hashed_password = hash_password(password)?;
    let conn = conn.lock().unwrap();

    // Check if user with this passphrase already exists
    let mut stmt = conn.prepare("SELECT id FROM users WHERE passphrase = ?1")?;
    let exists = stmt.exists(params![passphrase])?;

    if exists {
        return Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT),
            Some("User with this passphrase already exists".to_string()),
        ));
    }

    conn.execute(
        "INSERT INTO users (passphrase, password) VALUES (?1, ?2)",
        params![passphrase, hashed_password],
    )?;

    Ok(conn.last_insert_rowid())
}

pub async fn authenticate_user(
    conn: Arc<Mutex<Connection>>,
    passphrase: &str,
    password: &str,
) -> SqliteResult<Option<i64>> {
    let (user_id, stored_hash) = {
        let conn = conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, password FROM users WHERE passphrase = ?1")?;

        match stmt.query_row(params![passphrase], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        }) {
            Ok(result) => result,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(e),
        }
    };

    if password_matches(&stored_hash, password)? {
        Ok(Some(user_id))
    } else {
        Ok(None)
    }
}
