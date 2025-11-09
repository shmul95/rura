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
