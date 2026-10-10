# gecko-rs

[![Build & Test](https://github.com/franck-ledoux/gecko-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/franck-ledoux/gecko-rs/actions/workflows/ci.yml)

A Rust port of [GECKO](https://github.com/franck-ledoux/gecko), rebuilt step by
step.

The block structure sits on a **3-dimensional combinatorial map**
([`honeycomb-core`](https://github.com/LIHPC-Computational-Geometry/honeycomb)),
the Rust counterpart of what `CGAL::Combinatorial_map` gives the C++ version:
darts, attributes at every dimension, merge and split functors.

One kernel, reachable two ways.

## Static mode

Files you drop on a web site. The page carries the kernel, compiled to wasm. No
server, no Python, nothing to install.

```bash
./build-wasm.sh
cd front && npm install && npm run dev
```

## Station mode

A local Python module holds the kernel and serves it to a browser. The console
and the mouse drive the same live object, in both directions.

### Installing it

You need **Rust** (stable), **Node** 20 or later, and
[**uv**](https://docs.astral.sh/uv/getting-started/installation/). Nothing
else — uv brings its own Python, and the scripts fetch what they are missing.

```bash
git clone git@github.com:franck-ledoux/gecko-rs.git
cd gecko-rs

./build-wasm.sh                       # the kernel, compiled to wasm
cd front && npm install && npm run build && cd ..   # the viewer bundle

uv venv
uv pip install ./crates/gecko-py
```

**The order matters.** The wheel *embeds* the viewer, and the viewer *imports*
the wasm kernel. Install the package on its own and it works, but `show()`
serves a message telling you to come back and build them.

### Using it

```bash
uv run python
```

```python
>>> import gecko
>>> s = gecko.Session()
>>> s.create_cube([0, 0, 0], 1.0)
'block CellId(0) added — 8 nodes, 12 edges, 6 faces, 1 blocks'
>>> s.show()                    # opens a tab; the view follows the console
'http://127.0.0.1:52431/'

>>> s.create_cube([2, 0, 0], 0.5)   # the open tab redraws, unasked
>>> n = s.nodes[0]                  # {"id": …, "pos": [x, y, z]}
>>> s.move_node(n["id"], [0.5, -0.2, 0])
```

In the tab, nodes are **dragged with the mouse**, and `s.nodes` reports where
they ended up — it is the same object, not a copy kept in step.

Scripts and notebooks work the same way. On a headless machine, use
`s.show(port=8731, open=False)` and tunnel the port.

### Shortcuts

`./check.sh` does the whole sequence above — and runs every test — leaving a
ready-to-use `.venv`:

```bash
./check.sh && uv run python
```

When iterating on the Rust, reinstall with maturin rather than pip; it is the
same build, without re-resolving the wheel:

```bash
cd crates/gecko-py && maturin develop --release
```

## Working on it

```bash
./dev           # a Python console, a browser watching it, hot reload
./dev notebook  # the same, with JupyterLab instead of a console
./view          # the viewer alone, static mode, no Python
```

`./dev` is the whole loop in one command: it rebuilds what changed, starts a
Python session — empty, like the page — opens the view, and leaves you at a
prompt with that session as `s`. Editing TypeScript reloads the page **without
losing the session** — the page is served by the dev server, which forwards the
socket to the Python process behind it.

```bash
./check.sh      # format, lint, every suite, both modes in a real browser
./docs.sh       # the documentation site, into site/
```

`check.sh` bootstraps itself — virtual environment, `npm install`, the `wasm32`
target — so a fresh clone needs nothing but Rust, Node and uv. **CI runs
that same script**, so what passes locally passes there.

| Tier                                                  | Tool                     | Time    |
| ----------------------------------------------------- | ------------------------ | ------- |
| Kernel and session                                    | `cargo test`             | ~0.05 s |
| Decode, scene, transport                              | Vitest + `NullEngine`    | ~0.3 s  |
| Both modes, in a browser                              | Playwright + SwiftShader | ~7 s    |
| Python façade, and its agreement with the static mode | pytest                   | ~0.05 s |

## What it can do today

Every one of these from Rust, from Python, and from the page — one command set,
three ways in:

- **build**: create a cube, move a node, glue two faces together;
- **edit**: remove a block, cut a sheet of hexahedra, remove a sheet and close
  the gap behind it, pillow a patch of surface faces;
- **read a geometry**: a gmsh surface mesh (MSH 2.2) as a boundary
  representation — surfaces, curves and corners, drawn alongside the blocks;
- **look at it**: faces, blocks, edges and the geometry's three groups, each
  shown, coloured, faded and shrunk on its own, with a picture saved at
  publication size.

The project is being rebuilt one step at a time, and each step settles a design
question before the next is piled on top. Not there yet: **classification** —
the blocks and the geometry are two things in one window, and no cell knows
which surface it is supposed to lie on — undo, geometric validity, Bézier
curves and Coons surfaces, meshing.

## Documentation

`docs/`, built with MkDocs — start at [`docs/index.md`](docs/index.md), or run
`./docs.sh` and open `site/index.html`.

The [architecture notes](docs/developer-guide/architecture.md) are the ones
worth reading first: why there is a single door into the kernel, why a cell
needs two different identities, and what the view buffer promises.

## Licence

AGPL-3.0-only, like GECKO — see [LICENSE](LICENSE). Dependencies are MIT or
Apache-2.0, so compatible.

Section 13 is not decoration here: the station **serves the interface over
HTTP**, so anyone deploying a modified version owes its source to the people
connecting to it.
