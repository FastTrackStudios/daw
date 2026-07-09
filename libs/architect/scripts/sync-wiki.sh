#!/usr/bin/env bash
# Sync docs/content/ → the Forgejo wiki repo for this project.
#
# - Strips the TOML frontmatter (`+++ ... +++` block) so the wiki shows
#   clean prose, while leaving the source files untouched for dodeca.
# - Mirrors the directory layout into the wiki (Forgejo wiki supports
#   subdirectories, so `architecture/pattern.md` → `Architecture/Pattern.md`).
# - `_index.md` becomes the section's landing page: top-level becomes
#   `Home.md`, nested becomes `<Section>/Home.md`.
#
# Auth: pass a Forgejo token via FORGEJO_TOKEN env var, OR rely on
# system git credentials. The CI workflow injects the token.
#
# Usage:
#   FORGEJO_TOKEN=... ./scripts/sync-wiki.sh
#   FORGEJO_TOKEN=... ./scripts/sync-wiki.sh --dry-run
#
# Configure WIKI_URL to point at a different wiki repo if needed.

set -euo pipefail

WIKI_URL="${WIKI_URL:-https://codeberg.org/FastTrackStudios/architect.wiki.git}"
WIKI_USER="${WIKI_USER:-architect}"
SRC_DIR="${SRC_DIR:-docs/content}"
DRY_RUN=0
WORK_DIR="$(mktemp -d)"

trap 'rm -rf "$WORK_DIR"' EXIT

for arg in "$@"; do
    case "$arg" in
        --dry-run) DRY_RUN=1 ;;
        *) echo "unknown arg: $arg" >&2; exit 1 ;;
    esac
done

if [[ ! -d "$SRC_DIR" ]]; then
    echo "no $SRC_DIR — run from repo root" >&2
    exit 1
fi

# Clone the wiki. Use askpass when a token is provided.
if [[ -n "${FORGEJO_TOKEN:-}" ]]; then
    askpass="$WORK_DIR/askpass.sh"
    cat > "$askpass" <<EOF
#!/bin/sh
case "\$1" in
  Username*) echo "$WIKI_USER" ;;
  Password*) echo "\$ARCHITECT_WIKI_TOKEN_INNER" ;;
esac
EOF
    chmod 700 "$askpass"
    ARCHITECT_WIKI_TOKEN_INNER="$FORGEJO_TOKEN" GIT_ASKPASS="$askpass" \
        git clone "$WIKI_URL" "$WORK_DIR/wiki"
else
    git clone "$WIKI_URL" "$WORK_DIR/wiki"
fi

cd "$WORK_DIR/wiki"

# Wipe stale pages — anything not regenerated below is removed.
# Keep `.git` and Forgejo's special sidebar/footer files if they exist.
find . -mindepth 1 -maxdepth 1 \
    -name '.git' -prune -o \
    -name '_Sidebar.md' -prune -o \
    -name '_Footer.md' -prune -o \
    -exec rm -rf {} +

# Title-case a kebab-to-Title path segment ("getting-started" → "Getting-Started").
title_case() {
    echo "$1" | sed -E 's/(^|-)([a-z])/\1\u\2/g'
}

# Render one markdown file at $1 (absolute path) under $2 (wiki-relative path).
render_md() {
    local src_file="$1"
    local out_path="$2"
    mkdir -p "$(dirname "$out_path")"
    awk '
        BEGIN { in_fm = 0; done = 0 }
        !done && NR == 1 && /^\+\+\+[[:space:]]*$/ { in_fm = 1; next }
        in_fm && /^\+\+\+[[:space:]]*$/ { in_fm = 0; done = 1; next }
        in_fm { next }
        # Rewrite dodeca @/section/page.md links to wiki-friendly paths.
        { gsub(/\(@\/([^)]+)\.md\)/, "(\\1)"); print }
    ' "$src_file" > "$out_path"
}

repo_root="$OLDPWD"

# Render: for each markdown file under docs/content/, strip frontmatter
# and place at the wiki's mirrored path.
src="$repo_root/$SRC_DIR"
while IFS= read -r -d '' file; do
    rel="${file#$src/}"
    # docs/content/_index.md                  → Home.md
    # docs/content/getting-started/_index.md  → Getting-Started/Home.md
    # docs/content/architecture/pattern.md    → Architecture/Pattern.md
    out=""
    IFS='/' read -ra segs <<< "$rel"
    n=${#segs[@]}
    for (( i=0; i<n; i++ )); do
        seg="${segs[i]}"
        if (( i == n - 1 )); then
            case "$seg" in
                _index.md) seg="Home.md" ;;
                *) seg="$(title_case "${seg%.md}").md" ;;
            esac
        else
            seg="$(title_case "$seg")"
        fi
        out+="${seg}"
        (( i < n - 1 )) && out+="/"
    done
    render_md "$file" "$out"
done < <(find "$src" -type f -name '*.md' -print0)

# Feature specs live alongside the feature at features/<feature>/spec/*.md.
# Mirror them into Specs/<Feature>/<Page>.md so they're browsable from the
# wiki without leaving the feature crate as the source of truth.
if [[ -d "$repo_root/features" ]]; then
    while IFS= read -r -d '' file; do
        # file = $repo_root/features/<feature>/spec/<page>.md
        rel="${file#$repo_root/features/}"      # <feature>/spec/<page>.md
        feature="${rel%%/*}"                      # <feature>
        rest="${rel#*/spec/}"                     # <page>.md (possibly with subdir)
        feature_title="$(title_case "$feature")"
        # Title-case the leaf (e.g. repo.md → Repo.md).
        leaf="$(basename "$rest")"
        case "$leaf" in
            _index.md) leaf="Home.md" ;;
            *) leaf="$(title_case "${leaf%.md}").md" ;;
        esac
        sub="$(dirname "$rest")"
        if [[ "$sub" == "." ]]; then
            out="Specs/${feature_title}/${leaf}"
        else
            out="Specs/${feature_title}/${sub}/${leaf}"
        fi
        render_md "$file" "$out"
    done < <(find "$repo_root/features" -path '*/spec/*' -type f -name '*.md' -print0)
fi

# Show what changed.
git add -A
if git diff --cached --quiet; then
    echo "wiki already in sync"
    exit 0
fi

if (( DRY_RUN )); then
    echo "--- dry run, would commit: ---"
    git diff --cached --stat
    exit 0
fi

git -c user.name="$WIKI_USER" -c user.email="$WIKI_USER@architect" \
    commit -m "sync: regenerate wiki from docs/content/"

if [[ -n "${FORGEJO_TOKEN:-}" ]]; then
    ARCHITECT_WIKI_TOKEN_INNER="$FORGEJO_TOKEN" GIT_ASKPASS="$WORK_DIR/askpass.sh" \
        git push origin HEAD
else
    git push origin HEAD
fi

echo "wiki updated"
