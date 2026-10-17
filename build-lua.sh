#!/usr/bin/env bash
# The Lua interpreter the page runs, built for emscripten.
#
# It rides **beside** the kernel rather than inside it: mlua builds for
# `wasm32-unknown-emscripten` and the kernel is built by wasm-bindgen for
# `wasm32-unknown-unknown`, and the two cannot be linked into one module.
# So there are two modules and JavaScript between them.
set -euo pipefail
cd "$(dirname "$0")"

# Already on the PATH — a shell that sourced it, or CI — or in the usual
# place. Both, because `./setup` puts it in the usual place and the runner
# puts it somewhere else entirely.
if ! command -v emcc >/dev/null; then
  [ -f "$HOME/emsdk/emsdk_env.sh" ] || {
    echo "the Emscripten SDK is required to build the Lua module:" >&2
    echo "  ./setup   (or see https://emscripten.org)" >&2
    exit 1
  }
  # shellcheck disable=SC1091
  source "$HOME/emsdk/emsdk_env.sh" >/dev/null 2>&1
fi

rustup target list --installed | grep -qx wasm32-unknown-emscripten \
  || rustup target add wasm32-unknown-emscripten

here="$PWD"
out="$here/front/src/lua"
mkdir -p "$out"

# `--js-library` answers the four imports; the rest is the shape of the
# module Vite will load: an ES module, for the web, that grows its heap.
RUSTFLAGS="\
-Clink-arg=--js-library=$here/crates/gecko-lua-wasm/host.js \
-Clink-arg=-sEXPORTED_FUNCTIONS=_lua_run,_malloc,_free,_main \
-Clink-arg=-sEXPORTED_RUNTIME_METHODS=UTF8ToString,stringToUTF8,lengthBytesUTF8,ccall \
-Clink-arg=-sMODULARIZE=1 \
-Clink-arg=-sEXPORT_ES6=1 \
-Clink-arg=-sENVIRONMENT=web \
-Clink-arg=-sALLOW_MEMORY_GROWTH=1 \
-Clink-arg=-sINVOKE_RUN=0 \
-Clink-arg=-sEXPORT_NAME=createLua" \
  cargo build -p gecko-lua-wasm --target wasm32-unknown-emscripten --release

# Both files keep the names emscripten gave them: the glue looks its own
# wasm up by name, relative to itself, and renaming one means patching the
# other.
from="$here/target/wasm32-unknown-emscripten/release"
cp "$from/gecko-lua-wasm.js" "$out/"
cp "$from/gecko_lua_wasm.wasm" "$out/"
ls -lh "$out/gecko_lua_wasm.wasm" | awk '{print "front/src/lua/gecko_lua_wasm.wasm: " $5}'
