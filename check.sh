#!/usr/bin/env bash
# Everything, in the order that fails fastest — and in the order the
# artefacts require: the page embeds the kernel and the interpreter, so
# both are built before the bundle is, and the bundle is baked into the
# binary, so it is built before `cargo test` reaches `gecko-cli`.
#
# CI runs this very script, so what passes here is what passes there — with
# one exception that bit once: the site is built by a job of its own, which
# also checks the **formatting of the documentation**. That check is below,
# when `uv` is here to run it; building the site is not, because it wants
# mkdocs and a full `cargo doc`. Run `./docs.sh` when a link or the
# navigation changes.
set -euo pipefail
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"

# --- bootstrap, so a fresh clone needs rust, node and emsdk and no more ---
[ -d node_modules ] || npm install --no-audit --no-fund
[ -d front/node_modules ] || (cd front && npm install --no-audit --no-fund)
rustup target list --installed | grep -qx wasm32-unknown-unknown \
  || rustup target add wasm32-unknown-unknown
command -v emcc >/dev/null || [ -f "$HOME/emsdk/emsdk_env.sh" ] || {
  echo "the Emscripten SDK is required for the Lua module: ./setup" >&2
  exit 1
}

echo "── the kernel, for the page ──────────────────────────────────"
./build-wasm.sh

echo "── the interpreter, for the page ─────────────────────────────"
./build-lua.sh

echo "── the page itself ───────────────────────────────────────────"
(cd front && npm run build)

echo "── format and lint ───────────────────────────────────────────"
# The documentation's own formatting, which the site's job checks: a table
# left ragged there fails a run that everything else has passed, and it
# fails it after a quarter of an hour of browser tests.
if command -v uv >/dev/null; then
  [ -x .venv/bin/mdformat ] || {
    uv venv --quiet
    uv pip install --quiet -r requirements-docs.txt
  }
  ./.venv/bin/mdformat --check docs README.md
else
  echo "!! uv is missing, so the documentation's formatting is NOT checked here" >&2
fi
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings

echo "── kernel, session, and the Lua API (Rust) ───────────────────"
cargo test --workspace

echo "── decode and scene, headless (Vitest + NullEngine) ──────────"
(cd front && npm run test:unit)

echo "── the whole page in a real browser (Playwright) ─────────────"
(cd front && npx playwright test)
