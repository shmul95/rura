use crate::api::{ClientMessage, SESSIONS};
use base64::Engine as _;
use once_cell::sync::Lazy;
use rura_models::webrtc::{IceCandidate, RtcAnswer, RtcOffer};

// WebRTC crates
use std::sync::Arc;
use tokio::runtime::Runtime;
use webrtc::api::APIBuilder;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::data_channel::RTCDataChannel;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

/// Enqueue a JSON line to the open TLS stream for `user_id`.
fn enqueue(user_id: i64, msg: ClientMessage) -> Result<(), String> {
    let tx = {
        let g = SESSIONS.lock().map_err(|_| "session lock".to_string())?;
        g.get(&user_id).cloned()
    };
    let Some(tx) = tx else {
        return Err("No active stream session for user".to_string());
    };
    let mut line = serde_json::to_string(&msg).map_err(|e| format!("serialize: {e}"))?;
    line.push('\n');
    tx.send(line).map_err(|_| "send failed".to_string())
}

// Derive a stable numeric session id from an identity (base64) string.
pub(crate) fn session_id_from_identity(id_b64: &str) -> Result<i64, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(id_b64)
        .map_err(|e| format!("bad id base64: {e}"))?;
    if bytes.len() < 8 {
        return Err("identity too short".to_string());
    }
    let mut slice = [0u8; 8];
    slice.copy_from_slice(&bytes[0..8]);
    let v = u64::from_be_bytes(slice) & 0x7FFF_FFFF_FFFF_FFFF; // positive
    Ok(v as i64)
}

// Global runtime for asynchronous WebRTC tasks.
static RT: Lazy<Runtime> = Lazy::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("rura-rtc")
        .build()
        .expect("tokio runtime")
});

#[derive(Clone)]
struct Peer {
    pc: Arc<RTCPeerConnection>,
    dc: Arc<tokio::sync::Mutex<Option<Arc<RTCDataChannel>>>>,
    open: Arc<std::sync::atomic::AtomicBool>,
}

static PEERS: Lazy<std::sync::Mutex<std::collections::HashMap<i64, Peer>>> =
    Lazy::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

// Inbound messages from RTC DC to be forwarded to the app sink per user.
static DC_INBOUND: Lazy<
    std::sync::Mutex<std::collections::HashMap<i64, std::sync::mpsc::Sender<String>>>,
> = Lazy::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

pub fn register_inbound_sink(user_id: i64, tx: std::sync::mpsc::Sender<String>) {
    DC_INBOUND.lock().unwrap().insert(user_id, tx);
}

fn emit_inbound(user_id: i64, data: String) {
    if let Some(tx) = DC_INBOUND.lock().unwrap().get(&user_id).cloned() {
        let _ = tx.send(data);
    }
}

fn get_or_create_peer(user_id: i64, remote_id: i64) -> Result<Peer, String> {
    if let Some(p) = PEERS.lock().unwrap().get(&remote_id).cloned() {
        return Ok(p);
    }
    // Build WebRTC API
    let mut m = MediaEngine::default();
    // Register default codecs for data; no audio/video needed
    m.register_default_codecs()
        .map_err(|e| format!("me: {e}"))?;
    let mut r = Registry::new();
    r = register_default_interceptors(r, &mut m).map_err(|e| format!("int: {e}"))?;
    let api = APIBuilder::new()
        .with_media_engine(m)
        .with_interceptor_registry(r)
        .build();
    // Basic config with a public STUN server
    let cfg = RTCConfiguration {
        ice_servers: vec![RTCIceServer {
            urls: vec!["stun:stun.l.google.com:19302".to_string()],
            ..Default::default()
        }],
        ..Default::default()
    };
    let pc = RT
        .block_on(api.new_peer_connection(cfg))
        .map_err(|e| format!("pc: {e}"))?;
    let pc: Arc<RTCPeerConnection> = Arc::new(pc);
    let peer = Peer {
        pc: Arc::clone(&pc),
        dc: Arc::new(tokio::sync::Mutex::new(None)),
        open: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };

    // ICE candidates: forward to remote via server
    let ice_user = user_id;
    let ice_remote = remote_id;
    pc.on_ice_candidate(Box::new(
        move |cand: Option<webrtc::ice_transport::ice_candidate::RTCIceCandidate>| {
            let ice_user = ice_user;
            let ice_remote = ice_remote;
            Box::pin(async move {
                if let Some(c) = cand
                    && let Ok(json) = c.to_json()
                {
                    let _ = send_rtc_ice_over_stream(
                        ice_user,
                        ice_remote,
                        json.candidate,
                        json.sdp_mid,
                        json.sdp_mline_index.map(|v| v as u32),
                    );
                }
            })
        },
    ));

    // Data channel callbacks will be attached later (when created or received)
    let dc_open_flag = Arc::clone(&peer.open);
    let dc_slot = Arc::clone(&peer.dc);
    let this_user = ice_user;
    pc.on_data_channel(Box::new(move |dc: Arc<RTCDataChannel>| {
        let dc_open_flag = Arc::clone(&dc_open_flag);
        let dc_slot = Arc::clone(&dc_slot);
        Box::pin(async move {
            dc.on_open(Box::new(move || {
                dc_open_flag.store(true, std::sync::atomic::Ordering::SeqCst);
                println!("[rtc] data channel open");
                Box::pin(async {})
            }));
            // Forward incoming DC messages to the app sink via DC_INBOUND
            dc.on_message(Box::new(move |msg: DataChannelMessage| {
                let this_user = this_user;
                Box::pin(async move {
                    if let Ok(text) = std::str::from_utf8(&msg.data) {
                        emit_inbound(this_user, text.to_string());
                    }
                })
            }));
            // store channel
            *dc_slot.lock().await = Some(Arc::clone(&dc));
        })
    }));

    PEERS.lock().unwrap().insert(remote_id, peer.clone());
    Ok(peer)
}

pub fn ensure_offer_to_identity(user_id: i64, to_identity: &str) -> Result<(), String> {
    let remote_id = session_id_from_identity(to_identity)?;
    ensure_offer(user_id, remote_id)
}

pub fn ensure_offer(user_id: i64, remote_id: i64) -> Result<(), String> {
    let peer = get_or_create_peer(user_id, remote_id)?;
    // Create data channel as initiator and generate offer
    let pc = Arc::clone(&peer.pc);
    let dc_slot = Arc::clone(&peer.dc);
    RT.block_on(async move {
        let dc = pc
            .create_data_channel("msg", None)
            .await
            .map_err(|e| format!("dc: {e}"))?;
        {
            let mut slot = dc_slot.lock().await;
            *slot = Some(Arc::clone(&dc));
        }
        dc.on_open(Box::new({
            let open = Arc::clone(&peer.open);
            move || {
                open.store(true, std::sync::atomic::Ordering::SeqCst);
                Box::pin(async {})
            }
        }));
        let offer = pc
            .create_offer(None)
            .await
            .map_err(|e| format!("offer: {e}"))?;
        pc.set_local_description(offer.clone())
            .await
            .map_err(|e| format!("set_local: {e}"))?;
        let payload = serde_json::to_string(&offer).map_err(|e| format!("serde: {e}"))?;
        start_rtc_offer_over_stream(user_id, remote_id, payload)
    })
}

pub fn on_remote_offer(user_id: i64, offer: RtcOffer) -> Result<(), String> {
    let remote_id = offer.from_user_id;
    let peer = get_or_create_peer(user_id, remote_id)?;
    let pc = Arc::clone(&peer.pc);
    RT.block_on(async move {
        let sd: RTCSessionDescription =
            serde_json::from_str(&offer.sdp).map_err(|e| format!("offer json: {e}"))?;
        pc.set_remote_description(sd)
            .await
            .map_err(|e| format!("set_remote: {e}"))?;
        let answer = pc
            .create_answer(None)
            .await
            .map_err(|e| format!("answer: {e}"))?;
        pc.set_local_description(answer.clone())
            .await
            .map_err(|e| format!("set_local: {e}"))?;
        let payload = serde_json::to_string(&answer).map_err(|e| format!("serde: {e}"))?;
        send_rtc_answer_over_stream(user_id, remote_id, payload)
    })
}

pub fn on_remote_answer(user_id: i64, answer: RtcAnswer) -> Result<(), String> {
    let remote_id = answer.from_user_id;
    let peer = get_or_create_peer(user_id, remote_id)?;
    let pc = Arc::clone(&peer.pc);
    RT.block_on(async move {
        let sd: RTCSessionDescription =
            serde_json::from_str(&answer.sdp).map_err(|e| format!("answer json: {e}"))?;
        pc.set_remote_description(sd)
            .await
            .map_err(|e| format!("set_remote: {e}"))
    })
}

pub fn on_remote_ice(user_id: i64, ice: IceCandidate) -> Result<(), String> {
    let remote_id = ice.from_user_id;
    let peer = get_or_create_peer(user_id, remote_id)?;
    let pc = Arc::clone(&peer.pc);
    RT.block_on(async move {
        let init = RTCIceCandidateInit {
            candidate: ice.candidate,
            sdp_mid: ice.sdp_mid,
            sdp_mline_index: ice.sdp_mline_index.map(|v| v as u16),
            username_fragment: None,
        };
        pc.add_ice_candidate(init)
            .await
            .map_err(|e| format!("ice: {e}"))
    })
}

pub fn is_channel_open(remote_id: i64) -> bool {
    if let Some(peer) = PEERS.lock().unwrap().get(&remote_id) {
        return peer.open.load(std::sync::atomic::Ordering::SeqCst);
    }
    false
}

pub fn send_over_dc(remote_id: i64, data: String) -> Result<(), String> {
    let Some(peer) = PEERS.lock().unwrap().get(&remote_id).cloned() else {
        return Err("no rtc peer".into());
    };
    if !peer.open.load(std::sync::atomic::Ordering::SeqCst) {
        return Err("rtc channel not open".into());
    }
    let dc_opt = RT.block_on(async { peer.dc.lock().await.clone() });
    let Some(dc) = dc_opt else {
        return Err("no data channel".into());
    };
    RT.block_on(async move {
        dc.send_text(data)
            .await
            .map(|_| ())
            .map_err(|e| format!("dc send: {e}"))
    })
}

/// Send an SDP offer to a peer via server signaling (over the TLS stream).
pub fn start_rtc_offer_over_stream(
    user_id: i64,
    to_user_id: i64,
    sdp: String,
) -> Result<(), String> {
    let offer = RtcOffer {
        from_user_id: user_id,
        to_user_id,
        sdp,
    };
    let msg = ClientMessage {
        command: "rtc_offer".into(),
        data: serde_json::to_string(&offer).unwrap(),
    };
    enqueue(user_id, msg)
}

/// Send an SDP answer in response to an offer.
pub fn send_rtc_answer_over_stream(
    user_id: i64,
    to_user_id: i64,
    sdp: String,
) -> Result<(), String> {
    let answer = RtcAnswer {
        from_user_id: user_id,
        to_user_id,
        sdp,
    };
    let msg = ClientMessage {
        command: "rtc_answer".into(),
        data: serde_json::to_string(&answer).unwrap(),
    };
    enqueue(user_id, msg)
}

/// Send an ICE candidate to the remote peer.
pub fn send_rtc_ice_over_stream(
    user_id: i64,
    to_user_id: i64,
    candidate: String,
    sdp_mid: Option<String>,
    sdp_mline_index: Option<u32>,
) -> Result<(), String> {
    let ice = IceCandidate {
        from_user_id: user_id,
        to_user_id,
        candidate,
        sdp_mid,
        sdp_mline_index,
    };
    let msg = ClientMessage {
        command: "rtc_ice".into(),
        data: serde_json::to_string(&ice).unwrap(),
    };
    enqueue(user_id, msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offer_serializes_and_enqueues() {
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        {
            let mut g = SESSIONS.lock().unwrap();
            g.insert(1, tx);
        }
        start_rtc_offer_over_stream(1, 2, "sdpO".into()).unwrap();
        let line = rx.recv().unwrap();
        let wrap: ClientMessage = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(wrap.command, "rtc_offer");
        let v: serde_json::Value = serde_json::from_str(&wrap.data).unwrap();
        assert_eq!(v.get("from_user_id").and_then(|n| n.as_i64()), Some(1));
        assert_eq!(v.get("to_user_id").and_then(|n| n.as_i64()), Some(2));
        assert_eq!(v.get("sdp").and_then(|s| s.as_str()), Some("sdpO"));
    }

    #[test]
    fn answer_serializes_and_enqueues() {
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        {
            let mut g = SESSIONS.lock().unwrap();
            g.insert(3, tx);
        }
        send_rtc_answer_over_stream(3, 4, "sdpA".into()).unwrap();
        let line = rx.recv().unwrap();
        let wrap: ClientMessage = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(wrap.command, "rtc_answer");
        let v: serde_json::Value = serde_json::from_str(&wrap.data).unwrap();
        assert_eq!(v.get("from_user_id").and_then(|n| n.as_i64()), Some(3));
        assert_eq!(v.get("to_user_id").and_then(|n| n.as_i64()), Some(4));
        assert_eq!(v.get("sdp").and_then(|s| s.as_str()), Some("sdpA"));
    }

    #[test]
    fn ice_serializes_and_enqueues() {
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        {
            let mut g = SESSIONS.lock().unwrap();
            g.insert(5, tx);
        }
        send_rtc_ice_over_stream(5, 6, "cand".into(), Some("0".into()), Some(1)).unwrap();
        let line = rx.recv().unwrap();
        let wrap: ClientMessage = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(wrap.command, "rtc_ice");
        let v: serde_json::Value = serde_json::from_str(&wrap.data).unwrap();
        assert_eq!(v.get("from_user_id").and_then(|n| n.as_i64()), Some(5));
        assert_eq!(v.get("to_user_id").and_then(|n| n.as_i64()), Some(6));
        assert_eq!(v.get("candidate").and_then(|s| s.as_str()), Some("cand"));
    }
}
