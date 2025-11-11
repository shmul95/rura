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
    // Default: data/ subdirectory in current working directory (flutter_app)
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

/// Ensure and return the images directory used for media downloads.
pub fn ensure_images_dir() -> Result<PathBuf, String> {
    let dir = data_dir().join("images");
    ensure_dir(&dir)?;
    Ok(dir)
}

fn sanitize_filename(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    // Avoid empty or dot names
    let s = out.trim_matches('.');
    if s.is_empty() {
        "file".to_string()
    } else {
        s.to_string()
    }
}

/// Save raw bytes under the images directory. Returns an absolute path when possible.
pub fn save_bytes_to_images_dir(
    bytes: &[u8],
    suggested_name: Option<&str>,
) -> Result<PathBuf, String> {
    let dir = ensure_images_dir()?;
    let mut name = suggested_name.unwrap_or("file").to_string();
    name = sanitize_filename(&name);
    if name.is_empty() {
        name = "file".to_string();
    }
    let mut path = dir.join(&name);
    // Deduplicate if exists
    if path.exists() {
        let mut i = 1;
        loop {
            let candidate = dir.join(format!("{}_{}", name, i));
            if !candidate.exists() {
                path = candidate;
                break;
            }
            i += 1;
            if i > 1000 {
                break;
            }
        }
    }
    fs::write(&path, bytes).map_err(|e| format!("write {}: {}", path.display(), e))?;
    // Try to return absolute path
    if let Ok(abs) = fs::canonicalize(&path) {
        Ok(abs)
    } else {
        Ok(path)
    }
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

    // Contacts: user_id, pubkey, optional nickname
    conn.execute(
        "CREATE TABLE IF NOT EXISTS contacts (
            user_id TEXT PRIMARY KEY,
            pubkey TEXT,
            nickname TEXT
        )",
        [],
    )
    .map_err(|e| format!("create contacts failed: {e}"))?;
    // Add nickname column if upgrading older schema
    {
        let mut stmt = conn
            .prepare("PRAGMA table_info(contacts)")
            .map_err(|e| format!("pragma table_info contacts failed: {e}"))?;
        let mut rows = stmt
            .query([])
            .map_err(|e| format!("query pragma contacts failed: {e}"))?;
        let mut has_nickname = false;
        while let Some(row) = rows.next().map_err(|e| format!("row: {e}"))? {
            let col: String = row.get(1).map_err(|e| format!("col: {e}"))?;
            if col == "nickname" {
                has_nickname = true;
                break;
            }
        }
        drop(rows);
        if !has_nickname {
            conn.execute("ALTER TABLE contacts ADD COLUMN nickname TEXT", [])
                .map_err(|e| format!("alter contacts add nickname failed: {e}"))?;
        }
    }

    // Messages (persistent)
    conn.execute(
        "CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            from_user_id INTEGER NOT NULL,
            to_user_id INTEGER NOT NULL,
            body TEXT NOT NULL,
            timestamp TEXT NOT NULL
        )",
        [],
    )
    .map_err(|e| format!("create messages failed: {e}"))?;

    // Migrate legacy schemas that still include the `saved` column.
    {
        let mut stmt = conn
            .prepare("PRAGMA table_info(messages)")
            .map_err(|e| format!("pragma table_info messages failed: {e}"))?;
        let mut rows = stmt
            .query([])
            .map_err(|e| format!("query pragma messages failed: {e}"))?;
        let mut has_saved = false;
        while let Some(row) = rows.next().map_err(|e| format!("row: {e}"))? {
            let col: String = row.get(1).map_err(|e| format!("col: {e}"))?;
            if col == "saved" {
                has_saved = true;
                break;
            }
        }
        drop(rows);
        if has_saved {
            conn.execute_batch(
                "BEGIN IMMEDIATE;
                 DROP TABLE IF EXISTS messages_new;
                 CREATE TABLE messages_new (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     from_user_id INTEGER NOT NULL,
                     to_user_id INTEGER NOT NULL,
                     body TEXT NOT NULL,
                     timestamp TEXT NOT NULL
                 );
                 INSERT INTO messages_new (id, from_user_id, to_user_id, body, timestamp)
                 SELECT id, from_user_id, to_user_id, body, timestamp FROM messages;
                 DROP TABLE messages;
                 ALTER TABLE messages_new RENAME TO messages;
                 COMMIT;",
            )
            .map_err(|e| format!("migrate messages table failed: {e}"))?;
        }
    }

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
                "INSERT INTO messages (from_user_id, to_user_id, body, timestamp)
                 SELECT from_user_id, to_user_id, body, timestamp FROM disk.messages",
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
) -> Result<(), String> {
    with_store(|store| {
        store
            .persistent
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO messages (from_user_id, to_user_id, body, timestamp) VALUES (?1, ?2, ?3, ?4)",
                params![from_user_id, to_user_id, body, timestamp],
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
            .prepare("SELECT id, from_user_id, to_user_id, body, timestamp FROM messages")
            .map_err(|e| format!("prepare persistent query failed: {e}"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(HistoryMessage {
                    id: row.get(0)?,
                    from_user_id: row.get(1)?,
                    to_user_id: row.get(2)?,
                    body: row.get(3)?,
                    timestamp: row.get(4)?,
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
    if let Some(lim) = limit
        && merged.len() > lim
    {
        let start = merged.len() - lim;
        merged = merged.split_off(start);
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
        // Export in-memory DB to a temporary plain file next to the target snapshot
        let tmp_dir = store
            .persistent_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let tmp = tmp_dir.join("plain.tmp.db");
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

#[cfg(test)]
pub(crate) fn reset_store_for_tests() {
    *STORE.lock().unwrap() = None;
}

/// Update or set a contact nickname without changing the stored public key.
pub fn set_contact_nickname(user_id: String, nickname: Option<String>) -> Result<(), String> {
    with_store(|store| {
        let conn = store.persistent.lock().unwrap();
        let mut stmt = conn
            .prepare("UPDATE contacts SET nickname = ?1 WHERE user_id = ?2")
            .map_err(|e| format!("prepare update nickname failed: {e}"))?;
        let changed = stmt
            .execute(params![nickname, user_id])
            .map_err(|e| format!("update nickname failed: {e}"))?;
        if changed == 0 {
            // Insert a new row with empty pubkey and nickname
            conn.execute(
                "INSERT INTO contacts (user_id, pubkey, nickname) VALUES (?1, ?2, ?3)",
                params![user_id, "", nickname],
            )
            .map_err(|e| format!("insert nickname failed: {e}"))?;
        }
        Ok::<(), String>(())
    })?;
    flush_persistent()
}

/// Add or update a contact's public key by their user identity (base64 string).
pub fn add_contact(
    user_id: String,
    pubkey: String,
    nickname: Option<String>,
) -> Result<(), String> {
    // Normalize and validate pubkey (X25519 32-byte key expected)
    let canonical_pk = crate::security::canonicalize_pubkey_b64(&pubkey)?;
    with_store(|store| {
        store
            .persistent
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO contacts (user_id, pubkey, nickname) VALUES (?1, ?2, ?3)
                 ON CONFLICT(user_id) DO UPDATE SET
                   pubkey=excluded.pubkey,
                   nickname=COALESCE(excluded.nickname, contacts.nickname)",
                params![user_id, canonical_pk, nickname],
            )
            .map(|_| ())
            .map_err(|e| format!("upsert contact failed: {e}"))
    })?;
    flush_persistent()
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct ContactRow {
    pub user_id: String,
    pub pubkey: String,
    pub nickname: Option<String>,
}

pub fn list_contacts() -> Result<Vec<ContactRow>, String> {
    with_store(|store| {
        let mut out = Vec::new();
        let conn = store.persistent.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT user_id, pubkey, nickname FROM contacts")
            .map_err(|e| format!("prepare contacts list failed: {e}"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ContactRow {
                    user_id: row.get(0)?,
                    pubkey: row.get(1)?,
                    nickname: row.get::<_, Option<String>>(2)?,
                })
            })
            .map_err(|e| format!("query contacts failed: {e}"))?;
        for r in rows {
            out.push(r.map_err(|e| format!("row contacts: {e}"))?);
        }
        Ok::<_, String>(out)
    })
}

/// Look up a contact's public key by `user_id` (identity string).
pub fn get_contact_pubkey(user_id: &str) -> Result<Option<String>, String> {
    with_store(|store| {
        let conn = store.persistent.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT pubkey FROM contacts WHERE user_id = ?1")
            .map_err(|e| format!("prepare get_contact_pubkey failed: {e}"))?;
        let mut rows = stmt
            .query(rusqlite::params![user_id])
            .map_err(|e| format!("query get_contact_pubkey failed: {e}"))?;
        if let Some(row) = rows
            .next()
            .map_err(|e| format!("row get_contact_pubkey failed: {e}"))?
        {
            let pk: Option<String> = row.get(0).map_err(|e| format!("col get: {e}"))?;
            Ok(pk)
        } else {
            Ok(None)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use tempfile::tempdir;

    #[test]
    fn migrate_legacy_messages_table_drops_saved_column() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute(
            "CREATE TABLE messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                from_user_id INTEGER NOT NULL,
                to_user_id INTEGER NOT NULL,
                body TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                saved INTEGER NOT NULL DEFAULT 0
            )",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO messages (from_user_id, to_user_id, body, timestamp, saved)
             VALUES (1, 2, 'hello', '2024-01-01T00:00:00Z', 1)",
            [],
        )
        .unwrap();

        init_persistent_schema(&conn).unwrap();

        let mut stmt = conn.prepare("PRAGMA table_info(messages)").unwrap();
        let mut has_saved = false;
        let mut columns = Vec::new();
        let mut rows = stmt.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            let col: String = row.get(1).unwrap();
            if col == "saved" {
                has_saved = true;
            }
            columns.push(col);
        }
        assert!(!has_saved, "legacy saved column should be removed");
        assert!(columns.contains(&"timestamp".to_string()));

        let body: String = conn
            .query_row(
                "SELECT body FROM messages WHERE from_user_id = 1 AND to_user_id = 2",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(body, "hello");
    }

    #[test]
    #[serial_test::serial]
    fn append_and_load_history_persists_messages() {
        // Isolate into a temporary data directory for this test
        let temp = tempdir().unwrap();
        #[allow(unused_unsafe)]
        unsafe {
            std::env::set_var("RURA_CLIENT_DATA_DIR", temp.path());
        }
        crate::security::reset_key_for_tests();
        crate::security::unlock_local("test-pass").unwrap();

        reset_store_for_tests();
        init_storage().unwrap();

        append_persistent_message(10, 11, "hi".to_string(), "2024-02-02T00:00:00Z".to_string())
            .unwrap();

        let history = load_history(None).unwrap();
        assert_eq!(history.len(), 1);
        let entry = &history[0];
        assert_eq!(entry.from_user_id, 10);
        assert_eq!(entry.to_user_id, 11);
        assert_eq!(entry.body, "hi");
        assert_eq!(entry.timestamp, "2024-02-02T00:00:00Z");

        reset_store_for_tests();
        crate::security::reset_key_for_tests();
    }
}
