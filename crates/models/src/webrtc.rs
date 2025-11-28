use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RtcOffer {
    pub from_user_id: i64,
    pub to_user_id: i64,
    pub sdp: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RtcAnswer {
    pub from_user_id: i64,
    pub to_user_id: i64,
    pub sdp: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IceCandidate {
    pub from_user_id: i64,
    pub to_user_id: i64,
    pub candidate: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdp_mid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdp_mline_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track: Option<IceTrackMetadata>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IceTrackMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<CallMediaKind>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub enum CallMediaKind {
    Audio,
    Video,
    Data,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CallInvite {
    pub call_id: String,
    pub from_user_id: i64,
    pub to_user_id: i64,
    pub media: CallMediaProfile,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<CallPreview>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<CallClientMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ringing_timeout_ms: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CallAnswer {
    pub call_id: String,
    pub from_user_id: i64,
    pub to_user_id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_media: Option<CallMediaProfile>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CallReject {
    pub call_id: String,
    pub from_user_id: i64,
    pub to_user_id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<CallEndReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CallHangup {
    pub call_id: String,
    pub from_user_id: i64,
    pub to_user_id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<CallEndReason>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CallRinging {
    pub call_id: String,
    pub callee_user_id: i64,
    pub ringing: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CallEndReason {
    Rejected,
    Busy,
    Hangup,
    Failed,
    Timeout,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CallMediaProfile {
    pub audio_enabled: bool,
    pub video_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_muted: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_muted: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CallPreview {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CallClientMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_version: Option<String>,
}
