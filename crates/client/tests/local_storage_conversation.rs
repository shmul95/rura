use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

static STORAGE_GUARD: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
static STORAGE_INIT: Lazy<()> = Lazy::new(|| {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.into_path();
    std::env::set_var("RURA_CLIENT_DATA_DIR", &path);
    rura_client::local_storage::init_storage().expect("init storage");
});

static UNIQUE_BODY: AtomicUsize = AtomicUsize::new(0);

#[test]
fn close_conversation_moves_ephemeral_into_persistent() {
    let _guard = STORAGE_GUARD.lock().unwrap();
    Lazy::force(&STORAGE_INIT);

    rura_client::local_storage::wipe_ephemeral().expect("wipe ephemeral");

    let initial_len = rura_client::local_storage::load_history(None)
        .expect("load initial history")
        .len();

    let body_id = UNIQUE_BODY.fetch_add(1, Ordering::SeqCst);
    let body = format!("ephemeral-to-persistent-{body_id}");
    let timestamp = format!("2024-01-01T00:00:{body_id:02}Z");

    rura_client::local_storage::append_ephemeral_message(1, 2, body.clone(), timestamp.clone())
        .expect("append ephemeral");

    rura_client::local_storage::close_conversation().expect("close conversation");

    // Ephemeral cache can be wiped (e.g., on next session) without losing migrated data.
    rura_client::local_storage::wipe_ephemeral().expect("wipe after close");

    let history = rura_client::local_storage::load_history(None).expect("load history");
    assert_eq!(
        history.len(),
        initial_len + 1,
        "persistent history should grow"
    );

    let migrated = history
        .into_iter()
        .find(|m| m.body == body && m.from_user_id == 1 && m.to_user_id == 2)
        .expect("migrated message present");

    assert_eq!(migrated.timestamp, timestamp);
    assert!(!migrated.saved, "migrated messages remain unsaved");
}

#[test]
fn close_conversation_no_ephemeral_is_noop() {
    let _guard = STORAGE_GUARD.lock().unwrap();
    Lazy::force(&STORAGE_INIT);

    // Ensure no ephemeral rows remain.
    rura_client::local_storage::wipe_ephemeral().expect("wipe ephemeral");

    // Should succeed even when there is nothing to migrate.
    rura_client::local_storage::close_conversation().expect("close conversation noop");
}
