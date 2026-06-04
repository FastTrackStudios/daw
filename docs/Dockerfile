# Serves the prebuilt docsite (docs/output) — run docs/build.sh first.
# static-web-server: ~2MB scratch image, brotli/gzip, sane cache headers.
FROM ghcr.io/static-web-server/static-web-server:2

ENV SERVER_PORT=8080 \
    SERVER_ROOT=/public \
    SERVER_COMPRESSION=true \
    SERVER_CACHE_CONTROL_HEADERS=true

COPY output /public
