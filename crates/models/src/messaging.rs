use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct DirectMessageReq {
    pub to_user_id: i64,
    pub body: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DirectMessageEvent {
    pub from_user_id: i64,
    pub body: String,
}

// Note: per new design, messages are always persisted by default in the
// client's local message DB. The legacy save/unsave API has been removed.

// History fetch API

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HistoryRequest {
    pub limit: Option<usize>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HistoryMessage {
    pub id: i64,
    pub from_user_id: i64,
    pub to_user_id: i64,
    pub body: String,
    pub timestamp: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HistoryResponse {
    pub success: bool,
    pub message: String,
    pub messages: Vec<HistoryMessage>,
}
