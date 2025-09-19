use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct ClientMessage {
    pub command: String,
    pub data: String,
}
