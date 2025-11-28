# Calling Requirements & Current RTC Flow

This document inventories the existing RTC/data-channel support and captures
the requirements we will target while layering full audio/video calling on top
of the current signaling transport.

## Current Behavior

- **Signaling transport** — All signaling messages are newline-delimited JSON
  envelopes flowing over the existing TLS stream described in `PROTOCOL.md`.
  The server simply unwraps `ClientMessage` commands and hands WebRTC traffic
  to `crates/server/src/webrtc/handler.rs`.
- **Server registry** — `handler.rs` keeps a process-local `Lazy<HashMap>`
  keyed by ordered user id pairs. Sessions are updated whenever an
  `rtc_offer`/`rtc_answer`/`rtc_ice` payload arrives, but no state beyond the
  latest activity timestamp is persisted. Authorization is implicitly trusted
  because handlers are only called inside the authed command loop.
- **Forwarding logic** — `process_offer/answer/ice` wrap the DTOs
  (`rura_models::webrtc`) back into `ClientMessage` envelopes and reuse the
  existing authed-sender fan-out (`AppState::get_sender_by_session_id`) to push
  them over the recipient’s TLS stream. There is no validation of call state,
  duplicate in-flight sessions, or expiration of stale attempts.
- **Client responsibilities** — `crates/client/src/webrtc.rs` builds
  `webrtc-rs` peer connections with a single data channel. No audio/video
  tracks are provisioned; the API is wired for media file transfer over the
  data channel only. ICE candidates are forwarded using the same TLS stream via
  `send_rtc_ice_over_stream` helpers in `api.rs`.
- **Flutter surface** — The desktop Flutter app (`lib/main.dart`) is purely a
  messaging UI. No call controls, media surfaces, ringing UX, or permission
  prompts exist today. Incoming RTC events (offers/answers/ICE) are not exposed
  to the Dart layer either.

Net: we already possess TLS-authenticated signaling, a peer connection builder,
and chunked media transfer on the WebRTC data channel, but we lack any concept
of call orchestration, voice/video tracks, UX affordances, or diagnostics.

## Proposed Call Goals

- **Scope** — One-to-one calls only for the first iteration. Group calling is
  explicitly out of scope so the signaling state machine can remain linear.
- **Modalities** — Voice is the baseline. Video uses the same session but can
  be toggled on/off (camera mute) without renegotiating the entire call.
  Default start as voice-only with an optional “Enable video” button to reduce
  setup time.
- **Ringing UX** — When Alice initiates a call, Bob receives an incoming call
  sheet showing caller identity, call type (voice/video), and controls to
  accept, reject, or message back. Alice remains on a “ringing…” screen with a
  cancel button until Bob accepts, declines, or a timeout elapses.
- **Controls** — In-call overlay provides mute/unmute microphone, enable/disable
  camera, flip camera (future mobile), and hang up. Speaker output selection
  (default system device) is exposed through the native plugin hooks.
- **Failure paths** — Distinguish between:
  - callee offline/unreachable (show toast immediately)
  - callee busy (server rejects because of another active call)
  - RTC setup errors (ICE timeout, permission denied, device missing)
  - remote hangup vs. network drop
  Each path should surface a reason code back to Flutter for toast/logging.
- **Call log** — Maintain a lightweight recent-call list (even if in-memory) in
  the client to help QA. Persist minimal metadata in Rust so the Flutter UI can
  rebuild without losing the state of the current call.

## Constraints & Assumptions

- **Security** — TLS remains mandatory for signaling. Payloads must not leak
  call metadata outside the existing envelope. Media is E2EE via WebRTC DTLS,
  and we should preserve the option to add app-layer encryption later.
- **Desktop Flutter only** — We focus on the desktop client for now; mobile
  plugin support is explicitly deferred. The FRB API must therefore abstract
  native device enumeration and pass-through texture handles to Flutter.
- **Identity rules** — Reuse current user ids/identities. Block self-calls on
  the server; optionally reject if the target user is blocked or offline. The
  session registry must live inside `AppState` (not a `Lazy` global) so it can
  enforce per-tenant limits and participate in tests.
- **STUN/TURN** — Today only a public STUN (`stun.l.google.com`) is hard-coded.
  We must parameterize ICE servers and support injecting TURN credentials via
  config (env/CLI) for production readiness.
- **Resource cleanup** — Idle sessions should expire (e.g., ringing >30s,
  connected >5m idle). Server timers run on the call registry to avoid leaks.
- **Logging & privacy** — Structured logs may include call ids and states but
  must not log SDP blobs or ICE candidates verbatim. Debug logging should be
  behind feature flags or redact sensitive fields.

These notes seed the follow-on tasks (protocol updates, server orchestration,
client APIs, media plumbing, Flutter UI, and testing) outlined in `todo.yml`.
