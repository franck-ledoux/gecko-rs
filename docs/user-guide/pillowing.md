# Pillowing

Pillowing lays a layer of hexahedra along a patch of the model's surface.
The blocks behind the patch are drawn inwards and the layer fills the space
that opens up, so **the model's surface does not move** — which is the whole
point of it.

## Doing it

Click the faces you want, then the pillow button, or `P`. Every selected face
is lit, so what the layer will cover is visible before it exists.

The selection has no limit. A glue wants exactly two faces; a pillow takes as
many as are picked, and clicking a face again takes it back out.

Each selected face becomes one new block. One face on a cube gives two blocks
— which is the same thing a sheet cut gives, and on a single face it is the
same operation. All six faces give seven: the cube, drawn in, inside a shell
of six slabs welded to each other.

From Python, with the `(face, block)` pairs from `s.faces`:

```python
s.create_cube([0, 0, 0], 1.0)
s.pillow([(f["id"], f["block"]) for f in s.faces])
s.stats     # {'nodes': 16, 'edges': 32, 'faces': 24, 'blocks': 7}
```

## How thick

A third of the cell it is cut from, measured through the block from its
centre to the face and back. No setting: a layer you have to dial in before
you can see it is a layer you dial in twice.

Where a node sits on several selected faces at once, the copy goes to the
corner of the two — a **mitre**, as in a picture frame, so that it ends up
that third clear of every face it is on. Neither obvious answer does that:
adding the face normals sends a node in the middle of a flat patch four times
too far, and averaging them leaves a node on a corner short of both its
faces.

## What it will not do

**Faces inside the model are refused.** Pillowing between two blocks means
giving each of them its own copy of the shared nodes, and a node that is
merely *moved* is still shared — so there would be nowhere for a layer to go.
That one needs the nodes split, and is not built.

A node on the **rim** of the patch is shared with blocks outside it, and
moving it takes them with it. The mesh stays conforming and those blocks are
deformed a little. That is the honest consequence of a shared node being
shared, and a smoothing pass is the answer to it rather than a special case
here.

!!! note "Why this one is not dart surgery"
    A [sheet cut](sheets.md#cutting) had to rewire the map by hand, because
    nothing in the map library splits a cell. A pillow does not: moving
    nodes, adding a block and gluing it are three operations that already
    exist and already have tests behind them, and the topology that comes out
    is the same. The surgery would have been the more impressive way to lose
    a day.
