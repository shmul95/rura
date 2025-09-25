# Rura

A small asynchronous TCP server written in Rust (Tokio) with:
- Authentication (register/login) backed by SQLite
- Direct user-to-user messaging (online delivery only)
- Simple newline-delimited JSON protocol

Quick links
- Protocol: PROTOCOL.md
- Architecture: ARCHITECTURE.md
- Auth overview: README_AUTH.md
- Database overview: README_DB.md

## Quick Start

Build and run
- `cargo run -- --port 8080` (default: 8080)

Try it with two terminals (Alice and Bob)
- In two shells: `nc 127.0.0.1 8080`
- Register Alice:
  - `{"command":"register","data":"{\"passphrase\":\"alice\",\"password\":\"secret\"}"}`
- Register Bob:
  - `{"command":"register","data":"{\"passphrase\":\"bob\",\"password\":\"secret\"}"}`
- Note their returned `user_id` values from the `auth_response`.
- Send a message (Alice → Bob, replace 2 with Bob’s user_id):
  - `{"command":"message","data":"{\"to_user_id\":2,\"body\":\"hello world\"}"}`
- Bob receives:
  - `{"command":"message","data":"{\"from_user_id\":<alice_id>,\"body\":\"hello world\"}"}`

Login instead of register (if users already exist)
- `{"command":"login","data":"{\"passphrase\":\"alice\",\"password\":\"secret\"}"}`

## Protocol Summary
- Transport: newline-delimited JSON.
- Envelope: `{"command": String, "data": String}` (data holds JSON-encoded payload).
- Authentication
  - Server prompts with `auth_required` on connect
  - Client sends `register` or `login`
  - Server replies with `auth_response { success, user_id, message }`
- Messaging
  - Client → server: `message` with `data { to_user_id, body }`
  - Server → recipient: `message` with `data { from_user_id, body }`
- Errors
  - Before auth non-auth commands → `error: Authentication required...`
  - Invalid auth payload → `auth_response { success:false }`
  - Invalid top-level JSON → `error: Invalid JSON`
  - Invalid message payload → `error: Invalid message format`
  - Unknown recipient → dropped silently (no ack)

Full details: PROTOCOL.md

## Architecture Summary
- Connection lifecycle and read/write loop: `src/client/*`
- Auth domain logic: `src/auth/*`
- Messaging registry and routing: `src/messaging/*`
- DB helpers: `src/utils/db_utils.rs`
- Models: `src/models/*`

See: ARCHITECTURE.md for a module-by-module map and flow.

## Development
- Format: `cargo fmt`
- Lint: `cargo clippy -- -D warnings`
- Test: `cargo test`
- Toolchain: stable Rust; rusqlite uses the `bundled` feature (no external SQLite needed)

## Configuration
- CLI: `--port <PORT>` (default 8080). See `src/models/args.rs`.

## Limitations
- Messaging delivers only to online users (no persistence/history yet).
- No sender acknowledgement or error on unknown recipients (by design for now).
- Envelope uses a JSON string for `data` to keep parsing stable; consider migrating to structured payloads if you control all clients.

