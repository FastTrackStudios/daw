# architect dev recipes
#
# Run from the repo root inside the Dioxus dev shell:
#   nix develop -c just <recipe>

# Default: type-check the whole workspace + UI crates.
default: check

# Type-check workspace (server, db, proto) + the UI crates.
check:
    cargo check --workspace
    cd crates/example-ui && cargo check
    cd apps/web && cargo check --target wasm32-unknown-unknown
    cd apps/desktop && cargo check

# Build + run the migration binary.
migrate:
    cargo run -p example-app-db -- up

# Run the axum + vox server. Migrations auto-apply on boot.
server:
    cargo run -p example-app-server

# `dx serve` the web app — connects to the server on 4040 by default.
web:
    cd apps/web && dx serve --web --addr 0.0.0.0 --port 8765

# `dx serve` the desktop app.
desktop:
    cd apps/desktop && dx serve --desktop

# Format all Rust files in the workspace + UI crates.
fmt:
    cargo fmt --all
    cd crates/example-ui && cargo fmt
    cd apps/web && cargo fmt
    cd apps/desktop && cargo fmt
