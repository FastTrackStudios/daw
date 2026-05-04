{
  description = "Keyflow development shell";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      nixpkgs,
      flake-utils,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          config.allowUnfree = true;
        };

        linuxGuiPackages =
          with pkgs;
          lib.optionals stdenv.isLinux [
            atk
            cairo
            gdk-pixbuf
            glib
            gtk3
            libsoup_3
            pango
            webkitgtk_4_1
            xdotool
            libx11
            libxcb
            libxcursor
            libxi
            libxkbcommon
            libxrandr
            wayland
            libGL
            vulkan-loader
          ];
      in
      {
        devShells.default = pkgs.mkShell {
          packages =
            with pkgs;
            [
              cargo
              clippy
              cmake
              fontconfig
              freetype
              nodejs_22
              openssl
              pkg-config
              pnpm
              rustc
              rustfmt
            ]
            ++ linuxGuiPackages;

          shellHook = ''
            export RUST_BACKTRACE=1
            export OPENSSL_DIR=${pkgs.openssl.dev}
            export OPENSSL_LIB_DIR=${pkgs.openssl.out}/lib
          '';
        };
      }
    );
}
