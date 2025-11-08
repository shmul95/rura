use rusqlite::Connection;
use std::sync::Arc;
use std::sync::Mutex;

// No longer emitting ClientMessage from server for DM relay.

use super::models::DirectMessageReq;
use super::state::AppState;

pub async fn send_direct(
    state: Arc<AppState>,
    _conn: Arc<Mutex<Connection>>,
    from_user_id: i64,
    req: DirectMessageReq,
) -> tokio::io::Result<()> {
    // Server no longer relays message bodies over TCP/TLS. Messages must flow via WebRTC.
    // Intentionally do nothing here to avoid handling plaintext/ciphertext payloads.
    let _ = (state, from_user_id, req);
    Ok(())
}

/// Identity-based direct send (no DB ids), routing purely by in-memory identity keys.
pub async fn send_direct_identity(
    state: Arc<AppState>,
    _conn: Arc<Mutex<Connection>>,
    from_identity: String,
    to_identity: String,
    body: String,
) -> tokio::io::Result<()> {
    // Server no longer relays message bodies over TCP/TLS. Messages must flow via WebRTC.
    // Intentionally do nothing here to avoid handling plaintext/ciphertext payloads.
    let _ = (state, from_identity, to_identity, body);
    Ok(())
}
