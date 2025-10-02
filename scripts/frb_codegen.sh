#!/usr/bin/env bash
set -euo pipefail

# Usage: scripts/frb_codegen.sh /path/to/flutter_app [path/to/rust_repo]
# - Run from anywhere. Requires Dart/Flutter installed in PATH.
# - Generates Dart bindings in the Flutter app and Rust glue in this repo.

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 /path/to/flutter_app [path/to/rust_repo]" >&2
  exit 1
fi

FLUTTER_APP_DIR="$(cd "$1" && pwd)"
RUST_REPO_DIR="${2:-$(cd "$(dirname "$0")/.." && pwd)}"

# FRB v2 prefers module path input and a crate root.
RUST_ROOT="$RUST_REPO_DIR/crates/client"
RUST_INPUT_MOD="crate::api"
# Put Dart outputs in a clean folder without ".dart" suffix
DART_OUTPUT="$FLUTTER_APP_DIR/lib/frb"
RUST_OUTPUT="$RUST_REPO_DIR/crates/client/src/bridge_generated.rs"

if [[ ! -f "$RUST_ROOT/Cargo.toml" ]]; then
  echo "Rust crate root not found: $RUST_ROOT (missing Cargo.toml)" >&2
  exit 2
fi
if [[ ! -d "$FLUTTER_APP_DIR" ]]; then
  echo "Flutter app dir not found: $FLUTTER_APP_DIR" >&2
  exit 3
fi

echo "Running FRB codegen..."
echo "  Rust root:    $RUST_ROOT"
echo "  Rust input:   $RUST_INPUT_MOD"
  echo "  Dart output:  $DART_OUTPUT"
  echo "  Rust output:  $RUST_OUTPUT"

pushd "$FLUTTER_APP_DIR" >/dev/null

# Ensure flutter_rust_bridge is available in Flutter app
if command -v rg >/dev/null 2>&1; then
  HAVE_FRB_IN_PUBSPEC=$(rg -n "flutter_rust_bridge" pubspec.yaml >/dev/null 2>&1 && echo yes || echo no)
else
  HAVE_FRB_IN_PUBSPEC=$(grep -q "flutter_rust_bridge" pubspec.yaml >/dev/null 2>&1 && echo yes || echo no)
fi
if [[ "$HAVE_FRB_IN_PUBSPEC" != "yes" ]]; then
  echo "Adding flutter_rust_bridge to pubspec..."
  flutter pub add flutter_rust_bridge
fi

# Prefer the Rust CLI codegen if available (FRB v2 recommends cargo-based CLI)
if command -v flutter_rust_bridge_codegen >/dev/null 2>&1; then
  # Newer FRB CLI requires a subcommand, typically `generate`.
  flutter_rust_bridge_codegen generate \
    --rust-root "$RUST_ROOT" \
    --rust-input "$RUST_INPUT_MOD" \
    --dart-output "$DART_OUTPUT" \
    --rust-output "$RUST_OUTPUT"
else
  echo "flutter_rust_bridge_codegen not found."
  echo "Trying Dart-based codegen (deprecated in newer FRB versions)..."
  if dart run flutter_rust_bridge:run_codegen \
      --rust-input "$RUST_ROOT/src/lib.rs" \
      --dart-output "$DART_OUTPUT" \
      --rust-output "$RUST_OUTPUT"; then
    :
  else
    echo "ERROR: FRB codegen failed. Please install the Rust CLI:"
    echo "  cargo install flutter_rust_bridge_codegen"
    echo "Then re-run: $0 \"$FLUTTER_APP_DIR\" \"$RUST_REPO_DIR\""
    exit 4
  fi
fi

popd >/dev/null

echo "FRB codegen completed."
if [[ -d "$DART_OUTPUT" ]]; then
  echo "Generated Dart dir: $DART_OUTPUT"
elif [[ -f "$DART_OUTPUT" ]]; then
  echo "Generated Dart file: $DART_OUTPUT"
else
  echo "WARNING: Expected Dart output not found at $DART_OUTPUT (file or dir)"
fi
if [[ -f "$RUST_OUTPUT" ]]; then
  echo "Generated Rust: $RUST_OUTPUT"
fi
echo "Remember to set your dynamic library path before running Flutter (see docs/FRB_SETUP.md)."
