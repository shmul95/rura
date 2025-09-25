use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClientMessage {
    pub command: String,
    pub data: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AuthRequest {
    pub passphrase: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AuthResponse {
    pub success: bool,
    pub message: String,
    pub user_id: Option<i64>,
}
