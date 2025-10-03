# Rura Architecture Map

This document describes the workspace layout, key crates, and request flow after the split into server, models, and client crates.

## Workspace Overview
- `crates/server` (crate name: `rura_server`)
  - The server binary and library modules (auth, client, messaging, utils).
  - Re-exports shared models so paths like `rura_server::models::client_message::ClientMessage` are available to tests/consumers.
- `crates/models` (crate name: `rura_models`)
  - Shared protocol DTOs used by both server and future clients (e.g., Flutter via FRB).
- `crates/client` (crate name: `rura_client`)
  - Placeholder for a Rust client SDK (to be bridged with Flutter via flutter_rust_bridge).

## Workspace Diagram

```
                   (code dependency)
   +--------------------+       +--------------------+
   |    rura_server     | ----> |    rura_models     |
   |  (server binary)   |       |  (shared DTOs)     |
   +--------------------+       +--------------------+
             ^                             ^
             |                             |
             |         (code dependency)   |
             +-----------------------------+
                           |
                  +--------------------+
                  |    rura_client     |
                  | (client SDK/FRB)   |
                  +--------------------+

   (runtime connection over TLS/TCP)
   rura_client  <====================>  rura_server
```

Legend
- Code dependency: one crate imports types/modules from another at compile time.
- Runtime connection: rura_client connects to rura_server over TLS using the wire protocol.

Docs: [PROTOCOL.md](PROTOCOL.md) and [DATABASE.md](DATABASE.md) remain valid and describe wire format and persistence. The server is TLS-only.

## Server (crate `rura_server`)
- Entry: `crates/server/src/main.rs`
  - Parses CLI, initializes DB (`utils::db_utils::init_db`), creates `messaging::state::AppState`, builds Rustls `TlsAcceptor`, listens, accepts, and spawns `client::handle_client` per connection.
- Modules: `crates/server/src/lib.rs` exposes:
  - `auth` (login/register handlers and responses)
  - `client` (connection loop, unauth/authed dispatch, outbound messaging)
  - `messaging` (in-memory online registry + send handlers)
  - `models` (CLI args + re-exports of shared models)
  - `utils` (TLS, DB, IP helpers)

## Shared Models (crate `rura_models`)
- `client_message`:
  - `ClientMessage { command, data }`
  - `AuthRequest { passphrase, password }`, `AuthResponse { success, message, user_id }`
- `messaging`:
  - `DirectMessageReq { to_user_id, body, saved? }`
  - `DirectMessageEvent { from_user_id, body }`
  - `SaveRequest { message_id, saved? }`, `SaveResponse { success, message, message_id?, saved? }`

## Request Flow
1) TCP connect → TLS handshake → server sends `{"command":"auth_required", ...}`.
2) `auth::handlers::handle_auth` processes `login`/`register`, returns `Some(user_id)` on success; the loop registers the user and enables outbound channel.
3) Post-auth: `message` → persist to DB and deliver to online recipient; `save` → toggle `saved` flag and respond with `save_response`.
4) Errors: non-auth before auth → `error`; invalid JSON → `error`; invalid payloads → `error`; unknown recipient → persisted only.

## TLS Note
- The server requires TLS (`--tls-cert`/`--tls-key`). Use `openssl s_client` for manual testing; plain `telnet`/`nc` will fail the TLS handshake.
