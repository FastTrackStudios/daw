# dioxus-flake

A reusable Nix flake that provides a complete development environment for Dioxus applications across all platforms — web, desktop, mobile, and native.

## Usage

Add this flake as an input in your project's `flake.nix`:

```nix
{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
    dioxus-flake.url = "github:your-org/dioxus-flake";
  };

  outputs = { nixpkgs, dioxus-flake, ... }:
    # Use dioxus-flake's devShell, packages, and checks
    # in your own flake outputs.
}
```

## What's included

- Rust toolchain with targets for web (WASM), desktop, mobile (Android), and native
- `dioxus-cli`, `wasm-bindgen-cli`, and `tailwindcss`
- All native dependencies for Linux (GTK/WebView, X11, Wayland, Vulkan) and macOS
- Crane-based Nix builds for web and desktop packages
- CI checks for clippy, formatting, and tests
