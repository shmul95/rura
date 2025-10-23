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
  - Optional: `saved` boolean to mark the message as retained on the server (retention hint only; not application semantics)
    - `{"command":"message","data":"{\"to_user_id\":3,\"body\":\"v1:...\",\"saved\":true}"}`

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
