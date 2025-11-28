use crate::StreamSink;
use base64::{Engine as _, engine::general_purpose};
use flutter_rust_bridge::frb;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;
// Type aliases so FRB's `use crate::api::*` can refer to these types directly
pub type AuthRequest = rura_models::client_message::AuthRequest;
pub type AuthResponse = rura_models::client_message::AuthResponse;
pub type ClientMessage = rura_models::client_message::ClientMessage;
// NOTE: Keep client-local history/message structs to avoid tight coupling to rura_models.
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
// serde derives are referenced via fully qualified paths in this file; no direct import needed
use std::io::{self, Read, Write};
use std::net::TcpStream;
// no path utilities needed after removing legacy JSON cache
use crate::webrtc;
use rand::RngCore;
use rura_models::webrtc::{
    CallAnswer, CallEndReason, CallHangup, CallInvite, CallMediaProfile, CallReject,
};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Once};

fn session_id_from_identity(id_b64: &str) -> Result<i64, String> {
    // Derive a stable 63-bit numeric from the 256-bit base64 identity
    let bytes = general_purpose::STANDARD
        .decode(id_b64)
        .map_err(|e| format!("bad id base64: {e}"))?;
    if bytes.len() < 8 {
        return Err("identity too short".to_string());
    }
    let slice: [u8; 8] = bytes[0..8].try_into().unwrap();
    let v = u64::from_be_bytes(slice) & 0x7FFF_FFFF_FFFF_FFFF; // keep it positive
    Ok(v as i64)
}

/// Simple Dart-friendly login response.
#[frb]
#[derive(Clone, Debug)]
pub struct LoginResponse {
    pub success: bool,
    pub message: String,
    pub user_id: Option<i64>,
}

/// Dart-friendly history message mirrored from server-side model.
#[frb]
#[derive(Clone, Debug)]
pub struct HistoryMessage {
    pub id: i64,
    pub from_user_id: i64,
    pub to_user_id: i64,
    pub body: String,
    pub timestamp: String,
}

// Use shared protocol models from rura_models for internal serialization.
pub type ModelHistoryMessage = rura_models::messaging::HistoryMessage;

impl From<ModelHistoryMessage> for HistoryMessage {
    fn from(src: ModelHistoryMessage) -> Self {
        Self {
            id: src.id,
            from_user_id: src.from_user_id,
            to_user_id: src.to_user_id,
            body: src.body,
            timestamp: src.timestamp,
        }
    }
}

pub type HistoryRequest = rura_models::messaging::HistoryRequest;
pub type HistoryResponse = rura_models::messaging::HistoryResponse;

static CALL_STATE: Lazy<std::sync::Mutex<Option<CallState>>> =
    Lazy::new(|| std::sync::Mutex::new(None));

#[frb]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallDirection {
    Incoming,
    Outgoing,
}

#[frb]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallStatus {
    Ringing,
    Connected,
}

#[frb]
#[derive(Clone, Debug)]
pub struct CallState {
    pub call_id: String,
    pub remote_user_id: i64,
    pub direction: CallDirection,
    pub status: CallStatus,
    pub audio_enabled: bool,
    pub video_enabled: bool,
}

impl From<crate::local_storage::StoredCallState> for CallState {
    fn from(row: crate::local_storage::StoredCallState) -> Self {
        Self {
            call_id: row.call_id,
            remote_user_id: row.remote_user_id,
            direction: if row.direction == "incoming" {
                CallDirection::Incoming
            } else {
                CallDirection::Outgoing
            },
            status: if row.status == "connected" {
                CallStatus::Connected
            } else {
                CallStatus::Ringing
            },
            audio_enabled: row.audio_enabled,
            video_enabled: row.video_enabled,
        }
    }
}

impl From<&CallState> for crate::local_storage::StoredCallState {
    fn from(state: &CallState) -> Self {
        Self {
            call_id: state.call_id.clone(),
            remote_user_id: state.remote_user_id,
            direction: match state.direction {
                CallDirection::Incoming => "incoming".to_string(),
                CallDirection::Outgoing => "outgoing".to_string(),
            },
            status: match state.status {
                CallStatus::Connected => "connected".to_string(),
                CallStatus::Ringing => "ringing".to_string(),
            },
            audio_enabled: state.audio_enabled,
            video_enabled: state.video_enabled,
        }
    }
}

fn load_active_call_state() -> Result<Option<CallState>, String> {
    if let Some(state) = CALL_STATE.lock().unwrap().clone() {
        return Ok(Some(state));
    }
    crate::local_storage::init_storage()?;
    let loaded = crate::local_storage::load_call_state()?.map(CallState::from);
    let mut guard = CALL_STATE.lock().unwrap();
    *guard = loaded.clone();
    Ok(loaded)
}

fn persist_active_call_state(state: Option<CallState>) -> Result<(), String> {
    crate::local_storage::init_storage()?;
    {
        let mut guard = CALL_STATE.lock().unwrap();
        *guard = state.clone();
    }
    match state {
        Some(ref entry) => {
            let stored = crate::local_storage::StoredCallState::from(entry);
            crate::local_storage::save_call_state(&stored)
        }
        None => crate::local_storage::clear_call_state(),
    }
}

fn update_call_state_if_match<F>(call_id: &str, updater: F) -> Result<(), String>
where
    F: FnOnce(&mut CallState),
{
    if let Some(mut state) = load_active_call_state()? {
        if state.call_id == call_id {
            updater(&mut state);
            persist_active_call_state(Some(state))?;
        }
    }
    Ok(())
}

fn clear_call_state_if_match(call_id: &str) -> Result<(), String> {
    if let Some(state) = load_active_call_state()? {
        if state.call_id == call_id {
            persist_active_call_state(None)?;
        }
    }
    Ok(())
}

fn generate_call_id() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

fn send_command_over_stream<T: serde::Serialize>(
    user_id: i64,
    command: &str,
    payload: &T,
) -> Result<(), String> {
    let msg = ClientMessage {
        command: command.to_string(),
        data: serde_json::to_string(payload)
            .map_err(|e| format!("serialize {command} payload: {e}"))?,
    };
    enqueue_command(user_id, msg)
}

pub(crate) fn call_id_for_remote(remote_user_id: i64) -> Option<String> {
    CALL_STATE
        .lock()
        .unwrap()
        .clone()
        .filter(|state| state.remote_user_id == remote_user_id)
        .map(|state| state.call_id)
}

fn record_call_invite(user_id: i64, invite: &CallInvite) -> Result<(), String> {
    if invite.from_user_id != user_id && invite.to_user_id != user_id {
        return Ok(());
    }
    let direction = if invite.from_user_id == user_id {
        CallDirection::Outgoing
    } else {
        CallDirection::Incoming
    };
    let remote_user_id = if direction == CallDirection::Outgoing {
        invite.to_user_id
    } else {
        invite.from_user_id
    };
    let state = CallState {
        call_id: invite.call_id.clone(),
        remote_user_id,
        direction,
        status: CallStatus::Ringing,
        audio_enabled: invite.media.audio_enabled,
        video_enabled: invite.media.video_enabled,
    };
    persist_active_call_state(Some(state))?;
    Ok(())
}

fn record_call_answer(user_id: i64, answer: &CallAnswer) -> Result<(), String> {
    if answer.from_user_id != user_id && answer.to_user_id != user_id {
        return Ok(());
    }
    update_call_state_if_match(&answer.call_id, |state| {
        state.status = CallStatus::Connected;
        state.remote_user_id = if answer.from_user_id == user_id {
            answer.to_user_id
        } else {
            answer.from_user_id
        };
        if let Some(media) = &answer.resume_media {
            state.audio_enabled = media.audio_enabled;
            state.video_enabled = media.video_enabled;
        }
    })
}

fn record_call_ringing(call_id: &str) -> Result<(), String> {
    update_call_state_if_match(call_id, |state| {
        state.status = CallStatus::Ringing;
    })
}

fn record_call_end(call_id: &str) -> Result<(), String> {
    clear_call_state_if_match(call_id)
}

fn forward_call_event(sink: &StreamSink<String>, command: &str, payload: &str) {
    let line = format!(r#"{{"type":"{}","data":{}}}"#, command, payload);
    let _ = sink.add(line);
}

fn handle_incoming_call_command(user_id: i64, command: &str, payload: &str) {
    match command {
        "call_invite" => {
            if let Ok(invite) = serde_json::from_str::<CallInvite>(payload) {
                if let Err(e) = record_call_invite(user_id, &invite) {
                    eprintln!("[call] invite state error: {e}");
                }
            }
        }
        "call_ringing" => {
            #[derive(serde::Deserialize)]
            struct CallRingingView {
                call_id: String,
            }
            if let Ok(view) = serde_json::from_str::<CallRingingView>(payload) {
                if let Err(e) = record_call_ringing(&view.call_id) {
                    eprintln!("[call] ringing state error: {e}");
                }
            }
        }
        "call_answer" => {
            if let Ok(answer) = serde_json::from_str::<CallAnswer>(payload) {
                if let Err(e) = record_call_answer(user_id, &answer) {
                    eprintln!("[call] answer state error: {e}");
                }
            }
        }
        "call_reject" => {
            #[derive(serde::Deserialize)]
            struct CallRejectView {
                call_id: String,
            }
            if let Ok(view) = serde_json::from_str::<CallRejectView>(payload) {
                if let Err(e) = record_call_end(&view.call_id) {
                    eprintln!("[call] reject state error: {e}");
                }
            }
        }
        "call_hangup" => {
            #[derive(serde::Deserialize)]
            struct CallHangupView {
                call_id: String,
            }
            if let Ok(view) = serde_json::from_str::<CallHangupView>(payload) {
                if let Err(e) = record_call_end(&view.call_id) {
                    eprintln!("[call] hangup state error: {e}");
                }
            }
        }
        _ => {}
    }
}

// ---------- Local cache helpers ----------

// Removed legacy JSON chat cache helpers in favor of encrypted SQLite local storage.

#[frb]
pub fn append_local_message(
    from_user_id: i64,
    to_user_id: i64,
    body: String,
    timestamp: String,
) -> Result<(), String> {
    crate::local_storage::init_storage()?;
    // Persist messages to the on-disk database by default.
    crate::local_storage::append_persistent_message(from_user_id, to_user_id, body, timestamp)
}

#[frb]
pub fn load_local_history(limit: Option<usize>) -> Result<Vec<HistoryMessage>, String> {
    crate::local_storage::init_storage()?;
    crate::local_storage::load_history(limit)
}

/// Get the account's 256-bit random user_id (returns base64 string).
#[frb]
pub fn get_account_id() -> Result<String, String> {
    let identity = crate::security::load_identity()?;
    match identity {
        Some(bundle) => Ok(bundle.user_id),
        None => Err("No identity found. Please register first.".to_string()),
    }
}

/// Get the account's public key (base64).
#[frb]
pub fn get_account_pubkey() -> Result<String, String> {
    let identity = crate::security::load_identity()?;
    match identity {
        Some(bundle) => {
            // Prefer X25519 messaging key when available
            if let Some(xpk) = bundle.x25519_pub_b64 {
                Ok(xpk)
            } else {
                Ok(bundle.public_b64)
            }
        }
        None => Err("No identity found. Please register first.".to_string()),
    }
}

/// Add or update a contact locally with an ID (base64) and public key (base64).
#[frb]
pub fn add_contact(user_id: String, pubkey: String) -> Result<(), String> {
    crate::local_storage::init_storage()?;
    crate::local_storage::add_contact(user_id, pubkey, None)
}

/// Add or update a contact with optional nickname.
#[frb]
pub fn add_contact_with_nickname(
    user_id: String,
    pubkey: String,
    nickname: Option<String>,
) -> Result<(), String> {
    crate::local_storage::init_storage()?;
    crate::local_storage::add_contact(user_id, pubkey, nickname)
}

/// List contacts as a JSON array [{user_id, pubkey, nickname}].
#[frb]
pub fn list_contacts_json() -> Result<String, String> {
    crate::local_storage::init_storage()?;
    let rows = crate::local_storage::list_contacts()?;
    serde_json::to_string(&rows).map_err(|e| format!("serialize contacts: {e}"))
}

/// Encrypt plaintext for a contact identity using their published public key.
#[frb]
pub fn encrypt_message_for_identity(
    to_identity: String,
    plaintext: String,
) -> Result<String, String> {
    crate::local_storage::init_storage()?;
    let pk = crate::local_storage::get_contact_pubkey(&to_identity)?
        .ok_or_else(|| "Recipient not found or missing pubkey".to_string())?;
    crate::security::encrypt_for_recipient(plaintext.as_bytes(), &pk)
}

/// Decrypt a v1 envelope into plaintext using our private key.
#[frb]
pub fn decrypt_message_from_envelope(envelope: String) -> Result<String, String> {
    let pt = crate::security::decrypt_from_envelope(&envelope)?;
    String::from_utf8(pt).map_err(|_| "plaintext not valid UTF-8".to_string())
}

#[frb]
pub fn get_current_call_state() -> Result<Option<CallState>, String> {
    load_active_call_state()
}

#[frb]
pub fn start_call(
    user_id: i64,
    remote_user_id: i64,
    enable_video: bool,
) -> Result<CallState, String> {
    if user_id == remote_user_id {
        return Err("Cannot call yourself".to_string());
    }
    if load_active_call_state()?.is_some() {
        return Err("Another call is already active".to_string());
    }
    let call_id = generate_call_id();
    let media = CallMediaProfile {
        audio_enabled: true,
        video_enabled: enable_video,
        audio_muted: Some(false),
        video_muted: Some(!enable_video),
    };
    let invite = CallInvite {
        call_id: call_id.clone(),
        from_user_id: user_id,
        to_user_id: remote_user_id,
        media,
        preview: None,
        client: None,
        ringing_timeout_ms: None,
    };
    send_command_over_stream(user_id, "call_invite", &invite)?;
    let state = CallState {
        call_id,
        remote_user_id,
        direction: CallDirection::Outgoing,
        status: CallStatus::Ringing,
        audio_enabled: true,
        video_enabled: enable_video,
    };
    persist_active_call_state(Some(state.clone()))?;
    Ok(state)
}

#[frb]
pub fn accept_call(user_id: i64, call_id: String, enable_video: bool) -> Result<CallState, String> {
    let mut state = load_active_call_state()?
        .filter(|s| s.call_id == call_id)
        .ok_or_else(|| "No matching call to accept".to_string())?;
    let answer = CallAnswer {
        call_id: call_id.clone(),
        from_user_id: user_id,
        to_user_id: state.remote_user_id,
        resume_media: Some(CallMediaProfile {
            audio_enabled: true,
            video_enabled: enable_video,
            audio_muted: Some(false),
            video_muted: Some(!enable_video),
        }),
    };
    send_command_over_stream(user_id, "call_answer", &answer)?;
    state.status = CallStatus::Connected;
    state.video_enabled = enable_video;
    state.audio_enabled = true;
    persist_active_call_state(Some(state.clone()))?;
    Ok(state)
}

#[frb]
pub fn reject_call(user_id: i64, call_id: String, busy: bool) -> Result<(), String> {
    let state = load_active_call_state()?
        .filter(|s| s.call_id == call_id)
        .ok_or_else(|| "No matching call to reject".to_string())?;
    let reject = CallReject {
        call_id: call_id.clone(),
        from_user_id: user_id,
        to_user_id: state.remote_user_id,
        reason: Some(if busy {
            CallEndReason::Busy
        } else {
            CallEndReason::Rejected
        }),
        note: None,
    };
    send_command_over_stream(user_id, "call_reject", &reject)?;
    persist_active_call_state(None)
}

#[frb]
pub fn end_call(user_id: i64, call_id: String) -> Result<(), String> {
    let state = load_active_call_state()?
        .filter(|s| s.call_id == call_id)
        .ok_or_else(|| "No matching call to end".to_string())?;
    let hangup = CallHangup {
        call_id,
        from_user_id: user_id,
        to_user_id: state.remote_user_id,
        reason: Some(CallEndReason::Hangup),
    };
    send_command_over_stream(user_id, "call_hangup", &hangup)?;
    persist_active_call_state(None)
}

fn build_root_store_from_pem(pem: &str) -> Result<RootCertStore, String> {
    let mut reader = std::io::Cursor::new(pem.as_bytes());
    let certs_iter = rustls_pemfile::certs(&mut reader);
    let certs: Vec<CertificateDer<'static>> = certs_iter
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to parse PEM: {e}"))?;
    if certs.is_empty() {
        return Err("No certificates found in provided PEM".to_string());
    }
    let mut roots = RootCertStore::empty();
    for cert in certs {
        roots
            .add(cert)
            .map_err(|e| format!("Failed to add cert to RootCertStore: {e}"))?;
    }
    Ok(roots)
}

fn read_line(stream: &mut impl Read) -> io::Result<String> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    while let Ok(n) = stream.read(&mut byte) {
        if n == 0 {
            break;
        }
        if byte[0] == b'\n' {
            break;
        }
        buf.push(byte[0]);
    }
    Ok(String::from_utf8_lossy(&buf).to_string())
}

/// Login to the TLS-only server and return the auth response.
///
/// - `host`: e.g., "127.0.0.1" or "localhost"
/// - `port`: e.g., `8443`
/// - `ca_pem`: contents of the server's certificate (PEM) used as a root
/// - `passphrase`, `password`: user credentials
#[frb]
pub fn login_tls(
    host: String,
    port: u16,
    ca_pem: String,
    _passphrase: String,
    _password: String,
) -> Result<LoginResponse, String> {
    // Ensure a crypto provider is installed (rustls 0.23 requires this)
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
    // Build TLS client config with provided root
    let roots = build_root_store_from_pem(&ca_pem)?;
    let config: ClientConfig = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();

    let server_name = ServerName::try_from(host.as_str())
        .map_err(|e| format!("Invalid server name: {e}"))?
        .to_owned();
    let addr = format!("{}:{}", host, port);

    // TCP connect
    let tcp = TcpStream::connect(addr).map_err(|e| format!("TCP connect failed: {e}"))?;

    // TLS handshake
    let conn = ClientConnection::new(Arc::new(config), server_name)
        .map_err(|e| format!("TLS connect failed: {e}"))?;
    let mut tls = StreamOwned::new(conn, tcp);

    // Read initial auth_required line (ignore failures)
    let _ = read_line(&mut tls);

    // Send login envelope with identity id only
    let identity_id = crate::security::load_identity()?
        .map(|b| b.user_id)
        .unwrap_or_default();
    let payload = serde_json::json!({ "id": identity_id });
    let envelope = ClientMessage {
        command: "login".to_string(),
        data: payload.to_string(),
    };
    let mut line = serde_json::to_string(&envelope).map_err(|e| format!("Serialize error: {e}"))?;
    line.push('\n');
    tls.write_all(line.as_bytes())
        .map_err(|e| format!("Write failed: {e}"))?;
    tls.flush().map_err(|e| format!("Flush failed: {e}"))?;

    // Read auth_response
    let raw = read_line(&mut tls).map_err(|e| format!("Read failed: {e}"))?;
    let wrapper: ClientMessage = serde_json::from_str(&raw)
        .map_err(|e| format!("Invalid JSON from server: {e}; raw={raw}"))?;
    if wrapper.command != "auth_response" {
        return Err(format!("Unexpected command: {}", wrapper.command));
    }
    // Parse response and support identity-based field `id`
    let resp_val: serde_json::Value = serde_json::from_str(&wrapper.data)
        .map_err(|e| format!("Invalid auth_response data: {e}"))?;
    let resp: AuthResponse = serde_json::from_value(resp_val.clone())
        .map_err(|e| format!("Invalid auth_response shape: {e}"))?;
    let user_id = if let Some(uid) = resp.user_id {
        Some(uid)
    } else if let Some(id_str) = resp_val.get("id").and_then(|v| v.as_str()) {
        Some(session_id_from_identity(id_str)?)
    } else if let Some(id_local) = crate::security::load_identity()?.map(|b| b.user_id) {
        Some(session_id_from_identity(&id_local)?)
    } else {
        None
    };

    // Send a graceful TLS close_notify before dropping the connection so the
    // server does not report an unexpected EOF warning.
    tls.conn.send_close_notify();
    let _ = tls.flush();

    Ok(LoginResponse {
        success: resp.success,
        message: resp.message,
        user_id,
    })
}

/// Register a new user against the TLS-only server and return the auth response.
#[frb]
pub fn register_tls(
    host: String,
    port: u16,
    ca_pem: String,
    _passphrase: String,
    _password: String,
) -> Result<LoginResponse, String> {
    // Ensure a crypto provider is installed (rustls 0.23 requires this)
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });

    // Build TLS client config with provided root
    let roots = build_root_store_from_pem(&ca_pem)?;
    let config: ClientConfig = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();

    let server_name = ServerName::try_from(host.as_str())
        .map_err(|e| format!("Invalid server name: {e}"))?
        .to_owned();
    let addr = format!("{}:{}", host, port);

    // TCP connect
    let tcp = TcpStream::connect(addr).map_err(|e| format!("TCP connect failed: {e}"))?;

    // TLS handshake
    let conn = ClientConnection::new(Arc::new(config), server_name)
        .map_err(|e| format!("TLS connect failed: {e}"))?;
    let mut tls = StreamOwned::new(conn, tcp);

    // Read initial auth_required line (ignore failures)
    let _ = read_line(&mut tls);

    // Send register envelope with identity id only
    let identity_id = crate::security::load_identity()?
        .map(|b| b.user_id)
        .unwrap_or_default();
    let payload = serde_json::json!({ "id": identity_id });
    let envelope = ClientMessage {
        command: "register".to_string(),
        data: payload.to_string(),
    };
    let mut line = serde_json::to_string(&envelope).map_err(|e| format!("Serialize error: {e}"))?;
    line.push('\n');
    tls.write_all(line.as_bytes())
        .map_err(|e| format!("Write failed: {e}"))?;
    tls.flush().map_err(|e| format!("Flush failed: {e}"))?;

    // Read auth_response
    let raw = read_line(&mut tls).map_err(|e| format!("Read failed: {e}"))?;
    let wrapper: ClientMessage = serde_json::from_str(&raw)
        .map_err(|e| format!("Invalid JSON from server: {e}; raw={raw}"))?;
    if wrapper.command != "auth_response" {
        return Err(format!("Unexpected command: {}", wrapper.command));
    }
    let resp_val: serde_json::Value = serde_json::from_str(&wrapper.data)
        .map_err(|e| format!("Invalid auth_response data: {e}"))?;
    let resp: AuthResponse = serde_json::from_value(resp_val.clone())
        .map_err(|e| format!("Invalid auth_response shape: {e}"))?;
    let user_id = if let Some(uid) = resp.user_id {
        Some(uid)
    } else if let Some(id_str) = resp_val.get("id").and_then(|v| v.as_str()) {
        Some(session_id_from_identity(id_str)?)
    } else if let Some(id_local) = crate::security::load_identity()?.map(|b| b.user_id) {
        Some(session_id_from_identity(&id_local)?)
    } else {
        None
    };

    // Graceful TLS close
    tls.conn.send_close_notify();
    let _ = tls.flush();

    Ok(LoginResponse {
        success: resp.success,
        message: resp.message,
        user_id,
    })
}

/// Bundle returned by login/register + history.
#[frb]
#[derive(Clone, Debug)]
pub struct HistoryBundle {
    pub success: bool,
    pub message: String,
    pub user_id: Option<i64>,
    pub messages: Vec<HistoryMessage>,
}

fn fetch_history_over_stream(
    tls: &mut StreamOwned<ClientConnection, TcpStream>,
    limit: Option<usize>,
) -> Result<Vec<HistoryMessage>, String> {
    let req = HistoryRequest { limit };
    let env = ClientMessage {
        command: "history".to_string(),
        data: serde_json::to_string(&req).map_err(|e| format!("Serialize error: {e}"))?,
    };
    let mut line = serde_json::to_string(&env).map_err(|e| format!("Serialize error: {e}"))?;
    line.push('\n');
    tls.write_all(line.as_bytes())
        .map_err(|e| format!("Write failed: {e}"))?;
    tls.flush().map_err(|e| format!("Flush failed: {e}"))?;
    let raw = read_line(tls).map_err(|e| format!("Read failed: {e}"))?;
    let wrapper: ClientMessage = serde_json::from_str(&raw)
        .map_err(|e| format!("Invalid JSON from server: {e}; raw={raw}"))?;
    if wrapper.command != "history_response" {
        return Err(format!("Unexpected command: {}", wrapper.command));
    }
    let resp: HistoryResponse = serde_json::from_str(&wrapper.data)
        .map_err(|e| format!("Invalid history_response data: {e}"))?;
    if !resp.success {
        return Err(resp.message);
    }
    // Map model messages into FRB-friendly struct
    Ok(resp
        .messages
        .into_iter()
        .map(HistoryMessage::from)
        .collect())
}

/// Simple result type for one-off commands.
#[frb]
#[derive(Clone, Debug)]
pub struct SendResult {
    pub success: bool,
    pub message: String,
}

/// Login and send a direct message in a single TLS session.
#[frb]
#[allow(clippy::too_many_arguments)]
pub fn send_direct_message_tls(
    host: String,
    port: u16,
    ca_pem: String,
    passphrase: String,
    password: String,
    to_user_id: i64,
    body: String,
) -> Result<SendResult, String> {
    fn is_base64ish(s: &str) -> bool {
        !s.is_empty()
            && s.chars().all(|c| {
                matches!(
                    c,
                    'A'..='Z' | 'a'..='z' | '0'..='9' | '+' | '/' | '=' | '-' | '_'
                )
            })
    }
    fn is_e2ee_envelope(body: &str) -> bool {
        if !body.starts_with("v1:") {
            return false;
        }
        let parts: Vec<&str> = body.split(':').collect();
        if parts.len() != 4 {
            return false;
        }
        let (_v, eph, nonce, ct) = (parts[0], parts[1], parts[2], parts[3]);
        is_base64ish(eph) && is_base64ish(nonce) && is_base64ish(ct)
    }
    if !is_e2ee_envelope(&body) {
        return Ok(SendResult {
            success: false,
            message: "E2EE required: body must be an opaque v1 envelope".to_string(),
        });
    }
    let mut tls = make_tls_stream(&host, port, &ca_pem)?;
    let login = auth_over_stream(&mut tls, "login", passphrase, password)?;
    if !login.success {
        tls.conn.send_close_notify();
        let _ = tls.flush();
        return Ok(SendResult {
            success: false,
            message: login.message,
        });
    }

    #[derive(serde::Serialize)]
    struct OutgoingDM {
        to_user_id: i64,
        body: String,
    }
    let req = OutgoingDM { to_user_id, body };
    let env = ClientMessage {
        command: "message".to_string(),
        data: serde_json::to_string(&req).map_err(|e| format!("Serialize error: {e}"))?,
    };
    let mut line = serde_json::to_string(&env).map_err(|e| format!("Serialize error: {e}"))?;
    line.push('\n');
    tls.write_all(line.as_bytes())
        .map_err(|e| format!("Write failed: {e}"))?;
    tls.flush().map_err(|e| format!("Flush failed: {e}"))?;

    tls.conn.send_close_notify();
    let _ = tls.flush();
    Ok(SendResult {
        success: true,
        message: "Message sent".to_string(),
    })
}

/// Keep a TLS session open and stream incoming direct messages as JSON payloads.
/// Emits the `data` contents of `{"command":"message","data":...}` lines.
#[frb]
pub(crate) static SESSIONS: Lazy<std::sync::Mutex<HashMap<i64, Sender<String>>>> =
    Lazy::new(|| std::sync::Mutex::new(HashMap::new()));

fn enqueue_command(user_id: i64, msg: ClientMessage) -> Result<(), String> {
    let tx = {
        let guard = SESSIONS.lock().map_err(|_| "session lock".to_string())?;
        guard.get(&user_id).cloned()
    };
    let Some(tx) = tx else {
        return Err("No active stream session for user".to_string());
    };
    let mut line = serde_json::to_string(&msg).map_err(|e| format!("serialize: {e}"))?;
    line.push('\n');
    tx.send(line).map_err(|_| "send failed".to_string())
}

pub fn open_message_stream_tls(
    host: String,
    port: u16,
    ca_pem: String,
    passphrase: String,
    password: String,
    sink: StreamSink<String>,
) -> Result<(), String> {
    // Establish TLS and authenticate
    let mut tls = make_tls_stream(&host, port, &ca_pem)?;
    let login = auth_over_stream(&mut tls, "login", passphrase, password)?;
    if !login.success {
        tls.conn.send_close_notify();
        let _ = tls.flush();
        return Err(login.message);
    }
    let user_id = login.user_id.ok_or_else(|| "Missing user_id".to_string())?;

    // Emit an initial auth_ok event so Dart can capture user_id without a separate login
    let _ = sink.add(format!("{{\"type\":\"auth_ok\",\"user_id\":{}}}", user_id));

    // Configure a short read timeout to interleave reads with outgoing writes
    let tcp = tls.get_mut();
    let _ = tcp.set_read_timeout(Some(Duration::from_millis(200)));

    // Channel for outgoing writes from FRB API
    let (tx, rx): (Sender<String>, Receiver<String>) = mpsc::channel();
    // Channel for inbound RTC messages; forward them to sink on a dedicated thread
    let (rtc_tx, rtc_rx): (Sender<String>, Receiver<String>) = mpsc::channel();
    crate::webrtc::register_inbound_sink(user_id, rtc_tx);
    {
        let sink_clone = sink.clone();
        thread::spawn(move || {
            while let Ok(dc_msg) = rtc_rx.recv() {
                let _ = sink_clone.add(dc_msg);
            }
        });
    }
    {
        let mut g = SESSIONS.lock().unwrap();
        g.insert(user_id, tx);
    }

    // Spawn a dedicated thread to own the TLS stream, read incoming events, and perform writes.
    thread::spawn(move || {
        let mut tls = tls; // move into thread
        let mut buf = [0u8; 1024];
        let mut acc: Vec<u8> = Vec::new();
        loop {
            // 1) Drain outgoing writes, if any
            while let Ok(line) = rx.try_recv() {
                let _ = tls.write_all(line.as_bytes());
                let _ = tls.flush();
            }
            // RTC inbound is forwarded by a dedicated thread (see above)

            // 2) Attempt to read incoming data
            match tls.read(&mut buf) {
                Ok(0) => break, // closed
                Ok(n) => {
                    acc.extend_from_slice(&buf[..n]);
                    // Process complete lines
                    while let Some(pos) = acc.iter().position(|&b| b == b'\n') {
                        let line = acc.drain(..=pos).collect::<Vec<u8>>();
                        let line = String::from_utf8_lossy(&line[..line.len().saturating_sub(1)])
                            .to_string();
                        #[allow(clippy::collapsible_if)]
                        if let Ok(wrapper) = serde_json::from_str::<ClientMessage>(&line) {
                            match wrapper.command.as_str() {
                                "message" => {
                                    let _ = sink.add(wrapper.data);
                                }
                                "rtc_offer" => {
                                    if let Ok(ofr) =
                                        serde_json::from_str::<rura_models::webrtc::RtcOffer>(
                                            &wrapper.data,
                                        )
                                    {
                                        let _ = webrtc::on_remote_offer(user_id, ofr);
                                    }
                                }
                                "rtc_answer" => {
                                    if let Ok(ans) =
                                        serde_json::from_str::<rura_models::webrtc::RtcAnswer>(
                                            &wrapper.data,
                                        )
                                    {
                                        let _ = webrtc::on_remote_answer(user_id, ans);
                                    }
                                }
                                "rtc_ice" => {
                                    if let Ok(ice) =
                                        serde_json::from_str::<rura_models::webrtc::IceCandidate>(
                                            &wrapper.data,
                                        )
                                    {
                                        let _ = webrtc::on_remote_ice(user_id, ice);
                                    }
                                }
                                cmd @ ("call_invite" | "call_ringing" | "call_answer"
                                | "call_reject" | "call_hangup") => {
                                    handle_incoming_call_command(user_id, cmd, &wrapper.data);
                                    forward_call_event(&sink, cmd, &wrapper.data);
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Err(e) => {
                    if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut
                    {
                        // No data; loop and try writes again
                    } else {
                        break;
                    }
                }
            }
        }
        let _ = tls.flush();
        // Remove session entry when exiting
        let mut g = SESSIONS.lock().unwrap();
        g.remove(&user_id);
    });

    Ok(())
}

/// Keep a TLS session open and stream incoming direct messages (register flow).
/// Same behavior as `open_message_stream_tls` but authenticates via `register`.
#[frb]
pub fn open_message_stream_register_tls(
    host: String,
    port: u16,
    ca_pem: String,
    passphrase: String,
    password: String,
    sink: StreamSink<String>,
) -> Result<(), String> {
    // Establish TLS and authenticate via register
    let mut tls = make_tls_stream(&host, port, &ca_pem)?;
    let login = auth_over_stream(&mut tls, "register", passphrase, password)?;
    if !login.success {
        tls.conn.send_close_notify();
        let _ = tls.flush();
        return Err(login.message);
    }
    let user_id = login.user_id.ok_or_else(|| "Missing user_id".to_string())?;

    // Emit an initial auth_ok event so Dart can capture user_id without a separate login
    let _ = sink.add(format!("{{\"type\":\"auth_ok\",\"user_id\":{}}}", user_id));

    // Configure a short read timeout to interleave reads with outgoing writes
    let tcp = tls.get_mut();
    let _ = tcp.set_read_timeout(Some(Duration::from_millis(200)));

    // Channel for outgoing writes from FRB API
    let (tx, rx): (Sender<String>, Receiver<String>) = mpsc::channel();
    // Channel for inbound RTC messages; forward them to sink on a dedicated thread
    let (rtc_tx, rtc_rx): (Sender<String>, Receiver<String>) = mpsc::channel();
    crate::webrtc::register_inbound_sink(user_id, rtc_tx);
    {
        let sink_clone = sink.clone();
        thread::spawn(move || {
            while let Ok(dc_msg) = rtc_rx.recv() {
                let _ = sink_clone.add(dc_msg);
            }
        });
    }
    {
        let mut g = SESSIONS.lock().unwrap();
        g.insert(user_id, tx);
    }

    // Spawn a dedicated thread to own the TLS stream, read incoming events, and perform writes.
    thread::spawn(move || {
        let mut tls = tls; // move into thread
        let mut buf = [0u8; 1024];
        let mut acc: Vec<u8> = Vec::new();
        loop {
            while let Ok(line) = rx.try_recv() {
                let _ = tls.write_all(line.as_bytes());
                let _ = tls.flush();
            }
            // RTC inbound is forwarded by a dedicated thread (see above)
            match tls.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    acc.extend_from_slice(&buf[..n]);
                    while let Some(pos) = acc.iter().position(|&b| b == b'\n') {
                        let line = acc.drain(..=pos).collect::<Vec<u8>>();
                        let line = String::from_utf8_lossy(&line[..line.len().saturating_sub(1)])
                            .to_string();
                        if let Ok(wrapper) = serde_json::from_str::<ClientMessage>(&line) {
                            match wrapper.command.as_str() {
                                "message" => {
                                    let _ = sink.add(wrapper.data);
                                }
                                "rtc_offer" => {
                                    if let Ok(ofr) =
                                        serde_json::from_str::<rura_models::webrtc::RtcOffer>(
                                            &wrapper.data,
                                        )
                                    {
                                        let _ = webrtc::on_remote_offer(user_id, ofr);
                                    }
                                }
                                "rtc_answer" => {
                                    if let Ok(ans) =
                                        serde_json::from_str::<rura_models::webrtc::RtcAnswer>(
                                            &wrapper.data,
                                        )
                                    {
                                        let _ = webrtc::on_remote_answer(user_id, ans);
                                    }
                                }
                                "rtc_ice" => {
                                    if let Ok(ice) =
                                        serde_json::from_str::<rura_models::webrtc::IceCandidate>(
                                            &wrapper.data,
                                        )
                                    {
                                        let _ = webrtc::on_remote_ice(user_id, ice);
                                    }
                                }
                                cmd @ ("call_invite" | "call_ringing" | "call_answer"
                                | "call_reject" | "call_hangup") => {
                                    handle_incoming_call_command(user_id, cmd, &wrapper.data);
                                    forward_call_event(&sink, cmd, &wrapper.data);
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Err(e) => {
                    if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut
                    {
                        // continue
                    } else {
                        break;
                    }
                }
            }
        }
        let _ = tls.flush();
        let mut g = SESSIONS.lock().unwrap();
        g.remove(&user_id);
    });

    Ok(())
}

/// Send a direct message using an existing open stream session for the given user_id.
#[frb]
pub fn send_direct_message_over_stream(
    user_id: i64,
    to_user_id: i64,
    body: String,
) -> Result<(), String> {
    fn is_base64ish(s: &str) -> bool {
        !s.is_empty()
            && s.chars().all(|c| {
                matches!(
                    c,
                    'A'..='Z' | 'a'..='z' | '0'..='9' | '+' | '/' | '=' | '-' | '_'
                )
            })
    }
    fn is_e2ee_envelope(body: &str) -> bool {
        if !body.starts_with("v1:") {
            return false;
        }
        let parts: Vec<&str> = body.split(':').collect();
        if parts.len() != 4 {
            return false;
        }
        let (_v, eph, nonce, ct) = (parts[0], parts[1], parts[2], parts[3]);
        is_base64ish(eph) && is_base64ish(nonce) && is_base64ish(ct)
    }
    fn try_extract_dev_plaintext(body: &str) -> Option<String> {
        if !body.starts_with("v1:") {
            return None;
        }
        let parts: Vec<&str> = body.split(':').collect();
        if parts.len() != 4 {
            return None;
        }
        if parts[1] == "UGxhaW5FcGg=" && parts[2] == "Tm9uY2U=" {
            if let Ok(ct) = base64::engine::general_purpose::STANDARD.decode(parts[3]) {
                if let Ok(txt) = String::from_utf8(ct) {
                    return Some(txt);
                }
            }
        }
        None
    }
    // Try to encrypt with known pubkey: first exact numeric key, then by matching contacts on identity-derived numeric id.
    fn find_contact_pubkey_by_numeric(numeric: i64) -> Result<Option<String>, String> {
        let rows = crate::local_storage::list_contacts()?;
        for row in rows {
            if let Ok(id_numeric) = super::webrtc::session_id_from_identity(&row.user_id) {
                if id_numeric == numeric {
                    return Ok(Some(row.pubkey));
                }
            }
        }
        Ok(None)
    }
    let body = if let Some(plain) = try_extract_dev_plaintext(&body) {
        // Upgrade dev wrapper to real encryption when possible
        crate::local_storage::init_storage()?;
        let key = to_user_id.to_string();
        if let Some(pk) = crate::local_storage::get_contact_pubkey(&key)?
            .or_else(|| find_contact_pubkey_by_numeric(to_user_id).ok().flatten())
        {
            crate::security::encrypt_for_recipient(plain.as_bytes(), &pk)?
        } else {
            body
        }
    } else if is_e2ee_envelope(&body) {
        body
    } else {
        // Attempt to encrypt using a contact entry keyed by numeric id as string
        crate::local_storage::init_storage()?;
        let key = to_user_id.to_string();
        if let Some(pk) = crate::local_storage::get_contact_pubkey(&key)?
            .or_else(|| find_contact_pubkey_by_numeric(to_user_id).ok().flatten())
        {
            crate::security::encrypt_for_recipient(body.as_bytes(), &pk)?
        } else {
            body
        }
    };
    let req = rura_models::messaging::DirectMessageReq { to_user_id, body };
    send_command_over_stream(user_id, "message", &req)
}

/// Send media bytes to a peer over the WebRTC data channel using chunked messages.
///
/// The payload is split into fixed-size chunks. Each chunk is wrapped in a JSON envelope
/// and sent as a text message over the data channel. The receiver can reassemble chunks
/// using `msg_id`, verify the `checksum`, and reconstruct the original file.
///
/// This keeps server completely out of the data path; all transfer is P2P via WebRTC.
#[frb]
pub fn send_media_to_identity(
    user_id: i64,
    to_identity: String,
    mime: String,
    name: Option<String>,
    bytes: Vec<u8>,
    chunk_size: Option<usize>,
) -> Result<(), String> {
    // Ensure RTC path is negotiated
    let remote_id = crate::webrtc::session_id_from_identity(&to_identity)
        .map_err(|_| "Invalid recipient identity".to_string())?;
    let _ = crate::webrtc::ensure_offer_to_identity(user_id, &to_identity);

    // Compute checksum
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let digest = hasher.finalize();
    let checksum = hex::encode(digest);

    // Derive a random message id (u128 -> hex)
    let mut rng = rand::thread_rng();
    let mut hi = [0u8; 8];
    let mut lo = [0u8; 8];
    rng.fill_bytes(&mut hi);
    rng.fill_bytes(&mut lo);
    let msg_id = {
        let mut buf = [0u8; 16];
        buf[..8].copy_from_slice(&hi);
        buf[8..].copy_from_slice(&lo);
        hex::encode(buf)
    };

    // Chunk and send as JSON text events to stay compatible with existing DC handlers.
    let total = bytes.len();
    let csize = chunk_size.unwrap_or(12 * 1024); // conservative size for DC text frames
    let mut offset = 0usize;
    let mut idx: u32 = 0;
    let chunks = total.div_ceil(csize) as u32;
    let my_identity = crate::security::load_identity()
        .map_err(|e| format!("identity: {e}"))?
        .map(|b| b.user_id)
        .unwrap_or_default();

    while offset < total {
        let end = std::cmp::min(offset + csize, total);
        let slice = &bytes[offset..end];
        let data_b64 = base64::engine::general_purpose::STANDARD.encode(slice);
        let event = serde_json::json!({
            "type": "media",
            "from_user_id": user_id,
            "from_identity": my_identity,
            "to_identity": to_identity,
            "mime": mime,
            "name": name,
            "checksum": checksum,
            "total_size": total as u64,
            "msg_id": msg_id,
            "chunk_index": idx,
            "chunk_count": chunks,
            "data_b64": data_b64,
        })
        .to_string();
        crate::webrtc::queue_or_send(remote_id, event.clone())?;
        offset = end;
        idx += 1;
    }
    Ok(())
}

/// Send a direct message targeting a peer by identity (base64 string) using an existing stream.
#[frb]
pub fn send_direct_message_over_stream_to_identity(
    user_id: i64,
    to_identity: String,
    body: String,
) -> Result<(), String> {
    fn is_base64ish(s: &str) -> bool {
        !s.is_empty()
            && s.chars().all(
                |c| matches!(c, 'A'..='Z' | 'a'..='z' | '0'..='9' | '+' | '/' | '=' | '-' | '_'),
            )
    }
    fn is_e2ee_envelope(body: &str) -> bool {
        if !body.starts_with("v1:") {
            return false;
        }
        let parts: Vec<&str> = body.split(':').collect();
        if parts.len() != 4 {
            return false;
        }
        let (_v, eph, nonce, ct) = (parts[0], parts[1], parts[2], parts[3]);
        is_base64ish(eph) && is_base64ish(nonce) && is_base64ish(ct)
    }
    fn try_extract_dev_plaintext(body: &str) -> Option<String> {
        if !body.starts_with("v1:") {
            return None;
        }
        let parts: Vec<&str> = body.split(':').collect();
        if parts.len() != 4 {
            return None;
        }
        if parts[1] == "UGxhaW5FcGg=" && parts[2] == "Tm9uY2U=" {
            if let Ok(ct) = base64::engine::general_purpose::STANDARD.decode(parts[3]) {
                if let Ok(txt) = String::from_utf8(ct) {
                    return Some(txt);
                }
            }
        }
        None
    }
    let body = if let Some(plain) = try_extract_dev_plaintext(&body) {
        crate::local_storage::init_storage()?;
        if let Some(pk) = crate::local_storage::get_contact_pubkey(&to_identity)? {
            crate::security::encrypt_for_recipient(plain.as_bytes(), &pk)?
        } else {
            body
        }
    } else if is_e2ee_envelope(&body) {
        body
    } else {
        crate::local_storage::init_storage()?;
        if let Some(pk) = crate::local_storage::get_contact_pubkey(&to_identity)? {
            crate::security::encrypt_for_recipient(body.as_bytes(), &pk)?
        } else {
            body
        }
    };
    let remote_id = crate::webrtc::session_id_from_identity(&to_identity)
        .map_err(|_| "Invalid recipient identity".to_string())?;
    let req = rura_models::messaging::DirectMessageReq {
        to_user_id: remote_id,
        body,
    };
    send_command_over_stream(user_id, "message", &req)
}

/// Publish a public key for the authenticated user via an existing stream session.
#[frb]
pub fn set_pubkey_over_stream(user_id: i64, pubkey: String) -> Result<(), String> {
    let tx = {
        let g = SESSIONS.lock().unwrap();
        g.get(&user_id).cloned()
    };
    let Some(tx) = tx else {
        return Err("No active stream session for user".to_string());
    };
    #[derive(serde::Serialize)]
    struct SetPkReq {
        pubkey: String,
    }
    let req = SetPkReq { pubkey };
    let env = ClientMessage {
        command: "set_pubkey".to_string(),
        data: serde_json::to_string(&req).map_err(|e| format!("Serialize error: {e}"))?,
    };
    let mut line = serde_json::to_string(&env).map_err(|e| format!("Serialize error: {e}"))?;
    line.push('\n');
    tx.send(line)
        .map_err(|_| "Failed to enqueue set_pubkey".to_string())
}

/// Update (or set) a contact's nickname by identity (base64). Does not require a pubkey.
#[frb]
pub fn set_contact_nickname(user_id: String, nickname: Option<String>) -> Result<(), String> {
    crate::local_storage::init_storage()?;
    crate::local_storage::set_contact_nickname(user_id, nickname)
}

/// One-off helper: login and fetch another user's published public key.
#[frb]
pub fn get_pubkey_tls(
    host: String,
    port: u16,
    ca_pem: String,
    passphrase: String,
    password: String,
    user_id: i64,
) -> Result<Option<String>, String> {
    let mut tls = make_tls_stream(&host, port, &ca_pem)?;
    let login = auth_over_stream(&mut tls, "login", passphrase, password)?;
    if !login.success {
        tls.conn.send_close_notify();
        let _ = tls.flush();
        return Err(login.message);
    }
    #[derive(serde::Serialize)]
    struct GetPkReq {
        user_id: i64,
    }
    #[derive(serde::Deserialize)]
    struct GetPkResp {
        success: bool,
        pubkey: Option<String>,
    }

    let req = GetPkReq { user_id };
    let env = ClientMessage {
        command: "get_pubkey".to_string(),
        data: serde_json::to_string(&req).map_err(|e| format!("Serialize error: {e}"))?,
    };
    let mut line = serde_json::to_string(&env).map_err(|e| format!("Serialize error: {e}"))?;
    line.push('\n');
    tls.write_all(line.as_bytes())
        .map_err(|e| format!("Write failed: {e}"))?;
    tls.flush().map_err(|e| format!("Flush failed: {e}"))?;

    let raw = read_line(&mut tls).map_err(|e| format!("Read failed: {e}"))?;
    let wrapper: ClientMessage = serde_json::from_str(&raw)
        .map_err(|e| format!("Invalid JSON from server: {e}; raw={raw}"))?;
    if wrapper.command != "get_pubkey_response" {
        return Err(format!("Unexpected command: {}", wrapper.command));
    }
    let resp: GetPkResp = serde_json::from_str(&wrapper.data)
        .map_err(|e| format!("Invalid get_pubkey_response data: {e}"))?;

    tls.conn.send_close_notify();
    let _ = tls.flush();
    if resp.success {
        Ok(resp.pubkey)
    } else {
        Ok(None)
    }
}

fn make_tls_stream(
    host: &str,
    port: u16,
    ca_pem: &str,
) -> Result<StreamOwned<ClientConnection, TcpStream>, String> {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
    let roots = build_root_store_from_pem(ca_pem)?;
    let config: ClientConfig = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let server_name = ServerName::try_from(host)
        .map_err(|e| format!("Invalid server name: {e}"))?
        .to_owned();
    let addr = format!("{}:{}", host, port);
    let tcp = TcpStream::connect(addr).map_err(|e| format!("TCP connect failed: {e}"))?;
    let conn = ClientConnection::new(Arc::new(config), server_name)
        .map_err(|e| format!("TLS connect failed: {e}"))?;
    Ok(StreamOwned::new(conn, tcp))
}

fn auth_over_stream(
    tls: &mut StreamOwned<ClientConnection, TcpStream>,
    command: &str,
    _passphrase: String,
    password: String,
) -> Result<LoginResponse, String> {
    let _ = read_line(tls);
    // Ensure local store is unlocked and an identity exists so we can always send identity_key
    let _ = crate::security::unlock_local(&password);
    let identity_key = match crate::security::load_identity()? {
        Some(bundle) => Some(bundle.user_id),
        None => {
            // First run: generate identity (ed25519 pk + random user_id)
            let bundle = crate::security::generate_and_store_identity()?;
            Some(bundle.user_id)
        }
    };

    let payload = serde_json::json!({ "id": identity_key.unwrap_or_default() });
    let env = ClientMessage {
        command: command.to_string(),
        data: payload.to_string(),
    };
    let mut line = serde_json::to_string(&env).map_err(|e| format!("Serialize error: {e}"))?;
    line.push('\n');
    tls.write_all(line.as_bytes())
        .map_err(|e| format!("Write failed: {e}"))?;
    tls.flush().map_err(|e| format!("Flush failed: {e}"))?;
    let raw = read_line(tls).map_err(|e| format!("Read failed: {e}"))?;
    let wrapper: ClientMessage = serde_json::from_str(&raw)
        .map_err(|e| format!("Invalid JSON from server: {e}; raw={raw}"))?;
    if wrapper.command != "auth_response" {
        return Err(format!("Unexpected command: {}", wrapper.command));
    }
    let resp_val: serde_json::Value = serde_json::from_str(&wrapper.data)
        .map_err(|e| format!("Invalid auth_response data: {e}"))?;
    let resp: AuthResponse = serde_json::from_value(resp_val.clone())
        .map_err(|e| format!("Invalid auth_response shape: {e}"))?;
    let user_id = if let Some(uid) = resp.user_id {
        Some(uid)
    } else if let Some(id_str) = resp_val.get("id").and_then(|v| v.as_str()) {
        Some(session_id_from_identity(id_str)?)
    } else if let Some(id_local) = crate::security::load_identity()?.map(|b| b.user_id) {
        Some(session_id_from_identity(&id_local)?)
    } else {
        None
    };
    Ok(LoginResponse {
        success: resp.success,
        message: resp.message,
        user_id,
    })
}

/// Login and fetch message history in one TLS session.
#[frb]
pub fn login_and_fetch_history_tls(
    host: String,
    port: u16,
    ca_pem: String,
    passphrase: String,
    password: String,
    limit: Option<usize>,
) -> Result<HistoryBundle, String> {
    let mut tls = make_tls_stream(&host, port, &ca_pem)?;
    let login = auth_over_stream(&mut tls, "login", passphrase, password)?;
    let mut messages = Vec::new();
    if login.success {
        messages = fetch_history_over_stream(&mut tls, limit)?;
    }
    tls.conn.send_close_notify();
    let _ = tls.flush();
    Ok(HistoryBundle {
        success: login.success,
        message: login.message,
        user_id: login.user_id,
        messages,
    })
}

/// Login and load local cache history (no server history).
#[frb]
#[allow(clippy::too_many_arguments)]
pub fn login_and_load_local_history_tls(
    host: String,
    port: u16,
    ca_pem: String,
    passphrase: String,
    password: String,
    limit: Option<usize>,
) -> Result<HistoryBundle, String> {
    // Check if data directory exists - must exist for login
    if !crate::security::data_dir_exists() {
        return Err("No account found. Please register first.".to_string());
    }

    if host.trim().is_empty() || port == 0 {
        // Offline unlock: use existing account
        crate::security::unlock_local(&password)?;
        crate::local_storage::init_storage()?;
        let messages = load_local_history(limit)?;
        return Ok(HistoryBundle {
            success: true,
            message: "Unlocked local storage".to_string(),
            user_id: None,
            messages,
        });
    }

    let mut tls = make_tls_stream(&host, port, &ca_pem)?;
    let login = auth_over_stream(&mut tls, "login", passphrase, password)?;
    tls.conn.send_close_notify();
    let _ = tls.flush();
    let mut messages = Vec::new();
    if login.success {
        messages = load_local_history(limit)?;
    }
    Ok(HistoryBundle {
        success: login.success,
        message: login.message,
        user_id: login.user_id,
        messages,
    })
}

/// Register and load local cache history (no server history).
#[frb]
#[allow(clippy::too_many_arguments)]
pub fn register_and_load_local_history_tls(
    host: String,
    port: u16,
    ca_pem: String,
    passphrase: String,
    password: String,
    limit: Option<usize>,
) -> Result<HistoryBundle, String> {
    // Check if data directory exists - must NOT exist for register
    if crate::security::data_dir_exists() {
        return Err("Account already exists. Please login instead.".to_string());
    }

    if host.trim().is_empty() || port == 0 {
        // Offline register: create a new local account
        crate::security::unlock_local(&password)?;
        crate::local_storage::init_storage()?;
        let _ = crate::security::generate_and_store_identity()?;
        let _ = crate::local_storage::snapshot_persistent();
        let messages = load_local_history(limit)?;
        return Ok(HistoryBundle {
            success: true,
            message: "Local account created".to_string(),
            user_id: None,
            messages,
        });
    }

    let mut tls = make_tls_stream(&host, port, &ca_pem)?;
    let reg = auth_over_stream(&mut tls, "register", passphrase, password)?;
    tls.conn.send_close_notify();
    let _ = tls.flush();
    let mut messages = Vec::new();
    if reg.success {
        // Generate identity after successful server registration if missing
        if crate::security::load_identity()?.is_none() {
            let _ = crate::security::generate_and_store_identity()?;
        }
        messages = load_local_history(limit)?;
    }
    Ok(HistoryBundle {
        success: reg.success,
        message: reg.message,
        user_id: reg.user_id,
        messages,
    })
}

/// Register and fetch message history in one TLS session.
#[frb]
pub fn register_and_fetch_history_tls(
    host: String,
    port: u16,
    ca_pem: String,
    passphrase: String,
    password: String,
    limit: Option<usize>,
) -> Result<HistoryBundle, String> {
    let mut tls = make_tls_stream(&host, port, &ca_pem)?;
    let reg = auth_over_stream(&mut tls, "register", passphrase, password)?;
    let mut messages = Vec::new();
    if reg.success {
        messages = fetch_history_over_stream(&mut tls, limit)?;
    }
    tls.conn.send_close_notify();
    let _ = tls.flush();
    Ok(HistoryBundle {
        success: reg.success,
        message: reg.message,
        user_id: reg.user_id,
        messages,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn build_root_store_from_valid_pem() {
        // Generate a minimal self-signed CA cert via rcgen and ensure parsing succeeds
        let mut params = rcgen::CertificateParams::default();
        params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "Test CA");
        let ca = rcgen::Certificate::from_params(params).expect("rcgen ca");
        let ca_pem = ca.serialize_pem().expect("pem");
        let roots = build_root_store_from_pem(&ca_pem).expect("root store");
        assert!(!roots.is_empty());
    }

    #[test]
    fn build_root_store_from_empty_pem_fails() {
        let res = build_root_store_from_pem("");
        assert!(res.is_err());
    }

    #[test]
    fn read_line_reads_until_newline() {
        let mut c = Cursor::new(b"hello world\nrest ignored".as_slice());
        let line = read_line(&mut c).expect("read_line");
        assert_eq!(line, "hello world");
    }

    #[test]
    fn read_line_reads_all_without_newline() {
        let mut c = Cursor::new(b"no newline here".as_slice());
        let line = read_line(&mut c).expect("read_line");
        assert_eq!(line, "no newline here");
    }

    fn sample_envelope() -> String {
        // v1:<b64 eph>:<b64 nonce>:<b64 ciphertext>
        "v1:RU5WUEs=:Tk9OQ0U=:Q0lQSEVSVEVYVA==".to_string()
    }

    #[test]
    #[serial_test::serial]
    fn send_direct_message_over_stream_enqueues_to_session() {
        let body = sample_envelope();
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        {
            let mut g = SESSIONS.lock().unwrap();
            g.insert(42, tx);
        }
        send_direct_message_over_stream(42, 7, body.clone()).expect("send");
        let line = rx.recv().expect("line");
        let wrapper: ClientMessage = serde_json::from_str(line.trim_end()).expect("client message");
        assert_eq!(wrapper.command, "message");
        let payload: rura_models::messaging::DirectMessageReq =
            serde_json::from_str(&wrapper.data).expect("payload");
        assert_eq!(payload.to_user_id, 7);
        {
            let mut g = SESSIONS.lock().unwrap();
            g.remove(&42);
        }
    }

    #[test]
    #[serial_test::serial]
    fn send_direct_message_over_stream_identity_enqueues() {
        let body = sample_envelope();
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        {
            let mut g = SESSIONS.lock().unwrap();
            g.insert(55, tx);
        }
        let mut identity_bytes = [0u8; 16];
        identity_bytes[..8].copy_from_slice(&7_i64.to_be_bytes());
        let to_identity = base64::engine::general_purpose::STANDARD.encode(identity_bytes);
        send_direct_message_over_stream_to_identity(55, to_identity, body.clone()).expect("send");
        let line = rx.recv().expect("line");
        let wrapper: ClientMessage = serde_json::from_str(line.trim_end()).expect("client message");
        assert_eq!(wrapper.command, "message");
        let payload: rura_models::messaging::DirectMessageReq =
            serde_json::from_str(&wrapper.data).expect("payload");
        assert_eq!(payload.to_user_id, 7);
        {
            let mut g = SESSIONS.lock().unwrap();
            g.remove(&55);
        }
    }

    #[test]
    #[serial_test::serial]
    fn offline_login_loads_local_history() {
        // Isolate store to a temp dir to avoid cross-test interference
        let temp = tempfile::tempdir().expect("tempdir");
        #[allow(unused_unsafe)]
        unsafe {
            std::env::set_var("RURA_CLIENT_DATA_DIR", temp.path());
        }
        crate::security::reset_key_for_tests();
        crate::security::unlock_local("test-pass").expect("unlock");
        crate::local_storage::reset_store_for_tests();
        crate::local_storage::init_storage().expect("init storage");
        crate::local_storage::append_persistent_message(
            1,
            2,
            "hi".to_string(),
            "2024-01-01T00:00:00Z".to_string(),
        )
        .expect("append");

        let bundle = login_and_load_local_history_tls(
            "".to_string(),
            0,
            "".to_string(),
            "".to_string(),
            "test-pass".to_string(),
            Some(100),
        )
        .expect("offline load");
        assert!(bundle.success);
        assert!(bundle.messages.iter().any(|m| m.body == "hi"));
    }
}
