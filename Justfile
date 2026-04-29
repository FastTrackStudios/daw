# Task workspace recipes
# Run commands: just <recipe-name>

# Default: serve the desktop app with hot-reload
default: dx

# ── Desktop App ──────────────────────────────────────────────────────────

# Serve the desktop app with hot-reload (requires TASK_VAULT env var)
dx *args:
    cd apps/desktop && dx serve {{args}}

# Build the desktop app for release
dx-build:
    cd apps/desktop && dx build --release --platform desktop

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

# ── Deploy ───────────────────────────────────────────────────────────────

# Build task-cli (release) and ship it to starcommand for the
# task-email-watcher systemd service. The binary is placed at
# /var/lib/task-watcher/bin/task and the watcher is restarted.
#
# Called automatically from ~/.starcommand/justfile `deploy`, so
# `just deploy` in starcommand does the whole pipeline.
deploy-task-watcher host="root@192.168.0.106" remote="/var/lib/task-watcher/bin/task":
    cargo build --release -p task-cli
    scp target/release/task {{host}}:{{remote}}.new
    ssh {{host}} 'install -o task-watcher -g task-watcher -m 0755 {{remote}}.new {{remote}} && rm -f {{remote}}.new && systemctl restart task-email-watcher.service && sleep 2 && systemctl status task-email-watcher.service --no-pager | head -8'
