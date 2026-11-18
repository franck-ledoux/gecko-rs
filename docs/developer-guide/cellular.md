# The cellular block structure

A design note for 0.2.0, written before the code and meant to be argued with.
It replaces the combinatorial map with an explicit complex of cells, and it
turns the mesher into something that never holds the mesh it writes.

Two things drive it, and they are not the same thing:

- **a browser has one address space**, and the block structure has to fit in
  it beside everything else;
- **a block is going to carry a curve, a surface and a volume of its own**,
  and a cell that has no lasting identity cannot carry anything.

## What it costs today, measured

Not estimated. A counting allocator around `Blocking`, on a 7x7x7 grid of
blocks built by eighteen `split_sheet` calls:

|                                     | live bytes | per block    | per dart |
| ----------------------------------- | ---------- | ------------ | -------- |
| 1000 free blocks                    | 19.7 MB    | **19 749 B** | 822 B    |
| 343 blocks, glued                   | 7.06 MB    | **20 587 B** | 857 B    |
| the same darts as plain `u32` betas | 132 KB     | 384 B        | 16 B     |

The cause is not the combinatorial map. It is `fast-stm`. In honeycomb,
`BetaFunctions<4>` is a `Vec<[TVar<DartIdType>; 4]>`, `UnusedDarts` is a
`Vec<TVar<bool>>`, and `AttrSparseVec<T>` is a `Vec<TVar<Option<T>>>`
**indexed by dart**. With the vertex positions and four tag attributes that
is **ten `TVar` per dart**, and a `TVar` is an `Arc<VarControlBlock>` over a
`RwLock<Arc<dyn Any + Send + Sync>>`: about 84 bytes and two allocations to
hold a `u32`. Ten of those, twenty-four darts to a block, is the 20 KB.

Two consequences worth stating plainly, because they decide what follows:

- **a cell attribute is paid for by the dart.** Attaching a curve to the
  edges would cost twenty-four `TVar` per block — about 2 KB — to store
  twelve curves. Every further attribute adds its own layer. The
  representation works against the one extension this project is heading
  for.
- **the undo stack multiplies all of it.** `HISTORY_STEPS` is 5 and
  `MAX_HISTORY_STEPS` is 50, and a measurement with five steps taken shows
  exactly x6.0. Ten thousand blocks is therefore **1.17 GB**, not 200 MB.

## The three budgets

A block is a thousand cells at `SEGMENTS = 10`, a hundred thousand at fifty
segments an axis. Several hundred million cells is **10^4 to 10^5 blocks**.

|                                   | today                                                      | after                     |
| --------------------------------- | ---------------------------------------------------------- | ------------------------- |
| the blocking, 10^5 blocks         | 20 587 B/block -> **2.0 GB**                               | ~400 B/block -> **40 MB** |
| the materialised mesh, 10^8 cells | ~60 B/cell -> **6 GB**, plus ~3.5 GB of hash table at peak | never materialised        |
| what the tab allows               | 4 GiB of address space on wasm32, 2-4 GB in practice       | the same                  |

So the blocking was never the wall. It breaks the ceiling at 10^5 blocks,
which is why this rewrite is worth doing; but at 400 B/block it is 1% of the
budget, and squeezing it further would be wasted work. The wall is
`Mesh { points, hexes, blocks }` held whole in `Session.mesh`, the
`HashMap<Site, u32>` that deduplicates mesh nodes while it is built, and the
VTK file handed over as one `Uint8Array` in the same linear memory.

## What is decided

1. ~~**No patches.**~~ **Patches stay**, and this is the one decision here
   that practice has already overturned. It said that a thin volume would be
   created instead of a free-standing quadrangle: one cell type, and an
   operation fewer.

   It was tried. Laying a series of thin blocks along the boundary of a
   surface, by hand, five of them, does not work and cannot be made to: a
   block has an inside, so placing one means saying which side the material
   is on and how thick it is, and four clicks say neither. Along a boundary
   the winding alternated from one quadrangle to the next, putting
   consecutive blocks on opposite sides of the surface; the thickness, a
   share of each quadrangle's own diagonal, came out between 0.35 and 0.61 on
   a model whose features are two units; and the node inside the material
   sits a fraction of a pixel behind the one on the surface, so two clicks in
   five reached the wrong one and landed a thickness off the model.

   A sheet has no inside, so none of those questions is asked. The height is
   one drag, once, for the whole strip, which `extrude_patch` already does
   with a step per node along the mitre and a weld along every shared edge.

   What this costs the structure: a `Face` whose two sides are **both** empty
   has to be legal, and `validate()` has to accept it. The invariant lost is
   "every face bounds at least one block", which was worth something. What it
   buys is the only interactive way onto a surface that works.

1. **`Face.nodes` is stored**, redundantly with `Block.nodes` and the hex
   table, and `validate()` checks the redundancy. Deriving it instead would
   put a `match` on every access to a face.

1. **Ids are bare `u32` inside, packed `index:24 | generation:8` at the
   boundary.** A slot that reaches generation 255 retires instead of going
   back on the free list, so a stale name can never designate another cell.
   The `u32` protocol the page and Lua speak does not change.

1. **A closed shell of six welded faces is no longer a block.** It was a
   property of the representation that no test exercised. A `Block` is a
   block by construction.

1. **Node merging at a weld is immediate**, not a union-find resolved later:
   every stored `NodeId` is live at all times, which is what keeps the rest
   readable.

1. **The inner nodes of a thin volume are free**, at a small distance at
   creation. See the trap below.

1. **General polyhedra and T-junctions are out of scope**, deliberately, and
   the cost of reopening that door is stated below.

1. **Mesh node numbers stay `u32`.** Which puts a ceiling on the whole
   pipeline at 4 294 967 295 nodes — a little over four billion, so a few
   hundred million cells has room and a few billion does not. Said here so
   that the day it bites, it is a decision being revisited and not a surprise:
   going to `u64` would cross `Mesh`, the VTK output, the file writers and the
   wasm façade, which is why it is not being paid for in advance.

## The structure

The only state is four arenas. Everything that climbs — a node's edges, an
edge's faces, "does this edge already exist" — is derived.

```rust
pub struct Blocking {
    nodes:  Arena<Node>,    gen_nodes:  Vec<u8>,
    edges:  Arena<Edge>,    gen_edges:  Vec<u8>,
    faces:  Arena<Face>,    gen_faces:  Vec<u8>,
    blocks: Arena<Block>,   gen_blocks: Vec<u8>,
    up: RefCell<Option<Up>>,
}

struct Node  { at: [f64; 3], on: On, param: [f32; 2], flags: u8 }        // 40 B
struct Edge  { ends: [NodeId; 2], segments: u32, on: On,
               curve: Option<CurveId> }                                  // 32 B
struct Face  { nodes: [NodeId; 4], edges: [EdgeId; 4], sides: [Side; 2],
               on: On, surface: Option<SurfaceId> }                      // 56 B
struct Block { nodes: [NodeId; 8], edges: [EdgeId; 12], faces: [FaceId; 6],
               volume: Option<VolumeId>, groups: u64 }                   // 120 B
struct Side  { block: BlockId, slot: u8 }
```

An arena is a `Vec` of slots whose index **is** the id, with a free list. A
removal empties a slot and leaves its neighbours alone — compacting would
renumber, and renumbering is the thing being escaped. The ids are newtyped
per kind so that the compiler refuses a face where a node is wanted. What it
buys beyond the bytes: the graph becomes plain data, so `Clone` is derivable
and `snapshot()` stops being seventy lines of hand-written dart copying.

Three points of the drawing worth defending:

**`Face.nodes` is wound as side 0 sees it**, outward. Side 1 reads the same
list backwards. That is the whole of the orientation question: `BlockFace`,
which the page walks on every redraw, is a read or a reversed read.

**No twist byte.** Crossing from block `b` slot `s` to block `b'` slot `s'`,
both views are the same quadrangle, so one is the reverse cycle of the
other and a single shift `r` relates them: `view'(k) = view(r - k mod 4)`.
Four comparisons, no stored invariant that can rot. The same reasoning as
`across()`, applied to the crossing.

**The twelve edges are stored in canonical order**, 48 bytes a block. The
hottest loop in the program is the transfinite evaluation, a hundred million
times over, and it wants the twelve curves by direct read rather than through
the faces.

### The invariants

`fn validate(&self) -> Result<(), Vec<Breach>>`, called by the tests after
every operation:

1. every stored id designates a live cell of the right generation;
1. `block.faces[s]` and `face.sides[i] == Side { block, slot: s }` agree —
   the link holds in both directions;
1. `face.nodes` is a rotation of `block.nodes[HEX_FACES[s]]` for side 0, and
   of its reverse for side 1;
1. `face.edges[k]` joins `face.nodes[k]` to `face.nodes[k+1]`;
1. no two faces carry the same four nodes;
1. an interior face has two sides, a boundary face one.

Invariant 2 is the one that catches weld bugs, and it is free to write once.

### What is derived

```rust
struct Up {
    node_edges: Csr<EdgeId>, node_blocks: Csr<BlockId>, edge_faces: Csr<FaceId>,
    by_pair: HashMap<[NodeId; 2], EdgeId>,
    by_quad: HashMap<[NodeId; 4], FaceId>,
}
```

Local operations do not need it: from a block, `block.faces[s] -> face.sides -> neighbour` is O(1). Global passes amortise it — `smooth`, `classify_nodes`,
`census`, the mesher and the drawing all sweep everything anyway. **Updated on
insertion, dropped on removal**, so a script of ten thousand `add_block` calls
stays linear. And because a cache is not state, `snapshot()` does not copy it:
the undo stack carries the four arenas and nothing else.

`nodes_since` is not a counter any more. The arena keeps a journal of what it
inserts, the session clears it at the command boundary, and the journal **is**
the answer. Today's `corner_key` exists because the map renames — "so that the
ones the command is about to make can be told from the ones it renames" — and
nothing is renamed here. Zero bytes a node, and no operation can forget to
declare what it made, because the arena notes it rather than the operation.

## The hexahedron's tables

`HEX_FACES`, `HEX_CORNER_EDGES` and `HEX_EDGES` already exist — the last was
in the mesher and has moved in beside the other two. Two more, both derivable
from `HEX_FACES` and therefore asserted at compile time rather than trusted:

```rust
const FACE_EDGES: [[u8; 4]; 6];   // each face's four edges, in HEX_FACES order
const OPPOSITE: [u8; 6] = [1, 0, 4, 5, 2, 3];
const FACE_AXIS: [u8; 6] = [2, 2, 1, 0, 1, 0];
```

**Sheet and chord become one walk on a `(block, axis)` pair.** A chord through
face `f` leaves by `OPPOSITE[f]` and re-reads its axis in the neighbour
through `r`. A sheet through an edge holds, in each block, the four edges of
that axis, and propagates through the four faces **parallel** to it — those
with `FACE_AXIS != a`. One step, O(1). The concertina that `across()` guards
against becomes a sign per block, set once at the crossing.

## The numbering, and a mesher that streams

The mesh is not to be held. Per block: evaluate the twelve edge lattices and
six face lattices locally — a shared face is recomputed by the second block
rather than looked up, which is cheap and gives the same coordinates because
a face's lattice is computed from its own four edges in its own canonical
order — then the interior, then emit.

Node numbers come from arithmetic, not from a table. Four prefix-sum offset
tables over the cells of the **blocking**: one id a node, `seg-1` an edge,
`(a-1)(b-1)` a face, `(a-1)(b-1)(c-1)` a block. Two blocks sharing a face
compute the same number for a shared node without consulting anything. The
convention is already in the code — "counted from the end with the lower node
name so that both blocks count the same way" — and only the hash lookup is
replaced by the arithmetic.

Those tables are sized by the blocking, not by the mesh: 10^5 entries to
number 10^9 nodes exactly. `HashMap<Site, u32>`, which is sized by the mesh,
goes away. So does `Mesh` as a container; the skin the page draws becomes the
boundary faces only, decimated.

## The curved part

Curves, surfaces and volumes of the structure's own, independent of whatever
geometry it is projected onto. Arenas of them, referenced optionally, so that
straight and derived cost nothing. And one rule, which keeps conformity
structural the way `sheet_segments` already does:

> **Geometry is carried by the lowest-dimensional cell. Higher cells derive
> it by transfinite blending unless they carry their own, which must then
> coincide on the boundary.**

An edge carries a curve or is straight. A face blends a Coons patch from its
four curves unless it carries a surface, whose boundary must be those four
curves. A block blends a transfinite volume from its six faces unless it
carries one. Conformity across a shared face is then automatic, because the
face is **one object** that both blocks name.

That rule is what keeps the budget: with everything stored everywhere a
tri-cubic volume is 2 KB a block and the total is ~3.9 KB a block, 390 MB at
10^5 blocks. With the rule, the curved cells are the ones on the boundary —
some four thousand faces for a 10^5-block model, a couple of megabytes.
**The NURBS budget grows like the boundary, not like the volume.**

Two refinements in the same direction: the basis is **interned** — degree and
knot vectors in an arena shared by many cells, free for single-span Bezier —
and weights are stored only for a rational basis, 24 bytes a control point
instead of 32.

What the operations do to it:

- **cutting a sheet is exact.** `split_sheet(edge, t)` becomes a knot
  insertion: the split curve is the same geometry. A derived cell has nothing
  to do.
- **welding is not.** `glue_faces` has nodes meet in the middle, so two
  surfaces must become one: control points averaged in a common basis, or a
  least-squares refit. A modelling decision, to be taken on its own.

## Writing the mesh

A structured zone stores no connectivity at all, which is the whole point: at
10^9 nodes the coordinates are 24 GB in `f64`, where the current VTK legacy
output would add **32 GB of connectivity** for nothing.

**VTK XML structured multiblock first**: one `.vts` a block with its
`WholeExtent` and an appended binary point array, and a `.vtm` listing them.
No dependency, ParaView reads it natively, it streams.

**CGNS after.** It is the right format for this data — a structured `Zone_t`
is exactly a block, and `GridConnectivity1to1_t` carries a `Transform` which
is precisely the axis permutation `r` above — but modern CGNS is HDF5, which
is C, and the kernel is `wasm32-unknown-unknown` built by wasm-bindgen while C
lives only in the emscripten module. The way through is a minimal HDF5 writer
in pure Rust, for the subset CGNS uses. That matters when the mesh goes to a
solver rather than to ParaView.

Coordinates are written in `f32` or `f64` **by request**: half the file and
enough to look at, against what a computation needs.

## The polyhedra door, closed on purpose

A polyhedron is not meshable by a block-structured mesher: no `(i, j, k)`, no
transfinite map, no conformity of segment counts, no prefix-sum numbering.
LoopyCuts produces hex-*dominant* decompositions, and its non-hex cells are
either split into hexahedra or handed to another mesher. So the open door does
not widen a field in a struct — it opens **a second path through everything
downstream**: the mesher, the numbering, the curved evaluation, the drawing,
the export. That is where the cost is.

Reopening it later is bounded: `[NodeId; 8]`, `[EdgeId; 12]`, `[FaceId; 6]`
and `[Side; 2]` become flat arrays with offsets, plus a flip bit per
cell-face incidence. One to two weeks on the structure. And the insurance
costs nothing today: **everything goes through accessors** — `corners(block) -> [NodeId; 8]`, `edges_of`, `faces_of` — the hex template lives in one
module, and nothing outside it indexes the arrays.

## The trap in the thin volumes

A thin volume laid on a surface has four nodes on it and four free, at the
thickness. But `settle_fresh` runs after every command, keeps the fresh nodes
that are `beside_the_classified` — any neighbour classified — and settles them
onto the geometry. The four inner nodes each have their classified opposite as
a neighbour. So **if the thickness is under `SNAP_SHARE`, 2% of the model's
span, the automatic settling projects them onto the surface and the volume
flattens to nothing.**

The guard that saves a cube does not apply: a cube escapes because none of its
neighbours is classified, not because the code knows it was just made. So the
creating operation **declares what it leaves free**:

```rust
pub fn block_on_surface(&mut self, quad: [[f64; 3]; 4], thickness: f64)
    -> Result<(CellId, Vec<CellId>), BlockingError>;
```

and the session excludes that list from `settle_fresh`. The thickness is a
share rather than a length, `SLAB_SHARE`, and **signed** — the sign picks the
side, as the extrusion height already does.

Done, as `add_slab`, and the trap was there exactly as predicted. It is worth
keeping in mind that the trap is **not** why decision 1 was overturned: the
guard works. What does not work is asking four clicks which side the material
is on. A laid quadrangle has no free nodes at all, so it never comes near
this.

## What disappears

- `ids.rs` entirely: `CellId` becomes the arena id, and `CornerKey` goes with
  the attribute splitting that motivated it. Its four `HashMap`s were ~380
  B/block, more than the whole structure replacing them.
- `snapshot()`'s seventy lines of dart copying: `#[derive(Clone)]`.
- `mesh.rs::hexes()`'s recovery of the hex frame by walking beta_1 and
  beta_2: `block.nodes` is that frame, in VTK order.
- `BlockFace.volume`, which exists only because "finding a block by name is a
  scan of every volume". An arena lookup is O(1).
- ~~`add_face`, `remove_face`, `extrude_patch`, `patches()`, `nb_patches()`,
  `patches_json()`, `is_block`, `named_is_block`, and `tests/patches.rs`.~~
  **All of it stays**, with `lay_face` and `weld_sheet` besides — see
  decision 1. `is_block` is the one that can still go: a `Block` is a block
  by construction (decision 4), so telling a block from a patch stops being
  a question about whether a shell closes.
- ~~outside `gecko-core` — the only place this rewrite reaches past it —
  three Lua commands, `patchesJson` in `gecko-wasm`, two census lines in
  `gecko-cmd`, and in the page `patchIds`, `isPatch`, the displayed count,
  the hover text and the `extrude_patch` branch of the drag.~~ Stays too.

`attributes.rs` does not disappear: `AttributeUpdate::merge` and `split` stop
being callbacks the map invokes at moments nobody chose, and become explicit
code at each operation. Clearer, and more lines. That is not sold as a gain.

## Step nought, done

Before any of it, the boundary. `honeycomb` is reachable from nowhere but the
file that owns the representation:

|                                          | mentions of a dart type                                                  |
| ---------------------------------------- | ------------------------------------------------------------------------ |
| `blocking.rs`                            | 190 — the engine                                                         |
| `attributes.rs`                          | 10 — the trait impls honeycomb asks for                                  |
| `hexahedron_tests.rs`, `naming_tests.rs` | moved in from `tests/`: they are about the representation and go with it |
| everything else                          | **none**                                                                 |

**No public signature mentions a map identifier.** What changed:

- the accessors that spoke the map's language are `pub(crate)` and suffixed
  `_of`; the names they occupied — `position`, `edge_ends`, `hex_corners`,
  `neighbors`, `on_the_surface`, `is_block`, `faces_of` — now take a `CellId`,
  and `node_names`, `edge_names`, `face_names`, `component_names` and
  `edges_in_full` list cells by name;
- `map()` and `orbit()` are behind `#[cfg(test)]`;
- `Hex` and `HEX_EDGES` moved into `blocking.rs`, `Hex` by name, so the mesher
  no longer walks betas to recover a frame — it reads `block.nodes`;
- `BlockFace` gave up the volume it carried for speed and carries the two
  answers that were asked of it instead, `groups` and `patch`;
- `SheetEdge`, `GroupMasks` and `Unmet` are keyed by name;
- `BlockingError` carries the map's failures **as words** rather than as its
  types, so honeycomb is not in the signature of everything that can fail;
- the two test files that are about the map moved into `src/` as `#[cfg(test)]`
  modules, which is where something needing private access belongs.

The rest of the suite — some three thousand lines — now speaks only of named
cells. That is what makes it an oracle for the steps below, rather than a
description of the thing being replaced. And the mesh file is unchanged byte
for byte: `agree.lua` keeps the digests it had.

## The order of work

1. **The streaming mesher** — the prefix-sum numbering, per-block generation,
   `.vtm`/`.vts` streamed out, a decimated skin for the screen. Independent of
   the rest, and it carries the hundred-million-cell target.

1. **The cellular structure** — behind the current API, with the accessor
   discipline. **Taken first instead**, the mesher's own half of point 1
   being already done: the prefix-sum numbering is in `mesh.rs` with the VTK
   file unchanged byte for byte, and `hexes()` reads `Hex` rather than
   walking betas. What is left of point 1 is file writing, and it would have
   had to be written twice if the structure moved under it afterwards.

   How it is being done, since a five-thousand-line engine cannot be
   swapped in one commit:

   - `cells.rs` holds the vocabulary two engines agree on, owned by neither.
   - a `cellular` feature chooses which module the crate exports as
     `Blocking`. Both are always compiled, so one of each can be held at
     once.
   - **the suite is the oracle.** Not a line of it changes. `./cellular.sh`
     runs it under the feature, counts what passes, and fails if the count
     falls below `cellular.ratchet` — which is bumped in the commit that
     earns it. It started at 43 of 314, and stage 1 — construction, the reads, the
     settling, the invariants — took it to 123 of 316.
   - `tests/differential.rs` runs a script of operations on **both** engines
     and compares every answer after every step, which is the part no test
     written by hand can do: a test says what its author thought to ask.
   - `hexahedron_tests.rs` and `naming_tests.rs` name `blocking::Blocking`
     outright. They are about the map and they go with it.
   - the mesher needed one line: it is written against the public surface
     and names no dart, so it compiles over either engine unchanged.

1. **The curved cells** — the arenas, the carrying rule, interned bases, knot
   insertion for the cut, and the refitting question settled.

1. **CGNS** — the minimal HDF5 writer.

1. **Polyhedra** — reopened the day LoopyCuts is on the table, knowing the
   work will be downstream.
