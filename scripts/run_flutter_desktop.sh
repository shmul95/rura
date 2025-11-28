#!/usr/bin/env bash
# Helper to run a Flutter desktop app with the right env vars for this repo or packaged builds.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

find_app_dir() {
    local dir=""
    for cand in \
        "$SCRIPT_DIR/flutter_app" \
        "$SCRIPT_DIR/../flutter_app" \
        "$SCRIPT_DIR/../crates/client/flutter_app" \
        "$SCRIPT_DIR/../../crates/client/flutter_app"; do
        if [ -d "$cand" ]; then
            dir="$(cd "$cand" && pwd)"
            break
        fi
    done
    echo "$dir"
}

lib_dir_has_artifact() {
    local dir="$1"
    for lib in librura_client.so librura_client.dylib rura_client.dll; do
        if [ -f "$dir/$lib" ]; then
            return 0
        fi
    done
    return 1
}

find_lib_dir() {
    if [ -n "${RURA_LIB_DIR:-}" ]; then
        if [ -d "$RURA_LIB_DIR" ]; then
            echo "$RURA_LIB_DIR"
            return
        elif [ -f "$RURA_LIB_DIR" ]; then
            echo "$(cd "$(dirname "$RURA_LIB_DIR")" && pwd)"
            return
        fi
    fi
    local dir=""
    for cand in \
        "$SCRIPT_DIR/lib" \
        "$SCRIPT_DIR/../lib" \
        "$SCRIPT_DIR/target/debug" \
        "$SCRIPT_DIR/target/release" \
        "$SCRIPT_DIR/../target/debug" \
        "$SCRIPT_DIR/../target/release" \
        "$SCRIPT_DIR/../crates/client/target/debug" \
        "$SCRIPT_DIR/../crates/client/target/release"; do
        if [ -d "$cand" ] && lib_dir_has_artifact "$cand"; then
            dir="$cand"
            break
        fi
    done
    echo "$dir"
}

if [ $# -gt 0 ] && [ -d "$1" ]; then
    APP_DIR="$(cd "$1" && pwd)"
    shift
else
    APP_DIR="$(find_app_dir)"
fi

if [ -z "${APP_DIR:-}" ] || [ ! -d "$APP_DIR" ]; then
    echo "ERROR: could not locate Flutter app directory. Pass it explicitly as the first argument." >&2
    exit 1
fi

LIB_DIR="$(find_lib_dir)"
if [ -z "$LIB_DIR" ]; then
    echo "ERROR: could not locate librura_client.*. Build the Rust client first or set RURA_LIB_DIR." >&2
    exit 1
fi

if ! command -v flutter >/dev/null 2>&1; then
    echo "ERROR: Flutter is not installed or not in PATH." >&2
    exit 1
fi

DEVICE_ARGS=()

case "$(uname)" in
    Darwin)
        export DYLD_LIBRARY_PATH="$LIB_DIR:${DYLD_LIBRARY_PATH:-}"
        DEVICE_ARGS=(-d macos)
        ;;
    Linux)
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
        DEVICE_ARGS=(-d linux)
        ;;
    MINGW64_NT*|MSYS_NT*|CYGWIN_NT*|Windows_NT)
        export PATH="$LIB_DIR:$PATH"
        DEVICE_ARGS=(-d windows)
        ;;
    *)
        DEVICE_ARGS=()
        ;;
esac

echo "Running Flutter app from: $APP_DIR"
echo "Using native library path: $LIB_DIR"
if [ "${#DEVICE_ARGS[@]}" -gt 0 ]; then
    echo "Device args: ${DEVICE_ARGS[*]}"
else
    echo "Device args: auto"
fi

pushd "$APP_DIR" >/dev/null

EXTRA_ARGS=("$@")

if [[ "${DEVICE_ARGS[*]}" == "-d linux" ]]; then
    flutter run "${DEVICE_ARGS[@]}" --enable-software-rendering "${EXTRA_ARGS[@]}"
elif [[ ${#DEVICE_ARGS[@]} -gt 0 ]]; then
    flutter run "${DEVICE_ARGS[@]}" "${EXTRA_ARGS[@]}"
else
    flutter run "${EXTRA_ARGS[@]}"
fi

popd >/dev/null
