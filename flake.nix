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
      pkgs = import nixpkgs { inherit system overlays; };

      rustNightly = pkgs.rust-bin.nightly.latest.default.override {
        extensions = [ "rustfmt" "clippy" ];
      };

      rustTooling = with pkgs; [
        rustNightly
        (rust-bin.nightly.latest.rust-analyzer)
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
      ];
    in {
      devShells.${system}.default = pkgs.mkShell {
        packages = rustTooling ++ flutterDesktopDeps ++ miscTools;
        env = {
          RUST_BACKTRACE = "1";
        };
        shellHook = ''
          export CARGO_TARGET_DIR=''${CARGO_TARGET_DIR:-$PWD/target}
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
