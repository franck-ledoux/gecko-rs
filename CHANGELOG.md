# Changelog

Versions are what a reader can download, and this is what they get. Dates are
the day a tag was cut.

## Unreleased

- **Groups of blocks.** A group is a part of the model you have named: a set
  of blocks, and with them everything they are made of — a face, an edge or
  a node is in a group when a block it bounds is. A block can be in several
  at once, and sixty-four groups is what a session holds.
- A **Groups** band in the display drawer has a row each: show it, rename it
  in place, put the selected blocks in or take them out, and forget it. The
  last row is the blocks in no group, which is how the unnamed part of a
  model is hidden.
- A group marked **written** becomes a variable of the mesh: one cell array
  per group, named after it, holding 1 for the cells of its blocks. One
  array per group rather than one number per cell, because a cell can be in
  several and a column of numbers could only name one.
- A fourth **colouring**, `group`, for nodes, edges, faces and blocks alike.
- Membership lives on the block itself, one bit each, so the model carries
  it: cutting a block in two leaves both halves in whatever it was in, and
  welding two gives the result both their groups. Nothing recomputes it and
  nothing can go stale.
- `gecko.group`, `gecko.ungroup`, `gecko.rename_group`, `gecko.drop_group`
  and the `gecko.groups()` read, so a session's groups are in its script and
  replay with it.

## 0.1.3

Three things from a session with it, and one of them took the measuring.

- **A session no longer bleeds classification.** A node an operation makes
  among nodes that are on the model is now put on the model too, when it is
  near enough — the same question `classify` asks, asked at the moment the
  node appears. Before, a node made where there was nothing to inherit
  landed on the geometry claiming nothing, every edge through it was
  cleared, and anything cut out of *those* inherited nothing in turn.
  Measured on one session: twenty of seventy-two nodes sat on the model
  claiming nothing and a fifth of the edges had been cleared because of
  them; the same session now ends with none and with thirty more edges
  classified.
- The reach it is judged by is the one the **last `classify` was given**,
  not a number in the kernel: somebody working at five percent means five
  percent.
- And the limit, which is the other half of it: a block somebody has just
  **made** is never pinned behind their back, however exactly it sits on
  the geometry. Dropping a cube on a model is not a statement that the cube
  follows it.
- **An edge keeps what nothing contradicts.** It used to be cleared the
  moment either end stopped being classified — so a pillow, which rightly
  lets go of the four nodes it draws into the material, wiped every edge
  that touched one of them though none had moved. Now the end that is still
  classified has to disagree before the edge gives anything up, and an edge
  with nothing at either end — the inside of the model — still goes.
  Nothing is handed to an edge from one end alone. Faces follow the same
  rule, one dimension up.
- **The handles can be hidden**, like edges, faces and blocks — and hidden,
  they are out of the pointer's way as well: a handle you cannot see but
  can still grab moves the model with nothing on screen to say what you took
  hold of.
- The mesh's controls sit directly in their band: the **skin** fold inside
  it is gone. A mesh has no groups to tell apart, so the fold was a step to
  open before reaching three controls.

## 0.1.2

Smoothing, a session you can replay, and a handful of things from using it.

- **`R` smooths the blocking**: every node towards the middle of its
  neighbours, as far as its classification allows. A node on a curve slides
  along the curve, one on a surface over the surface, one on a geometric
  corner stays, and so does one on the outside of the blocking that is
  classified on nothing — it has no surface to be kept on, and moving it
  would pull the blocking off the model. How many passes and how far each
  one goes are two sliders under *Operations*.
- **Smart**, in the sense the word has in the literature as well as in the
  slider: a move is taken only if it does not make the worst corner of the
  cells round that node any worse, measured by the scaled Jacobian. A plain
  Laplacian on a concave boundary turns cells inside out; this does not. On
  a blocking that is already as good as it gets it does nothing, and says
  so.
- **Dragging a node still moves only that node** (and whatever was picked
  with it). Smoothing outwards from a drag, ring by ring, was built for this
  release and taken out again before it: what you want of a drag is the node
  under the pointer, and a neighbourhood rearranging itself around it is a
  surprise rather than a help.
- A **frozen** node is drawn as a **cube** rather than a ball, in whatever
  colour the colouring gives it. It was grey of its own until now, which hid
  the two things the colouring is for — which node it is, and what it
  follows. Being out of play is a thing about the cell, so it is said with
  the shape. The colour panel has one fewer swatch for it.
- **Open takes a `.lua`**, and replays it from an empty session: blocks,
  geometry and mesh go first, because a script describes a session from
  nothing and run on top of a model it would describe something else. The
  console opens, since that is where a script talks. One step of undo takes
  the reset and the whole replay back together. `gecko.reset()` is the same
  thing from a script. Dropping a `.lua` on the window works too — and
  dropping the `.msh` beside it is how a script that opens a geometry by
  name finds it.
- **`E` takes the mesh away again.** Before, the only thing that got rid of
  a mesh was editing the blocking, so a mesh was something you could put on
  screen and not take off. The **show** box in the display panel is still
  the other thing: that one hides it and keeps it.
- **`⇧Esc` drops the last thing you picked** and leaves the rest. `Esc`
  clears the whole selection, which is a lot to pay for one wrong click in a
  selection that took a while to build. It knows which of the selections
  your last click went to, so mixing nodes, faces, edges and geometric
  entities does not confuse it.
- **The classification colours are told apart.** They were three violets of
  one hue and a corner looked like a curve: measured in OKLab, the nearest
  pair was 11 units in the dark theme and 6 in the light. They are a ramp
  now — pink, magenta, violet, the lightness falling and the hue turning the
  same way — at 22 and 20, and each stays clear of the colour an
  unclassified cell wears, which the old deep violet was 7 from. The
  measurement is a test, so they cannot be quietly harmonised back together.
- **How big the blocking is drawn is a setting**: *handles* and *edges*
  under *Drawing*, in pixels on the screen. The wireframe is a ribbon rather
  than a GL line to make the second of those possible at all — a GL line is
  one pixel wide whatever anybody asks for. Measured on a blocking of 1500
  edges, the ribbon costs about 2% more to rebuild and 1.5% more per frame
  than the line it replaces at the default width, and 10% more per frame at
  the widest setting the panel offers.
- **Nodes, edges and faces start coloured by their classification.** Putting
  the blocking onto the model is what a session is for, so what each cell is
  attached to is the thing worth seeing first. Blocks keep their own
  colouring, being classified on nothing.
- **A bounding box hugs the model**: 2% of each half-side rather than 10%,
  in the kernel's default as well as in the drawer, because pulling its
  faces onto the surfaces nearby is the first thing anybody does with a box.
- **"follows" is gone** — this codebase's word for a cell's classification,
  which meant nothing to anybody else. The drawer says `class`, the fold
  line says `classification`, and the help card and the guide say "what a
  cell is classified on".
- **Fixed**: the blocking was drawn an origin-shift away from the model
  after an edit that took a lot of it out — a removed sheet, say. Everything
  in the scene is sent rebased on one origin so that f32 coordinates stay
  small, and the blocking's own view was choosing an origin of its own, over
  the blocking alone: a smaller world than the one the session settles over,
  and so one that walks past its own threshold first. The mesh and the
  geometry never heard about the new centre, so they stayed where they were
  and the block structure jumped. The view now takes the origin it is given.
  The page also fetches the geometry and the mesh again when the origin does
  move, which it had no reason to do before: neither buffer had changed, only
  the middle of the scene they are drawn around.
- **Fixed**: `gecko.move_node` dropped its `smooth` option without a word,
  so a script — and the session's own replayed history — got the plain move
  where a drag had rippled. The reference had documented the option all
  along.
- **Fixed**: a table returned to the console printed only eight of its
  fields, in hash order, so `gecko.stats()` hid `blocks` behind an ellipsis
  as soon as `patches` joined the table. Twenty-four fields print now, list
  entries in index order and named ones in name order, so adding a field no
  longer moves the others.

## 0.1.1

A readable help card, and a window that says which gecko it is.

- The card of shortcuts **fits the window**. It had grown past the bottom of
  the screen — at 1280×800 it wanted 1087 pixels and had 670 — because one
  group had reached twenty shortcuts. The keys that act on the geometry and
  the ones about meshing are groups of their own now, and the card lays them
  out in columns the browser balances rather than in a grid that aligns its
  rows. Measured at three ordinary window sizes: nothing to scroll at any of
  them.
- The **name and version** are on the card's title and at the left of the
  readout along the bottom, taken from the kernel itself — the same number a
  release is tagged with.

## 0.1.0

The first release. A blocking toolkit that runs in a browser tab, scripted in
Lua, with a kernel on a 3-dimensional combinatorial map.

### The application

- **One page, carrying everything.** The kernel compiled to wasm and a Lua
  5.4 interpreter beside it, so the mouse and a script drive the same session
  with nothing in between — and a static host is all it needs.
- **A script console**, with the session's own history in it: every
  interactive operation writes the Lua line that would repeat it, and the
  whole session saves as a script that runs.
- `gecko` on this machine: serves the page, opens it, and gives a script real
  files to read and write. `gecko run script.lua` runs one with no browser.

### Blocks

- Blocks made, glued, removed and extruded; **sheets** cut and removed;
  **chords** removed, and wrapped in an **o-grid** along their whole length.
- Gluing welds each pair of corners **at the point between them**, with
  classification overriding it: a frozen corner does not move, and a welded
  corner lands on the more constrained of the two entities — the curve where
  two surfaces meet, rather than either of them.

### Blocking a surface first

- A **quadrangle laid on a geometric surface**: pick it, press `Q`, and the
  four corners the boundary turns a right angle at become a face already
  classified — corners on corners, face on the surface, each side on the
  curves between two chosen corners.
- A side that runs along several curves is one entity, a **composite**, so a
  node slides along the whole of it and the mesher follows it.
- Those faces are cut, removed and welded like anything else, and a whole
  connected sheet of them is **extruded into blocks** in one go — either
  way, since a sheet has no inside — with the blocks glued to each other
  along the faces the shared edges were raised into.
- **Pillowing** a patch of faces, which draws the picked blocks in and lays a
  layer into the space, leaving the model's surface where it was.
- Five steps of **undo**, and the depth is a setting.

### Geometry and meshing

- A gmsh surface mesh as the geometry, with corners, curves and surfaces
  recovered from it, and a bounding box built round the whole model or round
  the surfaces you choose.
- **Classification** by proximity for nodes and by inference for edges and
  faces, projection onto whatever a cell is pinned to, and nodes frozen
  against being moved at all.
- **Transfinite meshing** of every block — Coons patches, then Gordon volumes,
  each projected before the level above reads it — with the segment count a
  property of the sheet, and export as a VTK legacy file.

### Files

- In a terminal, a name is a name. In a page **drop the file on the window**
  or open it with the toolbar, and a script opens it by name from then on: a
  browser cannot be given a path, so the file goes the other way.
- `gecko.files()` says what can be opened by name wherever the script runs.

### The interface

- Four views at once (3D, X, Y, Z), a lasso over the node handles, drag
  several nodes together, and every operation on the keyboard as well as in
  the toolbar.
- Colours, shrink, visibility and the geometry's groups all adjustable and
  remembered; a picture saved at the size you ask for.
