# gecko-rs

### [▶ Open gecko in your browser](https://franck-ledoux.github.io/gecko-rs/app/)

Nothing to install: the page carries the kernel and a Lua interpreter, and
runs on a static host. Everything works there except the filesystem — a
script that reads a file is told there is none, and one that writes gets a
download.

______________________________________________________________________

A Rust port of [GECKO](https://github.com/franck-ledoux/gecko), rebuilt step by
step.

The block structure sits on a **3-dimensional combinatorial map**
([`honeycomb-core`](https://github.com/LIHPC-Computational-Geometry/honeycomb)),
the Rust counterpart of what `CGAL::Combinatorial_map` gives the C++ version:
darts, attributes at every dimension, merge and split functors.

One kernel, in one place: **the page**. It carries the kernel compiled to
wasm and a Lua 5.4 interpreter beside it, so the mouse and a script drive the
same session with nothing between them and no server anywhere.

```bash
./setup     # once: toolchains, emsdk, node packages
./build     # the kernel, the interpreter, the page, the binary
./gecko     # open it
```

That is for working on it. To **use** it, there is nothing to build:
[open it in your browser](https://franck-ledoux.github.io/gecko-rs/app/).

```lua
gecko.load_geometry("cylinder.msh")
gecko.box_geometry{ margin = 0 }

for _, node in ipairs(gecko.nodes()) do
  if node.pos[3] < 0.5 then
    gecko.classify_node(node.id, 1, 201)
  end
end

gecko.mesh{ segments = 8 }
gecko.export_vtk("mesh.vtk")
```

The same script runs in the console, in the page, or from a terminal with
`gecko run script.lua` — and a test checks the two hosts produce the same
bytes. See [Scripting](user-guide/scripting.md).

## What it can do today

- **build**: a cube, a box round a geometry, a block grown out of a face;
- **edit**: glue, remove, cut a sheet, remove a sheet and close the gap,
  pillow a patch — with five steps of undo, settable;
- **read a geometry**: a gmsh surface mesh (MSH 2.2) as a boundary
  representation — surfaces, curves and corners;
- **classify**: put nodes on corners, curves and surfaces by hand or by
  proximity, slide them along what they follow, freeze them;
- **mesh**: every block discretised by transfinite interpolation, following
  the geometry the cells are classified on, written out as VTK legacy;
- **look at it**: every group shown, colored, faded and shrunk on its own,
  one view or four, with a picture saved at publication size;
- **script**: loops and conditionals over all of it, and the session's own
  history written back out as Lua.

Not there yet: geometric validity, Bézier curves and Coons surfaces for the
blocking itself, grading along an edge.

## Where to look next

- [The interface](user-guide/interface.md) — the toolbar, the keys, the panels.
- [Scripting](user-guide/scripting.md) — the console, the API, `gecko run`.
- [Architecture](developer-guide/architecture.md) — why there is one door into
  the kernel, and what that buys.
- [Building](developer-guide/building.md) — the toolchain and the one command
  that runs everything.
- [Testing](developer-guide/testing.md) — the tiers, and what belongs in each.

## Licence

AGPL-3.0-only, like GECKO. Section 13 is not decoration here: `gecko`
**serves the interface over HTTP**, so anyone deploying a modified version
owes its source to the people connecting to it.
