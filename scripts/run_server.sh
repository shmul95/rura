#!/usr/bin/env bash
set -euo pipefail

# Build and run the Rust server with TLS certs.
# Usage: scripts/run_server.sh [--port PORT] [--cert PATH] [--key PATH]
# Defaults:
#   PORT: 8443
#   CERT: <repo>/certs/server.crt
#   KEY:  <repo>/certs/server.key

PORT=8443

# Resolve repo root (this script lives under scripts/)
ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CERT_DEFAULT="$ROOT_DIR/certs/server.crt"
KEY_DEFAULT="$ROOT_DIR/certs/server.key"
CERT="$CERT_DEFAULT"
KEY="$KEY_DEFAULT"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --port)
      PORT="${2:?missing port value}"
      shift 2
      ;;
    --cert)
      CERT="${2:?missing cert path}"
      shift 2
      ;;
    --key)
      KEY="${2:?missing key path}"
      shift 2
      ;;
    -h|--help)
      echo "Usage: $0 [--port PORT] [--cert PATH] [--key PATH]"; exit 0;
      ;;
    *)
      echo "Unknown argument: $1" >&2; exit 2;
      ;;
  esac
done

if [[ ! -f "$CERT" ]]; then
  echo "[run_server] ERROR: TLS certificate not found: $CERT" >&2
  echo "             Generate or place server.crt at $CERT_DEFAULT or pass --cert PATH" >&2
  exit 3
fi
if [[ ! -f "$KEY" ]]; then
  echo "[run_server] ERROR: TLS private key not found: $KEY" >&2
  echo "             Generate or place server.key at $KEY_DEFAULT or pass --key PATH" >&2
  exit 4
fi

SERVER_DIR="$ROOT_DIR/crates/server"
if [[ ! -d "$SERVER_DIR" ]]; then
  echo "[run_server] ERROR: Server crate not found at $SERVER_DIR" >&2
  exit 5
fi

echo "[run_server] Building server (release)"
pushd "$SERVER_DIR" >/dev/null
cargo run --release -- --port "$PORT" --tls-cert "$CERT" --tls-key "$KEY"
popd >/dev/null
