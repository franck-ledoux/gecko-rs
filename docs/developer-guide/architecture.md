# Architecture

One kernel, in one place. Most of the design follows from refusing to let
that become two kernels kept in step by hand.

## One door

Every mutation goes through `gecko_cmd::Session::apply`. Everything above it
translates and does nothing else: the wasm façade is JSON in and bytes out,
and `gecko-lua` turns a Lua call into the same JSON.

That rule is what makes "a script and the mouse drive the same live object"
true rather than aspirational. It is also what makes it *checkable*: the
session keeps a journal of the commands it was given, and
[writes it back out as Lua](#the-session-as-a-script) — so what the mouse did
and what a script did are the same list.

A convenience written in one place would have to be written again in the
other, so anything worth offering is a command. `create_cube` is a command
rather than a helper for exactly that reason — and as a side effect the
journal records the intent instead of eight anonymous points.

```
            ┌───────────────────────────┐
            │ gecko-core                │  the block structure, on a 3-map
            └─────────────┬─────────────┘
            ┌─────────────┴─────────────┐
            │ gecko-cmd  Session::apply │  the only way in
            └─────────────┬─────────────┘
          ┌───────────────┴───────────────┐
    ┌─────┴──────┐                 ┌──────┴──────┐
    │ gecko-wasm │                 │ gecko-lua   │  the scripting API
    │ the kernel │                 │ 4 primitives│
    └─────┬──────┘                 └──────┬──────┘
          │                    ┌──────────┴──────────┐
          │              ┌─────┴─────┐        ┌──────┴──────┐
          │              │ gecko-cli │        │gecko-lua-   │
          │              │ (terminal)│        │wasm (page)  │
          │              └───────────┘        └──────┬──────┘
          └───────────────┬──────────────────────────┘
                   ┌──────┴──────┐
                   │ Babylon.js  │  the page: kernel and interpreter both
                   └─────────────┘
```

## One kernel, in the page

There is no server in the architecture. The page carries the kernel compiled
to wasm and a Lua interpreter beside it, so a script's `for` loop reaches the
session with nothing in between — no socket, no promise, no process.

| File                             | Role                                       |
| -------------------------------- | ------------------------------------------ |
| `front/src/kernel.ts`            | the interface: `apply`, `view`, `stats`, … |
| `front/src/kernel-wasm.ts`       | the one implementation, synchronous        |
| `front/src/lua.ts`               | the wire between the interpreter and it    |
| `front/src/viewer.ts`, `main.ts` | the application, written against the first |

The interface was asynchronous for as long as there was a second kernel at
the end of a socket, holding the session in another process. There is not any
more, and the promises went with it: a promise per call, for an answer that
is already there, is a story about a server that no longer exists.

What the local binary adds is **files**, and only files. `gecko` serves the
page and answers two endpoints; the session it serves is the page's, not its
own. `gecko run` has a session of its own and no page at all. They are two
instances of one program rather than two halves of one session, and that is
what keeps the architecture a single line.

### Two wasm modules, and the four primitives between them

mlua supports WebAssembly through `wasm32-unknown-emscripten` only, and
wasm-bindgen targets `wasm32-unknown-unknown`. An object built by one toolchain
cannot be linked into a module built by the other, so Lua rides **beside** the
kernel rather than inside it.

The boundary that makes this cheap is `gecko_lua::Kernel`, four methods wide:

| Method             | In the terminal       | In the page                           |
| ------------------ | --------------------- | ------------------------------------- |
| `apply(json)`      | `Session::apply_json` | JavaScript, into the other module     |
| `query(kind, arg)` | the read's method     | the same way                          |
| `read(path)`       | `std::fs`             | an endpoint `gecko` lends, or nothing |
| `write(path, …)`   | `std::fs`             | the same, or a download               |

Everything above those four — every call a script can make, every conversion
between JSON and Lua tables — is written once, in Rust. `agree.lua` runs the
same script through both hosts and checks the bytes agree, which is what
stops that claim from quietly becoming false.

The alternative was moving the kernel to emscripten as well, which would have
meant hand-writing the glue wasm-bindgen generates and putting everything
already shipping on a Tier 2 target. The risk belongs in a crate of a hundred
lines, not under the whole application.

## The session as a script

`Session` keeps the commands it was given, and `script::line` renders one as
the Lua call that would have made it. The `match` is over the whole command
set, so a command added without a line **does not compile** — a history with
a hole in it is worse than no history, because it looks complete.

Each outcome carries its own line, which is how what you do with the mouse
appears in the console. An undo truncates the journal to the length recorded
in the state it restores, so the script is a recipe for where the session is
rather than a diary of everything anybody tried. A geometry's *name* is kept
rather than the file it arrived with, which would be a copy of every mesh
ever loaded.

## Welding two blocks

Gluing is a 3-sew, and two facts about honeycomb shape how it is done.

**`Vertex3::merge` is the average.** Sewing two faces that are apart would
leave the shared corners half way between them. So the second face is moved
onto the first *before* the sewing, and the average is then of two points
that already coincide. The rule — the first face wins — is settled where it
belongs rather than by reaching into a merge policy that is the map's.

**`three_sew_tx` walks the face orbit itself**, β₁ down one side and β₀ up the
other, so one call links all four pairs and merges the faces, edges and
vertices along the way. It also refuses a pair of darts whose directions are
not opposed — a useful guard, but a partial one: its own comment says it
checks only the argument darts, so a half turn would pass. Choosing the
corner correspondence is entirely ours.

That choice is `best_pairing`, kept pure and apart because it is the only part
with a decision in it. A glue is necessarily orientation-reversing, which
leaves exactly four candidates, and the one that moves the corners least wins.
Ties go to the lowest turn, because both modes ship these bytes and compare
them.

## Cutting a sheet

`CMap3` has no split of any kind — not of an edge, a face or a volume — so a
sheet cut is dart surgery, in three passes over the whole sheet: every edge,
then every face it crosses, then every block.

Whole sheet at a time, not cell by cell. Cutting one block and then its
neighbour would leave their shared face half cut in between, and nothing
downstream could tell that state from a mistake.

**An edge is a ring of darts**, one per face-side around it, and every one
becomes two. The β₂ and β₃ partners are then re-paired *crosswise*: if `d`
runs A→B its partner runs B→A, so after the cut `d` is A→P and the partner's
new half is P→A — those two go together. Pairing the originals again would
join A→P to B→P, which share no edge at all.

**A face is cut by the half it keeps.** Which of the two new darts belongs to
which half is decided by the sheet's own direction: the half entered from a
`from` node is the `from` half. That is what lets the block split know which
of its quadrangles are above the cut and which below, without any geometry.

**A block is closed off along its ring of new edges.** Until then its two
halves are still joined, by the β₂ pairing each ring edge's two darts across
its own face. The cut face goes between them, β₃ to itself, which is what two
blocks sharing a face are.

### Why the raw setters

`link` and `unlink` are not the per-dart operations they look like.
`one_link` also links the β₃ mirror, to keep β₃∘β₁ = β₀∘β₃; `three_link`
walks whole faces and refuses a pair of different lengths. Both are right for
editing a finished map and wrong during surgery: halfway through a split the
two sides of a face genuinely are different lengths, and the mirror is
something the surgery already knows how to wire. Judged by those operators,
every intermediate state is invalid — which is how the first attempt failed,
with `AsymmetricalFaces` on a map that was simply not finished yet.

So the wiring is set outright with `set_beta`, and a coherence check in the
tests verifies afterwards what those operators would have checked throughout:
β₀ undoes β₁, β₂ and β₃ are involutions, and each joins the same two nodes
the other way round.

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

!!! danger "Resolving a name is a scan"
    `Blocking::node_by_id` walks the vertices. An index kept *across*
    operations would have to be invalidated by every topological change, and
    getting that wrong is silent — so there is none.

    Which makes **one of these inside a loop over cells** the standard way
    to make this project slow, and it is silent too: everything still
    answers correctly. It has happened twice. The second time, `stats` and
    `nodes` resolved a name per cell to report what each was classified on,
    and a single refresh of sixty-four blocks took nine seconds.

    The cure both times was the same: read what is wanted in **one pass**
    over the map. `nodes_in_full`, `edges`, `census`, `reclassify`
    and `classify_nodes` exist for that reason, and `BlockFace` carries
    what its face is classified on rather than leaving the caller to look
    it up. The reads are what a script calls in a loop, so a scan put
    back among them is felt immediately.

    Where it started to matter is inside one operation. A sheet cut resolves
    every edge of the sheet, and a scan each made the whole thing quadratic:
    on a 125-block grid a cut took 1240 ms and a sheet removal 8408 ms. Both
    now build one `edge_index()` at the top of the operation and look names
    up in it, which is safe because nothing is renamed while they run, and
    both came down to about 77 ms.

    The other half of that removal was naming. `remove_sheet` used to call
    `assign_missing_ids()` once per block it removed, which walks the whole
    map each time. The private `drop_volume` is the quiet primitive, and the
    naming pass runs once at the end.

## Going back

Undo is a **stack of previous states**, five deep by default and settable
from 0 to 50 (`Command::SetHistory`). A state is the block structure, the
geometry handle and the rebasing origin; the session keeps the last few in a
`VecDeque`, pushes one before any command that turns out to have changed
something, and pops one on `Command::Undo`.

`CMap3` is not `Clone`, and its `serialize` is one-way — there is no reader,
and none that would know about our attributes. So `Blocking::snapshot` writes
the copy out by hand. That is less of an imposition than it sounds: the map is
four things — the β functions, the vertex positions, the attribute storages
and the record of which darts are in use — and all four can be read and
written from outside. Nothing is re-sewn and nothing is re-derived. Darts are
allocated in runs of used and unused so that **each one lands on its own
number**, β is set before anything that hangs off it, the attributes are
copied cell by cell, and the name counters come across as they stand.

!!! note "Why not replay the commands"
    The obvious alternative is to keep the journal and replay it from the
    start, which this crate is unusually well set up for: every change is a
    `Command`, and a session is exactly the sum of the commands applied to
    it. It would even come back with the same names, because the dart
    allocator and the name counters are deterministic functions of the
    command sequence.

    Two things decide against it. The first is cost, in the wrong place: a
    copy is O(darts) and happens once per command — about 9 ms before a cut
    that itself takes 80 ms at fifty blocks — while a replay is O(the whole
    session) per undo, and so gets slower the longer you have been working,
    which is precisely when you most want to take something back.

    The second is that the replay is only right while every operation is
    deterministic down to the order darts come out in. It is today. An
    earlier prototype's `HashSet` in the gluing is why there is a
    cross-mode byte-comparison test, and that is the kind of change that
    would silently make a replay land somewhere else. A copy does not care.

    What both designs *must* do is bring the names back. `CellId`s are what
    a client holds between operations — the selected face, the lit sheet —
    and a model that came back renumbered would leave every one of them
    meaning something else, or nothing. The copy gets that structurally
    rather than as a consequence of determinism, which is the whole reason
    it is worth writing out by hand.

Two things are deliberately **not** on the stack. `Command::Undo` itself
leaves no state behind — an undo that remembered what it undid would be a
toggle rather than a step — and neither does `Command::SetHistory`, a
setting being not a change to the model. Nor is anything under [what does not
go through the kernel](#what-does-not-go-through-the-kernel): colours and the
camera are not changes to anything.

The geometry is **shared**, not copied (`Option<Arc<Geometry>>`). It is
read-only once loaded and only a load can replace it, so five steps cost five
block structures and no triangles. An undo that puts a different geometry
back still moves `geometry_generation` **forwards**: the number means "not the
one you last drew", and a client that saw it fall would have no reason to
trust it the next time it rose.

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
reads it straight out of wasm memory; a script asking for the same session
gets the identical bytes
a WebSocket. One producer, one decoder.

Little-endian throughout. Version 6.

| Offset | Type  | Count | Meaning                                                     |
| ------ | ----- | ----- | ----------------------------------------------------------- |
| 0      | `u8`  | 4     | magic, `GKB1`                                               |
| 4      | `u32` | 1     | format version                                              |
| 8      | `f64` | 3     | model centre                                                |
| 32     | `u32` | 1     | number of nodes, *N*                                        |
| 36     | `u32` | 1     | number of edges, *E*                                        |
| 40     | `u32` | 1     | number of quadrangles, *Q*                                  |
| 44     | `f32` | 3*N*  | positions, relative to the centre                           |
| …      | `u32` | *N*   | each node's `CellId`                                        |
| …      | `u32` | 2*E*  | node indices, two per edge                                  |
| …      | `u32` | *E*   | each edge's `CellId`                                        |
| …      | `u32` | 4*Q*  | node indices, four per quadrangle                           |
| …      | `u32` | *Q*   | each quadrangle's face `CellId`                             |
| …      | `u32` | *Q*   | each quadrangle's block `CellId`                            |
| …      | `i32` | *N*   | what each node is classified on: the dimension, −1 for none |
| …      | `i32` | *N*   | …and the entity's gmsh tag                                  |
| …      | `u32` | *N*   | 1 where a node is frozen                                    |
| …      | `i32` | *E*   | the same two for each edge                                  |
| …      | `i32` | *E*   |                                                             |

The edge names are what let a click on a line say which edge it was, and so
what lets the viewer ask for the [sheet](../user-guide/sheets.md) through it.

The classification is appended at the end rather than threaded in beside the
names it belongs with, so that every offset before it stayed where it was.
The frozen flag has an array of its own rather than a bit stolen from the
dimension beside it: a frozen node need not be classified at all.

Arrays rather than one interleaved record per quadrangle, like everything
above them: a client makes a typed-array view over each one instead of walking
a stride.

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

### One surface, read two ways

A quadrangle is listed **per (face, block) pair**, and carries both names. So
the faces and the blocks are the same geometry read twice — differing only in
which name picks the colour, and which centre a shrink pulls towards. That is
what lets both be on screen at once, each with its own colouring, transparency
and shrink, without the kernel shipping the surface twice.

A shared face arrives twice, once per block, wound each way — which is exactly
what gives each block a closed surface of its own. The format was given this
shape before anything could produce one, and so did not have to change when
[gluing](../user-guide/gluing.md) arrived.

It also means **internal is derived, not declared**: a face with a block on
both sides is one whose name appears twice in the list. No parallel flag to
disagree with the topology.

The colours themselves are not in here, and never will be. The buffer carries
names; what colour a name is drawn in is the viewer's business — see
[Appearance](../user-guide/display.md).

### The origin stays put

The centre is **not** recomputed from the bounding box each time. It is session
state, settled when the model changes, and left alone while it still serves.

Recomputing it means every edit shifts the whole scene: drag one node and the
other seven slide across the screen, because all of them are drawn relative to
a centre that just moved. The origin is there for precision, not for centring,
so it only has to stay *near* the model — it follows only once the model has
wandered half a diagonal away from it.

Settled on **edit**, never on read. An origin adopted when someone asks for the
view would make the bytes depend on how many times they had asked, and the two
façades — which ask at different moments — would quietly stop agreeing. The
cross-mode test caught exactly that.

## The geometry buffer

A **second** buffer, `GKG1`, fetched separately. Same encoding rules, same
rebasing origin, version 1.

| Offset | Type  | Count | Meaning                             |
| ------ | ----- | ----- | ----------------------------------- |
| 0      | `u8`  | 4     | magic, `GKG1`                       |
| 4      | `u32` | 1     | format version                      |
| 8      | `f64` | 3     | model centre, the view buffer's own |
| 32     | `u32` | 1     | number of nodes, *N*                |
| 36     | `u32` | 1     | number of triangles, *T*            |
| 40     | `u32` | 1     | number of segments, *S*             |
| 44     | `u32` | 1     | number of corners, *C*              |
| 48     | `f32` | 3*N*  | positions, relative to the centre   |
| …      | `u32` | 3*T*  | node indices, three per triangle    |
| …      | `i32` | *T*   | each triangle's surface tag         |
| …      | `u32` | 2*S*  | node indices, two per segment       |
| …      | `i32` | *S*   | each segment's curve tag            |
| …      | `u32` | *C*   | one node index per corner           |
| …      | `i32` | *C*   | each corner's tag                   |

Not a section of the view buffer, because **the two change on completely
different clocks**. The view is rebuilt after every edit and every dragged
node; a geometry is read once and then sits there. Sending three thousand
triangles down the socket because somebody moved a corner is the whole cost
this split avoids.

The tags are signed, because a gmsh tag is. Read as unsigned, a negative one
would be a four-billion-ish number in the colour palette rather than an
obvious wrong.

### How a client knows to fetch it

A **generation number**, in `stats`. It counts loads rather than describing
one; a client holds the number it last drew and re-fetches when it differs.

A flag on the outcome of the command would do for a load this client made
itself, and nothing at all for one made by a script. Counts would not do
either: two
geometries of identical size are still two geometries, and a client comparing
counts would go on drawing the first.

### One origin for both

The rebasing origin is settled over the blocking's nodes **and** the
geometry's. They share one scene, so they have to share one origin: a geometry
a kilometre from the blocks, rebased on its own centre, would be drawn on top
of them.

It also means loading a geometry reports `viewChanged` even though not one
block moved — the origin the blocks are drawn against may just have moved, and
a client that redrew only the geometry would leave them rebased on an origin
nobody uses any more.

## Reading a gmsh file

`gecko-geom` is a crate of its own, in two halves.

`msh` reads the file and **interprets nothing**: nodes, point/line/triangle
elements with both their tags, physical names, and a count of everything else
it passed over. `model` is what decides that triangles sharing an elementary
entity tag are a surface — the rule taken verbatim from the C++
`FacetedGeometry`, because the same `.msh` has to mean the same model in both.

A crate rather than a module of `gecko-core`, because the dependency has to
run **geometry ← blocking**: a cell of a block structure will be classified
onto one of these entities, not the other way round, so the geometry gets to
be testable with no block structure in sight. And a file reader has no
business inside a combinatorial map.

## Where a classification lives

Split across two crates, and the split is the point.

`gecko-core` stores **what** a cell is classified on: a dimension and a tag,
two plain numbers in the cell's attribute, plus a flag for frozen. It knows
nothing of triangles. `gecko-geom` answers **where** that is: the nearest
entity to a point, the point of an entity nearest a point, and which
entities contain which. `gecko-cmd`, which holds both, is the only place
that projects.

An attribute has to be `Copy`, so what a cell carries is **one** entity
rather than a set of them. That is not a concession: what a set would be
*for* is reading off the entities containing the one named, and the geometry
answers that from the name alone.

### Merging and splitting a classification

The two rules are different from the ones the names follow, and for
different reasons again.

**A merge** takes the first cell's name, as always — but the classification
and the freeze are taken from **either** side. A merge is two nodes becoming
one point: if either was pinned to the geometry, the point that remains is
pinned; if either was frozen, somebody asked for it not to move. Taking the
first blindly would throw away a constraint nobody withdrew.

**A split** clears the name and **keeps** the classification. Where a cell is
is not a claim about which cell it is, and a split moves nothing: both halves
of a node on a curve are on that curve.

## Meshing

`Blocking::mesh` turns every block into a structured grid of hexahedra by
transfinite interpolation, in three passes by dimension — edges, then faces
from their four edges, then blocks from their six faces — each read by the
one above it **after** its projection. The projection arrives as a closure,
as it does for `classify_nodes`: nothing in `gecko-core` knows what a
triangle is, and the crate that holds both the cells and the triangles is
the one above it.

How many segments an edge is divided into is a property of the **sheet**,
not of the edge: every edge of a sheet must carry the same number or the
mesh does not match across the faces between them. `sheet_segments` resolves
what it finds to the largest number on the sheet, so a sheet made by gluing
two that were set differently is still answered conformingly, and an edge
nobody has set picks up the number its sheet already had. That is the whole
conformity argument, and it is structural rather than checked.

### Nodes are owned by the cell they lie in

Two blocks glued along a face have to share the mesh nodes on that face. The
obvious implementation meshes each block and welds coincident points, which
works and needs a tolerance — and a tolerance is a number that is wrong for
somebody's model. Instead every mesh node is **named by the cell of the
blocking it lies in**:

| Site                    | What it is                                                               |
| ----------------------- | ------------------------------------------------------------------------ |
| `Node(cell)`            | a corner of the blocking                                                 |
| `Edge(cell, i)`         | a point along a block edge, counted from the end the map calls its first |
| `Face(cell, a, b)`      | a point inside a block face, on **that face's** lattice                  |
| `Inside(cell, i, j, k)` | a point inside a block, which belongs to nobody else                     |

Cells are shared by construction, which is what the combinatorial map is
for, so the sharing of nodes falls out of the naming. There is no tolerance
in the mesher.

The lattice of a face is read off the face's **own** β₁ cycle rather than
either block's, which is what makes the two agree: the blocks either side
see it wound opposite ways, and `Quad::of` undoes the turn and the
reflection by the one thing both agree about — which corner is which. The
divisions in it are exact, a side of a face running along one axis of the
block.

### What the browser gets

The mesh's **skin**: the quadrangles belonging to one hexahedron rather than
two, in `MeshBuffer` ("GKM1"), with only the nodes the skin uses. A hundred
thousand hexahedra are six hundred thousand quadrangles of which perhaps
twenty thousand are outside, so this is thirty times less for the same
picture. Its own buffer and its own generation number, for the reason the
geometry has them: it changes on a clock of its own.

The session holds the last mesh and **drops it the moment the blocking
changes**, bumping the generation so a client holding the picture learns it
has gone. A mesh of a model you have since edited is a lie that looks right.

## What does not go through the kernel

Exactly one thing, and it is worth naming because everything else does: **how
the scene looks**. Colours and the triad's visibility are view state. They are
kept in the browser, per viewer, and two tabs may look nothing alike.

The line is that the kernel describes what *exists*; it has no opinion on what
colour that is on your screen. Anything that would change what another client
sees belongs in a command; anything that only changes what *you* see does not.

## Direct manipulation

During a drag the scene **previews against its own copy** of the view — the
sphere and the edges reaching it follow the mouse — and the kernel is told once,
at the drop. Telling it on every mouse move would be a command, an undo step
and a rebuilt view per pixel.

So the view diverges from the kernel for the length of the gesture. That is a
deliberate exception to "the view is a function of the kernel", and it ends at
the drop, when the view is rebuilt from what the kernel actually holds.

Every node sphere carries its own drag behaviour rather than one being attached
on selection: attaching on the press that should *start* the drag misses that
press, and turns the gesture into click-then-drag.

Babylon eases a dragged node towards the pointer, a fifth of the way per frame,
to damp jitter from a tracked controller. With a mouse that means the node
lands wherever the easing had got to rather than where you pointed, so
`dragDeltaRatio` is set to 1. Placing a vertex is exactly the case where "close
to where you meant" is not good enough.

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
- **So is `scene.activeCamera`**, and Babylon's own drag behaviour reads it
  twice: to decide whose controls to suspend while dragging, and to build the
  ray the drag follows. Left alone, dragging a node turned the view *and* sent
  the node along a plane belonging to the corner gizmo. The viewer puts
  `activeCamera` back to the main camera once each frame is done.
- **A Babylon viewport measures y from the bottom; `Vector3.Project` wants it
  from the top.** Full-screen viewports hide the difference because the offset
  is zero either way. A corner one does not.

And one that cost a run-time crash in the browser while every headless test
passed: `Animation.CreateAndStartAnimation` calls `scene.beginDirectAnimation`,
which only exists if `@babylonjs/core/Animations/animatable` has been imported
for its side effect. Importing the `Animation` class alone compiles, ships, and
throws. There is now a test that asserts the method exists.
