# End-to-End Encryption (E2EE) in Rura

Goal: ensure the server can route and persist messages without ever learning their plaintext or semantics. The server only sees minimal metadata (from/to ids, timestamps, retention flag) and opaque ciphertext bodies.

## Threat Model
- The transport is TLS (already in place).
- The server is honest-but-curious: it must route and store messages but should not decrypt or infer their purpose beyond minimal metadata.
- Clients are responsible for key generation, distribution, and all crypto.

## Key Distribution (Public Keys Only)
- Each client generates a long-lived public/private key pair locally. Private keys never leave the client device.
- Publish your public key after auth: `set_pubkey { pubkey }`.
- Fetch a peer’s public key by `user_id`: `get_pubkey { user_id }`.
- The server stores keys in `users.pubkey` but cannot decrypt messages.

## Message Envelope (Opaque to Server)
- Use `message` with `data { to_user_id, body, saved? }`.
- Treat `body` as opaque ciphertext. Suggested payload format:
  - `v1:<b64_ephemeral_pub>:<b64_nonce>:<b64_ciphertext>`
  - Derive a symmetric key via X25519 ECDH between the sender’s ephemeral key and recipient’s long-term public key; then apply AEAD (e.g., XChaCha20-Poly1305) over the cleartext JSON payload your app defines.
  - Include optional associated data (AD) like `from_user_id`/`to_user_id` if desired; keep it client-side.

### Sample Cleartext Payload (inside the ciphertext)
```json
{
  "kind": "chat",            // app-defined semantics (server never sees this)
  "text": "hello world",
  "ts": 1710000000,
  "extras": { "saved": false }
}
```

The server forwards `body` unchanged and persists it verbatim.

## Client-side Implementation Notes

Flutter/Dart option (pure Dart, no Rust changes required):
- Add `package:cryptography`.
- Generate keys (once per device/user): `X25519.newKeyPair()`.
- Publish: call FRB `send` helper with `set_pubkey` after login.
- Encrypt per message:
  1) Get peer pubkey via `get_pubkey`.
  2) Generate ephemeral X25519 pair; derive shared secret `x25519(ephemeral_priv, peer_pub)`.
  3) Derive an AEAD key with HKDF.
  4) Encrypt payload with XChaCha20-Poly1305 → `(nonce, ciphertext)`.
  5) Build `body = "v1:<b64 eph pub>:<b64 nonce>:<b64 ct>"` and send.
- Decrypt on receipt: parse envelope; derive shared with `x25519(own_priv, eph_pub)`; HKDF → AEAD key; open ciphertext.

Dev placeholder in Flutter UI
- The sample app auto-wraps plaintext into a `v1:<eph>:<nonce>:<b64(plaintext)>` envelope so the server’s E2EE enforcement does not block sends.
- This is NOT real encryption. Replace it with the proper scheme above before shipping.

Rust client option (if you prefer encrypting before FRB):
- Add `x25519-dalek` + `chacha20poly1305` (requires network to fetch crates).

## Privacy Hygiene
- The server now avoids logging full payloads; only command names and lengths are logged.
- The `saved` flag is a server-side retention hint; if you want to hide even that, move it inside your encrypted payload and stop setting the server flag.

## Caveats & Hardening
- Identity/authentication of keys (TOFU or signatures): Consider signing ephemeral keys with a long-term signing key (Ed25519) and exposing the verify key as the published pubkey. Adds protection against MITM of the key directory.
- Key rotation: publish updated pubkeys via `set_pubkey` and embed key ids in your cleartext payload.
- Forward secrecy: ensured by per-message ephemeral ECDH.
