# The Gecko Lua API, for meshing

The full reference is the gecko-rs docs (`docs/user-guide/scripting.md`,
`docs/user-guide/programs.md`). This is the part a mesher uses, and the
pitfalls that cost time.

## Running

```bash
gecko run part.lua            # prints what the script prints
gecko run part.lua --json     # one JSON report on stdout: ok, error, printed,
                              # returned, stats, blocking, quality, fidelity
```

Paths are relative to where `gecko run` runs. A failed script still gets a
report, of the session at the error — read `error`, then `blocking.issues`.

## Geometry

| call | gives |
| ---- | ----- |
| `gecko.load_geometry("p.msh")` | the part (MSH 2.2 surface mesh) |
| `gecko.merge_smooth{ angle = 10 }` | merges smooth seams; **read its message**, it says what it merged and the nearest miss |
| `gecko.merge_curves{ a, b }`, `merge_surfaces{…}` | by hand |
| `gecko.geometry()` | `bounds`, `span`, `corners[]`, `curves[]`, `surfaces[]` with `shape` (line, circle, plane, cylinder, sphere, other), incidence, and `pieces` for merges |
| `gecko.polyline(tag)` | a curve's points in order |
| `gecko.closest({x,y,z}[, {dim=, tag=}])` | nearest corner, curve, surface — or onto one |
| `gecko.entities()` | the raw list, with `members` of each composite |

**Merges get negative tags**, in the order they were made: −1, −2, …
They stand in for their pieces from then on — classify on the merge, not
on a piece. The order depends on the merge calls, so read the tags back
from `gecko.geometry()` (or `gecko.entities()`) rather than hard-coding them
from memory of another run.

## Blocks

| call | |
| ---- | - |
| `gecko.add_block{ c0..c7 }` | 8 corners: bottom quad c0–c3, top quad c4–c7, c4 above c0. Must be right-handed: `(c1−c0)×(c3−c0)·(c4−c0) > 0` — `blocklib` flips it for you |
| `gecko.faces()` | each face once per block: `id`, `block`, `center`, `on` |
| `gecko.glue_faces(a, b)` | weld; coincident faces do not move |
| `gecko.create_cube{ origin, size }`, `box_geometry{ margin }` | quick starts |
| `gecko.remove_block(id)`, `split_sheet(edge, t)`, `remove_sheet(edge)`, `pillow{…}`, `pillow_chord(face, block)` | editing |
| `gecko.move_node(id, {x,y,z})` | moves that node only |
| `gecko.blocking()` | the whole structure + `issues` (see below) |

Block, face and node **ids are not 0, 1, 2…** — they are stable names
(0, 256, 512, … on B1). Get them from the reads; never compute them.

## Attaching to the geometry

| call | |
| ---- | - |
| `gecko.classify_node(ids, dim, tag)` | puts nodes on an entity and **projects** them onto it |
| `gecko.unclassify_node(ids)` | |
| `gecko.classify_edge(ids, dim, tag)`, `classify_face(…)` | by hand; `tag` may be a list, taken as one |
| `gecko.classify{ tolerance = 0.02 }` | by proximity, the whole blocking — fine for a box, wrong near thin walls and close features |
| `gecko.freeze_node(ids)` | keeps a node still when smoothing |

**Edges and faces are inferred** from their nodes: an edge goes on the
lowest-dimensional entity containing both its ends' classifications; a face
likewise from its four corners. Consequences:

- An edge between two nodes on **different curves of the same surface** goes
  on the surface, not along a curve. To follow a curve, both ends must be on
  that curve (or a corner of it).
- A merged curve sits in the surfaces that contain **all** its pieces. Merge a
  curve with a piece that lies on a different surface (CAD duplicates do
  this) and faces touching it go on **nothing** — `blocking.issues` shows them
  as `outer_face_on_nothing`.
- Unclassifying a node does **not** unclassify its faces: what a cell was told
  stands until something contradicts it.

## Sizing and meshing

| call | |
| ---- | - |
| `gecko.set_segments(edge, n)` | sets the **whole sheet** through that edge |
| `gecko.sheet(edge)` | `segments`, `blocks`, `edges` — a list of **tables** `{id, from, to}`, not ids |
| `gecko.mesh{ segments = n }` | `n` only for sheets nobody set; `gecko.mesh()` uses 10 |
| `gecko.quality(metric)` | `scaled_jacobian` (default), `skewness`, `aspect_ratio`: `min`, `max`, `mean`, `inverted`, `worst{block, center, value}`, `blocks` (worst first), `histogram`, `bins` |
| `gecko.fidelity{ tolerance = 0.25 }` | `curves{followed, checked, missed}`, `corners{missed}`, `skin.max`, `geometry.max`, `worst` |
| `gecko.export_vtk("p.vtk")` | meshes first if needed |
| `gecko.smooth{ rounds = 30, share = 0.5 }` | block corners towards cubes; **can make things worse** — compare |

## Pictures

`gecko.render(path, { view, width, height, show, color, below, center, radius, up })`
— see `docs/user-guide/programs.md`. Defaults: the geometry labelled when
that is all there is; blocks over a faded part; the mesh by quality. Labels
read `S`/`C`/`P`/`B` + tag. Terminal only.

## Lua pitfalls

- JSON `null` becomes `nil`, and a `nil` **inside a list is a hole**: `#t`
  stops before it. Reads that might hold nothing in a list use objects for
  that reason; when you build your own lists, do the same.
- `io.write` and `print` are captured into the `--json` report's `printed`;
  they do not reach stdout there.
- A refused command raises a Lua error: wrap in `pcall` if failing is an
  answer you want.
- `gecko.reset()` empties the session; a script run in the page is emptied
  first anyway.

## The blocking report

`gecko.blocking()` / `report.blocking`:

- `summary`: `blocks`, `inverted_blocks`, `worst_block{block, jacobian}`,
  `issues` (counts by kind), `not_followed{corners, curves, surfaces}`;
- `issues[]`: `kind`, the cells concerned, `message`;
- `blocks[]`: `id`, `corners` (8 node ids), `center`, `jacobian`;
- `nodes[]`, `edges[]` (with `sheet`), `faces[]` (with `blocks`, `outer`),
  `sheets[]` (with `segments`, `set`).

Kinds: `inverted_block`, `poor_block`, `outer_face_on_nothing`,
`outer_node_on_nothing`, `node_off_its_entity`, `corner_not_held`,
`curve_not_followed`, `surface_not_covered`.
