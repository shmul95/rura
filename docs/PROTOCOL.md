# Rura Wire Protocol

This document defines the on-the-wire protocol for authentication, direct messaging, and error reporting.

## Transport
- TLS (server-only) over TCP with newline-delimited JSON (one JSON object per line).
- Envelope type for all messages:
  - `{ "command": String, "data": String }`
  - `data` carries a JSON-encoded payload as a string (double-encoded JSON) to keep the envelope stable.

## Authentication

Flow
- On connect, server sends an auth prompt:
  - `{"command":"auth_required","data":"Please authenticate by sending 'login' or 'register' command with your credentials"}`
- Client must send `login` or `register`.

Client → Server
- Register:
  - `{"command":"register","data":"{\"passphrase\":\"alice\",\"password\":\"secret\"}"}`
- Login:
  - `{"command":"login","data":"{\"passphrase\":\"alice\",\"password\":\"secret\"}"}`

Server → Client
- Auth response wrapper:
  - `{"command":"auth_response","data":"{\"success\":true,\"message\":\"Registration successful\",\"user_id\":1}"}`
  - On failure: `success:false`, `user_id:null`, `message` explains the error.

Error cases (auth phase)
- Invalid command before auth:
  - `{"command":"error","data":"Authentication required. Please send 'login' or 'register' command first"}`
- Invalid JSON payload format for auth:
  - `{"command":"auth_response","data":"{\"success\":false,\"message\":\"Invalid authentication format\",\"user_id\":null}"}`

## Direct Messaging (user → user)

State
- After successful auth, the server tracks the connection’s `user_id`.
- Messages are delivered to online recipients; the server does not persist messages; clients store them locally.

Client → Server (send)
- Direct message request (inside `data`):
  - Plaintext example (legacy/testing):
    - `{"command":"message","data":"{\"to_user_id\":3,\"body\":\"hello world\"}"}`
  - E2EE recommended: treat `body` as opaque ciphertext (base64), containing an application-defined envelope. One suggested format:
    - `v1:<b64_ephemeral_pub>:<b64_nonce>:<b64_ciphertext>`
    - Where ciphertext is an AEAD over the cleartext payload; the server does not parse it.
  - No server-side retention flags are supported. Persistence is handled on the client.

Server → Recipient (deliver)
- Direct message event (inside `data`):
  - `{"command":"message","data":"{\"from_user_id\":1,\"body\":\"<opaque>\"}"}`
  - The `body` is not inspected or modified by the server.

### Enforcing E2EE
- E2EE is enforced by default. Messages whose `body` is not a `v1:<b64>:<b64>:<b64>` envelope are rejected with `{"command":"error","data":"E2EE required: invalid or missing envelope"}` and are not persisted or delivered.
- The client SDK also rejects non-envelope bodies (see FRB functions `send_direct_message_tls` and `send_direct_message_over_stream`).

Client stream (Flutter)
- The desktop client opens a persistent TLS session and listens for incoming lines.
- It filters for the `message` command, decrypts locally, and stores the plaintext in the client’s local cache.

Acknowledgements & Persistence
- Minimal implementation: no sender acknowledgement on success, and no explicit error for unknown recipients.
- Unknown recipient (offline/unknown `to_user_id`): delivery is skipped. No server persistence is performed.

## Save Command
Removed. The server does not support saving messages; clients manage their own local storage.

## E2EE Key Distribution

To enable end-to-end encryption without server access to plaintext, clients should exchange or publish public keys. The server provides a simple key directory for convenience; it only stores public keys.

Client → Server (set own pubkey)
- `{"command":"set_pubkey","data":"{\"pubkey\":\"<base64-public-key>\"}"}`

Server → Client
- `{"command":"set_pubkey_response","data":"{\"success\":true,\"message\":\"Pubkey stored\"}"}`

Client → Server (fetch another user's pubkey)
- `{"command":"get_pubkey","data":"{\"user_id\":123}"}`

Server → Client
- `{"command":"get_pubkey_response","data":"{\"success\":true,\"message\":\"OK\",\"user_id\":123,\"pubkey\":\"<base64-public-key>\"}"}`
- When unavailable: `success:false` and `pubkey:null` with a message.

Error cases (post-auth)
- Malformed `message` request (invalid `data` JSON):
  - Sent back to the sender:
    - `{"command":"error","data":"Invalid message format"}`
- Invalid top-level JSON (not a valid envelope):
  - Sent back to the sender:
    - `{"command":"error","data":"Invalid JSON"}`

## Session Lifecycle
- Connect → `auth_required` → `login`/`register` → `auth_response(success=true)` → normal messaging.
- On disconnect: server unregisters the user from the online registry.

## Client SDK mapping (FRB)
- The Flutter app calls Rust APIs that map to protocol operations:
  - `login_tls`/`register_tls` → `login`/`register` + read `auth_response`
  - `login_and_fetch_history_tls`/`register_and_fetch_history_tls` → auth + `history` → `history_response`
  - `send_direct_message_tls` → auth + `message`
- All TLS APIs require a CA PEM string to validate the server certificate.

## Notes and Future Extensions
- Envelope stability ensures additional commands can be added without breaking parsing.
- A persistence layer can add offline delivery with `delivered_at`/`read_at` fields in the future.
- Optional presence events (`presence` command) can be added without changing existing clients.

## WebRTC Data Channel: Media Messages

Overview
- Peers establish a WebRTC data channel via SDP offer/answer exchanged over the TLS stream.
- Once open, user-to-user payloads are sent P2P; the server does not relay message bodies.

Media Transfer Format (over data channel)
- Text JSON envelope for chunked media frames:
  - `{`
  - `  "type": "media",`
  - `  "from_user_id": <i64>,`
  - `  "from_identity": "<base64-identity>",`
  - `  "to_identity": "<base64-identity>",`
  - `  "mime": "image/jpeg" | "image/png" | "audio/mpeg" | ...,`
  - `  "name": "optional-filename.ext" | null,`
  - `  "checksum": "<hex-sha256-of-complete-file>",`
  - `  "total_size": <u64>,`
  - `  "msg_id": "<hex-16-byte-id>",`
  - `  "chunk_index": <u32>,`
  - `  "chunk_count": <u32>,`
  - `  "data_b64": "<base64-of-chunk-bytes>"`
  - `}`

Reassembly Event
- The receiver reassembles chunks per `msg_id` and verifies the `checksum`.
- On success, the client emits a final event to the app sink:
  - `{`
  - `  "type": "media_complete",`
  - `  "from_user_id": <i64>,`
  - `  "from_identity": "<base64-identity>",`
  - `  "mime": "...",`
  - `  "name": "..." | null,`
  - `  "checksum": "<hex-sha256>",`
  - `  "total_size": <u64>,`
  - `  "msg_id": "<hex>",`
  - `  "data_b64": "<base64-of-complete-file>"`
  - `}`

Security
- WebRTC provides transport encryption (DTLS/SRTP). The server does not see or log media contents.
- Application-level E2EE for media can be layered later by encrypting `data_b64` payloads prior to sending.

## Call Signaling Commands

High-level call control rides on the same TLS stream as chat signaling. All
call-related commands MUST include a stable `call_id` so both the server and
clients can correlate `rtc_*` payloads with a specific session.

### State Machine

1. `call_invite` — Initiator notifies the callee that a call is ringing. The
   payload includes desired media (audio/video) and optional preview metadata.
2. `call_ringing` — Server acknowledgement emitted to both parties to confirm
   that the invite is active. Contains an `expires_at` timestamp so clients can
   show countdown timers.
3. `call_answer` — Callee accepted. This transitions the server-side session to
   `connected` and allows `rtc_offer`/`rtc_answer`/`rtc_ice` messages to flow.
4. `call_reject` — Callee declined or is busy. The server tears down state and
   forwards the reason.
5. `call_hangup` — Either side can hang up while ringing or connected. The
   server marks the call `ended` and notifies the remote peer.

`rtc_offer` and `rtc_answer` messages now include an optional `call_id`.
Clients SHOULD populate it to guarantee that stale SDP blobs are not applied
to the wrong peer connection. ICE candidates also carry `call_id` plus an
optional `track` hint that names the MID/kind/stream for debugging purposes.

### Payloads

`call_invite` (client → server):
```json
{
  "call_id": "28c88b8e-1d27-4f8c-9ef9-8b480f91a9f1",
  "from_user_id": 1,
  "to_user_id": 2,
  "media": {
    "audio_enabled": true,
    "video_enabled": false,
    "audio_muted": false,
    "video_muted": true
  },
  "preview": {
    "avatar_url": "https://cdn.example/avatar/alice.jpg",
    "note": "Voice call"
  },
  "client": {
    "device_label": "Mac mini",
    "platform": "macos-x86_64",
    "app_version": "0.4.0-dev"
  },
  "ringing_timeout_ms": 30000
}
```

`call_ringing` (server → client):
```json
{
  "call_id": "28c88b8e-1d27-4f8c-9ef9-8b480f91a9f1",
  "callee_user_id": 2,
  "ringing": true,
  "expires_at": 1714418366000
}
```

`call_answer`:
```json
{
  "call_id": "28c88b8e-1d27-4f8c-9ef9-8b480f91a9f1",
  "from_user_id": 2,
  "to_user_id": 1,
  "resume_media": {
    "audio_enabled": true,
    "video_enabled": true,
    "audio_muted": false,
    "video_muted": false
  }
}
```

`call_reject` and `call_hangup` share the same shape:
```json
{
  "call_id": "28c88b8e-1d27-4f8c-9ef9-8b480f91a9f1",
  "from_user_id": 2,
  "to_user_id": 1,
  "reason": "busy",
  "note": "Already on a call"
}
```

`rtc_ice` extensions:
```json
{
  "from_user_id": 1,
  "to_user_id": 2,
  "call_id": "28c88b8e-1d27-4f8c-9ef9-8b480f91a9f1",
  "candidate": "candidate:842163049 1 udp 1686052607 1.2.3.4 56143 typ srflx raddr 0.0.0.0 rport 0 generation 0 ufrag wCwE network-cost 999",
  "sdp_mid": "0",
  "sdp_mline_index": 0,
  "track": {
    "mid": "0",
    "stream_id": "audio_123",
    "track_id": "local_audio",
    "kind": "audio"
  }
}
```

### Timing Expectations

- Ringing expires after `ringing_timeout_ms` (default 30s). The server emits a
  synthetic `call_hangup` with reason `timeout` when it cleans up.
- After `call_answer`, either peer must send `rtc_offer` within 5s (configurable)
  or the server downgrades the call to `failed`.
- ICE keepalives older than 30s are dropped to protect against stale sessions.
