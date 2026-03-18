# FastTrackStudio DAW — developer task runner
# https://github.com/casey/just
#
# Usage:
#   just                     — list available recipes
#   just build               — build the workspace
#   just test                — run unit tests
#   just setup-rigs          — install REAPER rig wrappers, icons, desktop entries
#   just integration-test    — run REAPER integration tests (spawns REAPER headless)
#   just ci                  — full CI suite (unit + integration)

default:
    @just --list

# Build the entire workspace
build:
    cargo build --workspace

# Run unit tests
test:
    cargo test --workspace

# Install REAPER instance rigs (wrapper scripts, icons, .desktop entries)
setup-rigs *ARGS:
    cargo xtask setup-rigs {{ARGS}}

# Run REAPER integration tests (spawns REAPER headless via fts-test)
integration-test *ARGS:
    cargo xtask reaper-test {{ARGS}}

# Run a single named integration test
# Usage: just integration-test-filter my_test_name
integration-test-filter FILTER:
    cargo xtask reaper-test {{FILTER}}

# Full CI suite: unit tests + REAPER integration tests
ci:
    @echo "=== unit tests ==="
    cargo test --workspace
    @echo ""
    @echo "=== integration tests ==="
    cargo xtask reaper-test
    @echo ""
    @echo "=== all tests passed ==="

# Check that everything compiles (fast, no codegen)
check:
    cargo check --workspace
