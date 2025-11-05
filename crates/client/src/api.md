# API Module Documentation (`api.rs`)

This module provides a complete TLS client API for Flutter via `flutter_rust_bridge` (FRB).  
It handles authentication, registration, encrypted messaging, history retrieval, and local storage synchronization for the **Rura** messaging client.

---

## Overview

- All network communication is performed over **TLS** using [`rustls`](https://docs.rs/rustls).
- Communication uses JSON-encoded `ClientMessage` envelopes defined in `rura_models`.
- Functions prefixed with `#[frb]` are exposed to Dart via Flutter Rust Bridge.
- Local storage and encryption are delegated to the `crate::local_storage` and `crate::security` modules.

---

## Data Structures

### `LoginResponse`
Represents the outcome of a login or registration attempt.

| Field | Type | Description |
|-------|------|-------------|
| `success` | `bool` | Whether authentication succeeded |
| `message` | `String` | Human-readable status message |
| `user_id` | `Option<i64>` | The assigned user ID (if successful) |

---

### `HistoryMessage`
A Dart-friendly version of the server’s message model.

| Field | Type | Description |
|-------|------|-------------|
| `id` | `i64` | Message ID |
| `from_user_id` | `i64` | Sender ID |
| `to_user_id` | `i64` | Recipient ID |
| `body` | `String` | Message body (E2EE envelope) |
| `timestamp` | `String` | ISO timestamp |

---

### `HistoryBundle`
Returned by combined login/history functions.

| Field | Type | Description |
|-------|------|-------------|
| `success` | `bool` | Whether login or registration succeeded |
| `message` | `String` | Status message |
| `user_id` | `Option<i64>` | Authenticated user ID |
| `messages` | `Vec<HistoryMessage>` | Retrieved or local history |

---

### `SendResult`
Simplified result for send operations.

| Field | Type | Description |
|-------|------|-------------|
| `success` | `bool` | Whether message sending succeeded |
| `message` | `String` | Status text |

---

## Local Cache Helpers

### `append_local_message(...)`
Appends a message to the on-disk local cache for the given user.

Used by Dart when offline messages are stored locally before syncing.

---

### `load_local_history(user_id, limit)`
Loads the locally cached chat history for the given user.

Returns a vector of `HistoryMessage` objects.

---

## TLS Setup and Utilities

### `build_root_store_from_pem(pem)`
Parses a PEM string containing one or more CA certificates and builds a `rustls::RootCertStore`.

---

### `read_line(stream)`
Reads a line from a stream until a newline (`\n`) or EOF.  
Used for line-delimited JSON protocol messages.

---

### `make_tls_stream(host, port, ca_pem)`
Creates a `rustls::StreamOwned<ClientConnection, TcpStream>` connected to the specified host and port with the given CA certificate.

---

### `auth_over_stream(tls, command, passphrase, password)`
Performs a login or register command over an established TLS stream.

- Sends an `AuthRequest`
- Waits for an `auth_response`
- Unlocks local storage and sets current user on success

---

## Authentication APIs

### `login_tls(host, port, ca_pem, passphrase, password)`
Connects to the server, performs TLS handshake, sends a `login` command, and returns a `LoginResponse`.

---

### `register_tls(host, port, ca_pem, passphrase, password)`
Same as `login_tls`, but sends a `register` command to create a new account.

---

## History Retrieval

### `fetch_history_over_stream(tls, limit)`
Requests message history via the active TLS connection.  
Parses a `history_response` and returns messages as `Vec<HistoryMessage>`.

---

### `login_and_fetch_history_tls(host, port, ca_pem, passphrase, password, limit)`
Performs login and retrieves history in a single TLS session.  
Returns a `HistoryBundle`.

---

### `register_and_fetch_history_tls(host, port, ca_pem, passphrase, password, limit)`
Registers a new user and immediately fetches history in one session.  
Returns a `HistoryBundle`.

---

## Offline / Local Mode

### `login_and_load_local_history_tls(host, port, ca_pem, passphrase, password, limit)`
If `host` is empty or `port == 0`, performs **offline unlock**:
- Uses last known user ID from local storage.
- Unlocks the encrypted database locally using the password.

If online, authenticates via TLS and loads **local** cached history only.

---

### `register_and_load_local_history_tls(host, port, ca_pem, passphrase, password, limit)`
If offline, creates a **new local user** (incremental user_id) and initializes encrypted storage.

If online, performs remote registration, ensures a local identity exists, and loads local cache.

---

## Messaging APIs

### `send_direct_message_tls(host, port, ca_pem, passphrase, password, to_user_id, body, saved)`
Performs login and sends a **direct message** in one TLS session.

- Verifies that the `body` is a valid `v1:` E2EE envelope.
- Sends `{ "command": "message" }` with the message data.

---

### `send_direct_message_over_stream(user_id, to_user_id, body, saved)`
Sends a direct message over an existing long-lived stream session.

Requires that the user already has an active stream in the global `SESSIONS` map.

---

### `set_pubkey_over_stream(user_id, pubkey)`
Publishes a new public key for the authenticated user over an existing stream.

---

### `get_pubkey_tls(host, port, ca_pem, passphrase, password, user_id)`
Logs in and fetches another user’s published public key.

Returns `Option<String>` with the key, or `None` if unavailable.

---

## Message Stream APIs

### `open_message_stream_tls(host, port, ca_pem, passphrase, password, sink)`
Opens a persistent TLS session for receiving live messages.

- Authenticates via `login`
- Emits an initial `{"type":"auth_ok","user_id":...}` event
- Listens for incoming messages (`{"command":"message"}`)
- Streams them to Dart via `StreamSink<String>`
- Stores an outgoing `Sender<String>` channel in `SESSIONS` for writes

---

### `open_message_stream_register_tls(host, port, ca_pem, passphrase, password, sink)`
Same as above, but authenticates via `register` instead of `login`.

Used for first-time users who have no account yet.

---

## Session Management

### `SESSIONS`
A global `Lazy<Mutex<HashMap<i64, Sender<String>>>>` mapping `user_id` → message send channel.

Maintains references to active TLS message stream sessions.

---

## Internal Helper Functions

### `is_base64ish(s)`
Checks whether a string contains only Base64 characters.

### `is_e2ee_envelope(body)`
Checks whether a message body matches the expected `v1:<eph>:<nonce>:<ct>` E2EE envelope format.

Used to enforce end-to-end encryption.

---

## Tests

### `build_root_store_from_valid_pem`
Validates that a self-signed certificate parses correctly into a `RootCertStore`.

### `build_root_store_from_empty_pem_fails`
Ensures an empty PEM returns an error.

### `read_line_reads_until_newline`
Verifies that `read_line` stops at `\n`.

### `read_line_reads_all_without_newline`
Verifies that `read_line` reads entire input if no newline is found.

---

## Module Responsibilities Summary

| Area | Functionality |
|-------|----------------|
| **TLS Setup** | Certificate parsing, stream creation, authentication |
| **Auth & Registration** | `login_tls`, `register_tls`, `auth_over_stream` |
| **Message History** | `fetch_history_over_stream`, `login_and_fetch_history_tls` |
| **Persistent Stream** | `open_message_stream_tls`, `send_direct_message_over_stream` |
| **Local Storage** | Offline caching, message persistence, user unlock |
| **E2EE Enforcement** | Envelope validation before sending |
| **Key Management** | `set_pubkey_over_stream`, `get_pubkey_tls` |

---

## Notes

- The module assumes a **line-delimited JSON protocol** between client and server.
- All outgoing and incoming packets are terminated with `\n`.
- The design allows seamless **online/offline hybrid** operation, enabling local chat persistence and end-to-end encryption.
