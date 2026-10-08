# Station mode

A Python process holds the kernel; a browser tab is a view of it. The link is
two-way: what the console does shows up in the page, and what the mouse does is
visible to the console. There is only one session.

## Installing it

You need **Rust** (stable), **Node** 20 or later, and **Python** 3.9 or later.
Nothing else: the scripts fetch what they are missing.

```bash
git clone git@github.com:franck-ledoux/gecko-rs.git
cd gecko-rs

./build-wasm.sh                                     # the kernel, as wasm
cd front && npm install && npm run build && cd ..   # the viewer bundle

python3 -m venv .venv
source .venv/bin/activate
pip install ./crates/gecko-py
```

!!! note "Why the first two steps come first"
    The wheel **embeds** the viewer, and the viewer **imports** the wasm
    kernel. Install the package on its own and it works, but `show()` serves a
    message telling you to come back and build them. See
    [Building](../developer-guide/building.md) for the full dependency order.

`./check.sh` does all of this — and runs every test — leaving a ready `.venv`.

When iterating on the Rust, reinstall with `maturin develop --release` from
`crates/gecko-py` rather than with pip.

## Starting a station

```python
import gecko

s = gecko.Session()
s.create_cube([0, 0, 0], 1.0)
print(s.show())            # http://127.0.0.1:52431/ — and opens a tab
```

`show()` starts a small server and returns the address. It is idempotent:
calling it again hands back the same address rather than starting a second
server on the same session.

## What makes it one object

The session lives in an `Arc<Mutex<…>>` shared by the Python handle and the
server's tasks. Nothing is copied and nothing is synchronised after the fact —
the console and the browser take the same lock on the same state.

So this works in either order:

```python
s.create_cube([0, 0, 0], 1.0)      # the open tab redraws, unasked
n = s.nodes[0]
s.move_node(n["id"], [2, 0, 0])    # so does this
```

and dragging that node in the browser changes what `s.nodes` reports.

Several tabs on one station all see each other's work. A change carries its
origin, and a tab ignores its own, so it does not redraw twice for its own
click.

## The port

`show()` asks the operating system for a free port. Hard-coding one is how a
second session fails to start, or silently attaches to the first.

To pin it anyway — for an SSH tunnel, say:

```python
s.show(port=8731, open=False)
```

`open=False` keeps it from launching a browser, which is what you want on a
headless machine. Tunnel the port and open the address locally.

## Notebooks

`show()` works from a notebook: it starts the server and opens a tab, and the
view keeps up with the cells you run. The view is a separate tab, not an output
cell — a widget that renders inline is a different piece of plumbing and is not
built yet.

## If the page says the bundle is missing

The viewer is embedded in the wheel at build time. A package built without it
serves an explanatory message instead of a blank page:

```bash
./build-wasm.sh
cd front && npm install && npm run build && cd ..
pip install ./crates/gecko-py        # or: maturin develop --release
```

The first line is not optional: the bundle imports the wasm kernel, so building
it without that step fails on a missing module. See
[Building](../developer-guide/building.md) for the full dependency order.
