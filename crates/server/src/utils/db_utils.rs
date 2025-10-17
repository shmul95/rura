// Use fully qualified path to shared model to avoid module import issues
use argon2::Argon2;
use argon2::password_hash::{
    Error as PasswordHashError, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use rand_core::OsRng;
use rusqlite::{Connection, Result as SqliteResult, ffi, params};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

fn init_db_with_path<P: AsRef<std::path::Path>>(path: P) -> SqliteResult<Connection> {
    let conn = Connection::open(path)?;

    // Create users table (without new columns that may be added later)
    conn.execute(
        "CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            passphrase TEXT NOT NULL UNIQUE,
            password TEXT NOT NULL
        )",
        [],
    )?;

    // Ensure `pubkey` column exists for E2EE key distribution (added in-place for older DBs)
    {
        let mut stmt = conn.prepare("PRAGMA table_info(users)")?;
        let mut rows = stmt.query([])?;
        let mut has_pubkey = false;
        while let Some(row) = rows.next()? {
            let col_name: String = row.get(1)?;
            if col_name == "pubkey" {
                has_pubkey = true;
                break;
            }
        }
        if !has_pubkey {
            conn.execute("ALTER TABLE users ADD COLUMN pubkey TEXT", [])?;
        }
    }

    // Note: Messages are no longer persisted on the server. Only `users` and `connections` are created.

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

pub fn init_db() -> SqliteResult<Connection> {
    init_db_with_path("rura.db")
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

// E2EE key distribution helpers
pub async fn set_user_pubkey(
    conn: Arc<Mutex<Connection>>,
    user_id: i64,
    pubkey: &str,
) -> SqliteResult<bool> {
    let conn = conn.lock().unwrap();
    let updated = conn.execute(
        "UPDATE users SET pubkey = ?1 WHERE id = ?2",
        params![pubkey, user_id],
    )?;
    Ok(updated == 1)
}

pub async fn get_user_pubkey(
    conn: Arc<Mutex<Connection>>,
    user_id: i64,
) -> SqliteResult<Option<String>> {
    let conn = conn.lock().unwrap();
    let mut stmt = conn.prepare("SELECT pubkey FROM users WHERE id = ?1")?;
    let mut rows = stmt.query(params![user_id])?;
    if let Some(row) = rows.next()? {
        let v: Option<String> = row.get(0)?;
        Ok(v)
    } else {
        Ok(None)
    }
}

// No message persistence functions: messages are stored only on clients.

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::{Arc, Mutex};

    fn columns_for(conn: &Connection, table: &str) -> SqliteResult<Vec<String>> {
        let sql = format!("PRAGMA table_info({table})");
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query([])?;
        let mut columns = Vec::new();
        while let Some(row) = rows.next()? {
            columns.push(row.get(1)?);
        }
        Ok(columns)
    }

    #[test]
    fn init_db_creates_required_tables() {
        let conn = init_db_with_path(":memory:").expect("failed to create in-memory db");

        for table in ["users", "connections"] {
            let columns = columns_for(&conn, table).expect("failed to read pragma");
            assert!(
                !columns.is_empty(),
                "expected columns to be created for table `{}`",
                table
            );
        }

        let user_columns = columns_for(&conn, "users").expect("failed to read user columns");
        assert!(user_columns.contains(&"id".to_string()));
        assert!(user_columns.contains(&"passphrase".to_string()));
        assert!(user_columns.contains(&"password".to_string()));
    }

    #[tokio::test]
    async fn register_user_stores_argon2_hash_and_enforces_uniqueness() {
        let conn = Arc::new(Mutex::new(
            init_db_with_path(":memory:").expect("failed to create db"),
        ));

        let user_id = register_user(Arc::clone(&conn), "alice", "password123")
            .await
            .expect("failed to register user");
        assert_eq!(user_id, 1);

        let stored_hash = {
            let guard = conn.lock().unwrap();
            let mut stmt = guard
                .prepare("SELECT password FROM users WHERE passphrase = ?1")
                .expect("prepare failed");
            stmt.query_row(params!["alice"], |row| row.get::<_, String>(0))
                .expect("query failed")
        };

        assert!(stored_hash.starts_with("$argon2"));
        assert_ne!(stored_hash, "password123");

        let err = register_user(Arc::clone(&conn), "alice", "another")
            .await
            .expect_err("duplicate registration should fail");
        assert!(err.to_string().contains("already exists"));
    }

    #[tokio::test]
    async fn authenticate_user_validates_credentials() {
        let conn = Arc::new(Mutex::new(
            init_db_with_path(":memory:").expect("failed to create db"),
        ));

        let user_id = register_user(Arc::clone(&conn), "bob", "secret")
            .await
            .expect("registration failed");

        let success = authenticate_user(Arc::clone(&conn), "bob", "secret")
            .await
            .expect("authentication errored");
        assert_eq!(success, Some(user_id));

        let wrong_password = authenticate_user(Arc::clone(&conn), "bob", "wrong")
            .await
            .expect("authentication errored");
        assert_eq!(wrong_password, None);

        let missing_user = authenticate_user(Arc::clone(&conn), "carol", "secret")
            .await
            .expect("authentication errored");
        assert_eq!(missing_user, None);
    }

    #[tokio::test]
    async fn log_client_connection_records_entry() {
        let conn = Arc::new(Mutex::new(
            init_db_with_path(":memory:").expect("failed to create db"),
        ));
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 4242);

        log_client_connection(Arc::clone(&conn), addr)
            .await
            .expect("logging connection failed");

        let (count, ip) = {
            let guard = conn.lock().unwrap();
            let count: i64 = guard
                .query_row("SELECT COUNT(*) FROM connections", [], |row| row.get(0))
                .expect("select count failed");
            let ip: String = guard
                .query_row("SELECT ip FROM connections LIMIT 1", [], |row| row.get(0))
                .expect("select ip failed");
            (count, ip)
        };

        assert_eq!(count, 1);
        assert_eq!(ip, addr.to_string());
    }

    #[tokio::test]
    async fn pubkey_set_and_get_roundtrip() {
        let conn = Arc::new(Mutex::new(
            init_db_with_path(":memory:").expect("failed to create db"),
        ));

        let uid = register_user(Arc::clone(&conn), "dave", "pw")
            .await
            .expect("reg");
        let pk = "BASE64PUBKEY";
        let ok = set_user_pubkey(Arc::clone(&conn), uid, pk)
            .await
            .expect("set pk");
        assert!(ok);
        let fetched = get_user_pubkey(Arc::clone(&conn), uid)
            .await
            .expect("get pk");
        assert_eq!(fetched.as_deref(), Some(pk));
    }
}
