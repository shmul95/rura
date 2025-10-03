use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct DirectMessageReq {
    pub to_user_id: i64,
    pub body: String,
    pub saved: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DirectMessageEvent {
    pub from_user_id: i64,
    pub body: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SaveRequest {
    pub message_id: i64,
    pub saved: Option<bool>, // default true when omitted
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SaveResponse {
    pub success: bool,
    pub message: String,
    pub message_id: Option<i64>,
    pub saved: Option<bool>,
}

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
    pub saved: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HistoryResponse {
    pub success: bool,
    pub message: String,
    pub messages: Vec<HistoryMessage>,
}
