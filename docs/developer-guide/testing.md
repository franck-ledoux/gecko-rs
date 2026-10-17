# Testing

Four tiers. Each answers a question the one below it cannot, and nothing is
tested twice.

| Tier               | Tool                      | What it covers                                      | Time   |
| ------------------ | ------------------------- | --------------------------------------------------- | ------ |
| Kernel and session | `cargo test`              | the map, naming, commands, the buffer layout        | ~0.3 s |
| The scripting API  | Lua scripts, run by Rust  | every call a script can make, as a reader writes it | ~0.2 s |
| Decode and scene   | Vitest + **`NullEngine`** | decoding, scene building, picking, the camera       | ~0.6 s |
| The whole page     | Playwright + SwiftShader  | the app, in a browser, with a real mouse            | ~35 s  |

## `NullEngine` is the one that earns its keep

Babylon will build a complete scene graph with no rendering context at all, in
Node. Decoding, mesh construction, **ray-cast picking** and camera framing are
all testable there in milliseconds — including turning a screen coordinate back
into a node's name, which sounds like it needs a GPU and does not.

That leaves Playwright to cover only what genuinely needs a browser: real
pointer events, the layout, and the interpreter in the page.

## The suite written in Lua

`crates/gecko-lua/tests/scripts/` is one file per subject, each run as a test
by a harness that reports the Lua line a failure came from. What is being
tested is the API a reader will write against, so the test *is* an example of
writing against it — and the examples cannot go stale, because they run.

## The test that justifies the architecture

Lua runs in two hosts: one holds the session, the other reaches it through
JavaScript. `agree.lua` builds a model, meshes it, and digests the VTK it
produces; the Rust suite pins that number by running the file, and a
Playwright test feeds **the same file** to the page's console.

It can only pass while everything above the four primitives is one piece of
code. The moment either host grows a habit of its own, this is what notices.

## Browser tests

SwiftShader rather than whatever GPU is present: slower, and identical on a
laptop and on a runner with no GPU, which matters more. The `webServer` serves
the **built** bundle, so the thing under test is the thing that ships.

Playwright reuses a server already listening rather than starting its own, so
a `vite preview` left running from an earlier look will serve a **stale**
bundle to the whole suite. It has caught me three times, each time as a test
that passed when it should not have. Stop the leftover first.

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
cargo test --workspace            # the kernel, and the Lua suite
cd front && npm run test:unit
cd front && npx playwright test
```

The last one needs artefacts the others do not; see [Building](building.md).
