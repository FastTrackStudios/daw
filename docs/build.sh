#!/usr/bin/env bash
# Build the keyflow docsite for production.
#
# Two stages, both required: `kf docs` renders every ```kf``` fenced block in
# docs/content into inline SVG under docs/.rendered (a generated mirror tree),
# then stock dodeca (`ddc`) builds that into docs/output. dodeca is never
# pointed at the raw authored content — only at the rendered tree.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "==> rendering kf blocks (docs/content → docs/.rendered)"
cargo run -q -p keyflow-cli -- docs --input docs/content --output docs/.rendered

echo "==> dodeca build"
ddc build

echo "==> done → docs/output"
