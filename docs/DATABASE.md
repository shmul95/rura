# Database Overview

## High-Level
- The Rust server persists its state in the local SQLite file `rura.db`.
- `init_db` (see `crates/server/src/utils/db_utils.rs`) auto-creates the tables when they are missing.
- A single SQLite connection is wrapped in `Arc<Mutex<Connection>>` so Tokio tasks can share it safely.

See also:
- Protocol: PROTOCOL.md
- Architecture: ARCHITECTURE.md

## Table Schemas
### `users`
- `id` INTEGER PRIMARY KEY AUTOINCREMENT
- `passphrase` TEXT UNIQUE: human-readable handle chosen by the user
- `password` TEXT: Argon2 hash encoded in PHC format (algorithm, parameters, salt)

### `messages`
- `id` INTEGER PRIMARY KEY AUTOINCREMENT
- `sender` / `receiver` INTEGER: foreign keys to `users.id`
- `content` TEXT: arbitrary JSON payload
- `timestamp` TEXT: ISO 8601 timestamp
- `saved` INTEGER (0/1): whether the message is marked to keep beyond the transient window

Notes:
- Existing databases created before this change are auto-migrated to add the `saved` column on startup.
- The server persists each direct message on send; delivery to offline users is not yet implemented.

### `connections`
- `id` INTEGER PRIMARY KEY AUTOINCREMENT
- `ip` TEXT: remote client IP address
- `timestamp` TEXT: ISO 8601 timestamp

## Core Operations
- `log_client_connection` records every incoming connection with its IP and timestamp.
- `register_user` enforces passphrase uniqueness, hashes the password, and inserts the user row.
- `authenticate_user` fetches the stored hash and validates credentials with Argon2.

These helpers are invoked from `crates/server/src/auth/handlers.rs` while handling `login` and `register` commands. The integration tests in `crates/server/src/auth/tests.rs` spin up an in-memory database to cover success and failure paths.

## Argon2 Adoption
- Passwords are derived through `hash_password`, which generates a random salt (`SaltString::generate`) and applies `Argon2::default()`.
- Verification happens in `password_matches`, parsing the PHC string and calling `PasswordVerifier`, ensuring constant-time comparisons.
- Hashing errors are coerced into `rusqlite::Error` by `map_password_error`, keeping error handling uniform in the database layer.
- PHC output embeds the salt and parameters, allowing future tuning without schema changes as long as the format remains supported.

## Maintenance Tips
- Inspect the local database via `sqlite3 rura.db` and standard SQL such as `SELECT * FROM users;`.
- Authentication-focused tests live in `crates/server/src/auth/tests.rs` and cover registration/login flows against the in-memory schema.
- When the schema evolves, update `init_db` so automatic creation stays in sync with your manual migrations or scripts.
