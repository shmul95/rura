use rura_client::webrtc;
use base64::Engine as _;
use sha2::{Digest, Sha256};
use std::sync::mpsc;
use std::time::Duration;

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let out = hasher.finalize();
    hex::encode(out)
}

#[test]
fn media_chunk_reassembly_end_to_end_local() {
    // Sample payload from test assets
    let data: &[u8] = include_bytes!("assets/sample.jpg");
    let checksum = sha256_hex(data);
    let total_size = data.len();
    let chunk_size = 500usize; // small to force multiple chunks for test stability
    let chunks = total_size.div_ceil(chunk_size) as u32;

    // Wire up an inbound sink for a test user to capture the final event
    let user_id = 42i64;
    let (tx, rx) = mpsc::channel::<String>();
    webrtc::register_inbound_sink(user_id, tx);

    let msg_id = "deadbeefcafebabe0011223344556677"; // 16 bytes hex
    let mime = "image/jpeg";
    let name = Some("sample.jpg");
    let from_user_id = 7i64;
    let from_identity = Some("dummy-b64-id");

    let mut offset = 0usize;
    let mut idx: u32 = 0;
    while offset < total_size {
        let end = std::cmp::min(offset + chunk_size, total_size);
        let slice = &data[offset..end];
        let data_b64 = base64::engine::general_purpose::STANDARD.encode(slice);
        let ev = serde_json::json!({
            "type": "media",
            "from_user_id": from_user_id,
            "from_identity": from_identity,
            "mime": mime,
            "name": name,
            "checksum": checksum,
            "total_size": total_size as u64,
            "msg_id": msg_id,
            "chunk_index": idx,
            "chunk_count": chunks,
            "data_b64": data_b64,
        })
        .to_string();
        webrtc::handle_media_chunk_json(user_id, &ev).expect("chunk accepted");
        offset = end;
        idx += 1;
    }

    // Expect a media_complete event
    let done = rx
        .recv_timeout(Duration::from_secs(1))
        .expect("no reassembled event delivered");
    let v: serde_json::Value = serde_json::from_str(&done).expect("valid json");
    assert_eq!(
        v.get("type").and_then(|s| s.as_str()),
        Some("media_complete")
    );
    assert_eq!(
        v.get("checksum").and_then(|s| s.as_str()),
        Some(&checksum[..])
    );
    assert_eq!(
        v.get("total_size").and_then(|n| n.as_u64()),
        Some(total_size as u64)
    );
    let data_b64 = v
        .get("data_b64")
        .and_then(|s| s.as_str())
        .expect("data_b64 present");
    let assembled = base64::engine::general_purpose::STANDARD
        .decode(data_b64)
        .expect("b64 decode");
    assert_eq!(assembled, data);
}
