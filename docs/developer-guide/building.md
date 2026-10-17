# Building

```bash
./setup     # once per machine
./build     # the four artefacts
./gecko     # open it
```

and, to know whether it works:

```bash
./check.sh
```

That is the whole thing: format, lint, every test suite, the page in a real
browser. It bootstraps what it can — `npm install`, the two `wasm32` targets
— so a fresh clone needs Rust, Node and the Emscripten SDK, and `./setup`
fetches the last of those.

**CI runs this same script.** There is one description of what passing means,
and what passes locally passes there.

And, to know how much of it the suites touch:

```bash
./coverage
```

Two numbers — the kernel and the page — with a floor under each. It is not
part of `./check.sh`: instrumenting everything costs minutes that a check run
before every commit should not, and a floor is a condition of **delivery**.
CI runs it on `main` and on tags, one half per job. See
[Testing](testing.md#coverage-in-two-numbers).

## What depends on what

The order in `build` and in `check.sh` is not arbitrary:

```
build-wasm.sh      the kernel, by wasm-bindgen   ->  front/src/pkg
build-lua.sh       the interpreter, by emcc      ->  front/src/lua
    ↓
npm run build      the page embeds both          ->  front/dist
    ↓
cargo build        the binary embeds the page    ->  target/release/gecko
```

Three couplings are worth knowing about:

**The page depends on both wasm modules.** `front/src/pkg` and
`front/src/lua` are build artefacts and Vite will not regenerate them. Change
the Rust and `tsc` fails with `Cannot find module './pkg/gecko'`, or — worse —
the tests run against a stale kernel and report a format version that no
longer exists.

**The binary embeds the page.** `front/dist` has to exist before
`cargo build -p gecko-cli`, or the binary serves an explanatory message
instead of a page. `crates/gecko-cli/build.rs` creates the directory so the
crate still *compiles* without it — making a Rust binary fail to compile
because a TypeScript bundle is missing would be the wrong dependency — and
declares `rerun-if-changed` on it, so rebuilding the front end re-embeds it
rather than leaving a stale page inside.

**Playwright serves the built bundle.** It reuses a server already on 5173 or
4173 rather than starting its own, so a `vite preview` left running from an
earlier look serves a *stale* bundle to the whole suite. The symptom is a
test passing that should not. Stop the leftover before running the suite.

## Two wasm modules, and why

mlua supports WebAssembly through `wasm32-unknown-emscripten` only — its own
README says so — and the kernel is built by wasm-bindgen, which targets
`wasm32-unknown-unknown`. An object built by one cannot be linked into the
other, so Lua rides **beside** the kernel rather than inside it, with
JavaScript between them answering four primitives.

That boundary is `gecko_lua::Kernel`: `apply`, `query`, `read`, `write`.
Everything above it — the whole API a script sees — is written once, in
Rust, and both hosts share it. A test runs the same script through both and
checks the bytes agree.

The alternative was migrating the kernel to emscripten too, which would have
meant hand-writing the JS glue wasm-bindgen generates and putting everything
already shipping on a Tier 2 target. The risk sits in a crate of a hundred
lines instead.

## The development loop

```bash
./dev       # the page from Vite with hot reload, and a file server behind it
./view      # the page alone, shorter loop, no files
```

A TypeScript edit reloads the page in place. The one thing it costs is the
session: the kernel is in the tab, so a reload starts a new one. That is the
bargain of putting the kernel where the user is, and `gecko.script()` is what
makes it bearable — save the session, reload, run the script.

`./dev` starts `gecko` behind Vite and forwards `/file` to it, so a script
can still read and write real files while the page is being edited.

### Fixed ports, and the leftovers that hold them

Both ports are fixed: 5173 so a bookmark keeps working, and the file server's
so Vite can be told in advance where to forward. A process left from an
earlier run is therefore in the way, and it fails quietly — Vite exits on
"port already in use", but it was started in the background, so the script
sails on and the readiness check is answered by the leftover.

Hence `scripts/ports.sh`: both scripts look before they start, and name the
process holding the port and the command to stop it. The check runs *before*
the wasm build, so the news arrives in a second rather than a minute.

## Building pieces on their own

```bash
cargo test --workspace     # kernel, session, and the Lua API
./build-wasm.sh            # the kernel for the page
./build-lua.sh             # the interpreter for the page
cd front && npm run dev    # the page, against whatever was last built
```

## wasm-pack, not cargo and wasm-bindgen by hand

`build-wasm.sh` goes through wasm-pack because it reads the `wasm-bindgen`
version out of `Cargo.lock` and installs the matching CLI. Driving the two
tools separately means keeping those versions in step by hand, and they drift
silently: the generated glue simply stops matching the module. wasm-pack also
runs `wasm-opt`, which is worth about a quarter of the binary.

`build-lua.sh` does the emscripten half: it sources `~/emsdk/emsdk_env.sh`,
passes `--js-library` for the four primitives, and asks for an ES module that
grows its heap.

## Dependencies that are deliberately absent

The browser is opened with the platform's own command (`open`, `xdg-open`,
`start`) rather than the `webbrowser` crate, which pulls in 39 others — `url`,
`idna`, the whole `icu` family — to run one command. Measured, not guessed.

Python is gone from the application, but `./docs.sh` still uses it: MkDocs is
a Python tool and there is no Rust equivalent worth the switch. It is a
documentation toolchain, not a dependency of anything that ships.

## Always `--release`

A debug build of the kernel is **eight to fifteen times slower**, and it is
not a constant you can ignore: a combinatorial map finds a vertex by taking
the smallest dart of its orbit, so everything that reads the model walks
darts, and nothing in that is optimised away at `opt-level=0`.

Measured on a fifty-block model: a refresh — one view buffer and one stats —
takes about 30 ms built with `--release` and about 350 ms without.

It matters for `gecko run`, where the kernel is in the binary. For `gecko`
serving the page it does not: the work happens in the tab, in wasm built by
`build-wasm.sh`, which optimises either way.
