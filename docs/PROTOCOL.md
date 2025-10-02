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
- Messages are delivered only to online recipients (no persistence yet).

Client → Server (send)
- Direct message request (inside `data`):
  - `{"command":"message","data":"{\"to_user_id\":3,\"body\":\"hello world\"}"}`
  - Optional: `saved` boolean to request marking the message as saved
    - `{"command":"message","data":"{\"to_user_id\":3,\"body\":\"hi\",\"saved\":true}"}`

Server → Recipient (deliver)
- Direct message event (inside `data`):
  - `{"command":"message","data":"{\"from_user_id\":1,\"body\":\"hello world\"}"}`

Acknowledgements & Persistence
- Minimal implementation: no sender acknowledgement on success, and no explicit error for unknown recipients.
- Unknown recipient (offline/unknown `to_user_id`): delivery is skipped, but the message is still persisted.
- All direct messages are persisted with an ISO 8601 `timestamp`. A `saved` flag is stored (default false).

## Save Command

Clients can mark/unmark a message as saved.

Client → Server
- `{"command":"save","data":"{\"message_id\":123,\"saved\":true}"}`
  - `saved` defaults to true if omitted.

Server → Client
- `{"command":"save_response","data":"{\"success\":true,\"message\":\"Message updated\",\"message_id\":123,\"saved\":true}"}`
- On failure (message not found or not owned by the caller):
  - `{"command":"save_response","data":"{\"success\":false,\"message\":\"Message not found or not authorized\",\"message_id\":123,\"saved\":true}"}`
- Invalid request format:
  - `{"command":"error","data":"Invalid save format"}`

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

## Notes and Future Extensions
- Envelope stability ensures additional commands can be added without breaking parsing.
- A persistence layer can add offline delivery with `delivered_at`/`read_at` fields in the future.
- Optional presence events (`presence` command) can be added without changing existing clients.
