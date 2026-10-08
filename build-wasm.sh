#!/usr/bin/env bash
# Build the browser façade.
#
# wasm-pack rather than cargo + wasm-bindgen by hand: it reads the wasm-bindgen
# version out of Cargo.lock and uses the matching CLI. Driving the two tools
# separately means keeping those versions in step by hand, and they drift
# silently — the generated glue simply stops matching the module.
#
# It also runs wasm-opt over the result, which the hand-rolled path did not.
set -euo pipefail
cd "$(dirname "$0")"

# Self-sufficient, so this is one command on a fresh clone rather than one
# command plus a prerequisite nobody reads.
rustup target list --installed | grep -qx wasm32-unknown-unknown \
  || rustup target add wasm32-unknown-unknown
# Without this, npx refetches wasm-pack from the registry on every run.
[ -d node_modules ] || npm install --no-audit --no-fund

npx wasm-pack build crates/gecko-wasm \
  --target web --out-dir ../../front/src/pkg --out-name gecko
echo "front/src/pkg/gecko_bg.wasm: $(du -h front/src/pkg/gecko_bg.wasm | cut -f1)"
