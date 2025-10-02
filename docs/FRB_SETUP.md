# Flutter + flutter_rust_bridge (FRB) Setup

This repo exposes a client crate (`crates/client`) to bridge Rust to Flutter using FRB. Follow these steps to generate bindings and call a simple Rust function.

## 1) Build the Rust client crate

- From the repo root:
  - `cargo build -p rura_client`
- Dynamic library output (debug profile):
  - Linux: `target/debug/librura_client.so`
  - macOS: `target/debug/librura_client.dylib`
  - Windows: `target/debug/rura_client.dll`

## 2) Create a Flutter app (in this repo) and run FRB codegen

- Option A (auto): Run the helper script to create a Flutter app under `crates/client/flutter_app` and generate bindings:
  - `scripts/init_flutter_client.sh`
  - If codegen fails, install the Rust CLI and re-run:
    - `cargo install flutter_rust_bridge_codegen`
    - `scripts/frb_codegen.sh crates/client/flutter_app`

- Option B (manual): In your own Flutter project, add FRB and run codegen:
  - Recommended (Rust CLI):
    - `cargo install flutter_rust_bridge_codegen`
    - `flutter_rust_bridge_codegen generate \
        --rust-root ../rura/crates/client \
        --rust-input crate::api \
        --dart-output lib/frb \
        --rust-output ../rura/crates/client/src/bridge_generated.rs`
  - Legacy (Dart CLI; may be unavailable in newer FRB):
    - `flutter pub add flutter_rust_bridge`
    - `dart run flutter_rust_bridge:run_codegen --rust-input ../rura/crates/client/src/lib.rs --dart-output lib/frb --rust-output ../rura/crates/client/src/bridge_generated.rs`
  - Adjust the paths if your Flutter app lives elsewhere. `--rust-input` must point to `crates/client/src/lib.rs`.

## 3) Make the dynamic library discoverable (desktop dev)

- Before `flutter run`, set the library search path so Flutter finds the compiled Rust library:
  - Linux: `export LD_LIBRARY_PATH=../rura/target/debug:$LD_LIBRARY_PATH`
  - macOS: `export DYLD_LIBRARY_PATH=../rura/target/debug:$DYLD_LIBRARY_PATH`
  - Windows (PowerShell): `$env:PATH += ';..\\rura\\target\\debug'`

## 4) Call Rust from Dart (hello example)

- After codegen, import the generated files in your Flutter app (e.g., `lib/main.dart`):
  - `import 'frb/api.dart';`
  - `import 'frb/frb_generated.dart';`
- Then call the generated API that exposes the `hello()` function from Rust. The exact shape may vary by FRB version (class vs. top-level function). Look for `hello` in `lib/bridge_generated.dart` and invoke it, e.g.:
  - `final result = await hello(); // or await api.hello();`
  - `print(result); // "Hello from Rust"`

Notes
- FRB will regenerate `crates/client/src/bridge_generated.rs` and `lib/bridge_generated.dart` on each codegen run.
- For packaging, consider copying the built `.so/.dylib/.dll` next to your Flutter app binary or customizing the loader.
- To add real features, extend `crates/client/src/api.rs` with `#[frb]` functions (connect/login/send/streams) and re-run codegen.
