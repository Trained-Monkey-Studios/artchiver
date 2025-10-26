{
  description = "eframe devShell";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        rustVersion = "1.90.0";
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
      in with pkgs; {
        devShells.default = mkShell rec {
          buildInputs = [
            # Rust
            (rust-bin.stable.${rustVersion}.default.override {
              extensions = [
                "rust-std"
                "rustfmt"
                "rust-src" # for rust-analyzer
                "rust-analyzer"
              ];
              targets = [ "wasm32-unknown-unknown" ];
            })
            trunk

            # misc. libraries
            SDL2.dev
            clang
            cmake
            ffmpeg.dev
            gdb
            libclang
            libffi.dev
            mold-wrapped
            mpv
            pipewire.dev
            pkg-config

            # GUI libs
            libxkbcommon
            libGL
            fontconfig

            # wayland libraries
            wayland

            # x11 libraries
            xorg.libXcursor
            xorg.libXrandr
            xorg.libXi
            xorg.libX11
            libdrm.dev
          ];

          packages = [
            just
            rlwrap
            sqlite
            vmtouch
          ];

          LD_LIBRARY_PATH = "${lib.makeLibraryPath buildInputs}";
        };
      });
}
