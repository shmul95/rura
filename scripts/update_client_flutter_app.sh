#!/usr/bin/env bash
# Update only the Flutter client app inside an existing packaged Rura client.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

if [ -z "${1:-}" ]; then
    echo "Usage: $0 <existing_package_directory_name>"
    echo "Example: $0 tel"
    exit 1
fi

PACKAGE_NAME="$1"
OUTPUT_DIR="$REPO_ROOT/$PACKAGE_NAME"

if [ ! -d "$OUTPUT_DIR" ]; then
    echo "[update_client_flutter_app] ERROR: Package directory not found: $OUTPUT_DIR" >&2
    echo "Run scripts/package_client.sh $PACKAGE_NAME first to create it." >&2
    exit 1
fi

echo "[update_client_flutter_app] Updating Flutter app in: $OUTPUT_DIR"

# Optionally refresh FRB bindings so the Flutter app matches the Rust API surface.
if command -v flutter_rust_bridge_codegen >/dev/null 2>&1; then
    echo "[update_client_flutter_app] Running FRB codegen..."
    "$REPO_ROOT/scripts/frb_codegen.sh" "$REPO_ROOT/crates/client/flutter_app" "$REPO_ROOT" || true
else
    echo "[update_client_flutter_app] WARNING: flutter_rust_bridge_codegen not found; skipping FRB codegen." >&2
fi

SRC_FLUTTER_APP="$REPO_ROOT/crates/client/flutter_app"
DEST_FLUTTER_APP="$OUTPUT_DIR/flutter_app"

if [ -d "$DEST_FLUTTER_APP" ]; then
    echo "[update_client_flutter_app] Removing existing flutter_app in package..."
    rm -rf "$DEST_FLUTTER_APP"
fi

echo "[update_client_flutter_app] Copying fresh flutter_app from repo..."
cp -r "$SRC_FLUTTER_APP" "$DEST_FLUTTER_APP"

echo "[update_client_flutter_app] Cleaning Flutter build cache inside package..."
rm -rf "$DEST_FLUTTER_APP/build"
rm -rf "$DEST_FLUTTER_APP/.dart_tool"

echo "[update_client_flutter_app] Done. Existing lib/, certs/, and run scripts were left untouched."

