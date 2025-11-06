use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RtcOffer {
    pub from_user_id: i64,
    pub to_user_id: i64,
    pub sdp: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RtcAnswer {
    pub from_user_id: i64,
    pub to_user_id: i64,
    pub sdp: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IceCandidate {
    pub from_user_id: i64,
    pub to_user_id: i64,
    pub candidate: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdp_mid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdp_mline_index: Option<u32>,
}
