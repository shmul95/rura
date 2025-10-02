use rusqlite::Connection;
use std::sync::Arc;
use std::sync::Mutex;

use crate::models::client_message::ClientMessage;

use super::models::{DirectMessageEvent, DirectMessageReq};
use super::state::AppState;
use crate::utils::db_utils::store_message;

pub async fn send_direct(
    state: Arc<AppState>,
    conn: Arc<Mutex<Connection>>,
    from_user_id: i64,
    req: DirectMessageReq,
) -> tokio::io::Result<()> {
    // Persist the message regardless of recipient online status
    let _ = store_message(
        Arc::clone(&conn),
        from_user_id,
        req.to_user_id,
        &req.body,
        req.saved.unwrap_or(false),
    )
    .await;
    if let Some(tx) = state.get_sender(req.to_user_id).await {
        let event = DirectMessageEvent {
            from_user_id,
            body: req.body,
        };
        let msg = ClientMessage {
            command: "message".to_string(),
            data: serde_json::to_string(&event).unwrap(),
        };
        // Ignore send errors (receiver might have just disconnected)
        let _ = tx.send(msg);
    }
    Ok(())
}

