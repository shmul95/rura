use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug)]
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
