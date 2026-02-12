{
  description = "Rura Project - Cross-platform communication app";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.05";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ rust-overlay.overlays.default ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        # Rust toolchain with nightly for Cargo.lock v4 support
        rustNightly = pkgs.rust-bin.nightly.latest.default.override {
          extensions = [ "rustfmt" "clippy" ];
        };

        # Common Rust dependencies
        rustDeps = with pkgs; [
          rustNightly
          pkg-config
          openssl
          sqlite
          alsa-lib
          alsa-lib.dev
        ];

        # Flutter and desktop dependencies
        flutterDeps = with pkgs; [
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
        ];

        # Graphics and X11 libraries for desktop
        graphicsDeps = with pkgs; [
          mesa
          libdrm
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

        # Android development dependencies
        androidDeps = with pkgs; [
          android-tools
          jdk17
        ];

        # Common environment setup
        commonEnv = {
          RUST_BACKTRACE = "1";
          PKG_CONFIG_PATH = "${pkgs.alsa-lib.dev}/lib/pkgconfig:${pkgs.openssl.dev}/lib/pkgconfig";
          LIBGL_DRIVERS_PATH = "${pkgs.mesa.drivers}/lib/dri";
          MESA_LOADER_DRIVER_OVERRIDE = "swrast";
        };

        # Helper function to find repository root
        findRepoScript = ''
          find_repo_root() {
            local current="$PWD"
            while [[ "$current" != "/" ]]; do
              if [[ -f "$current/flake.nix" && -f "$current/Cargo.toml" ]]; then
                echo "$current"
                return 0
              fi
              current=$(dirname "$current")
            done
            # Fallback locations
            for fallback in "$HOME/Repositories/rura" "$HOME/rura" "/tmp/rura"; do
              if [[ -d "$fallback" && -f "$fallback/flake.nix" ]]; then
                echo "$fallback"
                return 0
              fi
            done
            echo "Error: Could not find rura repository root!" >&2
            return 1
          }
        '';

        # Server script with default arguments
        serverScript = pkgs.writeShellScript "rura-server" ''
          set -e
          ${findRepoScript}
          
          REPO_ROOT=$(find_repo_root)
          cd "$REPO_ROOT"
          
          echo "Building and running rura-server from: $REPO_ROOT"
          
          # Set up environment
          export PATH="${pkgs.lib.makeBinPath rustDeps}:$PATH"
          export PKG_CONFIG_PATH="${commonEnv.PKG_CONFIG_PATH}"
          
          # Check if arguments were provided
          if [ $# -eq 0 ]; then
            echo "Using default arguments: --tls-cert certs/server.crt --tls-key certs/server.key --port 8443 --debug-io"
            echo "To override, pass your own arguments: nix run .#server -- --tls-cert /path/to/cert ..."
            set -- --tls-cert certs/server.crt --tls-key certs/server.key --port 8443 --debug-io
          fi
          
          # Build and run
          cargo build --release --bin rura_server
          exec cargo run --release --bin rura_server -- "$@"
        '';

        # Build script for development
        buildScript = pkgs.writeShellScript "rura-build" ''
          set -e
          ${findRepoScript}
          
          REPO_ROOT=$(find_repo_root)
          cd "$REPO_ROOT"
          
          echo "Building all Rust components from: $REPO_ROOT"
          
          export PATH="${pkgs.lib.makeBinPath rustDeps}:$PATH"
          export PKG_CONFIG_PATH="${commonEnv.PKG_CONFIG_PATH}"
          
          echo "Building server..."
          cargo build --release --bin rura_server
          
          echo "Building client library..."
          cargo build --release --lib -p rura_client
          
          echo ""
          echo "Build completed! Generated files:"
          echo "  Server binary: $REPO_ROOT/target/release/rura_server"
          if [ -f "$REPO_ROOT/target/release/librura_client.so" ]; then
            echo "  Client library: $REPO_ROOT/target/release/librura_client.so"
          elif [ -f "$REPO_ROOT/target/release/librura_client.dylib" ]; then
            echo "  Client library: $REPO_ROOT/target/release/librura_client.dylib"
          else
            echo "  Client library: $(ls -la $REPO_ROOT/target/release/librura_client.* 2>/dev/null || echo 'Not found')"
          fi
        '';

        # Client library script (for desktop app)
        clientLibScript = pkgs.writeShellScript "rura-client-lib" ''
          set -e
          ${findRepoScript}
          
          REPO_ROOT=$(find_repo_root)
          cd "$REPO_ROOT"
          
          echo "Building rura-client library from: $REPO_ROOT"
          
          export PATH="${pkgs.lib.makeBinPath rustDeps}:$PATH"
          export PKG_CONFIG_PATH="${commonEnv.PKG_CONFIG_PATH}"
          
          cargo build --release --lib -p client --crate-type cdylib
          echo "Client library built: $REPO_ROOT/target/release/libclient.so"
        '';

        # Desktop Flutter app script
        desktopScript = pkgs.writeShellScript "rura-desktop" ''
          set -e
          ${findRepoScript}
          
          REPO_ROOT=$(find_repo_root)
          cd "$REPO_ROOT"
          
          echo "Building and running desktop app from: $REPO_ROOT"
          
          # Set up environment
          export PATH="${pkgs.lib.makeBinPath (rustDeps ++ flutterDeps)}:$PATH"
          export PKG_CONFIG_PATH="${commonEnv.PKG_CONFIG_PATH}"
          export LD_LIBRARY_PATH="$REPO_ROOT/target/release:${pkgs.lib.makeLibraryPath graphicsDeps}:''${LD_LIBRARY_PATH:-}"
          export LIBGL_DRIVERS_PATH="${commonEnv.LIBGL_DRIVERS_PATH}"
          export MESA_LOADER_DRIVER_OVERRIDE="${commonEnv.MESA_LOADER_DRIVER_OVERRIDE}"
          
          # Build Rust client library first
          echo "Building Rust client library..."
          cargo build --release --lib -p client --crate-type cdylib
          
          # Build and run Flutter desktop app
          cd pc
          echo "Building Flutter desktop app..."
          flutter pub get
          flutter build linux --release
          
          echo "Running desktop app..."
          exec flutter run -d linux
        '';

        # Phone app build script
        phoneScript = pkgs.writeShellScript "rura-phone" ''
          set -e
          ${findRepoScript}
          
          REPO_ROOT=$(find_repo_root)
          cd "$REPO_ROOT"
          
          echo "Building phone app from: $REPO_ROOT"
          
          export PATH="${pkgs.lib.makeBinPath (rustDeps ++ flutterDeps ++ androidDeps)}:$PATH"
          export PKG_CONFIG_PATH="${commonEnv.PKG_CONFIG_PATH}"
          
          if [[ -z "''${ANDROID_SDK_ROOT:-}" ]]; then
            echo "Warning: ANDROID_SDK_ROOT not set. Android builds may fail."
            echo "Set it to your Android SDK location, e.g.: export ANDROID_SDK_ROOT=\$HOME/Android/Sdk"
          fi
          
          cd tel
          echo "Getting Flutter dependencies..."
          flutter pub get
          
          echo "Building Android APK..."
          flutter build apk --release
          
          echo "Building Android App Bundle..."
          flutter build appbundle --release
          
          echo ""
          echo "Build completed! Generated files:"
          echo "  APK: $REPO_ROOT/tel/build/app/outputs/flutter-apk/app-release.apk"
          echo "  AAB: $REPO_ROOT/tel/build/app/outputs/bundle/release/app-release.aab"
        '';

      in {
        # Packages for nix build
        packages = {
          server = pkgs.stdenv.mkDerivation {
            name = "rura-server";
            src = ./.;
            buildPhase = "echo 'Built rura-server'";
            installPhase = ''
              mkdir -p $out/bin
              cp ${serverScript} $out/bin/rura-server
              chmod +x $out/bin/rura-server
            '';
          };

          build = pkgs.stdenv.mkDerivation {
            name = "rura-build";
            src = ./.;
            buildPhase = "echo 'Built rura-build'";
            installPhase = ''
              mkdir -p $out/bin
              cp ${buildScript} $out/bin/rura-build
              chmod +x $out/bin/rura-build
            '';
          };

          desktop = pkgs.stdenv.mkDerivation {
            name = "rura-desktop";
            src = ./.;
            buildPhase = "echo 'Built rura-desktop'";
            installPhase = ''
              mkdir -p $out/bin
              cp ${desktopScript} $out/bin/rura-desktop
              chmod +x $out/bin/rura-desktop
            '';
          };

          phone = pkgs.stdenv.mkDerivation {
            name = "rura-phone";
            src = ./.;
            buildPhase = "echo 'Built rura-phone'";
            installPhase = ''
              mkdir -p $out/bin
              cp ${phoneScript} $out/bin/rura-phone
              chmod +x $out/bin/rura-phone
            '';
          };

          client-lib = pkgs.stdenv.mkDerivation {
            name = "rura-client-lib";
            src = ./.;
            buildPhase = "echo 'Built rura-client-lib'";
            installPhase = ''
              mkdir -p $out/bin
              cp ${clientLibScript} $out/bin/rura-client-lib
              chmod +x $out/bin/rura-client-lib
            '';
          };

          default = self.packages.${system}.server;
        };

        # Apps for nix run
        apps = {
          server = {
            type = "app";
            program = "${self.packages.${system}.server}/bin/rura-server";
          };

          build = {
            type = "app";
            program = "${self.packages.${system}.build}/bin/rura-build";
          };

          desktop = {
            type = "app";
            program = "${self.packages.${system}.desktop}/bin/rura-desktop";
          };

          phone = {
            type = "app";
            program = "${self.packages.${system}.phone}/bin/rura-phone";
          };

          client-lib = {
            type = "app";
            program = "${self.packages.${system}.client-lib}/bin/rura-client-lib";
          };

          default = self.apps.${system}.server;
        };

        # Development shell
        devShells.default = pkgs.mkShell {
          packages = rustDeps ++ flutterDeps ++ graphicsDeps ++ androidDeps ++ [
            pkgs.git
            pkgs.python3Full
            pkgs.which
            pkgs.unzip
          ];
          
          env = commonEnv;
          
          shellHook = ''
            echo "🦀 Rura Development Environment"
            echo "Available commands:"
            echo "  nix run .#server   - Run server (with default TLS certs)"
            echo "  nix run .#build    - Build all Rust components"
            echo "  nix run .#desktop  - Run Flutter desktop app"  
            echo "  nix run .#phone    - Build Android APK/AAB"
            echo ""
            echo "Environment ready!"
            
            export CARGO_TARGET_DIR=''${CARGO_TARGET_DIR:-$PWD/target}
            
            # Check for Android SDK
            if [[ -z "''${ANDROID_SDK_ROOT:-}" ]]; then
              if [[ -d "$HOME/Android/Sdk" ]]; then
                export ANDROID_SDK_ROOT="$HOME/Android/Sdk"
                echo "📱 Found Android SDK at: $ANDROID_SDK_ROOT"
              else
                echo "⚠️  Android SDK not found. For phone builds, set ANDROID_SDK_ROOT"
              fi
            fi
          '';
        };
      });
}
