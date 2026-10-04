# Python API

One class. Every method goes through the same door as the browser, so anything
a script does here is something the page could have done, and the other way
round.

```bash
pip install ./crates/gecko-py
```

## `gecko.Session`

### Creating geometry

#### `create_cube(origin=[0, 0, 0], size=1.0)`

Add an axis-aligned cube, `origin` being its lowest corner. Returns a
description of what happened.

```python
s.create_cube([0, 0, 0], 1.0)
# 'block CellId(0) added — 8 nodes, 12 edges, 6 faces, 1 blocks'
```

`size` must be strictly positive; `0`, a negative, or a NaN raises `ValueError`
and changes nothing.

#### `add_block(corners)`

Add one hexahedral block from eight corner points, in the usual VTK/Gmsh order:
the bottom face counter-clockwise, then the top face the same way.

```python
s.add_block([[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0],
             [0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]])
```

`create_cube` is exactly this with the corners computed for you — the two
produce byte-identical sessions, which is checked by a test.

### Moving a node

#### `nodes`

Every node, as `{"id": …, "pos": [x, y, z]}`.

```python
>>> s.nodes[0]
{'id': 0, 'pos': [0.0, 0.0, 0.0]}
```

#### `move_node(node, to)`

Move one node. `node` is the **`id` from `nodes`**, not a position in that list.

```python
n = s.nodes[0]
s.move_node(n["id"], [0.5, -0.2, 0.0])
```

!!! warning "Use the id, not the index"
    The list is reordered by edits; the id is not. A name that no node answers
    to raises `ValueError` and changes nothing.

    This is not pedantry — it is why nodes have a stable identity at all. See
    [Architecture](../developer-guide/architecture.md#naming-a-cell).

### Looking at the session

#### `stats`

```python
>>> s.stats
{'nodes': 8, 'edges': 12, 'faces': 6, 'blocks': 1}
```

Cells exist at all four dimensions, which is the point of the representation:
only a cell that exists can carry a curve, a surface, or a classification.

#### `view_buffer`

The bytes the browser decodes — the same ones in either mode. Useful for
offline rendering, and for checking that the two modes agree. The layout is in
[Architecture](../developer-guide/architecture.md#the-view-buffer).

#### `apply(command)`

Send a raw JSON command. The same door the browser uses, exposed so a script
can reach anything the typed methods have not caught up with yet.

```python
s.apply('{"op": "create_cube", "origin": [2, 0, 0], "size": 0.5}')
```

### Showing it in a browser

#### `show(port=0, open=True)`

Serve this session and return the address. See [Station mode](station.md).

#### `url`

The address `show()` is serving on, or `None` if it has not been called.

## Errors

Everything the kernel refuses raises `ValueError` with the kernel's own
message, and leaves the session untouched. There is no partial application: a
command either happens or does not.
