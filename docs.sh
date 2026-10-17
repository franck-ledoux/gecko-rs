#!/usr/bin/env bash
# Build the documentation site: the Markdown pages, and the rustdoc reference
# they link to, into site/.
#
# --strict so a warning fails: in practice that means a dead link or a page
# missing from the navigation, and a dead link in documentation costs the reader
# more than no link would.
set -euo pipefail
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"

command -v uv >/dev/null || {
  echo "uv is required: https://docs.astral.sh/uv/getting-started/installation/" >&2
  exit 1
}
[ -x .venv/bin/python ] || uv venv --quiet
uv pip install --quiet -r requirements-docs.txt

echo "── markdown formatting ───────────────────────────────────────"
./.venv/bin/mdformat --check docs README.md

echo "── site ──────────────────────────────────────────────────────"
./.venv/bin/mkdocs build --strict

echo "── rust reference ────────────────────────────────────────────"
cargo doc --workspace --no-deps
rm -rf site/api
cp -r target/doc site/api

echo "site/ is ready — open site/index.html"
