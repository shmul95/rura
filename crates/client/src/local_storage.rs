use once_cell::sync::Lazy;
use rusqlite::{Connection, params};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::api::HistoryMessage;

// Store a single LocalStorage instance (single account only).
static STORE: Lazy<Mutex<Option<LocalStorage>>> = Lazy::new(|| Mutex::new(None));

pub struct LocalStorage {
    pub persistent_path: PathBuf,
    persistent: Arc<Mutex<Connection>>,
    ephemeral: Arc<Mutex<Connection>>,
}

fn data_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("RURA_CLIENT_DATA_DIR") {
        return PathBuf::from(custom);
    }
    // Default: inside client crate (parent of flutter_app)
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("../data")
}

fn ensure_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|e| format!("Failed to create dir {}: {e}", path.display()))
}

fn encrypted_db_path() -> PathBuf {
    data_dir().join("persistent.enc")
}

pub fn data_dir_exists() -> bool {
    data_dir().exists()
}

fn init_persistent_schema(conn: &Connection) -> Result<(), String> {
    // Basic PRAGMAs suitable for local storage
    conn.execute_batch(
        r#"
        PRAGMA journal_mode=WAL;
        PRAGMA foreign_keys=ON;
        PRAGMA synchronous=NORMAL;
        "#,
    )
    .map_err(|e| format!("PRAGMA failed: {e}"))?;

    // Contacts: two columns (user_id, pubkey)
    conn.execute(
        "CREATE TABLE IF NOT EXISTS contacts (
            user_id TEXT PRIMARY KEY,
            pubkey TEXT
        )",
        [],
    )
    .map_err(|e| format!("create contacts failed: {e}"))?;

    // Messages (persistent)
    conn.execute(
        "CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            from_user_id INTEGER NOT NULL,
            to_user_id INTEGER NOT NULL,
            body TEXT NOT NULL,
            timestamp TEXT NOT NULL,
            saved INTEGER NOT NULL DEFAULT 0
        )",
        [],
    )
    .map_err(|e| format!("create messages failed: {e}"))?;

    // Note: No `settings` or `keys` tables per current requirements.

    // Helpful index for reading conversations
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_messages_peer_time ON messages (from_user_id, to_user_id, timestamp)",
        [],
    )
    .map_err(|e| format!("create idx failed: {e}"))?;

    Ok(())
}

fn init_ephemeral_schema(conn: &Connection) -> Result<(), String> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS ephemeral_messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            from_user_id INTEGER NOT NULL,
            to_user_id INTEGER NOT NULL,
            body TEXT NOT NULL,
            timestamp TEXT NOT NULL
        )",
        [],
    )
    .map_err(|e| format!("create ephemeral_messages failed: {e}"))?;
    Ok(())
}

/// Initialize local storage (single account only).
pub fn init_storage() -> Result<(), String> {
    // Fast path: already initialized
    if STORE.lock().unwrap().is_some() {
        return Ok(());
    }

    let dir = data_dir();
    ensure_dir(&dir)?;
    let enc_path = encrypted_db_path();

    // Always keep persistent in memory; on disk is encrypted snapshot
    let persistent_conn = Connection::open_in_memory()
        .map_err(|e| format!("open in-memory persistent failed: {e}"))?;
    init_persistent_schema(&persistent_conn)?;

    // If an encrypted snapshot exists, restore it into memory
    if enc_path.exists() {
        let data = fs::read(&enc_path).map_err(|e| format!("read {}: {e}", enc_path.display()))?;
        let plain = crate::security::decrypt_blob(&data)?;
        // Write to a temp file, import into memory, then delete temp
        let tmp = data_dir().join("restore.tmp.db");
        fs::write(&tmp, plain).map_err(|e| format!("write {}: {e}", tmp.display()))?;
        // Attach tmp and copy known tables
        {
            let attach = format!("ATTACH DATABASE '{}' AS disk;", tmp.display());
            persistent_conn
                .execute_batch(&attach)
                .map_err(|e| format!("attach tmp: {e}"))?;
            // Create schema in memory first (already done), then copy if tables exist
            let _ = persistent_conn.execute(
                "INSERT INTO messages (from_user_id, to_user_id, body, timestamp, saved)
                 SELECT from_user_id, to_user_id, body, timestamp, saved FROM disk.messages",
                [],
            );
            let _ = persistent_conn.execute(
                "INSERT INTO contacts (user_id, pubkey)
                 SELECT user_id, pubkey FROM disk.contacts",
                [],
            );
            persistent_conn
                .execute_batch("DETACH DATABASE disk;")
                .map_err(|e| format!("detach tmp: {e}"))?;
        }
        let _ = fs::remove_file(&tmp);
    }
    let persistent = Arc::new(Mutex::new(persistent_conn));

    let ephemeral_conn =
        Connection::open_in_memory().map_err(|e| format!("open in-memory db failed: {e}"))?;
    init_ephemeral_schema(&ephemeral_conn)?;
    let ephemeral = Arc::new(Mutex::new(ephemeral_conn));

    // Auto-wipe ephemeral on init: start clean every session
    ephemeral
        .lock()
        .unwrap()
        .execute("DELETE FROM ephemeral_messages", [])
        .map_err(|e| format!("wipe ephemeral failed: {e}"))?;

    let storage = LocalStorage {
        persistent_path: enc_path,
        persistent,
        ephemeral,
    };
    *STORE.lock().unwrap() = Some(storage);
    Ok(())
}

fn with_store<R>(f: impl FnOnce(&LocalStorage) -> Result<R, String>) -> Result<R, String> {
    let g = STORE.lock().unwrap();
    let Some(store) = g.as_ref() else {
        return Err("Local storage not initialized".to_string());
    };
    f(store)
}

/// Append a message to the ephemeral database (unsaved, self-destructing).
pub fn append_ephemeral_message(
    from_user_id: i64,
    to_user_id: i64,
    body: String,
    timestamp: String,
) -> Result<(), String> {
    with_store(|store| {
        store
            .ephemeral
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO ephemeral_messages (from_user_id, to_user_id, body, timestamp) VALUES (?1, ?2, ?3, ?4)",
                params![from_user_id, to_user_id, body, timestamp],
            )
            .map(|_| ())
            .map_err(|e| format!("insert ephemeral failed: {e}"))
    })
}

/// Append a message to the persistent database (saved).
pub fn append_persistent_message(
    from_user_id: i64,
    to_user_id: i64,
    body: String,
    timestamp: String,
    saved: bool,
) -> Result<(), String> {
    with_store(|store| {
        store
            .persistent
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO messages (from_user_id, to_user_id, body, timestamp, saved) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![from_user_id, to_user_id, body, timestamp, if saved { 1 } else { 0 }],
            )
            .map(|_| ())
            .map_err(|e| format!("insert persistent failed: {e}"))
    })?;
    // After modification, flush to encrypted snapshot
    flush_persistent()
}

/// Load merged history from both persistent and ephemeral stores, ordered by time.
pub fn load_history(limit: Option<usize>) -> Result<Vec<HistoryMessage>, String> {
    let mut merged: Vec<HistoryMessage> = Vec::new();

    with_store(|store| {
        // Persistent
        let persistent = store.persistent.lock().unwrap();
        let mut stmt = persistent
            .prepare("SELECT id, from_user_id, to_user_id, body, timestamp, saved FROM messages")
            .map_err(|e| format!("prepare persistent query failed: {e}"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(HistoryMessage {
                    id: row.get(0)?,
                    from_user_id: row.get(1)?,
                    to_user_id: row.get(2)?,
                    body: row.get(3)?,
                    timestamp: row.get(4)?,
                    saved: {
                        let v: i64 = row.get(5)?;
                        v != 0
                    },
                })
            })
            .map_err(|e| format!("query persistent failed: {e}"))?;
        for r in rows {
            merged.push(r.map_err(|e| format!("row error: {e}"))?);
        }

        // Ephemeral
        let ephemeral = store.ephemeral.lock().unwrap();
        let mut stmt_e = ephemeral
            .prepare("SELECT id, from_user_id, to_user_id, body, timestamp FROM ephemeral_messages")
            .map_err(|e| format!("prepare ephemeral query failed: {e}"))?;
        let rows_e = stmt_e
            .query_map([], |row| {
                Ok(HistoryMessage {
                    id: row.get(0)?,
                    from_user_id: row.get(1)?,
                    to_user_id: row.get(2)?,
                    body: row.get(3)?,
                    timestamp: row.get(4)?,
                    saved: false,
                })
            })
            .map_err(|e| format!("query ephemeral failed: {e}"))?;
        for r in rows_e {
            merged.push(r.map_err(|e| format!("row error: {e}"))?);
        }

        Ok::<(), String>(())
    })?;

    // Order by (timestamp, id) and apply limit
    merged.sort_by(|a, b| a.timestamp.cmp(&b.timestamp).then_with(|| a.id.cmp(&b.id)));
    if let Some(lim) = limit {
        if merged.len() > lim {
            let start = merged.len() - lim;
            merged = merged.split_off(start);
        }
    }
    Ok(merged)
}

/// Explicit wipe for ephemeral messages.
pub fn wipe_ephemeral() -> Result<(), String> {
    with_store(|store| {
        store
            .ephemeral
            .lock()
            .unwrap()
            .execute("DELETE FROM ephemeral_messages", [])
            .map(|_| ())
            .map_err(|e| format!("wipe ephemeral failed: {e}"))
    })
}

fn flush_persistent() -> Result<(), String> {
    with_store(|store| {
        // Export in-memory DB to a temporary plain file
        let tmp = data_dir().join("plain.tmp.db");
        let tmp_str = tmp.to_string_lossy();
        store
            .persistent
            .lock()
            .unwrap()
            .execute(&format!("VACUUM INTO '{}';", tmp_str), [])
            .map_err(|e| format!("vacuum into failed: {e}"))?;
        let bytes = fs::read(&tmp).map_err(|e| format!("read {}: {e}", tmp.display()))?;
        let enc = crate::security::encrypt_blob(&bytes)?;
        fs::write(&store.persistent_path, enc)
            .map_err(|e| format!("write {}: {e}", store.persistent_path.display()))?;
        let _ = fs::remove_file(&tmp);
        Ok(())
    })
}

pub fn snapshot_persistent() -> Result<(), String> {
    flush_persistent()
}

/// Add or update a contact's public key by their user identity (base64 string).
pub fn add_contact(user_id: String, pubkey: String) -> Result<(), String> {
    with_store(|store| {
        store
            .persistent
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO contacts (user_id, pubkey) VALUES (?1, ?2)
                 ON CONFLICT(user_id) DO UPDATE SET pubkey=excluded.pubkey",
                params![user_id, pubkey],
            )
            .map(|_| ())
            .map_err(|e| format!("upsert contact failed: {e}"))
    })?;
    flush_persistent()
}
