# Changelog

Versions are what a reader can download, and this is what they get. Dates are
the day a tag was cut.

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
