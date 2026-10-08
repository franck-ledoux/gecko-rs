# Building

```bash
./check.sh
```

That is the whole thing: format, lint, every test suite in every language, both
modes in a real browser. It bootstraps itself — virtual environment, `npm install`, the `wasm32` target — so a fresh clone needs nothing but Rust, Node
and Python.

**CI runs this same script.** There is one description of what passing means,
and what passes locally passes there.

## What depends on what

The order in `check.sh` is not arbitrary:

```
cargo test            the kernel and the session
    ↓
build-wasm.sh         the wasm façade  ->  front/src/pkg
    ↓
npm run test:unit     needs the wasm; writes the file pytest replays
    ↓
npm run build         the viewer bundle  ->  front/dist
    ↓
maturin develop       the wheel embeds that bundle
    ↓
playwright test       the station test needs the wheel
    ↓
pytest                replays what Vitest recorded
```

Two couplings are worth knowing about, because they surprise people:

**The front end depends on the wasm façade.** `front/src/pkg` is a build
artefact and Vite will not regenerate it. Change the Rust and `tsc` will fail
with `Cannot find module './pkg/gecko'`, or — worse — the tests will run against
a stale kernel and report a format version that no longer exists. Run
`./build-wasm.sh`.

**The wheel embeds the viewer bundle.** `front/dist` has to exist before
maturin runs, or the installed package serves an explanatory message instead of
a page. `crates/gecko-py/build.rs` creates the directory so the crate still
*compiles* without it — making a Rust extension fail to compile because a
TypeScript bundle is missing would be the wrong dependency — and declares
`rerun-if-changed` on it, so rebuilding the front end re-embeds it rather than
leaving a stale viewer inside the wheel.

## The development loop

```bash
./dev           # a Python console, a browser watching it, hot reload
./dev notebook  # the same, with JupyterLab instead of a console
./view          # the viewer alone, static mode, no Python
```

`./dev` rebuilds what changed, starts a station, opens the view and leaves you
at a Python prompt with that session as `s`.

The part worth understanding is **why a TypeScript edit does not cost you the
session**. In station mode the page normally comes from the wheel's embedded
bundle, so changing the viewer means rebuilding, reinstalling and restarting —
and the kernel's state goes with it.

Under `./dev` the page comes from the dev server instead, and Vite forwards the
two things the page asks of its origin — the mode probe and the socket — to the
station behind it. The kernel is on the far side of that proxy and never hears
about the reload.

```
browser ──▶ Vite :5173 ──── /mode, /ws ───▶ station :8731 ──▶ kernel
             (hot reload)                    (Python, keeps its state)
```

It is a configuration-only arrangement: `front/vite.config.ts` turns the proxy
on when `GECKO_PORT` is set, and the station is an ordinary station. The
dev-loop spec starts both and checks a page served by Vite really is in station
mode and really drives the Python session.

`./dev` builds the extension in debug rather than release — this is the
edit-and-try loop, and the difference is twenty seconds a go.

## Building pieces on their own

```bash
cargo test --workspace --exclude gecko-py   # the kernel
./build-wasm.sh                              # the wasm façade
cd front && npm run dev                      # the viewer, static mode
uv pip install ./crates/gecko-py             # the Python package
```

`gecko-py` is excluded from plain `cargo` commands on purpose: linking an
extension module needs a Python interpreter's symbols, which only maturin
arranges. `cargo build -p gecko-py` fails at the link step with a wall of
undefined `Py_*` symbols, and that is expected.

## wasm-pack, not cargo and wasm-bindgen by hand

`build-wasm.sh` goes through wasm-pack because it reads the `wasm-bindgen`
version out of `Cargo.lock` and installs the matching CLI. Driving the two
tools separately means keeping those versions in step by hand, and they drift
silently: the generated glue simply stops matching the module. wasm-pack also
runs `wasm-opt`, which is worth about a quarter of the binary.

## Dependencies that are deliberately absent

The browser is opened with the platform's own command (`open`, `xdg-open`,
`start`) rather than the `webbrowser` crate, which pulls in 39 others — `url`,
`idna`, the whole `icu` family — to run one command. Measured, not guessed.
