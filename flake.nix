{
  description = "fts-ui — FastTrackStudio UI design system";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    devenv.url = "github:cachix/devenv";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
    fts-flake.url = "github:FastTrackStudios/fts-flake";
    fts-flake.inputs.nixpkgs.follows = "nixpkgs";
  };
  nixConfig = {
    extra-trusted-public-keys = [ "devenv.cachix.org-1:w1cLUi8dv3hnoSPGAuibQv+f9TZLr6cv/Hm9XgU50cw=" "fasttrackstudio.cachix.org-1:r7v7WXBeSZ7m5meL6w0wttnvsOltRvTpXeVNItcy9f4=" ];
    extra-substituters = [ "https://devenv.cachix.org" "https://fasttrackstudio.cachix.org" ];
  };
  outputs = { self, nixpkgs, devenv, flake-utils, rust-overlay, fts-flake, } @ inputs:
    flake-utils.lib.eachSystem [ "x86_64-linux" "aarch64-linux" ] (system:
      let
        pkgs = import nixpkgs { inherit system; };
        ftsReaperConfig = "$HOME/.config/FastTrackStudio/Reaper";
        ftsDev = fts-flake.lib.mkFtsPackages { inherit pkgs; cfg = fts-flake.presets.dev // { reaper.configDir = ftsReaperConfig; }; };
        cargoLock = {
          lockFile = ./Cargo.lock;
          outputHashes = {
            "lucide-dioxus-2.26.0" = "sha256-baNzMCjfJ1k9dNhTval9OrUby1cq7073eRlofKn2LI4=";
          };
        };

        ftsUiSrc = pkgs.runCommand "fts-ui-src" { } ''
          set -euo pipefail

          mkdir -p "$out/crates"
          cp -r ${./Cargo.lock} "$out/Cargo.lock"
          cp -r ${./crates/fts-ui} "$out/crates/fts-ui"

          cat > "$out/Cargo.toml" <<'EOF'
[workspace]
members = ["crates/fts-ui"]
resolver = "2"

[workspace.package]
version = "0.1.0"
edition = "2021"
authors = ["FastTrack Studio"]
license = "MIT OR Apache-2.0"

[workspace.dependencies]
dioxus = { version = "0.7.2", default-features = false, features = ["lib"] }
lucide-dioxus = { git = "https://github.com/Leaf-Computer/lucide.git", rev = "e3e509d5855324e893aef838747cccb2d115dc15", features = ["all-icons"] }
tracing = { version = "0.1", features = ["std"] }

[workspace.lints.rust]
unused = "warn"
EOF

          chmod -R u+w "$out"
        '';

        commonRustArgs = {
          version = "0.1.0";
          src = ftsUiSrc;
          inherit cargoLock;
          nativeBuildInputs = with pkgs; [ pkg-config ];
          buildInputs = with pkgs; [ openssl ];
        };
      in {
        packages = {
          fts-ui = pkgs.rustPlatform.buildRustPackage (commonRustArgs // {
            pname = "fts-ui";
            cargoBuildFlags = [ "--package" "fts-ui" ];
            cargoTestFlags = [ "--package" "fts-ui" ];
          });

          default = self.packages.${system}.fts-ui;
        };

        checks = {
          fts-ui = self.packages.${system}.fts-ui;
          default = self.packages.${system}.fts-ui;
        };

        devShells.default = devenv.lib.mkShell {
          inherit inputs pkgs;
          modules = [({ pkgs, config, ... }: {
            devenv.root = builtins.toString ./.;
            cachix.pull = [ "fasttrackstudio" ];
            packages = with pkgs; [
              pkg-config openssl
              libx11 libxi libxext libxrandr libxcursor libxinerama libxcomposite libxdamage libxfixes libxrender libxtst libxcb libxscrnsaver
              libxkbcommon vulkan-loader vulkan-headers vulkan-tools libGL mesa
              gtk3 glib gdk-pixbuf pango cairo atk wayland wayland-protocols fontconfig freetype
              alsa-lib pipewire.jack llvmPackages.libclang dbus zlib stdenv.cc.cc.lib
            ];
            languages.rust = { enable = true; channel = "stable"; };
            env = {
              LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
              LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [ pkgs.vulkan-loader pkgs.libGL pkgs.wayland pkgs.libxkbcommon ];
            };
            scripts = {
              fts-build.exec = "cargo build --workspace"; fts-build.description = "Build fts-ui workspace";
              fts-test.exec = "cargo test --workspace"; fts-test.description = "Run all unit tests";
              fts-check.exec = "cargo check --workspace"; fts-check.description = "Type-check the workspace";
              fts-clippy.exec = "cargo clippy --workspace -- -D warnings"; fts-clippy.description = "Run clippy lints";
            };
            enterShell = ''
              echo ""
              echo "  fts-ui dev shell (devenv + fts-flake)"
              echo "  ────────────────────────────────────────"
              echo "  fts-build     — cargo build --workspace"
              echo "  fts-test      — cargo test --workspace"
              echo "  fts-check     — cargo check --workspace"
              echo "  fts-clippy    — run clippy lints"
              echo ""
            '';
          })];
        };
      }
    );
}
