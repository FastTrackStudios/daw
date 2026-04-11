# Task workspace recipes
# Run commands: just <recipe-name>

# Default: serve the desktop app with hot-reload
default: dx

# ── Desktop App ──────────────────────────────────────────────────────────

# Serve the desktop app with hot-reload (requires TASK_VAULT env var)
dx *args:
    cd apps/desktop && dx serve {{args}}

# Serve the web app with hot-reload
web *args:
    cd apps/web && dx serve {{args}}

# Build the desktop app for release
dx-build:
    cd apps/desktop && dx build --release --platform desktop

# Build the web app for release
web-build:
    cd apps/web && dx build --release --platform web

# ── CLI ──────────────────────────────────────────────────────────────────

# Run the task CLI
task *args:
    cargo run -p task-cli -- {{args}}

# ── Build & Test ─────────────────────────────────────────────────────────

# Check all crates compile
check:
    cargo check --workspace

# Build all crates
build:
    cargo build --workspace

# Run tests
test:
    cargo test --workspace

# ── Aliases ──────────────────────────────────────────────────────────────

alias c := check
alias b := build
alias t := test
