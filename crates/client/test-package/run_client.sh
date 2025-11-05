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

# Try to detect available device
if flutter devices | grep -q "Linux"; then
    flutter run -d linux "$@"
elif flutter devices | grep -q "macOS"; then
    flutter run -d macos "$@"
elif flutter devices | grep -q "Windows"; then
    flutter run -d windows "$@"
else
    echo "WARNING: No desktop device detected, trying default..."
    flutter run "$@"
fi
