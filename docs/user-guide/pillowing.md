# Pillowing

Pillowing lays a layer of hexahedra along a patch of the model's surface.
The blocks behind the patch are drawn inwards and the layer fills the space
that opens up, so **the model's surface does not move** — which is the whole
point of it.

## Doing it

Click the faces you want, then the pillow button, or `P`. Every selected face
is lit, so what the layer will cover is visible before it exists.

To put a layer all the way along a **chord** instead of round a patch you
picked by hand, see [Chords](chords.md): one click, `⇧P`, and the o-grid runs
the length of the thread.

The selection has no limit. A glue wants exactly two faces; a pillow takes as
many as are picked, and clicking a face again takes it back out.

Each selected face becomes one new block. One face on a cube gives two blocks
— which is the same thing a sheet cut gives, and on a single face it is the
same operation. All six faces give seven: the cube, drawn in, inside a shell
of six slabs welded to each other.

From a script, with the `{face, block}` pairs from `gecko.faces()`:

```lua
gecko.create_cube()
local patch = {}
for _, face in ipairs(gecko.faces()) do
  patch[#patch + 1] = { face.id, face.block }
end
gecko.pillow(patch)
gecko.stats()   -- {nodes = 16, edges = 32, faces = 24, blocks = 7}
```

## How thick

A third of the cell it is cut from, measured through the block from its
center to the face and back. No setting: a layer you have to dial in before
you can see it is a layer you dial in twice.

Where a node sits on several selected faces at once, the copy goes to the
corner of the two — a **mitre**, as in a picture frame, so that it ends up
that third clear of every face it is on. Neither obvious answer does that:
adding the face normals sends a node in the middle of a flat patch four times
too far, and averaging them leaves a node on a corner short of both its
faces.

## What happens to the classification

The layer leaves a copy of each node exactly where it was, and draws the
original in. **The copy is the one on the geometry**, so it is the copy that
keeps what the node was classified on — and its freeze with it, a frozen
node having been frozen where it stood rather than wherever it is pushed to.

The node drawn in is somewhere new, so where it is is decided again, by
proximity, within the **snap** of the settings panel. It is not simply
freed: the corner of a box drawn in off the rim of a disc is still sitting
*on* that disc, and the corner of a box drawn in along an upright of the
model slides down that upright and is still on it. Only the nodes that
moved are looked at again, so nothing you classified by hand elsewhere is
disturbed.

Getting this backwards is worth describing, because it is invisible until
you mesh: a node pinned to a curve it is no longer anywhere near drags the
mesh along that curve out through the boundary, and the cells come out
inside out.

## Inside the model

A face with a block on each side works too, and the layer goes between them.
Which side draws in is the one you clicked on; the other does not move.

That case needs one thing the surface case does not: the two blocks have to
stop sharing their nodes first. A node that is merely *moved* is still
shared, so drawing one block in would drag the other with it and leave no
space at all. Unsewing the face is exactly that — the map splits the shared
vertices and each side comes away with its own copy — and it is why the
operation does the unsewing for the whole patch before it reads anything
again: two blocks joined along several faces keep sharing their nodes until
the last of those is undone.

Two blocks joined along **another** face as well as the one you picked are
refused, for the same reason: the nodes stay shared and there is nowhere for
the layer to go.

## What it will not do

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
