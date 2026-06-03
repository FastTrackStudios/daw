#!/usr/bin/env bash
# Dev server for the keyflow docsite, with live reload.
#
# Runs two watchers: `kf docs --watch` regenerates docs/.rendered whenever an
# authored page under docs/content changes, and `ddc serve` watches
# docs/.rendered and live-reloads the browser. Edit docs/content; the rendered
# charts and the page refresh on save.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "==> initial render (docs/content → docs/.rendered)"
cargo run -q -p keyflow-cli -- docs --input docs/content --output docs/.rendered

echo "==> watching docs/content for kf changes"
cargo run -q -p keyflow-cli -- docs --input docs/content --output docs/.rendered --watch &
WATCH_PID=$!
trap 'kill "$WATCH_PID" 2>/dev/null || true' EXIT

echo "==> ddc serve (http://localhost:8080)"
ddc serve
