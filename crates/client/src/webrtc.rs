use crate::api::{ClientMessage, SESSIONS};
use base64::Engine as _;
use once_cell::sync::Lazy;
use rura_models::webrtc::{CallMediaProfile, IceCandidate, RtcAnswer, RtcOffer};

// WebRTC crates
use rand::Rng;
use sha2::{Digest, Sha256};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::task;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::{MediaEngine, MIME_TYPE_OPUS, MIME_TYPE_VP8};
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::rtp_transceiver::rtp_codec::{RTCRtpCodecCapability, RTPCodecType};
use webrtc::rtp_transceiver::rtp_receiver::RTCRtpReceiver;
use webrtc::rtp_transceiver::RTCRtpTransceiver;
use webrtc::rtp_transceiver::rtp_sender::RTCRtpSender;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;
use webrtc::track::track_local::TrackLocal;
use webrtc::track::track_remote::TrackRemote;

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
        let file_path =
            match crate::local_storage::save_bytes_by_mime(&all, Some(&filename), &entry.mime) {
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

fn run_on_rt<F, T>(fut: F) -> T
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    if tokio::runtime::Handle::try_current().is_ok() {
        task::block_in_place(|| RT.block_on(fut))
    } else {
        RT.block_on(fut)
    }
}

#[derive(Default)]
struct DevicePreference {
    microphone: Option<String>,
    camera: Option<String>,
}

static DEVICE_PREF: Lazy<std::sync::Mutex<DevicePreference>> =
    Lazy::new(|| std::sync::Mutex::new(DevicePreference::default()));

struct MediaState {
    audio_track: Arc<TrackLocalStaticSample>,
    video_track: Arc<TrackLocalStaticSample>,
    audio_sender: tokio::sync::Mutex<Option<Arc<RTCRtpSender>>>,
    video_sender: tokio::sync::Mutex<Option<Arc<RTCRtpSender>>>,
    audio_enabled: AtomicBool,
    video_enabled: AtomicBool,
    audio_muted: AtomicBool,
    video_muted: AtomicBool,
}

impl MediaState {
    fn new(pc: &Arc<RTCPeerConnection>) -> Result<Self, String> {
        let pc = Arc::clone(pc);
        run_on_rt(async move {
            let audio_track = Arc::new(TrackLocalStaticSample::new(
                RTCRtpCodecCapability {
                    mime_type: MIME_TYPE_OPUS.to_owned(),
                    clock_rate: 48_000,
                    channels: 2,
                    ..Default::default()
                },
                random_label("audio"),
                random_label("audio-stream"),
            ));
            let video_track = Arc::new(TrackLocalStaticSample::new(
                RTCRtpCodecCapability {
                    mime_type: MIME_TYPE_VP8.to_owned(),
                    clock_rate: 90_000,
                    ..Default::default()
                },
                random_label("video"),
                random_label("video-stream"),
            ));

            let audio_sender = pc
                .add_track(Arc::clone(&audio_track) as Arc<dyn TrackLocal + Send + Sync>)
                .await
                .map_err(|e| format!("audio track: {e}"))?;

            let video_sender = pc
                .add_track(Arc::clone(&video_track) as Arc<dyn TrackLocal + Send + Sync>)
                .await
                .map_err(|e| format!("video track: {e}"))?;

            Ok(Self {
                audio_track,
                video_track,
                audio_sender: tokio::sync::Mutex::new(Some(audio_sender)),
                video_sender: tokio::sync::Mutex::new(Some(video_sender)),
                audio_enabled: AtomicBool::new(false),
                video_enabled: AtomicBool::new(false),
                audio_muted: AtomicBool::new(false),
                video_muted: AtomicBool::new(false),
            })
        })
    }

    async fn apply_sender_state(&self) -> Result<(), String> {
        self.apply_single_sender(
            &self.audio_sender,
            &self.audio_track,
            self.audio_enabled.load(Ordering::SeqCst)
                && !self.audio_muted.load(Ordering::SeqCst),
        )
        .await?;
        self.apply_single_sender(
            &self.video_sender,
            &self.video_track,
            self.video_enabled.load(Ordering::SeqCst)
                && !self.video_muted.load(Ordering::SeqCst),
        )
        .await?;
        Ok(())
    }

    async fn apply_single_sender(
        &self,
        sender_slot: &tokio::sync::Mutex<Option<Arc<RTCRtpSender>>>,
        track: &Arc<TrackLocalStaticSample>,
        should_send: bool,
    ) -> Result<(), String> {
        let _ = (sender_slot, track, should_send);
        // Media sending is not yet wired up; keep tracks as created to avoid
        // hitting replace_track() envelope errors in the underlying WebRTC
        // implementation. This function exists to preserve the async signature
        // and future-proof media toggling without impacting current behavior.
        Ok(())
    }

    fn summary(&self) -> serde_json::Value {
        serde_json::json!({
            "audio_enabled": self.audio_enabled.load(Ordering::SeqCst),
            "video_enabled": self.video_enabled.load(Ordering::SeqCst),
            "audio_muted": self.audio_muted.load(Ordering::SeqCst),
            "video_muted": self.video_muted.load(Ordering::SeqCst),
        })
    }

    fn update_flags(
        &self,
        audio_enabled: bool,
        video_enabled: bool,
        audio_muted: bool,
        video_muted: bool,
    ) {
        self.audio_enabled.store(audio_enabled, Ordering::SeqCst);
        self.video_enabled.store(video_enabled, Ordering::SeqCst);
        self.audio_muted.store(audio_muted, Ordering::SeqCst);
        self.video_muted.store(video_muted, Ordering::SeqCst);
    }
}

fn random_label(prefix: &str) -> String {
    let mut rng = rand::thread_rng();
    format!("{}-{:08x}", prefix, rng.r#gen::<u32>())
}

#[derive(Clone)]
struct Peer {
    pc: Arc<RTCPeerConnection>,
    dc: Arc<tokio::sync::Mutex<Option<Arc<RTCDataChannel>>>>,
    open: Arc<AtomicBool>,
    negotiating: Arc<AtomicBool>,
    remote_id: i64,
    owner_id: i64,
    media: Arc<MediaState>,
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

fn emit_media_event(owner_id: i64, remote_id: i64, event: &str, data: serde_json::Value) {
    let envelope = serde_json::json!({
        "type": "rtc_media",
        "event": event,
        "remote_user_id": remote_id,
        "call_id": crate::api::call_id_for_remote(remote_id),
        "data": data,
    });
    emit_inbound(owner_id, envelope.to_string());
}

fn track_snapshot(track: &TrackRemote) -> serde_json::Value {
    serde_json::json!({
        "track_id": track.id(),
        "stream_id": track.stream_id(),
        "kind": codec_kind(track.kind()),
        "ssrc": track.ssrc(),
        "rid": track.rid(),
        "texture_id": serde_json::Value::Null,
    })
}

fn codec_kind(kind: RTPCodecType) -> &'static str {
    match kind {
        RTPCodecType::Audio => "audio",
        RTPCodecType::Video => "video",
        _ => "unknown",
    }
}

fn wire_data_channel(
    dc: &Arc<RTCDataChannel>,
    remote_id: i64,
    owner_id: i64,
    open_flag: Arc<AtomicBool>,
    neg_flag: Arc<AtomicBool>,
    slot: Arc<tokio::sync::Mutex<Option<Arc<RTCDataChannel>>>>,
    initiator: bool,
) {
    let rid = remote_id;
    let open_for_open = Arc::clone(&open_flag);
    let neg_for_open = Arc::clone(&neg_flag);
    dc.on_open(Box::new(move || {
        open_for_open.store(true, Ordering::SeqCst);
        neg_for_open.store(false, Ordering::SeqCst);
        println!(
            "[rtc] data channel open{}",
            if initiator { " (initiator)" } else { "" }
        );
        if let Some(mut pending) = QUEUES.lock().unwrap().remove(&rid) {
            for msg in pending.drain(..) {
                let _ = crate::webrtc::send_over_dc(rid, msg);
            }
        }
        Box::pin(async {})
    }));

    let open_for_close = Arc::clone(&open_flag);
    let neg_for_close = Arc::clone(&neg_flag);
    let slot_for_close = Arc::clone(&slot);
    dc.on_close(Box::new(move || {
        open_for_close.store(false, Ordering::SeqCst);
        neg_for_close.store(false, Ordering::SeqCst);
        let slot_inner = Arc::clone(&slot_for_close);
        run_on_rt(async move {
            let mut guard = slot_inner.lock().await;
            guard.take();
        });
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200));
            let _ = crate::webrtc::ensure_offer(owner_id, rid);
        });
        Box::pin(async {})
    }));

    dc.on_message(Box::new(move |msg: DataChannelMessage| {
        let owner = owner_id;
        Box::pin(async move {
            if let Ok(text) = std::str::from_utf8(&msg.data) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(text)
                    && v.get("type").and_then(|s| s.as_str()) == Some("media")
                {
                    if let Err(e) = handle_media_chunk(v, owner) {
                        eprintln!("[rtc] media chunk error: {}", e);
                    }
                    return;
                }
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(text)
                    && v.get("from_identity").and_then(|s| s.as_str()).is_some()
                {
                    let fid = v.get("from_identity").and_then(|s| s.as_str()).unwrap();
                    let _ = crate::local_storage::init_storage();
                    if matches!(crate::local_storage::get_contact_pubkey(fid), Ok(None)) {
                        return;
                    }
                }
                println!("[rtc] rx (user {}) {} bytes: {}", owner, text.len(), text);
                let forward = decrypt_body_in_event(text);
                emit_inbound(owner, forward);
            }
        })
    }));
}

async fn ensure_data_channel(peer: &Peer) -> Result<(), String> {
    let slot = Arc::clone(&peer.dc);
    {
        if slot.lock().await.is_some() {
            return Ok(());
        }
    }
    let dc = peer
        .pc
        .create_data_channel("msg", None)
        .await
        .map_err(|e| format!("dc: {e}"))?;
    {
        let mut guard = slot.lock().await;
        *guard = Some(Arc::clone(&dc));
    }
    wire_data_channel(
        &dc,
        peer.remote_id,
        peer.owner_id,
        Arc::clone(&peer.open),
        Arc::clone(&peer.negotiating),
        slot,
        true,
    );
    Ok(())
}

fn spawn_offer(
    peer: Peer,
    user_id: i64,
    remote_id: i64,
    ensure_dc: bool,
    skip_if_open: bool,
) -> Result<(), String> {
    if skip_if_open && peer.open.load(Ordering::SeqCst) {
        return Ok(());
    }
    if peer
        .negotiating
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        // A negotiation is already in flight for this peer. Rather than
        // failing the higher-level API (which surfaces as user-visible
        // errors like "accept failed: negotiation already in progress"),
        // treat this as a no-op and let the in-flight negotiation settle.
        return Ok(());
    }
    let pc = Arc::clone(&peer.pc);
    let neg_flag = Arc::clone(&peer.negotiating);
    let media = Arc::clone(&peer.media);
    let owner = peer.owner_id;
    let peer_clone = peer.clone();
    let neg_flag_for_fut = Arc::clone(&neg_flag);
    let fut = async move {
        if ensure_dc {
            ensure_data_channel(&peer_clone).await?;
        }
        media
            .apply_sender_state()
            .await
            .map_err(|e| format!("media state: {e}"))?;
        emit_media_event(owner, remote_id, "local_state", media.summary());
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
            neg_flag_for_fut.store(false, Ordering::SeqCst);
        }
        res
    };
    if tokio::runtime::Handle::try_current().is_ok() {
        let neg_flag_for_spawn = Arc::clone(&neg_flag);
        tokio::spawn(async move {
            if let Err(e) = fut.await {
                eprintln!("[rtc] offer error: {e}");
                neg_flag_for_spawn.store(false, Ordering::SeqCst);
            }
        });
        Ok(())
    } else {
        let res = RT.block_on(fut);
        if res.is_err() {
            neg_flag.store(false, Ordering::SeqCst);
        }
        res
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
    let media_state = MediaState::new(&pc)?;
    let media = Arc::new(media_state);
    let peer = Peer {
        pc: Arc::clone(&pc),
        dc: Arc::new(tokio::sync::Mutex::new(None)),
        open: Arc::new(AtomicBool::new(false)),
        negotiating: Arc::new(AtomicBool::new(false)),
        remote_id,
        owner_id: user_id,
        media: Arc::clone(&media),
    };

    // ICE candidates: forward to remote via server
    let ice_user = user_id;
    let ice_remote = remote_id;
    pc.on_ice_candidate(Box::new(
        move |cand: Option<webrtc::ice_transport::ice_candidate::RTCIceCandidate>| {
            let ice_user = ice_user;
            let ice_remote = ice_remote;
            Box::pin(async move {
                if let Some(c) = cand && let Ok(json) = c.to_json() {
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

    // Remote media tracks -> forward metadata back to Flutter/UI layer
    let track_owner = user_id;
    let track_remote = remote_id;
    pc.on_track(Box::new(
        move |track: Arc<TrackRemote>,
              _receiver: Arc<RTCRtpReceiver>,
              _transceiver: Arc<RTCRtpTransceiver>| {
        let added_owner = track_owner;
        let added_remote = track_remote;
        Box::pin(async move {
            emit_media_event(added_owner, added_remote, "track_added", track_snapshot(&track));

            let mute_track: Arc<TrackRemote> = Arc::clone(&track);
            track.onmute(Box::new(move || {
                let mute_track: Arc<TrackRemote> = Arc::clone(&mute_track);
                let fut: Pin<Box<dyn Future<Output = ()> + Send + 'static>> =
                    Box::pin(async move {
                        emit_media_event(
                            added_owner,
                            added_remote,
                            "track_muted",
                            serde_json::json!({
                                "track_id": mute_track.id(),
                            }),
                        );
                    });
                fut
            }));

            let unmute_track: Arc<TrackRemote> = Arc::clone(&track);
            track.onunmute(Box::new(move || {
                let unmute_track: Arc<TrackRemote> = Arc::clone(&unmute_track);
                let fut: Pin<Box<dyn Future<Output = ()> + Send + 'static>> =
                    Box::pin(async move {
                        emit_media_event(
                            added_owner,
                            added_remote,
                            "track_unmuted",
                            serde_json::json!({
                                "track_id": unmute_track.id(),
                            }),
                        );
                    });
                fut
            }));

            let reader_track: Arc<TrackRemote> = Arc::clone(&track);
            let owner_for_reader = added_owner;
            let remote_for_reader = added_remote;
            RT.spawn(async move {
                let mut buf = vec![0u8; 1_200];
                loop {
                    match reader_track.read(&mut buf).await {
                        Ok(_) => {}
                        Err(e) => {
                            emit_media_event(
                                owner_for_reader,
                                remote_for_reader,
                                "track_closed",
                                serde_json::json!({
                                    "track_id": reader_track.id(),
                                    "error": e.to_string(),
                                }),
                            );
                            break;
                        }
                    }
                }
            });
        })
    }));

    let dc_slot = Arc::clone(&peer.dc);
    let dc_open_flag = Arc::clone(&peer.open);
    let dc_neg_flag = Arc::clone(&peer.negotiating);
    pc.on_data_channel(Box::new(move |dc: Arc<RTCDataChannel>| {
        let slot = Arc::clone(&dc_slot);
        let open_flag = Arc::clone(&dc_open_flag);
        let neg_flag = Arc::clone(&dc_neg_flag);
        let rid = remote_id;
        let owner = user_id;
        Box::pin(async move {
            {
                let mut guard = slot.lock().await;
                *guard = Some(Arc::clone(&dc));
            }
            wire_data_channel(&dc, rid, owner, open_flag, neg_flag, slot, false);
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
    spawn_offer(peer, user_id, remote_id, true, true)
}

pub fn renegotiate_media(user_id: i64, remote_id: i64) -> Result<(), String> {
    let peer = get_or_create_peer(user_id, remote_id)?;
    spawn_offer(peer, user_id, remote_id, false, false)
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
        return peer.open.load(Ordering::SeqCst);
    }
    false
}

pub fn send_over_dc(remote_id: i64, data: String) -> Result<(), String> {
    let Some(peer) = PEERS.lock().unwrap().get(&remote_id).cloned() else {
        return Err("no rtc peer".into());
    };
    if !peer.open.load(Ordering::SeqCst) {
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

pub fn set_media_devices(microphone: Option<String>, camera: Option<String>) {
    let mut prefs = DEVICE_PREF.lock().unwrap();
    prefs.microphone = microphone;
    prefs.camera = camera;
}

pub fn current_media_devices() -> (Option<String>, Option<String>) {
    let prefs = DEVICE_PREF.lock().unwrap();
    (prefs.microphone.clone(), prefs.camera.clone())
}

pub fn teardown_peer(remote_id: i64) {
    if let Some(peer) = PEERS.lock().unwrap().remove(&remote_id) {
        let pc = Arc::clone(&peer.pc);
        run_on_rt(async move {
            let _ = pc.close().await;
        });
        emit_media_event(
            peer.owner_id,
            remote_id,
            "peer_closed",
            serde_json::json!({}),
        );
    }
}

pub fn apply_call_media_profile(
    user_id: i64,
    remote_id: i64,
    profile: &CallMediaProfile,
) -> Result<(), String> {
    let peer = get_or_create_peer(user_id, remote_id)?;
    peer.media.update_flags(
        profile.audio_enabled,
        profile.video_enabled,
        profile.audio_muted.unwrap_or(false),
        profile.video_muted.unwrap_or(false),
    );
    spawn_offer(peer, user_id, remote_id, false, false)
}

pub fn update_mute_state(
    user_id: i64,
    remote_id: i64,
    audio_muted: Option<bool>,
    video_muted: Option<bool>,
) -> Result<(), String> {
    let peer = get_or_create_peer(user_id, remote_id)?;
    let audio_enabled = peer.media.audio_enabled.load(Ordering::SeqCst);
    let video_enabled = peer.media.video_enabled.load(Ordering::SeqCst);
    let mut audio_flag = peer.media.audio_muted.load(Ordering::SeqCst);
    let mut video_flag = peer.media.video_muted.load(Ordering::SeqCst);
    if let Some(flag) = audio_muted {
        audio_flag = flag;
    }
    if let Some(flag) = video_muted {
        video_flag = flag;
    }
    peer
        .media
        .update_flags(audio_enabled, video_enabled, audio_flag, video_flag);
    spawn_offer(peer, user_id, remote_id, false, false)
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
        call_id: crate::api::call_id_for_remote(to_user_id),
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
        call_id: crate::api::call_id_for_remote(to_user_id),
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
        call_id: crate::api::call_id_for_remote(to_user_id),
        sdp_mid,
        sdp_mline_index,
        track: None,
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

    #[test]
    fn device_preferences_round_trip() {
        set_media_devices(Some("mic".into()), Some("cam".into()));
        let (mic, cam) = current_media_devices();
        assert_eq!(mic.as_deref(), Some("mic"));
        assert_eq!(cam.as_deref(), Some("cam"));
        set_media_devices(None, None);
    }
}
