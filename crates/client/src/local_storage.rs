use once_cell::sync::Lazy;
use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::api::HistoryMessage;

// Store one LocalStorage per user id.
static STORES: Lazy<Mutex<HashMap<i64, LocalStorage>>> = Lazy::new(|| Mutex::new(HashMap::new()));

pub struct LocalStorage {
    pub user_id: i64,
    pub persistent_path: PathBuf,
    persistent: Arc<Mutex<Connection>>,
    ephemeral: Arc<Mutex<Connection>>,
}

fn cache_base_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("RURA_CLIENT_CACHE_DIR") {
        return PathBuf::from(custom);
    }
    // Default: inside client crate (parent of flutter_app)
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("../.cache")
}

fn user_dir(user_id: i64) -> PathBuf {
    cache_base_dir().join("users").join(user_id.to_string())
}

fn ensure_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|e| format!("Failed to create dir {}: {e}", path.display()))
}

fn persistent_db_path(user_id: i64) -> PathBuf {
    user_dir(user_id).join("persistent.db")
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
            user_id INTEGER PRIMARY KEY,
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

/// Ensure a LocalStorage exists for given user id.
pub fn init_for_user(user_id: i64) -> Result<(), String> {
    // Fast path: already initialized
    if STORES.lock().unwrap().contains_key(&user_id) {
        return Ok(());
    }

    let dir = user_dir(user_id);
    ensure_dir(&dir)?;
    let path = persistent_db_path(user_id);

    let persistent_conn =
        Connection::open(&path).map_err(|e| format!("open persistent db failed: {e}"))?;
    init_persistent_schema(&persistent_conn)?;
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
        user_id,
        persistent_path: path,
        persistent,
        ephemeral,
    };
    STORES.lock().unwrap().insert(user_id, storage);
    Ok(())
}

fn with_store<R>(
    user_id: i64,
    f: impl FnOnce(&LocalStorage) -> Result<R, String>,
) -> Result<R, String> {
    let g = STORES.lock().unwrap();
    let Some(store) = g.get(&user_id) else {
        return Err("Local storage not initialized for user".to_string());
    };
    f(store)
}

/// Append a message to the ephemeral database (unsaved, self-destructing).
pub fn append_ephemeral_message(
    user_id: i64,
    from_user_id: i64,
    to_user_id: i64,
    body: String,
    timestamp: String,
) -> Result<(), String> {
    with_store(user_id, |store| {
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
    user_id: i64,
    from_user_id: i64,
    to_user_id: i64,
    body: String,
    timestamp: String,
    saved: bool,
) -> Result<(), String> {
    with_store(user_id, |store| {
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
    })
}

/// Load merged history from both persistent and ephemeral stores, ordered by time.
pub fn load_history(user_id: i64, limit: Option<usize>) -> Result<Vec<HistoryMessage>, String> {
    let mut merged: Vec<HistoryMessage> = Vec::new();

    with_store(user_id, |store| {
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

/// Explicit wipe for ephemeral messages for this user.
pub fn wipe_ephemeral(user_id: i64) -> Result<(), String> {
    with_store(user_id, |store| {
        store
            .ephemeral
            .lock()
            .unwrap()
            .execute("DELETE FROM ephemeral_messages", [])
            .map(|_| ())
            .map_err(|e| format!("wipe ephemeral failed: {e}"))
    })
}
