#!/usr/bin/env bash
# Package the Rura client into a standalone directory
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

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

echo "[package_client] Building Rust client library (release)..."
cd "$REPO_ROOT/crates/client"
cargo build --release --quiet

echo "[package_client] Copying Flutter app..."
cp -r "$REPO_ROOT/crates/client/flutter_app" "$OUTPUT_DIR/"

echo "[package_client] Cleaning Flutter build cache..."
rm -rf "$OUTPUT_DIR/flutter_app/build"
rm -rf "$OUTPUT_DIR/flutter_app/.dart_tool"

echo "[package_client] Copying Rust library..."
mkdir -p "$OUTPUT_DIR/lib"
if [ -f "$REPO_ROOT/crates/client/target/release/librura_client.so" ]; then
    cp "$REPO_ROOT/crates/client/target/release/librura_client.so" "$OUTPUT_DIR/lib/"
elif [ -f "$REPO_ROOT/crates/client/target/release/librura_client.dylib" ]; then
    cp "$REPO_ROOT/crates/client/target/release/librura_client.dylib" "$OUTPUT_DIR/lib/"
elif [ -f "$REPO_ROOT/crates/client/target/release/rura_client.dll" ]; then
    cp "$REPO_ROOT/crates/client/target/release/rura_client.dll" "$OUTPUT_DIR/lib/"
else
    echo "[package_client] ERROR: Could not find compiled library"
    exit 1
fi

echo "[package_client] Copying certificates (optional)..."
if [ -d "$REPO_ROOT/certs" ]; then
    cp -r "$REPO_ROOT/certs" "$OUTPUT_DIR/"
fi

echo "[package_client] Creating run script..."
cat > "$OUTPUT_DIR/run_client.sh" << 'RUNSCRIPT'
#!/usr/bin/env bash
# Run the packaged Rura client
set -e

PACKAGE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_DIR="$PACKAGE_DIR/flutter_app"
LIB_DIR="$PACKAGE_DIR/lib"

# Detect platform and set library path
if [ "$(uname)" = "Darwin" ]; then
    export DYLD_LIBRARY_PATH="$LIB_DIR:${DYLD_LIBRARY_PATH:-}"
    LIB_EXT="dylib"
elif [ "$(uname)" = "Linux" ]; then
    export LD_LIBRARY_PATH="$LIB_DIR:${LD_LIBRARY_PATH:-}"
    LIB_EXT="so"
else
    # Windows (assuming Git Bash or similar)
    export PATH="$LIB_DIR:$PATH"
    LIB_EXT="dll"
fi

# Check if library exists
if [ ! -f "$LIB_DIR/librura_client.$LIB_EXT" ] && [ ! -f "$LIB_DIR/rura_client.dll" ]; then
    echo "ERROR: Rust library not found in $LIB_DIR"
    exit 1
fi

# Check if Flutter is installed
if ! command -v flutter &> /dev/null; then
    echo "ERROR: Flutter is not installed or not in PATH"
    echo "Please install Flutter from: https://flutter.dev/docs/get-started/install"
    exit 1
fi

echo "Starting Rura client..."
echo "Library path: $LIB_DIR"
echo "App directory: $APP_DIR"

cd "$APP_DIR"

# Detect available device (suppress broken pipe errors)
DEVICES=$(flutter devices 2>/dev/null || true)
if echo "$DEVICES" | grep -qi "Linux"; then
    flutter run -d linux "$@"
elif echo "$DEVICES" | grep -qi "macOS"; then
    flutter run -d macos "$@"
elif echo "$DEVICES" | grep -qi "Windows"; then
    flutter run -d windows "$@"
else
    echo "WARNING: No desktop device detected, trying default..."
    flutter run "$@"
fi
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
   ./run_client.sh
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
├── run_client.sh       # Launch script
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
