#!/usr/bin/env bash
# Package the Rura client into a standalone directory
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CARGO_BIN_DIR="${CARGO_HOME:-$HOME/.cargo}/bin"
export PATH="$CARGO_BIN_DIR:$PATH"

if [ -z "$1" ]; then
    echo "Usage: $0 <output_directory_name>"
    echo "Example: $0 rura-client-package"
    exit 1
fi

OUTPUT_DIR="$REPO_ROOT/$1"

echo "[package_client] Creating package in: $OUTPUT_DIR"

# Remove existing package directory if it exists
if [ -d "$OUTPUT_DIR" ]; then
    echo "[package_client] Removing existing directory: $OUTPUT_DIR"
    rm -rf "$OUTPUT_DIR"
fi

# Create package structure
mkdir -p "$OUTPUT_DIR"

if ! command -v flutter_rust_bridge_codegen >/dev/null 2>&1; then
    echo "[package_client] ERROR: flutter_rust_bridge_codegen not found."
    echo "Install it via: cargo install flutter_rust_bridge_codegen" >&2
    exit 1
fi

echo "[package_client] Building Rust client library (release)..."
cd "$REPO_ROOT"
cargo build --release -p rura_client --quiet

TARGET_LIB_DIR="$REPO_ROOT/target/release"
case "$(uname)" in
    Darwin) LIB_ARTIFACT="librura_client.dylib" ;;
    Linux) LIB_ARTIFACT="librura_client.so" ;;
    MINGW64_NT*|MSYS_NT*|CYGWIN_NT*|Windows_NT|Windows)
        LIB_ARTIFACT="rura_client.dll"
        ;;
    *)
        LIB_ARTIFACT=""
        ;;
esac

if [ -z "$LIB_ARTIFACT" ]; then
    echo "[package_client] ERROR: Unsupported OS for packaging (uname=$(uname))." >&2
    exit 1
fi

if [ ! -f "$TARGET_LIB_DIR/$LIB_ARTIFACT" ]; then
    echo "[package_client] ERROR: Expected artifact $TARGET_LIB_DIR/$LIB_ARTIFACT not found. Did the build succeed?" >&2
    exit 1
fi

echo "[package_client] Running FRB codegen to update bindings..."
"$REPO_ROOT/scripts/frb_codegen.sh" "$REPO_ROOT/crates/client/flutter_app" "$REPO_ROOT" || true

echo "[package_client] Copying Flutter app..."
cp -r "$REPO_ROOT/crates/client/flutter_app" "$OUTPUT_DIR/"

echo "[package_client] Cleaning Flutter build cache..."
rm -rf "$OUTPUT_DIR/flutter_app/build"
rm -rf "$OUTPUT_DIR/flutter_app/.dart_tool"

echo "[package_client] Copying Rust library..."
mkdir -p "$OUTPUT_DIR/lib"
cp "$TARGET_LIB_DIR/$LIB_ARTIFACT" "$OUTPUT_DIR/lib/$LIB_ARTIFACT"

echo "[package_client] Copying certificates (optional)..."
if [ -d "$REPO_ROOT/certs" ]; then
    cp -r "$REPO_ROOT/certs" "$OUTPUT_DIR/"
fi

echo "[package_client] Copying run helper script..."
cp "$REPO_ROOT/scripts/run_flutter_desktop.sh" "$OUTPUT_DIR/run_flutter_desktop.sh"
chmod +x "$OUTPUT_DIR/run_flutter_desktop.sh"

echo "[package_client] Creating run script..."
cat > "$OUTPUT_DIR/run_client.sh" << 'RUNSCRIPT'
#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
exec "$SCRIPT_DIR/run_flutter_desktop.sh" "$SCRIPT_DIR/flutter_app" "$@"
RUNSCRIPT

chmod +x "$OUTPUT_DIR/run_client.sh"

echo "[package_client] Creating README..."
cat > "$OUTPUT_DIR/README.md" << 'README'
# Rura Client Package

This is a standalone package of the Rura messaging client.

## Requirements

- Flutter SDK (https://flutter.dev/docs/get-started/install)
- Linux/macOS/Windows desktop support enabled in Flutter

## Quick Start

1. Ensure Flutter is installed and in your PATH:
   ```bash
   flutter --version
   ```

2. Run the client:
   ```bash
   ./run_client.sh   # or ./run_flutter_desktop.sh for direct access
   ```

## Configuration

- Default CA certificate path: `../../../certs/ca.crt` (relative to flutter_app/)
- You can change the server host, port, and certificate path in the UI
- User data is stored in `.cache/` directory (created automatically)

## Directory Structure

```
.
├── flutter_app/         # Flutter application code
│   ├── lib/            # Dart source code
│   ├── linux/          # Linux platform files
│   ├── macos/          # macOS platform files
│   └── windows/        # Windows platform files
├── lib/                # Rust shared library
│   └── librura_client.so (or .dylib/.dll)
├── certs/              # TLS certificates (optional)
├── run_client.sh       # Wrapper around the helper script
├── run_flutter_desktop.sh # Cross-platform helper (forces software rendering on Linux)
└── README.md           # This file
```

## Troubleshooting

### Library not found
If you get a library error, ensure the environment variable is set:
- Linux: `export LD_LIBRARY_PATH=./lib:$LD_LIBRARY_PATH`
- macOS: `export DYLD_LIBRARY_PATH=./lib:$DYLD_LIBRARY_PATH`
- Windows: Add `lib/` to your PATH

### Flutter not found
Install Flutter and ensure it's in your PATH:
```bash
export PATH="$PATH:/path/to/flutter/bin"
```

### Linux GL errors
If you see `FL_IS_ENGINE(self)` or `Failed to initialize GLArea`, run via `./run_flutter_desktop.sh` (or keep using `run_client.sh`, which already delegates to it). The helper exports Mesa software-rendering variables and passes `--enable-software-rendering` to Flutter.

### No desktop support
Enable desktop support for your platform:
```bash
flutter config --enable-linux-desktop
flutter config --enable-macos-desktop
flutter config --enable-windows-desktop
```
README

echo ""
echo "✅ Package created successfully!"
echo ""
echo "Package location: $OUTPUT_DIR"
echo ""
echo "To run the client:"
echo "  cd $OUTPUT_DIR"
echo "  ./run_client.sh"
echo ""
echo "To distribute, archive the entire directory:"
echo "  tar -czf rura-client.tar.gz $OUTPUT_DIR"
echo "  # or"
echo "  zip -r rura-client.zip $OUTPUT_DIR"
