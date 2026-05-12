# architect dev recipes
#
# Run from the repo root inside the Dioxus dev shell:
#   nix develop -c just <recipe>

# Default: type-check the whole workspace + UI crates.
default: check

# Type-check workspace (server, db, proto) + the UI crates.
check:
    cargo check --workspace
    cd apps/app/ui && cargo check
    cd apps/app/web && cargo check --target wasm32-unknown-unknown
    cd apps/app/desktop && cargo check

# Build + run the migration binary.
migrate:
    cargo run -p app-db -- up

# Run the axum + vox server. Migrations auto-apply on boot.
server:
    cargo run -p app-server

# Run the wasm browser integration tests against a server.
# Start `just server` in another terminal first.
test-wasm:
    cd features/example/tests/web && cargo test --target wasm32-unknown-unknown --release

# Run server + wasm tests together with the default (db) backend.
test-e2e: (_e2e "")

# Same e2e against the in-memory backend. Proves the contract is
# truly backend-agnostic — the wasm tests don't change.
test-e2e-memory: (_e2e "--no-default-features --features backend-memory")

# Internal: build + run server with given cargo features, drive wasm tests.
_e2e features:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo build -p app-server {{features}}
    rm -f example.db
    ./target/debug/app-server &
    server_pid=$!
    trap "kill $server_pid 2>/dev/null || true; rm -f example.db" EXIT
    for i in {1..30}; do
        if curl -fsS http://127.0.0.1:4040/api/health >/dev/null 2>&1; then break; fi
        sleep 0.2
    done
    cd features/example/tests/web && cargo test --target wasm32-unknown-unknown --release

# `dx serve` the web app — connects to the server on 4040 by default.
web:
    cd apps/app/web && dx serve --web --addr 0.0.0.0 --port 8765

# `dx serve` the desktop app.
desktop:
    cd apps/app/desktop && dx serve --desktop

# ── Docs ──────────────────────────────────────────────────────────────

# Serve the dodeca docs site locally with live reload.
# Requires `ddc` on PATH — install with:
#   curl --proto '=https' --tlsv1.2 -LsSf \
#     https://github.com/bearcove/dodeca/releases/latest/download/dodeca-installer.sh | sh
docs:
    cd docs && ddc serve

# Build the dodeca docs site for production.
docs-build:
    cd docs && ddc build

# Sync docs/content/ → the Forgejo wiki repo. Dry-run first to preview.
sync-wiki:
    ./scripts/sync-wiki.sh

sync-wiki-dry-run:
    ./scripts/sync-wiki.sh --dry-run

# Format all Rust files in the workspace + UI crates.
fmt:
    cargo fmt --all
    cd apps/app/ui && cargo fmt
    cd apps/app/web && cargo fmt
    cd apps/app/desktop && cargo fmt
