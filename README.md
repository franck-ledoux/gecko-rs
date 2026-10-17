# gecko-rs

[![Build & Test](https://github.com/franck-ledoux/gecko-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/franck-ledoux/gecko-rs/actions/workflows/ci.yml)

A Rust port of [GECKO](https://github.com/franck-ledoux/gecko), rebuilt step by
step.

The block structure sits on a **3-dimensional combinatorial map**
([`honeycomb-core`](https://github.com/LIHPC-Computational-Geometry/honeycomb)),
the Rust counterpart of what `CGAL::Combinatorial_map` gives the C++ version:
darts, attributes at every dimension, merge and split functors.

One kernel, in one place: **the page**. It carries the kernel compiled to wasm
and a Lua 5.4 interpreter beside it, so the mouse and a script drive the same
session with nothing in between.

## Three commands

```bash
./setup     # once per machine: toolchains, the Emscripten SDK, node packages
./build     # the kernel, the interpreter, the page, the binary
./gecko     # serve the page and open it
```

`./setup` is safe to run again — each step looks before it leaps. `./build`
has to be run again whenever the Rust or the TypeScript changes.

You need **Rust** (stable) and **Node** 20 or later before `./setup`; it
fetches everything else, including the Emscripten SDK into `~/emsdk` — about a
gigabyte, once, and only because mlua compiles to WebAssembly that way.

To have it anywhere rather than from this clone:

```bash
cargo install --path crates/gecko-cli
gecko                      # in a folder of meshes
gecko run sweep.lua        # no browser at all
gecko --help
```

**The folder matters.** `gecko` lends the page the files of the directory it
was started in, so start it where your `.msh` files are.

## Scripting it

`;` opens the console. It is a real interpreter, so loops and conditionals
drive the model the same way the mouse does:

```lua
gecko.load_geometry("cylinder.msh")
gecko.box_geometry{ margin = 0 }

-- The four corners of each end, onto the rim they are meant to follow.
for _, rim in ipairs({ { z = 0, tag = 201 }, { z = 2, tag = 202 } }) do
  local four = {}
  for _, node in ipairs(gecko.nodes()) do
    if math.abs(node.pos[3] - rim.z) < 0.5 then four[#four + 1] = node.id end
  end
  gecko.classify_node(four, 1, rim.tag)
end

gecko.mesh{ segments = 8 }
gecko.export_vtk("mesh.vtk")
```

**Every command the interface sends is written in the console as Lua**, in
order, beside what you typed — so the history is one list whichever door you
used. `gecko.script()` reads it back and **save** writes it out as a `.lua`
you can run again, with `gecko run` or by pasting it in.

The same script runs in both places, and a test checks the two hosts produce
the same bytes. See [Scripting](docs/user-guide/scripting.md).

## Working on it

```bash
./dev           # the page from Vite with hot reload, a file server behind it
./view          # the page alone, shorter loop
./check.sh      # format, lint, every suite, the page in a real browser
./docs.sh       # the documentation site, into site/
```

`check.sh` bootstraps what it can, and **CI runs that same script**, so what
passes locally passes there.

| Tier               | Tool                     | Time   |
| ------------------ | ------------------------ | ------ |
| Kernel and session | `cargo test`             | ~0.3 s |
| The scripting API  | Lua scripts, run by Rust | ~0.2 s |
| Decode and scene   | Vitest + `NullEngine`    | ~0.6 s |
| The whole page     | Playwright + SwiftShader | ~35 s  |

## What it can do today

- **build**: a cube, a box round a geometry, a block grown out of a face;
- **edit**: glue, remove, cut a sheet, remove a sheet and close the gap,
  pillow a patch — with five steps of undo, settable;
- **read a geometry**: a gmsh surface mesh (MSH 2.2) as a boundary
  representation — surfaces, curves and corners, drawn beside the blocks;
- **classify**: nodes onto corners, curves and surfaces, by hand or by
  proximity; edges and faces inferred from them; sliding, freezing, and a
  band to catch a handful of handles at once;
- **mesh**: every block discretised by transfinite interpolation, following
  the geometry its cells are classified on, written out as VTK legacy;
- **look at it**: every group shown, coloured, faded and shrunk on its own,
  one view or four, with a picture saved at publication size;
- **script**: all of the above, in Lua, in the page or from a terminal.

Not there yet: geometric validity, Bézier curves and Coons surfaces for the
blocking itself, grading along an edge.

## Documentation

`docs/`, built with MkDocs — start at [`docs/index.md`](docs/index.md), or run
`./docs.sh` and open `site/index.html`. MkDocs is a Python tool; that is a
documentation toolchain, not a dependency of anything that ships.

The [architecture notes](docs/developer-guide/architecture.md) are the ones
worth reading first: why there is a single door into the kernel, why Lua rides
in a second wasm module, and what the view buffer promises.

## Licence

AGPL-3.0-only, like GECKO — see [LICENSE](LICENSE). Dependencies are MIT or
Apache-2.0, so compatible.

Section 13 is not decoration here: `gecko` **serves the interface over HTTP**,
so anyone deploying a modified version owes its source to the people
connecting to it.
