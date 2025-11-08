use argon2::Argon2;
use base64::{Engine as _, engine::general_purpose};
use chacha20poly1305::XChaCha20Poly1305;
use chacha20poly1305::XNonce;
use chacha20poly1305::aead::{Aead, KeyInit};
use once_cell::sync::Lazy;
use rand::RngCore;
use ring::rand::SystemRandom;
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

// Store the single encryption key (single account only).
static KEY: Lazy<Mutex<Option<[u8; 32]>>> = Lazy::new(|| Mutex::new(None));

#[derive(Serialize, Deserialize)]
struct Config {
    salt_b64: String,
}

fn data_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("RURA_CLIENT_DATA_DIR") {
        return PathBuf::from(custom);
    }
    // Default: data/ subdirectory in current working directory (flutter_app)
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("../data")
}

fn ensure_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|e| format!("mkdir {}: {e}", path.display()))
}

fn config_path() -> PathBuf {
    data_dir().join("rura_config.json")
}

fn identity_path() -> PathBuf {
    data_dir().join("identity.enc")
}

pub fn data_dir_exists() -> bool {
    data_dir().exists()
}

fn read_or_create_salt() -> Result<Vec<u8>, String> {
    let cfg_path = config_path();
    if cfg_path.exists() {
        let data = fs::read_to_string(&cfg_path)
            .map_err(|e| format!("read {}: {e}", cfg_path.display()))?;
        let cfg: Config = serde_json::from_str(&data)
            .map_err(|e| format!("parse {}: {e}", cfg_path.display()))?;
        let salt = general_purpose::STANDARD
            .decode(cfg.salt_b64)
            .map_err(|e| format!("salt b64: {e}"))?;
        return Ok(salt);
    }
    let mut salt = vec![0u8; 16];
    rand::thread_rng().fill_bytes(&mut salt);
    ensure_dir(&data_dir())?;
    let cfg = Config {
        salt_b64: general_purpose::STANDARD.encode(&salt),
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

pub fn unlock_local(password: &str) -> Result<(), String> {
    let salt = read_or_create_salt()?;
    let key = derive_key(password, &salt)?;
    *KEY.lock().unwrap() = Some(key);
    Ok(())
}

fn get_key() -> Result<[u8; 32], String> {
    KEY.lock()
        .unwrap()
        .ok_or_else(|| "Storage locked: call unlock_local first".to_string())
}

pub fn encrypt_blob(plaintext: &[u8]) -> Result<Vec<u8>, String> {
    let key = get_key()?;
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

pub fn decrypt_blob(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < 5 + 24 + 16 {
        return Err("encrypted blob too small".into());
    }
    if &data[..5] != b"RURA1" {
        return Err("bad header".into());
    }
    let key = get_key()?;
    let cipher = XChaCha20Poly1305::new((&key).into());
    let nonce = XNonce::from_slice(&data[5..29]);
    let ct = &data[29..];
    cipher
        .decrypt(nonce, ct)
        .map_err(|_| "decrypt failed".to_string())
}

#[cfg(test)]
pub(crate) fn reset_key_for_tests() {
    *KEY.lock().unwrap() = None;
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IdentityBundle {
    pub scheme: String,     // e.g., "ed25519-v1"
    pub public_b64: String, // Ed25519 public key (base64)
    pub pkcs8_b64: String,  // Ed25519 PKCS#8 private+public (base64)
    pub user_id: String,    // 256-bit random identifier (base64)
    // Static X25519 keys for E2EE (optional for legacy identities)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x25519_pub_b64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x25519_priv_b64: Option<String>,
}

pub fn generate_and_store_identity() -> Result<IdentityBundle, String> {
    let rng = SystemRandom::new();
    let pkcs8 =
        Ed25519KeyPair::generate_pkcs8(&rng).map_err(|_| "ed25519 pkcs8 generation failed")?;
    let key_pair =
        Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).map_err(|_| "ed25519 from pkcs8 failed")?;
    let public = key_pair.public_key().as_ref();

    // Generate 256-bit (32-byte) random user_id
    let mut user_id_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut user_id_bytes);

    // Generate static X25519 key pair for encryption (distinct from Ed25519)
    let x_sk = x25519_dalek::StaticSecret::random_from_rng(rand::thread_rng());
    let x_pk = x25519_dalek::PublicKey::from(&x_sk);

    let bundle = IdentityBundle {
        scheme: "ed25519-v1".to_string(),
        public_b64: general_purpose::STANDARD.encode(public),
        pkcs8_b64: general_purpose::STANDARD.encode(pkcs8.as_ref()),
        user_id: general_purpose::STANDARD.encode(user_id_bytes),
        x25519_pub_b64: Some(general_purpose::STANDARD.encode(x_pk.as_bytes())),
        x25519_priv_b64: Some(general_purpose::STANDARD.encode(x_sk.to_bytes())),
    };
    let plain = serde_json::to_vec(&bundle).map_err(|e| format!("identity serialize: {e}"))?;
    let enc = encrypt_blob(&plain)?;
    ensure_dir(&data_dir())?;
    fs::write(identity_path(), enc).map_err(|e| format!("write identity: {e}"))?;
    // TEMPORARY: Print only messaging key to avoid confusion
    println!("(TEMPORARY) Your ID: {}", bundle.user_id);
    if let Some(xpk) = &bundle.x25519_pub_b64 {
        println!("(TEMPORARY) Your Public Key: {}", xpk);
    } else {
        println!("(TEMPORARY) Your Public Key: {}", bundle.public_b64);
    }
    Ok(bundle)
}

pub fn load_identity() -> Result<Option<IdentityBundle>, String> {
    let p = identity_path();
    if !p.exists() {
        return Ok(None);
    }
    let enc = fs::read(&p).map_err(|e| format!("read identity: {e}"))?;
    let plain = decrypt_blob(&enc)?;
    let mut b: IdentityBundle =
        serde_json::from_slice(&plain).map_err(|e| format!("identity parse: {e}"))?;
    // Backfill X25519 keys for legacy identities that lack them.
    if b.x25519_priv_b64.is_none() || b.x25519_pub_b64.is_none() {
        let x_sk = x25519_dalek::StaticSecret::random_from_rng(rand::thread_rng());
        let x_pk = x25519_dalek::PublicKey::from(&x_sk);
        b.x25519_priv_b64 = Some(general_purpose::STANDARD.encode(x_sk.to_bytes()));
        b.x25519_pub_b64 = Some(general_purpose::STANDARD.encode(x_pk.as_bytes()));
        // Persist upgraded identity
        let enc =
            encrypt_blob(&serde_json::to_vec(&b).map_err(|e| format!("identity serialize: {e}"))?)?;
        fs::write(identity_path(), enc).map_err(|e| format!("write identity: {e}"))?;
    }
    Ok(Some(b))
}

/// Normalize a base64 public key into raw 32 bytes (accepts unpadded and URL-safe variants).
pub fn decode_pubkey_b64(pk_b64: &str) -> Result<[u8; 32], String> {
    let mut s = pk_b64.trim().replace(['\n', '\r', ' '], "");
    // Accept URL-safe variants
    s = s.replace('-', "+").replace('_', "/");
    // Add padding if missing
    let rem = s.len() % 4;
    if rem != 0 {
        s.push_str(&"=".repeat(4 - rem));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&s)
        .map_err(|e| format!("pubkey b64 decode: {e}"))?;
    if bytes.len() != 32 {
        return Err(format!(
            "pubkey length must be 32 bytes, got {}",
            bytes.len()
        ));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

/// Return canonical base64 (padded, standard alphabet) for a given input public key string.
pub fn canonicalize_pubkey_b64(pk_b64: &str) -> Result<String, String> {
    let raw = decode_pubkey_b64(pk_b64)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(raw))
}

fn hkdf_derive_key(shared: &[u8]) -> [u8; 32] {
    use hkdf::Hkdf;
    use sha2::Sha256;
    let salt = b"rura-msg-v1";
    let hk = Hkdf::<Sha256>::new(Some(salt), shared);
    let mut okm = [0u8; 32];
    hk.expand(b"xchacha20poly1305", &mut okm)
        .expect("hkdf expand");
    okm
}

fn x25519_shared_secret(priv_b: &[u8; 32], peer_pub: &[u8; 32]) -> [u8; 32] {
    let sk = x25519_dalek::StaticSecret::from(*priv_b);
    let pk = x25519_dalek::PublicKey::from(*peer_pub);
    sk.diffie_hellman(&pk).to_bytes()
}

/// Encrypt plaintext for a recipient's X25519 public key. Returns a v1 envelope string.
pub fn encrypt_for_recipient(plaintext: &[u8], recipient_pub_b64: &str) -> Result<String, String> {
    use chacha20poly1305::aead::Aead;
    let recip = decode_pubkey_b64(recipient_pub_b64)?;

    // Generate ephemeral key pair
    let eph_sk = x25519_dalek::StaticSecret::random_from_rng(rand::thread_rng());
    let eph_pk = x25519_dalek::PublicKey::from(&eph_sk);
    let shared = eph_sk
        .diffie_hellman(&x25519_dalek::PublicKey::from(recip))
        .to_bytes();
    let key_bytes = hkdf_derive_key(&shared);
    let cipher = XChaCha20Poly1305::new((&key_bytes).into());
    let mut nonce = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut nonce);
    let n = XNonce::from_slice(&nonce);
    let ct = cipher
        .encrypt(n, plaintext)
        .map_err(|e| format!("encrypt: {e}"))?;
    let env = format!(
        "v1:{}:{}:{}",
        general_purpose::STANDARD.encode(eph_pk.as_bytes()),
        general_purpose::STANDARD.encode(nonce),
        general_purpose::STANDARD.encode(ct)
    );
    Ok(env)
}

/// Decrypt a v1 envelope using our static X25519 private key.
pub fn decrypt_from_envelope(envelope: &str) -> Result<Vec<u8>, String> {
    if !envelope.starts_with("v1:") {
        return Err("unsupported envelope version".into());
    }
    let parts: Vec<&str> = envelope.split(':').collect();
    if parts.len() != 4 {
        return Err("invalid envelope format".into());
    }
    let eph_b = general_purpose::STANDARD
        .decode(parts[1])
        .map_err(|e| format!("eph b64: {e}"))?;
    let nonce_b = general_purpose::STANDARD
        .decode(parts[2])
        .map_err(|e| format!("nonce b64: {e}"))?;
    let ct_b = general_purpose::STANDARD
        .decode(parts[3])
        .map_err(|e| format!("ct b64: {e}"))?;
    if eph_b.len() != 32 || nonce_b.len() != 24 {
        return Err("invalid envelope sizes".into());
    }
    let mut eph = [0u8; 32];
    eph.copy_from_slice(&eph_b);
    let mut nonce = [0u8; 24];
    nonce.copy_from_slice(&nonce_b);

    let me = load_identity()?.ok_or_else(|| "No identity found".to_string())?;
    let priv_b64 = me
        .x25519_priv_b64
        .ok_or_else(|| "missing x25519 identity".to_string())?;
    let priv_arr = decode_pubkey_b64(&priv_b64)?; // reuse decoder (same size)

    let shared = x25519_shared_secret(&priv_arr, &eph);
    let key_bytes = hkdf_derive_key(&shared);
    let cipher = XChaCha20Poly1305::new((&key_bytes).into());
    let n = XNonce::from_slice(&nonce);
    use chacha20poly1305::aead::Aead;
    cipher
        .decrypt(n, ct_b.as_ref())
        .map_err(|_| "decrypt failed".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[serial_test::serial]
    fn e2ee_round_trip_self() {
        // Isolate to a temp dir
        let tmp = tempfile::tempdir().expect("tmp");
        #[allow(unused_unsafe)]
        unsafe {
            std::env::set_var("RURA_CLIENT_DATA_DIR", tmp.path());
        }
        crate::security::reset_key_for_tests();
        crate::security::unlock_local("pw").expect("unlock");
        let id = generate_and_store_identity().expect("gen id");
        let pubx = id.x25519_pub_b64.clone().unwrap();
        let msg = b"hello e2ee";
        let env = encrypt_for_recipient(msg, &pubx).expect("encrypt");
        let pt = decrypt_from_envelope(&env).expect("decrypt");
        assert_eq!(pt, msg);
    }
}
