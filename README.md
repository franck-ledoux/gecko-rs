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

You need **Rust** (stable), **Node** 20 or later, and **Python** 3.9 or later.
Nothing else: the scripts fetch what they are missing.

```bash
git clone git@github.com:franck-ledoux/gecko-rs.git
cd gecko-rs

./build-wasm.sh                       # the kernel, compiled to wasm
cd front && npm install && npm run build && cd ..   # the viewer bundle

python3 -m venv .venv
source .venv/bin/activate
pip install ./crates/gecko-py
```

**The order matters.** The wheel *embeds* the viewer, and the viewer *imports*
the wasm kernel. Install the package on its own and it works, but `show()`
serves a message telling you to come back and build them.

### Using it

```bash
python
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
./check.sh && source .venv/bin/activate && python
```

When iterating on the Rust, reinstall with maturin rather than pip; it is the
same build, without re-resolving the wheel:

```bash
cd crates/gecko-py && maturin develop --release
```

## Building and testing

```bash
./check.sh      # format, lint, every suite, both modes in a real browser
./docs.sh       # the documentation site, into site/
```

`check.sh` bootstraps itself — virtual environment, `npm install`, the `wasm32`
target — so a fresh clone needs nothing but Rust, Node and Python. **CI runs
that same script**, so what passes locally passes there.

| Tier                                                  | Tool                     | Time    |
| ----------------------------------------------------- | ------------------------ | ------- |
| Kernel and session                                    | `cargo test`             | ~0.05 s |
| Decode, scene, transport                              | Vitest + `NullEngine`    | ~0.3 s  |
| Both modes, in a browser                              | Playwright + SwiftShader | ~7 s    |
| Python façade, and its agreement with the static mode | pytest                   | ~0.05 s |

## What it can do today

Create a cube, and move a node — from Rust, from Python, and from the page.

That is deliberately little: the project is being rebuilt one step at a time,
and each step settles a design question before the next is piled on top. Not
there yet: undo, geometric validity, classification, Bézier curves and Coons
surfaces, any other topological operation, meshing, file reading.

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
