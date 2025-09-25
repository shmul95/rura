use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug)]
pub struct DirectMessageReq {
    pub to_user_id: i64,
    pub body: String,
}

#[derive(Serialize, Debug)]
pub struct DirectMessageEvent {
    pub from_user_id: i64,
    pub body: String,
}

