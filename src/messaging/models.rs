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
