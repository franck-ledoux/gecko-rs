# Testing

Four tiers. Each answers a question the one below it cannot, and nothing is
tested twice.

| Tier                     | Tool                      | What it covers                                         | Time    |
| ------------------------ | ------------------------- | ------------------------------------------------------ | ------- |
| Kernel and session       | `cargo test`              | the map, naming, commands, the buffer layout           | ~0.05 s |
| Decode, scene, transport | Vitest + **`NullEngine`** | decoding, scene building, picking, the camera          | ~0.3 s  |
| Both modes               | Playwright + SwiftShader  | the app, in a browser, with a real mouse               | ~7 s    |
| The façades agree        | pytest                    | the Python API, and its agreement with the static mode | ~0.05 s |

## `NullEngine` is the one that earns its keep

Babylon will build a complete scene graph with no rendering context at all, in
Node. Decoding, mesh construction, **ray-cast picking** and camera framing are
all testable there in milliseconds — including turning a screen coordinate back
into a node's name, which sounds like it needs a GPU and does not.

That leaves Playwright to cover only what genuinely needs a browser: real
pointer events, the layout, the two transports end to end.

## The test that justifies the architecture

`front/tests/unit/kernel.test.ts` drives a sequence through the wasm façade and
records what came out — the commands, the stats, a digest of the buffer.
`python/test_modes.py` replays that same sequence through PyO3 and checks the
bytes agree.

It can only pass while both façades are pure transports onto
`gecko_cmd::Session`. The moment either grows logic of its own, this is what
notices. It skips, loudly, if the Vitest suite has not run first.

## Browser tests

SwiftShader rather than whatever GPU is present: slower, and identical on a
laptop and on a runner with no GPU, which matters more. The `webServer` serves
the **built** bundle, so the thing under test is the thing that ships.

The station spec starts its own Python process — the point of that mode is that
Python is the server — and talks to it over stdin. That is what lets a test say:
drag a node in a browser, then ask Python where it thinks that node is.

## Two things the tests had to be taught

**The browser and the kernel agree to the precision the wire carries, not to the
last bit.** The view is f32 by design and the kernel is the authority in f64, so
a round trip through the buffer is lossy on purpose. A test demanding exact
equality is asserting something the format does not promise.

**Screen coordinates come off the camera's transform matrix, which the *next*
render updates.** Reading them before a reframe has been drawn aims the mouse
with a stale matrix — which is right often enough to look like it works, and
then fails one run in three. `__gecko.settled()` waits for it.

## The test surface

`window.__gecko` exists so end-to-end tests can assert on **state** rather than
pixels: what the kernel holds, which nodes are on screen, where one of them is.
It is deliberately not the application's API — nothing in `main.ts` uses it.

There are no screenshot comparisons yet. When there are, they will be for the
one thing state cannot express, and they will come after a test that proves the
renderer is reproducible — a baseline image over a renderer that is not is a
rebaselining chore, not a test.

## Running one tier

```bash
cargo test --workspace --exclude gecko-py
cd front && npm run test:unit
cd front && npx playwright test
uv run python -m pytest python -q
```

The last two need artefacts the earlier ones produce; see
[Building](building.md).
