# gecko-rs

A Rust port of [GECKO](https://github.com/franck-ledoux/gecko), rebuilt step by
step.

The block structure sits on a **3-dimensional combinatorial map**
([`honeycomb-core`](https://github.com/LIHPC-Computational-Geometry/honeycomb)),
the Rust counterpart of what `CGAL::Combinatorial_map` gives the C++ version:
darts, attributes at every dimension, merge and split functors.

One kernel, reachable two ways.

## Static mode

Files you drop on a web site. The page carries the kernel, compiled to wasm.
No server, no Python, nothing to install.

```bash
./build-wasm.sh
cd front && npm install && npm run dev
```

See [Static mode](user-guide/static.md).

## Station mode

A local Python module holds the kernel and serves it to a browser. The console
and the mouse drive the same live object, in both directions.

```python
import gecko

s = gecko.Session()
s.create_cube([0, 0, 0], 1.0)
s.show()                        # opens a tab; the view follows the console

n = s.nodes[0]                  # {"id": …, "pos": [x, y, z]}
s.move_node(n["id"], [0.5, -0.2, 0])
```

See [Station mode](user-guide/station.md) and the [Python API](user-guide/python.md).

## What it can do today

Create a cube, and move a node — from Rust, from Python, and from the page,
where nodes are dragged with the mouse.

That is deliberately little. The project is being rebuilt one step at a time,
and each step settles a design question before the next is piled on top. What
is not there yet: undo, geometric validity, classification, Bézier curves and
Coons surfaces, any other topological operation (sewing, sheet cutting,
deletion, pillowing), meshing, file reading.

## Where to look next

- [Architecture](developer-guide/architecture.md) — why there is one door into
  the kernel, and what that buys.
- [Building](developer-guide/building.md) — the toolchain and the one command
  that runs everything.
- [Testing](developer-guide/testing.md) — four tiers, and what belongs in each.

## Licence

AGPL-3.0-only, like GECKO. Section 13 is not decoration here: the station
**serves the interface over HTTP**, so anyone deploying a modified version owes
its source to the people connecting to it.
