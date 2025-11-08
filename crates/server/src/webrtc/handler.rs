use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use once_cell::sync::Lazy;

use crate::messaging::state::AppState;
use crate::models::client_message::ClientMessage;
use base64::Engine as _;
use rura_models::webrtc::{IceCandidate, RtcAnswer, RtcOffer};

/// Global, in-memory WebRTC session registry.
/// This will be wired into AppState in later tasks.
static SESSIONS: Lazy<Arc<Mutex<RtcSessionManager>>> = Lazy::new(|| {
    Arc::new(Mutex::new(RtcSessionManager {
        sessions: HashMap::new(),
    }))
});

/// Keyed by a pair of user ids (lower, higher) to avoid ordering issues.
#[derive(Default)]
pub struct RtcSessionManager {
    sessions: HashMap<(i64, i64), RtcSessionMeta>,
}

#[derive(Clone, Debug)]
pub struct RtcSessionMeta {
    pub last_updated: std::time::SystemTime,
}

impl Default for RtcSessionMeta {
    fn default() -> Self {
        Self {
            last_updated: std::time::SystemTime::now(),
        }
    }
}

/// Call at server start to ensure the module is linked and ready.
pub fn register() {
    // For now, just advertise that WebRTC signaling is compiled in.
    println!("[webrtc] signaling module registered (skeleton)");
}

pub fn handle_offer(_from: i64, _to: i64, _offer: &RtcOffer, _from_addr: SocketAddr) {
    let mut guard = SESSIONS.lock().expect("webrtc sessions lock poisoned");
    let key = ordered_pair(_from, _to);
    let entry = guard.sessions.entry(key).or_default();
    entry.last_updated = std::time::SystemTime::now();
}

pub fn handle_answer(_from: i64, _to: i64, _answer: &RtcAnswer, _from_addr: SocketAddr) {
    let mut guard = SESSIONS.lock().expect("webrtc sessions lock poisoned");
    let key = ordered_pair(_from, _to);
    let entry = guard.sessions.entry(key).or_default();
    entry.last_updated = std::time::SystemTime::now();
}

pub fn handle_ice(_from: i64, _to: i64, _ice: &IceCandidate, _from_addr: SocketAddr) {
    let mut guard = SESSIONS.lock().expect("webrtc sessions lock poisoned");
    let key = ordered_pair(_from, _to);
    let entry = guard.sessions.entry(key).or_default();
    entry.last_updated = std::time::SystemTime::now();
}

fn ordered_pair(a: i64, b: i64) -> (i64, i64) {
    if a <= b { (a, b) } else { (b, a) }
}

async fn send_if_online(state: &AppState, to_user_id: i64, msg: ClientMessage) {
    if let Some(tx) = state.get_sender_by_session_id(to_user_id).await {
        let _ = tx.send(msg);
    }
}

pub async fn process_offer(state: std::sync::Arc<AppState>, offer: RtcOffer) {
    handle_offer(
        offer.from_user_id,
        offer.to_user_id,
        &offer,
        "0.0.0.0:0".parse().unwrap(),
    );
    let wrapper = ClientMessage {
        command: "rtc_offer".into(),
        data: serde_json::to_string(&offer).unwrap(),
    };
    send_if_online(&state, offer.to_user_id, wrapper).await;
}

pub async fn process_answer(state: std::sync::Arc<AppState>, answer: RtcAnswer) {
    handle_answer(
        answer.from_user_id,
        answer.to_user_id,
        &answer,
        "0.0.0.0:0".parse().unwrap(),
    );
    let wrapper = ClientMessage {
        command: "rtc_answer".into(),
        data: serde_json::to_string(&answer).unwrap(),
    };
    send_if_online(&state, answer.to_user_id, wrapper).await;
}

pub async fn process_ice(state: std::sync::Arc<AppState>, ice: IceCandidate) {
    handle_ice(
        ice.from_user_id,
        ice.to_user_id,
        &ice,
        "0.0.0.0:0".parse().unwrap(),
    );
    let wrapper = ClientMessage {
        command: "rtc_ice".into(),
        data: serde_json::to_string(&ice).unwrap(),
    };
    send_if_online(&state, ice.to_user_id, wrapper).await;
}

/// Check if there is a currently tracked RTC session between two users.
pub fn has_active_session(a: i64, b: i64) -> bool {
    let guard = SESSIONS.lock().expect("webrtc sessions lock poisoned");
    guard.sessions.contains_key(&ordered_pair(a, b))
}

/// Convert a base64 identity string to a stable positive i64 for RTC session keys.
pub fn identity_to_session_id(id_b64: &str) -> Option<i64> {
    if let Ok(n) = id_b64.parse::<i64>() {
        return Some(n);
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(id_b64)
        .ok()?;
    if bytes.len() < 8 {
        return None;
    }
    let mut slice = [0u8; 8];
    slice.copy_from_slice(&bytes[0..8]);
    let v = u64::from_be_bytes(slice) & 0x7FFF_FFFF_FFFF_FFFF;
    Some(v as i64)
}
