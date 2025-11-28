#!/usr/bin/env bash
# Build the Rust client, refresh FRB bindings, and launch the Flutter desktop app.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
APP_DIR="$REPO_ROOT/crates/client/flutter_app"
LIB_DIR="$REPO_ROOT/target/release"
CARGO_BIN_DIR="${CARGO_HOME:-$HOME/.cargo}/bin"
export PATH="$CARGO_BIN_DIR:$PATH"

if ! command -v flutter >/dev/null 2>&1; then
    echo "[run_client] ERROR: Flutter is not installed or not in PATH." >&2
    exit 1
fi

if ! command -v flutter_rust_bridge_codegen >/dev/null 2>&1; then
    cat >&2 <<'ERR'
[run_client] ERROR: flutter_rust_bridge_codegen not found.
Install it once via:
  cargo install flutter_rust_bridge_codegen
Then rerun ./scripts/run_client.sh
ERR
    exit 1
fi

echo "[run_client] Running flutter_rust_bridge codegen..."
"$REPO_ROOT/scripts/frb_codegen.sh" "$APP_DIR" "$REPO_ROOT"

echo "[run_client] Building rura_client (release)..."
cargo build --release -p rura_client

if [ ! -d "$LIB_DIR" ]; then
    echo "[run_client] ERROR: expected $LIB_DIR to exist after build." >&2
    exit 1
fi

FOUND_LIB=false
for lib in "$LIB_DIR/librura_client.so" "$LIB_DIR/librura_client.dylib" "$LIB_DIR/rura_client.dll"; do
    if [ -f "$lib" ]; then
        FOUND_LIB=true
        break
    fi
done
if [ "$FOUND_LIB" = false ]; then
    echo "[run_client] ERROR: could not find librura_client.{so|dylib}/rura_client.dll in $LIB_DIR." >&2
    exit 1
fi

echo "[run_client] Launching Flutter desktop client..."
RURA_LIB_DIR="$LIB_DIR" "$REPO_ROOT/scripts/run_flutter_desktop.sh" "$APP_DIR" "$@"
