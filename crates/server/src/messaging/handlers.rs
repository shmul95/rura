use rusqlite::Connection;
use std::sync::Arc;
use std::sync::Mutex;

use crate::models::client_message::ClientMessage;

use super::models::DirectMessageReq;
use super::state::AppState;

pub async fn send_direct(
    state: Arc<AppState>,
    _conn: Arc<Mutex<Connection>>,
    from_user_id: i64,
    req: DirectMessageReq,
) -> tokio::io::Result<()> {
    // No server-side persistence: messages are stored only on clients.
    if let Some(tx) = state.get_sender(&req.to_user_id.to_string()).await {
        let event = serde_json::json!({
            // Back-compat numeric sender for legacy clients (0 when unknown).
            "from_user_id": from_user_id,
            // New identity field for clients using identity-based routing.
            "from_identity": from_user_id.to_string(),
            "body": req.body,
        });
        let msg = ClientMessage {
            command: "message".to_string(),
            data: event.to_string(),
        };
        // Ignore send errors (receiver might have just disconnected)
        let _ = tx.send(msg);
    }
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
    if let Some(tx) = state.get_sender(&to_identity).await {
        let event = serde_json::json!({
            // Keep numeric for legacy receivers as 0, and include identity explicitly
            "from_user_id": 0,
            "from_identity": from_identity,
            "body": body,
        });
        let msg = ClientMessage {
            command: "message".to_string(),
            data: event.to_string(),
        };
        let _ = tx.send(msg);
    }
    Ok(())
}
