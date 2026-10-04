# Station mode

A Python process holds the kernel; a browser tab is a view of it. The link is
two-way: what the console does shows up in the page, and what the mouse does is
visible to the console. There is only one session.

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
cd front && npm install && npm run build
pip install ./crates/gecko-py        # or: maturin develop --release
```

See [Building](../developer-guide/building.md) for why that ordering matters.
