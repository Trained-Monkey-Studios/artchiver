let
  rust_overlay = import (builtins.fetchTarball "https://github.com/oxalica/rust-overlay/archive/master.tar.gz");
  pkgs = import <nixpkgs> { overlays = [ rust_overlay ]; };

  # Pin rather than using "latest" so we can make clippy errors sticky
  # Note: stable toolchain
  rustVersion = "1.88.0";
  rust = pkgs.rust-bin.stable.${rustVersion}.default.override {
    extensions = [
      "rust-std"
      "rustfmt"
      "rust-src" # for rust-analyzer
      "rust-analyzer"
    ];
    targets = [ "wasm32-unknown-unknown"];
  };

  # If we need to do macro-backtrace or other nightly only analysis
  #rust = pkgs.rust-bin.nightly.latest.default;
in
pkgs.mkShell {
  # Binaries to build with
  nativeBuildInputs = (with pkgs; [
    assimp
    awscli2
    clang
    gnumake
    just
    mold-wrapped
    ninja
    pkg-config
    rust
    sccache
    xz
  ]);

  # Libs to build and run with
  buildInputs = (with pkgs; [
    alsa-lib.dev
    atk
    fontconfig
    gdb
    gdk-pixbuf
    glxinfo
    gtest
    gtk3
    libxkbcommon
    openssl
    pango
    sqlite
    udev
    vulkan-tools
    wayland
    xorg.libX11
    xorg.libXcursor
    xorg.libXi
    xorg.libXrandr
    xorg.libXxf86vm
  ]);

  shellHook = ''
    sccache --stop-server
    sccache --start-server
  '';

  LD_LIBRARY_PATH = (pkgs.lib.concatStrings (builtins.map (a: ''${a}/lib:'') (with pkgs; [
    libxkbcommon
    mesa
    vulkan-loader
    wayland
    xorg.libX11
    xorg.libXcursor
    xorg.libXi
    xorg.libXrandr
    xorg.libXxf86vm
  ])));

  RUSTC_WRAPPER = "${pkgs.sccache}/bin/sccache";
  SCCACHE_CACHE_SIZE = "120G";
  RUST_BACKTRACE = 1;
  LIBCLANG_PATH = pkgs.lib.makeLibraryPath [ pkgs.llvmPackages_latest.libclang.lib ];
  DISPLAY = ":0";
}
