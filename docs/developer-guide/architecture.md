# Architecture

Two modes, one kernel. Most of the design follows from refusing to let that be
two kernels that are kept in step by hand.

## One door

Every mutation goes through `gecko_cmd::Session::apply`. The wasm façade and
the PyO3 façade translate and do nothing else — `gecko-wasm/src/lib.rs` and
`gecko-py/src/lib.rs` read side by side, with the same methods in the same
order.

That rule is what makes "the console and the mouse drive the same live object"
true rather than aspirational. It is also what makes it *checkable*: replay one
sequence through both façades and compare the bytes that come out. If either
grows logic of its own, the bytes diverge and
`python/test_modes.py` says so.

A convenience written in one façade would have to be written again in the
other, so anything worth offering is a command. `create_cube` is a command
rather than a Python helper for exactly that reason — and as a side effect the
journal records the intent instead of eight anonymous points.

```
            ┌───────────────────────────┐
            │ gecko-core                │  the block structure, on a 3-map
            └─────────────┬─────────────┘
            ┌─────────────┴─────────────┐
            │ gecko-cmd  Session::apply │  the only way in
            └─────────────┬─────────────┘
          ┌───────────────┴───────────────┐
    ┌─────┴──────┐                 ┌──────┴─────┐
    │ gecko-wasm │                 │ gecko-py   │
    │  (static)  │                 │ (station)  │
    └─────┬──────┘                 └──────┬─────┘
          └───────────────┬───────────────┘
                   ┌──────┴──────┐
                   │ Babylon.js  │  the same front end either way
                   └─────────────┘
```

## Two transports, one front end

The TypeScript is identical in both modes. Only where it gets its data differs:

| File                             | Role                                                 |
| -------------------------------- | ---------------------------------------------------- |
| `front/src/kernel.ts`            | the interface: `apply`, `view`, `stats`, `onChanged` |
| `front/src/kernel-wasm.ts`       | static — the kernel this tab owns                    |
| `front/src/kernel-remote.ts`     | station — the kernel Python holds                    |
| `front/src/viewer.ts`, `main.ts` | cannot tell them apart                               |

Two decisions there are expensive to retrofit, so they were made at the start:

**The interface is asynchronous everywhere**, even in the static mode where
every answer is immediate. Otherwise every call site would assume a synchronous
reply, and the station could only be added by rewriting all of them.

**`onChanged` exists from the start.** In the station the kernel moves without
this tab asking — someone typed in the Python console. A client that only polls
after its own commands would never see it.

A change also carries its **origin**, and a tab ignores its own. Without that a
tab refreshes twice for its own click, and announces that Python did it.

## Naming a cell

A combinatorial map names its cells by darts: `vertex_id`, `edge_id` and the
rest all return the smallest dart of an orbit. That is cheap, and it is what
topological code wants — and it is useless as a name to hold on to, because
every insertion and removal rewrites it.

So cells carry names of their own, and there are two because they answer two
different questions (`crates/gecko-core/src/ids.rs`):

**`CellId`** — a name a cell keeps for as long as it exists, unique among cells
of its dimension and never reused. This is what a caller holds between
operations. It is deliberately **cleared on a split**: what comes out of a split
is a new cell with a new name, not a second claimant to an old one.

**`CornerKey`** — which *corner* a node is, as against which node it is. A map
splits a node's attribute when the vertex orbit behind it comes apart; deleting
a neighbouring block does that. Nothing was cut in two — geometrically, and to
the cells that had it, it is still the same corner — so the key is **copied
across** while the `CellId` is cleared.

Both are taken straight from GECKO's `CellData.h`. The problem is not specific
to CGAL: it is inherent to identifiers derived from darts, and honeycomb has it
too.

!!! note "Resolving a name is a scan"
    `Blocking::node_by_id` walks the vertices. An index would have to be
    invalidated by every topological change, and getting that wrong is silent.
    At this size the scan is not worth the hazard; the comment marks where the
    index goes when it starts to matter.

## What an attribute may hold

`honeycomb_core::attributes::AttributeUpdate` requires `Copy`, so nothing of
variable size can live in an attribute — no list of classification targets, no
vector of control points.

That constraint is not in the way, it *is* the design: an attribute carries only
a stable name, and everything that grows hangs off that name in tables beside
the map. GECKO arrived at the same arrangement for its own reasons.

The `merge` and `split` behaviour is the whole contract with the map, so it is
spelled out rather than defaulted: a merge keeps one existing name (it creates
no cell, so it must consume no new name); a split clears both and leaves them
for `assign_missing_ids`.

## The view buffer

What the browser draws, in the one shape both modes deliver it. The static page
reads it straight out of wasm memory; the station sends the identical bytes down
a WebSocket. One producer, one decoder.

Little-endian throughout. Version 2.

| Offset | Type  | Count | Meaning                           |
| ------ | ----- | ----- | --------------------------------- |
| 0      | `u8`  | 4     | magic, `GKB1`                     |
| 4      | `u32` | 1     | format version                    |
| 8      | `f64` | 3     | model centre                      |
| 32     | `u32` | 1     | number of nodes, *N*              |
| 36     | `u32` | 1     | number of edges, *E*              |
| 40     | `f32` | 3*N*  | positions, relative to the centre |
| …      | `u32` | *N*   | each node's `CellId`              |
| …      | `u32` | 2*E*  | node indices, two per edge        |

The f64 centre sits 8-byte aligned and everything after it 4-byte aligned, so a
client can view the bytes as typed arrays with no copy.

**Positions are f32 relative to the centre, with the centre alongside in f64.**
WebGL has nothing but f32, and a model parked a million units from the origin
quantises to steps of about 0.06 — visible jitter and z-fighting. Rebasing costs
nothing and cannot be retrofitted cheaply once a client assumes absolute
coordinates.

The consequence is that **a round trip through the buffer is lossy on purpose**.
The kernel is the authority, in f64; the view is a picture of it, in f32. A test
that demands bit equality between the two is asserting something the format does
not promise.

The node names are there so the browser has something to call a sphere it
picked. Its position in the array is not a name: edits reorder it.

## What does not go through the kernel

Exactly one thing, and it is worth naming because everything else does: **how
the scene looks**. Colours and the triad's visibility are view state. They are
kept in the browser, per viewer, and in station mode two tabs on one session
may look nothing alike.

The line is that the kernel describes what *exists*; it has no opinion on what
colour that is on your screen. Anything that would change what another client
sees belongs in a command; anything that only changes what *you* see does not.

## Direct manipulation

During a drag the scene **previews against its own copy** of the view — the
sphere and the edges reaching it follow the mouse — and the kernel is told once,
at the drop. Telling it on every mouse move would be a network round trip per
pixel in the station.

So the view diverges from the kernel for the length of the gesture. That is a
deliberate exception to "the view is a function of the kernel", and it ends at
the drop, when the view is rebuilt from what the kernel actually holds.

Every node sphere carries its own drag behaviour rather than one being attached
on selection: attaching on the press that should *start* the drag misses that
press, and turns the gesture into click-then-drag.

## Two cameras

The orientation triad is a second camera on a corner viewport, separated from
the main one by layer mask. Three consequences, each of which cost a test to
find:

- **Every mesh must opt into a layer.** A mesh created without one renders in
  both viewports, so the viewer sets the main layer on everything it builds.
- **`scene.getTransformMatrix()` is whichever camera rendered last**, which is
  the triad. Anything projecting a world point to the screen has to use its own
  camera's view and projection matrices, and anything picking has to name its
  camera rather than relying on the active one.
- **A Babylon viewport measures y from the bottom; `Vector3.Project` wants it
  from the top.** Full-screen viewports hide the difference because the offset
  is zero either way. A corner one does not.

And one that cost a run-time crash in the browser while every headless test
passed: `Animation.CreateAndStartAnimation` calls `scene.beginDirectAnimation`,
which only exists if `@babylonjs/core/Animations/animatable` has been imported
for its side effect. Importing the `Animation` class alone compiles, ships, and
throws. There is now a test that asserts the method exists.
