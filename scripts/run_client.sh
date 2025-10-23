#!/usr/bin/env bash
set -euo pipefail

# Launch the Flutter client app with flutter_rust_bridge bindings.
# Usage: scripts/run_client.sh [device]
#   device: flutter device id (default: linux). Examples: linux, macos, windows

DEVICE="${1:-linux}"

# Resolve repo root (this script lives under scripts/)
ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
APP_DIR="$ROOT_DIR/crates/client/flutter_app"

echo "[run_client] Repo root: $ROOT_DIR"

if ! command -v flutter >/dev/null 2>&1; then
  echo "[run_client] ERROR: 'flutter' not found on PATH. Please install Flutter and ensure 'flutter --version' works." >&2
  exit 127
fi

# Create the Flutter app if it doesn't exist yet
if [[ ! -d "$APP_DIR" ]]; then
  echo "[run_client] Flutter app not found; creating at $APP_DIR"
  "$ROOT_DIR/scripts/init_flutter_client.sh"
fi

echo "[run_client] Running FRB codegen"
"$ROOT_DIR/scripts/frb_codegen.sh" "$APP_DIR" "$ROOT_DIR"

echo "[run_client] Formatting Rust workspace (cargo fmt --all)"
pushd "$ROOT_DIR" >/dev/null
cargo fmt --all
popd >/dev/null

echo "[run_client] Building Rust client (release) into crates/client/target"
export CARGO_TARGET_DIR="$ROOT_DIR/crates/client/target"
pushd "$ROOT_DIR/crates/client" >/dev/null
cargo build --release
popd >/dev/null

LIB_PATH="$CARGO_TARGET_DIR/release/librura_client.so"
if [[ "$OSTYPE" == darwin* ]]; then
  LIB_PATH="$CARGO_TARGET_DIR/release/librura_client.dylib"
elif [[ "${OS:-}" == Windows_NT ]]; then
  LIB_PATH="$CARGO_TARGET_DIR/release/rura_client.dll"
fi

if [[ ! -f "$LIB_PATH" ]]; then
  echo "[run_client] ERROR: Expected dynamic library not found at: $LIB_PATH" >&2
  echo "            Make sure the build succeeded and the file exists." >&2
  exit 3
fi

echo "[run_client] Launching Flutter app on device: $DEVICE (E2EE enforced)"
pushd "$APP_DIR" >/dev/null
flutter pub get
# Enforce E2EE in the client UI via a compile-time define; the Rust layer also rejects plaintext bodies.
flutter run -d "$DEVICE" --dart-define=REQUIRE_E2EE=true
popd >/dev/null
