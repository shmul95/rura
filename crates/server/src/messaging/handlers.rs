use rusqlite::Connection;
use std::sync::Arc;
use std::sync::Mutex;

use super::models::{DirectMessageEvent, DirectMessageReq};
use super::state::AppState;
use crate::models::client_message::ClientMessage;
use crate::webrtc::handler::identity_to_session_id;

pub async fn send_direct(
    state: Arc<AppState>,
    _conn: Arc<Mutex<Connection>>,
    from_user_id: i64,
    req: DirectMessageReq,
) -> tokio::io::Result<()> {
    let event = DirectMessageEvent {
        from_user_id,
        body: req.body,
    };
    if let Some(tx) = state.get_sender_by_session_id(req.to_user_id).await {
        let wrapper = ClientMessage {
            command: "message".to_string(),
            data: serde_json::to_string(&event).unwrap(),
        };
        let _ = tx.send(wrapper);
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
        let from_user_id = identity_to_session_id(&from_identity).unwrap_or_default();
        let event = DirectMessageEvent { from_user_id, body };
        let wrapper = ClientMessage {
            command: "message".to_string(),
            data: serde_json::to_string(&event).unwrap(),
        };
        let _ = tx.send(wrapper);
    }
    Ok(())
}
