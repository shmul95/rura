# CLI Usage Examples (Headless)

These examples show how to use the Rust client APIs (exposed to Flutter via FRB) from a headless context to send messages and media over WebRTC.

Prerequisites
- A running server (TLS), and two clients registered/logged in.
- Each client has exchanged public keys (for text E2EE) via `set_pubkey`/`get_pubkey`.

Open a stream and start WebRTC
```rust
use rura_client::api::{open_message_stream_tls, get_account_id};

// Provide server CA PEM, passphrase/password as configured
let ca_pem = std::fs::read_to_string("certs/ca.crt").unwrap();
let (sink_tx, sink_rx) = (/* FRB StreamSink or a channel to receive events */);

open_message_stream_tls(
    "127.0.0.1".into(),
    8443,
    ca_pem,
    "user".into(),
    "pass".into(),
    sink_tx,
).unwrap();

let my_id_b64 = get_account_id().unwrap();
// When sending, the client ensures the WebRTC offer/answer is created automatically.
```

Send media to a peer
```rust
use rura_client::api::send_media_to_identity;

let to_identity = "<peer-base64-identity>".to_string();
let bytes = std::fs::read("/path/to/picture.jpg").unwrap();
send_media_to_identity(
    /* user_id */      1,
    /* to_identity */  to_identity,
    /* mime */         "image/jpeg".into(),
    /* name */         Some("picture.jpg".into()),
    /* bytes */        bytes,
    /* chunk_size */   Some(12 * 1024),
).unwrap();
```

Receive events
```text
{"type":"media_complete","from_user_id":7,"mime":"image/jpeg","name":"picture.jpg","checksum":"...","total_size":12345,"msg_id":"...","data_b64":"..."}
```

Notes
- The server does not relay media over TCP. All media data flows P2P via WebRTC.
- The receiver verifies integrity using the `checksum` field before emitting the `media_complete` event.
