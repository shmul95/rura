{
  description = "Rura Project";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.05";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, rust-overlay }:
    let
      system = "x86_64-linux";
      overlays = [ rust-overlay.overlays.default ];
      pkgs = import nixpkgs {
        inherit system overlays;
        config = {
          allowUnfree = true;
        };
      };

      rustNightly = pkgs.rust-bin.nightly.latest.default.override {
        extensions = [ "rustfmt" "clippy" ];
      };

      rustTooling = with pkgs; [
        rustNightly
        (rust-bin.nightly.latest.rust-analyzer)
        rustup
        pkg-config
        openssl
        sqlite
      ];

      flutterDesktopDeps = with pkgs; [
        flutter
        dart
        cmake
        ninja
        gtk3
        atk
        at-spi2-atk
        pango
        cairo
        glib
        libGL
        libGLU
        libepoxy
        libxkbcommon
        wayland
        libsecret
        libnotify
        mesa
        libdrm
        alsa-lib
        libpulseaudio
        xorg.libX11
        xorg.libXcursor
        xorg.libXi
        xorg.libXtst
        xorg.libXrandr
        xorg.libXinerama
        xorg.libXext
        xorg.libXdamage
        xorg.libXcomposite
        xorg.libXfixes
        xorg.libxcb
      ];

      miscTools = with pkgs; [
        git
        python3Full
        which
        unzip
        android-studio
      ];
    in {
      devShells.${system}.default = pkgs.mkShell {
        packages = rustTooling ++ flutterDesktopDeps ++ miscTools;
        env = {
          RUST_BACKTRACE = "1";
        };
        shellHook = ''
          export CARGO_TARGET_DIR=''${CARGO_TARGET_DIR:-$PWD/target}
          # Ensure rustup + cargo-installed tools (cargo-ndk, flutter_rust_bridge_codegen) are available.
          export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$HOME/.cargo/bin:$PATH"
          # Default Android SDK/NDK locations (can be overridden in your shell)
          if [ -z "''${ANDROID_SDK_ROOT:-}" ]; then
            export ANDROID_SDK_ROOT="$HOME/Android/Sdk"
          fi
          # Prefer a specific NDK version that you installed via sdkmanager.
          if [ -z "''${ANDROID_NDK_HOME:-}" ] && [ -d "$ANDROID_SDK_ROOT/ndk/26.1.10909125" ]; then
            export ANDROID_NDK_HOME="$ANDROID_SDK_ROOT/ndk/26.1.10909125"
          elif [ -z "''${ANDROID_NDK_HOME:-}" ] && [ -d "$ANDROID_SDK_ROOT/ndk" ]; then
            # Fallback: pick the latest NDK directory under $ANDROID_SDK_ROOT/ndk
            latest_ndk=$(ls -d "$ANDROID_SDK_ROOT"/ndk/* 2>/dev/null | sort | tail -n1)
            if [ -n "$latest_ndk" ]; then
              export ANDROID_NDK_HOME="$latest_ndk"
            fi
          fi
          export LIBGL_DRIVERS_PATH=${pkgs.mesa.drivers}/lib/dri:''${LIBGL_DRIVERS_PATH:-}
          export MESA_LOADER_DRIVER_OVERRIDE=''${MESA_LOADER_DRIVER_OVERRIDE:-swrast}
          if ! command -v flutter_rust_bridge_codegen >/dev/null 2>&1; then
            echo "[devshell] Installing flutter_rust_bridge_codegen via cargo..."
            cargo install flutter_rust_bridge_codegen
          fi
        '';
      };
    };
}
