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
    export LIBGL_ALWAYS_SOFTWARE=1
    export MESA_LOADER_DRIVER_OVERRIDE="${MESA_LOADER_DRIVER_OVERRIDE:-swrast}"
    export GDK_BACKEND="${GDK_BACKEND:-x11}"
    if [ -z "${LIBGL_DRIVERS_PATH:-}" ] && command -v nix >/dev/null 2>&1; then
        NIX_MESA_PATH="$(nix path-info nixpkgs#mesa.drivers 2>/dev/null || true)"
        if [ -n "$NIX_MESA_PATH" ]; then
            export LIBGL_DRIVERS_PATH="$NIX_MESA_PATH/lib/dri"
        fi
    fi
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
FLUTTER_ARGS=("$@")

if echo "$DEVICES" | grep -qi "Linux"; then
    FLUTTER_ARGS+=("--enable-software-rendering")
    flutter run -d linux "${FLUTTER_ARGS[@]}"
elif echo "$DEVICES" | grep -qi "macOS"; then
    flutter run -d macos "$@"
elif echo "$DEVICES" | grep -qi "Windows"; then
    flutter run -d windows "$@"
else
    echo "WARNING: No desktop device detected, trying default..."
    flutter run "$@"
fi
