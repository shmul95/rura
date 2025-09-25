use std::sync::Arc;

use crate::models::client_message::ClientMessage;

use super::models::{DirectMessageEvent, DirectMessageReq};
use super::state::AppState;

pub async fn send_direct(
    state: Arc<AppState>,
    from_user_id: i64,
    req: DirectMessageReq,
) -> tokio::io::Result<()> {
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

