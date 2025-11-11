use crate::api::{ClientMessage, SESSIONS};
use base64::Engine as _;
use once_cell::sync::Lazy;
use rura_models::webrtc::{IceCandidate, RtcAnswer, RtcOffer};

// WebRTC crates
use sha2::{Digest, Sha256};
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

#[derive(Default)]
struct MediaAssembly {
    mime: String,
    name: Option<String>,
    checksum: String,
    total_size: usize,
    chunk_count: u32,
    received: u32,
    chunks: Vec<Option<Vec<u8>>>,
    from_user_id: i64,
    from_identity: Option<String>,
}

static MEDIA_INBOUND: Lazy<std::sync::Mutex<std::collections::HashMap<String, MediaAssembly>>> =
    Lazy::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

fn handle_media_chunk(v: serde_json::Value, user_id: i64) -> Result<(), String> {
    // Reject chunks from unknown identities
    if let Some(fid) = v.get("from_identity").and_then(|s| s.as_str())
        && {
            let _ = crate::local_storage::init_storage();
            matches!(crate::local_storage::get_contact_pubkey(fid), Ok(None))
        }
    {
        return Ok(());
    }
    let msg_id = v
        .get("msg_id")
        .and_then(|s| s.as_str())
        .ok_or_else(|| "missing msg_id".to_string())?
        .to_string();
    let chunk_index = v
        .get("chunk_index")
        .and_then(|n| n.as_u64())
        .ok_or_else(|| "missing chunk_index".to_string())? as usize;
    let chunk_count = v
        .get("chunk_count")
        .and_then(|n| n.as_u64())
        .ok_or_else(|| "missing chunk_count".to_string())? as u32;
    let total_size = v
        .get("total_size")
        .and_then(|n| n.as_u64())
        .ok_or_else(|| "missing total_size".to_string())? as usize;
    let checksum = v
        .get("checksum")
        .and_then(|s| s.as_str())
        .ok_or_else(|| "missing checksum".to_string())?
        .to_string();
    let mime = v
        .get("mime")
        .and_then(|s| s.as_str())
        .unwrap_or("application/octet-stream")
        .to_string();
    let name = v
        .get("name")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string());
    let from_user_id = v
        .get("from_user_id")
        .and_then(|n| n.as_i64())
        .unwrap_or_default();
    let from_identity = v
        .get("from_identity")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string());
    let data_b64 = v
        .get("data_b64")
        .and_then(|s| s.as_str())
        .ok_or_else(|| "missing data_b64".to_string())?;
    let chunk_bytes = base64::engine::general_purpose::STANDARD
        .decode(data_b64)
        .map_err(|e| format!("base64: {e}"))?;

    let mut map = MEDIA_INBOUND.lock().map_err(|_| "media lock".to_string())?;
    let entry = map.entry(msg_id.clone()).or_insert_with(|| MediaAssembly {
        mime: mime.clone(),
        name: name.clone(),
        checksum: checksum.clone(),
        total_size,
        chunk_count,
        received: 0,
        chunks: vec![None; chunk_count as usize],
        from_user_id,
        from_identity: from_identity.clone(),
    });
    if entry.chunks.len() != chunk_count as usize {
        entry.chunks.resize(chunk_count as usize, None);
        entry.chunk_count = chunk_count;
    }
    if entry.chunks.get(chunk_index).is_none() {
        return Err("chunk index out of bounds".into());
    }
    if entry.chunks[chunk_index].is_none() {
        entry.chunks[chunk_index] = Some(chunk_bytes);
        entry.received += 1;
    }
    if entry.received == entry.chunk_count {
        // Assemble
        let mut all = Vec::with_capacity(entry.total_size);
        for i in 0..(entry.chunk_count as usize) {
            if let Some(ref part) = entry.chunks[i] {
                all.extend_from_slice(part);
            } else {
                return Err(format!("missing chunk {}", i));
            }
        }
        if all.len() != entry.total_size {
            return Err("size mismatch".into());
        }
        let mut hasher = Sha256::new();
        hasher.update(&all);
        let got = hex::encode(hasher.finalize());
        if got != entry.checksum {
            return Err("checksum mismatch".into());
        }
        // Persist to images dir and include file_path for UI rendering
        let filename = match (&entry.name, entry.mime.as_str()) {
            (Some(n), _) if !n.is_empty() => n.clone(),
            (None, "image/jpeg") => format!(
                "img_{}.jpg",
                &entry.checksum.chars().take(8).collect::<String>()
            ),
            (None, "image/png") => format!(
                "img_{}.png",
                &entry.checksum.chars().take(8).collect::<String>()
            ),
            (None, "image/gif") => format!(
                "img_{}.gif",
                &entry.checksum.chars().take(8).collect::<String>()
            ),
            (None, "image/webp") => format!(
                "img_{}.webp",
                &entry.checksum.chars().take(8).collect::<String>()
            ),
            (None, "image/bmp") => format!(
                "img_{}.bmp",
                &entry.checksum.chars().take(8).collect::<String>()
            ),
            (None, "image/heic") | (None, "image/heif") => format!(
                "img_{}.heic",
                &entry.checksum.chars().take(8).collect::<String>()
            ),
            (None, "video/mp4") => format!(
                "vid_{}.mp4",
                &entry.checksum.chars().take(8).collect::<String>()
            ),
            (None, "video/webm") => format!(
                "vid_{}.webm",
                &entry.checksum.chars().take(8).collect::<String>()
            ),
            (None, "video/quicktime") => format!(
                "vid_{}.mov",
                &entry.checksum.chars().take(8).collect::<String>()
            ),
            (None, "video/x-matroska") => format!(
                "vid_{}.mkv",
                &entry.checksum.chars().take(8).collect::<String>()
            ),
            (None, "audio/mpeg") => format!(
                "aud_{}.mp3",
                &entry.checksum.chars().take(8).collect::<String>()
            ),
            (None, "audio/wav") => format!(
                "aud_{}.wav",
                &entry.checksum.chars().take(8).collect::<String>()
            ),
            (None, "audio/ogg") => format!(
                "aud_{}.ogg",
                &entry.checksum.chars().take(8).collect::<String>()
            ),
            (None, "application/pdf") => format!(
                "doc_{}.pdf",
                &entry.checksum.chars().take(8).collect::<String>()
            ),
            _ => format!(
                "file_{}.bin",
                &entry.checksum.chars().take(8).collect::<String>()
            ),
        };
        let file_path = match crate::local_storage::save_bytes_to_images_dir(&all, Some(&filename))
        {
            Ok(p) => p.to_string_lossy().to_string(),
            Err(_) => String::new(),
        };
        let data_b64 = base64::engine::general_purpose::STANDARD.encode(&all);
        let ev = serde_json::json!({
            "type": "media_complete",
            "from_user_id": entry.from_user_id,
            "from_identity": entry.from_identity,
            "mime": entry.mime,
            "name": entry.name,
            "checksum": entry.checksum,
            "total_size": entry.total_size as u64,
            "msg_id": msg_id,
            "file_path": if file_path.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(file_path) },
            "data_b64": data_b64,
        })
        .to_string();
        map.remove(&msg_id);
        drop(map);
        emit_inbound(user_id, ev);
    }
    Ok(())
}

/// Test helper and external entry to feed a media-chunk JSON envelope into the
/// reassembly logic. Accepts the exact JSON sent over the data channel as text.
pub fn handle_media_chunk_json(user_id: i64, json: &str) -> Result<(), String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("invalid media json: {e}"))?;
    handle_media_chunk(v, user_id)
}

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
    negotiating: Arc<std::sync::atomic::AtomicBool>,
    remote_id: i64,
}

static PEERS: Lazy<std::sync::Mutex<std::collections::HashMap<i64, Peer>>> =
    Lazy::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

// Inbound messages from RTC DC to be forwarded to the app sink per user.
static DC_INBOUND: Lazy<
    std::sync::Mutex<std::collections::HashMap<i64, std::sync::mpsc::Sender<String>>>,
> = Lazy::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

// Outgoing message queues per remote peer when DC is not yet open
static QUEUES: Lazy<std::sync::Mutex<std::collections::HashMap<i64, Vec<String>>>> =
    Lazy::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

pub fn register_inbound_sink(user_id: i64, tx: std::sync::mpsc::Sender<String>) {
    DC_INBOUND.lock().unwrap().insert(user_id, tx);
}

fn emit_inbound(user_id: i64, data: String) {
    if let Some(tx) = DC_INBOUND.lock().unwrap().get(&user_id).cloned() {
        let _ = tx.send(data);
    }
}

#[allow(clippy::collapsible_if, clippy::needless_borrow)]
fn decrypt_body_in_event(text: &str) -> String {
    let mut forward = text.to_string();
    if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(text) {
        if let Some(b) = v.get("body").and_then(|s| s.as_str()) {
            // First try proper E2EE decrypt
            if let Ok(pt) = crate::security::decrypt_from_envelope(b) {
                if let Ok(s) = String::from_utf8(pt) {
                    if let Some(slot) = v.get_mut("body") {
                        *slot = serde_json::Value::String(s);
                    }
                    forward = v.to_string();
                    return forward;
                }
            }
            // Fallback: dev wrapper v1:PlainEph:Nonce:<b64-plaintext>
            let parts: Vec<&str> = b.split(':').collect();
            if parts.len() == 4
                && parts[0] == "v1"
                && parts[1] == "UGxhaW5FcGg="
                && parts[2] == "Tm9uY2U="
            {
                if let Ok(ct) = base64::engine::general_purpose::STANDARD.decode(parts[3]) {
                    if let Ok(s) = String::from_utf8(ct) {
                        if let Some(slot) = v.get_mut("body") {
                            *slot = serde_json::Value::String(s);
                        }
                        forward = v.to_string();
                    }
                }
            }
        }
    }
    forward
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
        negotiating: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        remote_id,
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
    let dc_neg_flag = Arc::clone(&peer.negotiating);
    let dc_slot = Arc::clone(&peer.dc);
    let this_user = ice_user;
    pc.on_data_channel(Box::new(move |dc: Arc<RTCDataChannel>| {
        let dc_open_flag_open = Arc::clone(&dc_open_flag);
        let dc_neg_flag_open = Arc::clone(&dc_neg_flag);
        let dc_slot_set = Arc::clone(&dc_slot);
        let dc_open_flag_close = Arc::clone(&dc_open_flag);
        let dc_neg_flag_close = Arc::clone(&dc_neg_flag);
        let dc_slot_clear = Arc::clone(&dc_slot);
        Box::pin(async move {
            let rid = peer.remote_id;
            dc.on_open(Box::new(move || {
                dc_open_flag_open.store(true, std::sync::atomic::Ordering::SeqCst);
                dc_neg_flag_open.store(false, std::sync::atomic::Ordering::SeqCst);
                println!("[rtc] data channel open");
                // Flush queued messages for this remote id
                if let Some(mut pending) = QUEUES.lock().unwrap().remove(&rid) {
                    for msg in pending.drain(..) {
                        let _ = crate::webrtc::send_over_dc(rid, msg);
                    }
                }
                Box::pin(async {})
            }));
            // When DC closes, mark flags and attempt graceful re-offer later.
            let my_user_id = this_user;
            dc.on_close(Box::new(move || {
                let rid = rid;
                dc_open_flag_close.store(false, std::sync::atomic::Ordering::SeqCst);
                dc_neg_flag_close.store(false, std::sync::atomic::Ordering::SeqCst);
                // clear dc slot asynchronously
                let dc_slot_for_close = Arc::clone(&dc_slot_clear);
                let fut = async move {
                    let mut slot = dc_slot_for_close.lock().await;
                    *slot = None;
                };
                RT.block_on(fut);
                // Small delayed re-offer; use thread sleep to avoid needing timers here
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    let _ = crate::webrtc::ensure_offer(my_user_id, rid);
                });
                Box::pin(async {})
            }));
            // Forward incoming DC messages to the app sink via DC_INBOUND
            dc.on_message(Box::new(move |msg: DataChannelMessage| {
                let this_user = this_user;
                Box::pin(async move {
                    if let Ok(text) = std::str::from_utf8(&msg.data) {
                        // Attempt media reassembly first (JSON media envelope)
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(text)
                            && v.get("type").and_then(|s| s.as_str()) == Some("media")
                        {
                            if let Err(e) = handle_media_chunk(v, this_user) {
                                eprintln!("[rtc] media chunk error: {}", e);
                            }
                            return;
                        }
                        // Drop messages from unknown contacts when identity is provided
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(text)
                            && v.get("from_identity").and_then(|s| s.as_str()).is_some()
                        {
                            let fid = v.get("from_identity").and_then(|s| s.as_str()).unwrap();
                            let _ = crate::local_storage::init_storage();
                            if matches!(crate::local_storage::get_contact_pubkey(fid), Ok(None)) {
                                return;
                            }
                        }
                        println!(
                            "[rtc] rx (user {}) {} bytes: {}",
                            this_user,
                            text.len(),
                            text
                        );
                        let forward = decrypt_body_in_event(text);
                        emit_inbound(this_user, forward);
                    } else {
                        // For now ignore raw binary frames in this revision.
                    }
                })
            }));
            // store channel
            *dc_slot_set.lock().await = Some(Arc::clone(&dc));
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
    // If already open or in progress, do nothing.
    if peer.open.load(std::sync::atomic::Ordering::SeqCst)
        || peer.negotiating.load(std::sync::atomic::Ordering::SeqCst)
    {
        return Ok(());
    }
    // Check if a channel already exists in the slot; if so, assume negotiation pending.
    let has_dc = if tokio::runtime::Handle::try_current().is_ok() {
        // Can't block here; approximate by checking without await (use try_lock via now_or_never pattern)
        false
    } else {
        RT.block_on(async { peer.dc.lock().await.is_some() })
    };
    if has_dc {
        return Ok(());
    }
    peer.negotiating
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let pc = Arc::clone(&peer.pc);
    let dc_slot = Arc::clone(&peer.dc);
    let open_flag = Arc::clone(&peer.open);
    let neg_flag = Arc::clone(&peer.negotiating);
    let fut = async move {
        // Create data channel as initiator and generate offer
        // Skip if slot already filled by a remote-initiated channel
        {
            let existing = dc_slot.lock().await;
            if existing.is_some() {
                drop(existing);
            } else {
                drop(existing);
                let dc = pc
                    .create_data_channel("msg", None)
                    .await
                    .map_err(|e| format!("dc: {e}"))?;
                {
                    let mut slot = dc_slot.lock().await;
                    *slot = Some(Arc::clone(&dc));
                }
                dc.on_open(Box::new({
                    let open = Arc::clone(&open_flag);
                    let neg = Arc::clone(&neg_flag);
                    let rid = remote_id;
                    move || {
                        open.store(true, std::sync::atomic::Ordering::SeqCst);
                        neg.store(false, std::sync::atomic::Ordering::SeqCst);
                        println!("[rtc] data channel open (initiator)");
                        // Flush queued messages for this remote id
                        if let Some(mut pending) = QUEUES.lock().unwrap().remove(&rid) {
                            for msg in pending.drain(..) {
                                let _ = crate::webrtc::send_over_dc(rid, msg);
                            }
                        }
                        Box::pin(async {})
                    }
                }));
                // Attach on_message for initiator as well, so it can receive.
                let my_user = user_id;
                let rid2 = remote_id;
                dc.on_message(Box::new(move |msg: DataChannelMessage| {
                    let my_user = my_user;
                    let rid2 = rid2;
                    Box::pin(async move {
                        if let Ok(text) = std::str::from_utf8(&msg.data) {
                            // Attempt media reassembly first (JSON media envelope)
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(text)
                                && v.get("type").and_then(|s| s.as_str()) == Some("media")
                            {
                                if let Err(e) = handle_media_chunk(v, my_user) {
                                    eprintln!("[rtc] media chunk error: {}", e);
                                }
                                return;
                            }
                            // Drop messages from unknown contacts when identity is provided
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(text)
                                && v.get("from_identity").and_then(|s| s.as_str()).is_some()
                            {
                                let fid = v.get("from_identity").and_then(|s| s.as_str()).unwrap();
                                let _ = crate::local_storage::init_storage();
                                if matches!(crate::local_storage::get_contact_pubkey(fid), Ok(None))
                                {
                                    return;
                                }
                            }
                            println!(
                                "[rtc] rx (user {} from {}) {} bytes: {}",
                                my_user,
                                rid2,
                                text.len(),
                                text
                            );
                            let forward = decrypt_body_in_event(text);
                            emit_inbound(my_user, forward);
                        } else {
                            // For now ignore raw binary frames in this revision.
                        }
                    })
                }));
                // Attach on_close symmetric to receiver path
                let dc_slot_clear = Arc::clone(&dc_slot);
                let open_for_close = Arc::clone(&open_flag);
                let neg_for_close = Arc::clone(&neg_flag);
                dc.on_close(Box::new(move || {
                    open_for_close.store(false, std::sync::atomic::Ordering::SeqCst);
                    neg_for_close.store(false, std::sync::atomic::Ordering::SeqCst);
                    let fut = async {
                        let mut slot = dc_slot_clear.lock().await;
                        *slot = None;
                    };
                    RT.block_on(fut);
                    // Re-offer after a short delay
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(200));
                        let _ = crate::webrtc::ensure_offer(user_id, remote_id);
                    });
                    Box::pin(async {})
                }));
            }
        }
        let offer = pc
            .create_offer(None)
            .await
            .map_err(|e| format!("offer: {e}"))?;
        pc.set_local_description(offer.clone())
            .await
            .map_err(|e| format!("set_local: {e}"))?;
        let payload = serde_json::to_string(&offer).map_err(|e| format!("serde: {e}"))?;
        let res = start_rtc_offer_over_stream(user_id, remote_id, payload);
        if res.is_err() {
            // allow retry later
            neg_flag.store(false, std::sync::atomic::Ordering::SeqCst);
        }
        res
    };
    if tokio::runtime::Handle::try_current().is_ok() {
        tokio::spawn(async move {
            let _ = fut.await;
        });
        Ok(())
    } else {
        RT.block_on(fut)
    }
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
    let fut = async move {
        let dc_opt = peer.dc.lock().await.clone();
        let Some(dc) = dc_opt else {
            return Err::<(), String>("no data channel".into());
        };
        let payload = data;
        let len = payload.len();
        dc.send_text(payload.clone())
            .await
            .map(|_| {
                println!("[rtc] tx (remote {}) {} bytes", remote_id, len);
            })
            .map_err(|e| format!("dc send: {e}"))
    };
    if tokio::runtime::Handle::try_current().is_ok() {
        tokio::spawn(fut);
        Ok(())
    } else {
        RT.block_on(fut)
    }
}

pub fn queue_or_send(remote_id: i64, event_json: String) -> Result<(), String> {
    if is_channel_open(remote_id) {
        return send_over_dc(remote_id, event_json);
    }
    // Queue until DC opens
    let mut g = QUEUES.lock().unwrap();
    println!(
        "[rtc] queue (remote {}) {} bytes (channel not open)",
        remote_id,
        event_json.len()
    );
    g.entry(remote_id).or_default().push(event_json);
    Ok(())
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
