# Rura Architecture Map

This document summarizes what each module and file does, and how requests flow through the system.

## Top Level
- src/main.rs
  - Parses CLI args, initializes DB (`init_db`) and in-memory `AppState`, starts TCP listener, and spawns a task per connection calling `client::handle_client`.
- src/lib.rs
  - Re-exports modules: `auth`, `client`, `messaging`, `models`, `utils`.
- PROTOCOL.md
  - Wire protocol for authentication, direct messaging, and error handling (newline-delimited JSON with `{command, data}` envelope).
- README_AUTH.md / README_DB.md
  - Overviews for authentication flow and database schema/utilities.

## Client (Connection Orchestration)
- src/client/mod.rs
  - `handle_client(stream, conn, state)`
    - Logs the connection, sends `auth_required`, then delegates to the connection loop.
- src/client/loop_task.rs
  - `handle_client_loop(stream, conn, state, client_addr)`
    - Per-connection loop using `tokio::select!` to service:
      - Reads from the socket (incoming commands)
      - Outbound channel (messages destined for this client)
    - After successful auth, registers the user in `AppState` and manages the outbound channel.
- src/client/dispatch.rs
  - `handle_read_success(...)`
    - Routes unauthenticated input to the auth gateway; routes authenticated input to `authed::handle_client_message` with `state` and the outbound sender.
- src/client/unauth.rs
  - `handle_unauthenticated_message(...)`, `handle_unauthenticated_parse_error(...)`
    - Gate that only accepts `login`/`register` until authenticated. Sets the authenticated user on success.
- src/client/authed.rs
  - `handle_client_message(state, outbound, client_addr, user_id, buffer)`
    - Parses the top-level envelope and dispatches post-auth commands:
      - `message`: parse `DirectMessageReq` and call `messaging::handlers::send_direct`.
      - `save`: parse `SaveRequest` and update the `saved` flag via `db_utils::set_message_saved`, respond with `save_response`.
      - other commands: echoed back to the sender via the outbound channel.
    - Sends `{"command":"error","data":"Invalid message format"}` for malformed message payloads.
- src/client/io_helpers.rs
  - `handle_connection_closed`, `handle_read_error`
    - Small logging helpers (and a currently unused writer task helper).

## Auth (Login/Register Domain)
- src/auth/handlers.rs
  - `handle_auth(stream, conn, client_addr, &ClientMessage) -> io::Result<Option<i64>>`
    - Parses `login`/`register` payloads, calls DB helpers, returns `Some(user_id)` on success, and writes `auth_response` messages.
- src/auth/responses.rs
  - `send_auth_success_response`, `send_auth_error_response`
    - JSON helpers to build and send `auth_response` messages.
- src/auth/tests.rs
  - Unit tests for auth flows (in-memory DB), including success/error paths.

## Messaging (User→User Delivery)
- src/messaging/state.rs
  - `struct AppState { users: RwLock<HashMap<i64, ClientHandle>> }`
  - `register`, `unregister`, `get_sender`
    - Tracks online users and provides their outbound senders.
- src/messaging/models.rs
  - `DirectMessageReq { to_user_id, body }`
  - `DirectMessageEvent { from_user_id, body }`
- src/messaging/handlers.rs
  - `send_direct(state, conn, from_user_id, DirectMessageReq)`
    - Persists the message to the `messages` table with `saved` flag (default false).
    - If the recipient is online, pushes a `ClientMessage { command: "message", data: DirectMessageEvent as JSON }` into their outbound channel.
    - If the recipient is offline/unknown, delivery is skipped but the message remains persisted.

## Models
- src/models/client_message.rs
  - `ClientMessage { command, data }` envelope
  - `AuthRequest { passphrase, password }`, `AuthResponse { success, message, user_id }`
- src/models/args.rs
  - CLI `Args { port }` definition.

## Utils (Database/Helpers)
- src/utils/db_utils.rs
  - `init_db()` creates tables: `users`, `messages` (reserved), `connections`.
  - Adds `saved` column to `messages` if missing (migration for older DBs).
  - `store_message(conn, from_user_id, to_user_id, content, saved)` persists direct messages.
  - `set_message_saved(conn, user_id, message_id, saved)` marks/unmarks a message owned by `user_id`.
  - `log_client_connection(client_addr)` records incoming connection IP and timestamp.
  - `register_user`, `authenticate_user` implement credential storage and validation (Argon2).
- src/utils/get_local_ip.rs
  - Finds and prints the host’s local IP for convenience.

## Tests
- tests/integration_tests.rs
  - Integration coverage of auth flow using in-memory DB and duplex streams.
- tests/messaging_tests.rs
  - Direct messaging to an online recipient (delivery) and to an unknown recipient (no delivery).
- src/client/authed.rs (cfg[test])
  - Unit test for malformed message payload → sender gets an `error` event.

## Flow Summary
1) Client connects → `client::handle_client` sends `auth_required`.
2) `auth::handlers::handle_auth` processes `login`/`register`; on success, loop registers the user in `AppState` and enables outbound.
3) Authed client sends `{command:"message", data:"{to_user_id, body}"}` → `client/authed` → `messaging::send_direct` → recipient’s outbound.
4) Errors:
   - Before auth non-auth commands → `error` with prompt to authenticate.
   - Invalid auth payload → `auth_response { success:false }`.
   - Invalid top-level JSON → `error: Invalid JSON`.
   - Invalid message payload → `error: Invalid message format`.
   - Unknown recipient → dropped (no ack).
