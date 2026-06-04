{
  # Lean, CI-safe environment for building + deploying the docsite.
  #
  # Deliberately separate from the repo root flake: that one's inputs include
  # a local-path dioxus-flake and the private fts-repo forge, neither of which
  # resolve on a CI runner. The docs build needs none of it — just the rust
  # toolchain (kf docs), native libs for the engraver, and flyctl.
  #
  # `ddc` (dodeca) is NOT packaged here — it ships prebuilt binaries via
  # bearcove's install script (see .github/workflows/deploy-docs.yml).
  description = "keyflow docsite build/deploy shell";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      nixpkgs,
      flake-utils,
      rust-overlay,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
      in
      {
        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            # Keep in sync with ../rust-toolchain.toml (a flake can't read
            # files outside its own root, so the channel is repeated here).
            rust-bin.stable."1.94.1".minimal
            pkg-config
            cmake
            fontconfig
            freetype
            openssl
            flyctl
          ];
          shellHook = ''
            export OPENSSL_DIR=${pkgs.openssl.dev}
            export OPENSSL_LIB_DIR=${pkgs.openssl.out}/lib
          '';
        };
      }
    );
}
