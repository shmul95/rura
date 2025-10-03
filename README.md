# Rura

A small asynchronous TCP server written in Rust (Tokio) with:
- Authentication (register/login) backed by SQLite
- Direct user-to-user messaging (online delivery only)
- Simple newline-delimited JSON protocol
- Desktop Flutter client (WhatsApp-like chat UI) bridged via flutter_rust_bridge

Quick links
- Protocol: [docs/PROTOCOL.md](docs/PROTOCOL.md)
- Architecture: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
- Database & Auth: [docs/DATABASE.md](docs/DATABASE.md)
- Flutter/FRB Setup: [docs/FRB_SETUP.md](docs/FRB_SETUP.md)

Client quick start
- Run the desktop Flutter client with FRB bridging: `./scripts/run_client.sh`
  - The client defaults to reading a CA certificate from `certs/ca.crt` (repo root).
  - You can change Host/Port/Cert path, passphrase, and password in the UI.
  - After login/register, you land on a Chats list and can open a chat to send messages.
  - Live updates: the client opens a persistent TLS stream and updates chats in real time when new messages arrive.

## Quick Start

Build and run (TLS-only)
- Generate a self-signed cert (dev):
  - `openssl req -x509 -newkey rsa:2048 -keyout server.key -out server.crt -days 365 -nodes -subj '/CN=localhost'`
- Start the server (from server crate):
  - `cd crates/server`
  - `cargo run -- --port 8443 --tls-cert server.crt --tls-key server.key`
  - If using the dev certs in this repo: `cargo run -- --port 8443 --tls-cert ../../certs/server.crt --tls-key ../../certs/server.key`

Connect with TLS (two terminals)
- Open two TLS clients using OpenSSL:
  - Terminal A: `openssl s_client -connect 127.0.0.1:8443 -servername localhost -CAfile server.crt -quiet`
  - Terminal B: `openssl s_client -connect 127.0.0.1:8443 -servername localhost -CAfile server.crt -quiet`
- Register Alice (A):
  - `{"command":"register","data":"{\"passphrase\":\"alice\",\"password\":\"secret\"}"}`
- Register Bob (B):
  - `{"command":"register","data":"{\"passphrase\":\"bob\",\"password\":\"secret\"}"}`
- Send a message (A → B; replace 2 with Bob’s user_id):
  - `{"command":"message","data":"{\"to_user_id\":2,\"body\":\"hello world\"}"}`
- Bob receives:
  - `{"command":"message","data":"{\"from_user_id\":<alice_id>,\"body\":\"hello world\"}"}`

Flutter client (desktop)
- One-liner: `./scripts/run_client.sh`
  - Script runs FRB codegen, builds the Rust client library, and launches Flutter.
  - Default CA path in the UI: `../../../certs/ca.crt` (relative to the Flutter app folder).
  - Enter passphrase/password; tap Login or Register.
  - Chats list groups conversations by peer user id; tap to open a chat and send messages.
  - Note: live incoming messages require a persistent connection and are not yet enabled; use history refresh in future iterations.

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

Full details: [docs/PROTOCOL.md](docs/PROTOCOL.md)

## Architecture Summary
- Workspace
  - `crates/server` (crate name: `rura_server`): server binary + modules
  - `crates/models` (crate name: `rura_models`): shared protocol DTOs
  - `crates/client` (crate name: `rura_client`): Rust client SDK (skeleton for FRB)
- Server modules (`rura_server`)
  - Connection lifecycle: `crates/server/src/client/*`
  - Auth logic: `crates/server/src/auth/*`
  - Messaging: `crates/server/src/messaging/*`
  - DB/TLS/IP utils: `crates/server/src/utils/*`
  - CLI args: `crates/server/src/models/args.rs`
- Shared models (re-exported from `rura_server`)
  - `rura_server::models::client_message::*` and `rura_server::messaging::models::*`

See: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for a module-by-module map and flow.

## Development
- Format: `cargo fmt --all`
- Lint: `cargo clippy --all-targets --all-features -- -D warnings`
- Test (server crate): `cd crates/server && cargo test`
- Toolchain: stable Rust; rusqlite uses the `bundled` feature (no external SQLite needed)

## Tests & Coverage

- Run all tests (server crate):
  - `cd crates/server && cargo test`

- Run specific server tests:
  - Unit tests only: `cargo test --lib`
  - Integration tests only: `cargo test --test integration_tests`
  - End-to-end messaging: `cargo test --test end_to_end_messaging`
  - Messaging tests: `cargo test --test messaging_tests`
  - AppState tests: `cargo test --test app_state_tests`

- Generate HTML coverage (Linux)
  - Install once: `cargo install cargo-tarpaulin`
  - Generate: `cargo tarpaulin --all-features --workspace --out Html`
  - Open report: `tarpaulin-report.html`
  - Note: Tarpaulin uses ptrace; if your distro restricts it, you may need to allow ptrace temporarily (e.g., `sudo sysctl kernel.yama.ptrace_scope=0`).

- Cross-platform coverage alternative
  - Install: `rustup component add llvm-tools-preview && cargo install cargo-llvm-cov`
  - Generate: `cargo llvm-cov --all-features --workspace --html`
  - Open report: `target/llvm-cov/html/index.html`

- CI coverage
  - GitHub Actions generates and uploads coverage reports; target ≥80% locally.

## Configuration
- CLI: `--port <PORT>` (default 8080). See `crates/server/src/models/args.rs`.
- TLS (required): `--tls-cert <PATH>` and `--tls-key <PATH>` (PEM; PKCS#8 or RSA key). The server refuses to start without them.

## Limitations
- TLS-only endpoint: plain `telnet`/`nc` cannot connect; use a TLS client (`openssl s_client`) or build a proper client.
- Delivery occurs only to online users (no offline delivery yet), but messages are persisted in the database with a `saved` flag.
- No sender acknowledgement or error on unknown recipients (by design for now).
- Envelope uses a JSON string for `data` to keep parsing stable; consider migrating to structured payloads if you control all clients.
