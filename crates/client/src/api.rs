use crate::StreamSink;
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
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Once};

fn session_id_from_identity(id_b64: &str) -> Result<i64, String> {
    // Derive a stable 63-bit numeric from the 256-bit base64 identity
    let bytes = base64::decode(id_b64).map_err(|e| format!("bad id base64: {e}"))?;
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

// ---------- Local cache helpers ----------

#[derive(Clone, Debug, Serialize, Deserialize)]
struct LocalMsg {
    id: i64,
    from_user_id: i64,
    to_user_id: i64,
    body: String,
    timestamp: String,
}

fn cache_base_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("RURA_CLIENT_CACHE_DIR") {
        return PathBuf::from(custom);
    }
    // Default: inside client crate (parent of flutter_app)
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("../.cache")
}

fn ensure_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|e| format!("Failed to create dir {}: {e}", path.display()))
}

fn user_dir(user_id: i64) -> PathBuf {
    cache_base_dir().join("users").join(user_id.to_string())
}
fn chats_dir(user_id: i64) -> PathBuf {
    user_dir(user_id).join("chats")
}
fn chat_file(user_id: i64, peer_id: i64) -> PathBuf {
    chats_dir(user_id).join(format!("{peer_id}.json"))
}

fn read_chat(user_id: i64, peer_id: i64) -> Result<Vec<LocalMsg>, String> {
    let path = chat_file(user_id, peer_id);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data =
        fs::read_to_string(&path).map_err(|e| format!("Read {} failed: {e}", path.display()))?;
    let v: Vec<LocalMsg> =
        serde_json::from_str(&data).map_err(|e| format!("Parse {} failed: {e}", path.display()))?;
    Ok(v)
}

fn write_chat(user_id: i64, peer_id: i64, msgs: &[LocalMsg]) -> Result<(), String> {
    let dir = chats_dir(user_id);
    ensure_dir(&dir)?;
    let path = chat_file(user_id, peer_id);
    let data = serde_json::to_string_pretty(msgs).map_err(|e| format!("Serialize failed: {e}"))?;
    fs::write(&path, data).map_err(|e| format!("Write {} failed: {e}", path.display()))
}

fn list_chat_files(user_id: i64) -> Vec<PathBuf> {
    let dir = chats_dir(user_id);
    match fs::read_dir(dir) {
        Ok(rd) => rd.filter_map(|e| e.ok()).map(|e| e.path()).collect(),
        Err(_) => Vec::new(),
    }
}

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
        Some(bundle) => Ok(bundle.public_b64),
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
    passphrase: String,
    password: String,
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
    passphrase: String,
    password: String,
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
static SESSIONS: Lazy<std::sync::Mutex<HashMap<i64, Sender<String>>>> =
    Lazy::new(|| std::sync::Mutex::new(HashMap::new()));

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
                            if wrapper.command == "message" {
                                let _ = sink.add(wrapper.data);
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
            match tls.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    acc.extend_from_slice(&buf[..n]);
                    while let Some(pos) = acc.iter().position(|&b| b == b'\n') {
                        let line = acc.drain(..=pos).collect::<Vec<u8>>();
                        let line = String::from_utf8_lossy(&line[..line.len().saturating_sub(1)])
                            .to_string();
                        if let Ok(wrapper) = serde_json::from_str::<ClientMessage>(&line) {
                            if wrapper.command == "message" {
                                let _ = sink.add(wrapper.data);
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
    if !is_e2ee_envelope(&body) {
        return Err("E2EE required: body must be an opaque v1 envelope".to_string());
    }
    let tx = {
        let g = SESSIONS.lock().unwrap();
        g.get(&user_id).cloned()
    };
    let Some(tx) = tx else {
        return Err("No active stream session for user".to_string());
    };
    #[derive(serde::Serialize)]
    struct OutgoingDM2 {
        to_user_id: i64,
        body: String,
    }
    let req = OutgoingDM2 { to_user_id, body };
    let env = ClientMessage {
        command: "message".to_string(),
        data: serde_json::to_string(&req).map_err(|e| format!("Serialize error: {e}"))?,
    };
    let mut line = serde_json::to_string(&env).map_err(|e| format!("Serialize error: {e}"))?;
    line.push('\n');
    tx.send(line)
        .map_err(|_| "Failed to enqueue send".to_string())
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
    if !is_e2ee_envelope(&body) {
        return Err("E2EE required: body must be an opaque v1 envelope".to_string());
    }
    let tx = {
        let g = SESSIONS.lock().unwrap();
        g.get(&user_id).cloned()
    };
    let Some(tx) = tx else {
        return Err("No active stream session for user".to_string());
    };

    #[derive(serde::Serialize)]
    struct OutgoingDMIdent {
        to_identity: String,
        body: String,
    }
    let req = OutgoingDMIdent { to_identity, body };
    let env = ClientMessage {
        command: "message".to_string(),
        data: serde_json::to_string(&req).map_err(|e| format!("Serialize error: {e}"))?,
    };
    let mut line = serde_json::to_string(&env).map_err(|e| format!("Serialize error: {e}"))?;
    line.push('\n');
    tx.send(line)
        .map_err(|_| "Failed to enqueue send".to_string())
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
    passphrase: String,
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
}
