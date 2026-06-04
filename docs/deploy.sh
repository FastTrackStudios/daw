#!/usr/bin/env bash
# Build and deploy the docsite to fly.io (app: keyflow-docs).
#
# One-time setup:
#   fly apps create keyflow-docs
#   fly certs add keyflow.fasttrackstudio.app -a keyflow-docs
#   → then point Cloudflare DNS at keyflow-docs.fly.dev (see fly certs show)
set -euo pipefail
cd "$(dirname "$0")"

./build.sh
fly deploy
