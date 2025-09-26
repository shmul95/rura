# Tests Overview

This document maps each test file to what it verifies and how to run it.

## Integration Tests
- tests/integration_tests.rs
  - Verifies authentication flows with in-memory DB using duplex streams.
  - Tests:
    - test_complete_client_server_auth_flow: invalid command → error, register → success, login → success.
    - test_registration_then_login_different_sessions: register in one session, login in another.
    - test_multiple_failed_login_attempts: repeated invalid login responses.
  - Run: `cargo test --test integration_tests`

## End-to-End (E2E)
- tests/end_to_end_messaging.rs
  - Spins up a real TcpListener and uses `client::handle_client`.
  - Verifies: auth_required prompt, register success for two users, direct message delivery, DB persistence, and the `save` command flipping the saved flag.
  - Run: `cargo test --test end_to_end_messaging`

## Messaging
- tests/messaging_tests.rs
  - Direct handler-level tests (no sockets) using `AppState` and an in-memory DB.
  - Verifies: delivery to online recipient, no delivery to unknown recipient, and that messages are persisted regardless of delivery.
  - Run: `cargo test --test messaging_tests`

## App State
- tests/app_state_tests.rs
  - Verifies the in-memory online user registry.
  - Tests: register user handle, retrieve sender, send through it, unregister.
  - Run: `cargo test --test app_state_tests`

## Unit Tests in Source
- src/auth/tests.rs
  - Covers: invalid command before auth, register success/duplicate, login success/failure, invalid JSON formats.
  - Run all unit tests: `cargo test --lib` or filter by name, e.g. `cargo test test_login_valid_user_success`.

- src/client/authed.rs (cfg[test])
  - Covers: invalid envelope → error, malformed message payload → error, echo of non-"message" commands, and `save` command success/unauthorized paths.

- src/client/unauth.rs (cfg[test])
  - Covers: invalid JSON before authentication → error.

- src/utils/db_utils.rs (cfg[test])
  - Covers: DB schema creation, Argon2 hashing + uniqueness, authentication validation, and connection logging.

## Running and Filtering
- All tests: `cargo test`
- By file (integration-style): `cargo test --test <file_stem>` (e.g., `integration_tests`)
- By name substring: `cargo test <name_substring>`

## Coverage (HTML)
- Linux (tarpaulin):
  - Install: `cargo install cargo-tarpaulin`
  - Generate: `cargo tarpaulin --all-features --workspace --out Html`
  - Open: `tarpaulin-report.html`
- Cross-platform (llvm-cov):
  - Install: `rustup component add llvm-tools-preview && cargo install cargo-llvm-cov`
  - Generate: `cargo llvm-cov --all-features --workspace --html`
  - Open: `target/llvm-cov/html/index.html`

