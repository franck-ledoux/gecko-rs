#!/usr/bin/env bash
# Everything, in the order that fails fastest — and in the order the artefacts
# require: the front bundle is embedded in the Python wheel, so it has to be
# built before maturin runs, and the Vitest suite writes the file pytest
# replays.
#
# CI runs this very script, so what passes here is what passes there.
set -euo pipefail
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"

# --- bootstrap, so a fresh clone needs nothing but rust, node and uv ---
command -v uv >/dev/null || {
  echo "uv is required: https://docs.astral.sh/uv/getting-started/installation/" >&2
  exit 1
}
[ -d node_modules ] || npm install --no-audit --no-fund
[ -d front/node_modules ] || (cd front && npm install --no-audit --no-fund)
[ -x .venv/bin/python ] || uv venv --quiet
uv pip install --quiet maturin pytest
rustup target list --installed | grep -qx wasm32-unknown-unknown \
  || rustup target add wasm32-unknown-unknown

echo "── format and lint ───────────────────────────────────────────"
cargo fmt --all --check
cargo clippy --workspace --exclude gecko-py --all-targets -- -D warnings

echo "── kernel and session (Rust) ─────────────────────────────────"
cargo test --workspace --exclude gecko-py

echo "── browser façade (wasm) ─────────────────────────────────────"
./build-wasm.sh

echo "── decode, scene and transport, headless (Vitest + NullEngine) "
(cd front && npm run test:unit)

echo "── front bundle, then the wheel that embeds it ───────────────"
(cd front && npm run build)
(cd crates/gecko-py && VIRTUAL_ENV="$PWD/../../.venv" ../../.venv/bin/maturin develop --release -q)

echo "── both modes in a real browser (Playwright) ─────────────────"
(cd front && npx playwright test)

echo "── Python façade, and its agreement with the static mode ─────"
./.venv/bin/python -m pytest python -q
