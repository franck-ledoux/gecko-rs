# Python API

One class. Every method goes through the same door as the browser, so anything
a script does here is something the page could have done, and the other way
round.

```bash
uv pip install ./crates/gecko-py
```

See [Station mode](station.md#installing-it) for the two build steps that come
first.

## `gecko.Session`

### Creating blocks

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
{'id': 0, 'pos': [0.0, 0.0, 0.0], 'on': None, 'frozen': False}
```

`on` is what the node is [classified](classification.md) on, and `frozen`
whether it may move at all.

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

#### `glue_faces(first, second)`

Weld two blocks along a face. `first` stays put and `second` is brought onto
it, so the order decides where the seam lands. Only that face moves — see
[Joining and removing blocks](gluing.md).

```python
s.create_cube([0, 0, 0], 1.0)
s.create_cube([1.5, 0, 0], 1.0)
seam = [f for f in s.faces if abs(f["centre"][0] - 1.0) < 1e-9
        or abs(f["centre"][0] - 1.5) < 1e-9]
s.glue_faces(seam[0]["id"], seam[1]["id"])
s.stats    # {'nodes': 12, 'edges': 20, 'faces': 11, 'blocks': 2}
```

#### `extrude_face(face, block, height)`

Grow a new block out of a face, along the normal at its centre. The face does
not move; the new block is laid against it and welded on. A face inside the
model is refused.

```python
>>> top = max(s.faces, key=lambda f: f["centre"][2])
>>> s.extrude_face(top["id"], top["block"], 2.0)
'block CellId(1) grown from face CellId(5) by 2.000 — 12 nodes, 20 edges, …'
```

#### `remove_block(block)`

Take one block away. Whatever it was glued to keeps its own cells: the shared
ones are split off on the way out rather than going with it.

```python
s.remove_block(s.blocks[0]["id"])
```

### The geometry

#### `load_geometry(path)`

Read a gmsh surface mesh — MSH 2.2 ASCII — as the geometry the blocks are
meant to follow. Returns a line saying what was in it. See
[Geometry](geometry.md) for what the file has to say and how it is drawn.

```python
>>> s.load_geometry("two_cubes.msh")
'two_cubes.msh loaded — 12 corners, 20 curves, 11 surfaces (286 triangles),
 368 elements of other kinds ignored'
```

The path is read here and the **contents** go into the command, because the
command set has to be the one the browser can send too and a browser has no
path to give.

A file that is not a mesh, is not version 2.x, has no triangles, or whose
triangles carry no entity tag raises `ValueError` with a message that says
which.

#### `box_geometry(surfaces=None, margin=0.1)`

Add one block: a box round the geometry, or round the surfaces whose gmsh
tags are given. `margin` grows each half-side by that share.

```python
>>> s.box_geometry()
'block CellId(0) boxes the whole geometry — 8 nodes, 12 edges, 6 faces, …'
>>> s.box_geometry(surfaces=[3, 5], margin=0)
```

A flat selection, an unknown tag, a margin of −1 or less, or no geometry at
all raises `ValueError` saying which.

#### `classify(tolerance=0.02)`

Put every node on the geometric entity nearest it, and every edge on what
its two ends agree about. The tolerance is a share of the geometry's own
size. See [Classification](classification.md).

```python
>>> s.classify()
'classified 8 nodes and 12 edges onto the geometry'
```

#### `classify_node(node, dim, tag)` and `unclassify_node(node)`

Put one node on an entity by hand — `dim` 0 a corner, 1 a curve, 2 a surface
— or take it off one. Classifying pulls the node onto it; unclassifying
leaves it where it is.

#### `freeze_node(node)` and `thaw_node(node)`

Stop a node moving, or let it move again. A frozen node is refused to every
move and left out of `classify()`.

#### `clear_geometry()`

Forget it. The blocks are untouched either way.

#### `geometry_buffer`

The geometry's bytes, as the browser decodes them — a buffer of its own,
separate from `view_buffer`, because the two change on completely different
clocks.

### Looking at the session

#### `edges`

Every edge, by stable name, with its two ends and what it is classified on.

```python
>>> s.edges[0]
{'id': 0, 'nodes': [0, 1], 'on': {'dim': 1, 'tag': 205}}
```

#### `faces` and `blocks`

What a script names to `glue_faces` and `remove_block`, since it has no mouse.

```python
>>> s.faces[0]
{'id': 0, 'block': 0, 'centre': [0.5, 0.5, 0.0]}
>>> s.blocks[0]
{'id': 0, 'centre': [0.5, 0.5, 0.5]}
```

A face with a block on each side is listed once per block, as the drawn
surface is.

#### `stats`

```python
>>> s.stats
{'nodes': 8, 'edges': 12, 'faces': 6, 'blocks': 1,
 'geometry': {'generation': 0}}
```

Cells exist at all four dimensions, which is the point of the representation:
only a cell that exists can carry a curve, a surface, or a classification.

`geometry` reports which geometry is loaded, and `generation` counts loads
rather than describing one: a client holds the number it last drew and
re-fetches when it differs. Two geometries of identical size are still two
geometries, which is what comparing the counts would miss. With one loaded it
also carries `corners`, `curves`, `surfaces`, `segments` and `triangles`.

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
