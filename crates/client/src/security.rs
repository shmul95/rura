use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHasher};
use chacha20poly1305::XChaCha20Poly1305;
use chacha20poly1305::XNonce;
use chacha20poly1305::aead::{Aead, KeyInit};
use once_cell::sync::Lazy;
use rand::RngCore;
use ring::rand::SystemRandom;
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static KEYS: Lazy<Mutex<HashMap<i64, [u8; 32]>>> = Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Serialize, Deserialize)]
struct Config {
    salt_b64: String,
}

fn cache_base_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("RURA_CLIENT_CACHE_DIR") {
        return PathBuf::from(custom);
    }
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("../.cache")
}

fn user_dir(user_id: i64) -> PathBuf {
    cache_base_dir().join("users").join(user_id.to_string())
}
fn ensure_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|e| format!("mkdir {}: {e}", path.display()))
}
fn config_path(user_id: i64) -> PathBuf {
    user_dir(user_id).join("rura_config.json")
}
fn identity_path(user_id: i64) -> PathBuf {
    user_dir(user_id).join("identity.enc")
}

fn read_or_create_salt(user_id: i64) -> Result<Vec<u8>, String> {
    let cfg_path = config_path(user_id);
    if cfg_path.exists() {
        let data = fs::read_to_string(&cfg_path)
            .map_err(|e| format!("read {}: {e}", cfg_path.display()))?;
        let cfg: Config = serde_json::from_str(&data)
            .map_err(|e| format!("parse {}: {e}", cfg_path.display()))?;
        let salt = base64::decode(cfg.salt_b64).map_err(|e| format!("salt b64: {e}"))?;
        return Ok(salt);
    }
    let mut salt = vec![0u8; 16];
    rand::thread_rng().fill_bytes(&mut salt);
    ensure_dir(&user_dir(user_id))?;
    let cfg = Config {
        salt_b64: base64::encode(&salt),
    };
    fs::write(&cfg_path, serde_json::to_string_pretty(&cfg).unwrap())
        .map_err(|e| format!("write {}: {e}", cfg_path.display()))?;
    Ok(salt)
}

fn derive_key(password: &str, salt: &[u8]) -> Result<[u8; 32], String> {
    // Argon2id default params; produce 32-byte key
    let argon = Argon2::default();
    let mut out = [0u8; 32];
    argon
        .hash_password_into(password.as_bytes(), salt, &mut out)
        .map_err(|e| format!("argon2 error: {e}"))?;
    Ok(out)
}

pub fn unlock_local(user_id: i64, password: &str) -> Result<(), String> {
    let salt = read_or_create_salt(user_id)?;
    let key = derive_key(password, &salt)?;
    KEYS.lock().unwrap().insert(user_id, key);
    Ok(())
}

fn key_for(user_id: i64) -> Result<[u8; 32], String> {
    KEYS.lock()
        .unwrap()
        .get(&user_id)
        .copied()
        .ok_or_else(|| "Storage locked: call unlock_local first".to_string())
}

pub fn encrypt_blob(user_id: i64, plaintext: &[u8]) -> Result<Vec<u8>, String> {
    let key = key_for(user_id)?;
    let cipher = XChaCha20Poly1305::new((&key).into());
    let mut nonce_bytes = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = XNonce::from_slice(&nonce_bytes);
    let mut out = Vec::with_capacity(5 + 24 + plaintext.len() + 16);
    out.extend_from_slice(b"RURA1");
    out.extend_from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| format!("encrypt: {e}"))?;
    out.extend_from_slice(&ct);
    Ok(out)
}

pub fn decrypt_blob(user_id: i64, data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < 5 + 24 + 16 {
        return Err("encrypted blob too small".into());
    }
    if &data[..5] != b"RURA1" {
        return Err("bad header".into());
    }
    let key = key_for(user_id)?;
    let cipher = XChaCha20Poly1305::new((&key).into());
    let nonce = XNonce::from_slice(&data[5..29]);
    let ct = &data[29..];
    cipher
        .decrypt(nonce, ct)
        .map_err(|_| "decrypt failed".to_string())
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IdentityBundle {
    pub scheme: String,     // e.g., "ed25519-v1"
    pub public_b64: String, // public key bytes (base64)
    pub pkcs8_b64: String,  // PKCS#8 private+public (base64)
}

pub fn generate_and_store_identity(user_id: i64) -> Result<IdentityBundle, String> {
    let rng = SystemRandom::new();
    let pkcs8 =
        Ed25519KeyPair::generate_pkcs8(&rng).map_err(|_| "ed25519 pkcs8 generation failed")?;
    let key_pair =
        Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).map_err(|_| "ed25519 from pkcs8 failed")?;
    let public = key_pair.public_key().as_ref();
    let bundle = IdentityBundle {
        scheme: "ed25519-v1".to_string(),
        public_b64: base64::encode(public),
        pkcs8_b64: base64::encode(pkcs8.as_ref()),
    };
    let plain = serde_json::to_vec(&bundle).map_err(|e| format!("identity serialize: {e}"))?;
    let enc = encrypt_blob(user_id, &plain)?;
    ensure_dir(&user_dir(user_id))?;
    fs::write(identity_path(user_id), enc).map_err(|e| format!("write identity: {e}"))?;
    Ok(bundle)
}

pub fn load_identity(user_id: i64) -> Result<Option<IdentityBundle>, String> {
    let p = identity_path(user_id);
    if !p.exists() {
        return Ok(None);
    }
    let enc = fs::read(&p).map_err(|e| format!("read identity: {e}"))?;
    let plain = decrypt_blob(user_id, &enc)?;
    let b: IdentityBundle =
        serde_json::from_slice(&plain).map_err(|e| format!("identity parse: {e}"))?;
    Ok(Some(b))
}
