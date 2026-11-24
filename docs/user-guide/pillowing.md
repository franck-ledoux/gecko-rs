# Pillowing

Pillowing lays a layer of hexahedra along a patch of the model's surface.
The blocks behind the patch are drawn inwards and the layer fills the space
that opens up, so **the model's surface does not move** — which is the whole
point of it.

## Doing it

Click the faces you want, then the pillow button, or `P`. Every selected face
is lit, so what the layer will cover is visible before it exists.

A face between two blocks can be taken from either side, and the two give
different layers. You only say it for the first face: its side is the one
it was clicked from, and it is carried across the rest of the pillow surface
from face to face, so every face takes the same side of it whichever way the
camera was turned when it was clicked. A closed pillow surface given no side
takes its inside.

Or pick **blocks**, on the Blocks layer, and press `P`: the pillow surface is
their boundary, and the layer goes towards them — they are drawn in, and the
layer fills the space between them and the rest. Their faces on the outer
boundary of the model are part of it, so the layer runs along the model's
wall too; untick *pillowing blocks takes their outer faces* in the
**Operations** settings to leave them out, and the pillow surface is then
only the faces between the blocks and the rest. Either way it is checked as
below.

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

A face can be given bare, `gecko.pillow({ 12, {13, 4}, 14 })`, and its block
is found as on the page. Round a set of blocks:

```lua
gecko.pillow_blocks({ 3, 4 })                    -- outer faces too
gecko.pillow_blocks({ 3, 4 }, { outer = false }) -- only between them and the rest
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

## What can be pillowed

The faces you pick, each with the block the layer goes into, form the
**pillow surface**. Whatever the model is classified on, it has to be one
surface, and one that a layer can follow from end to end:

- **Connected.** Its faces hold together through shared edges. Two faces
  that only share a node are two surfaces.
- **No branching.** No edge is shared by more than two of its faces.
- **One side.** Every face takes the same side of it. A closed pillow surface
  — every face of a block, say — takes its inside.
- **Closed, or ending on the outer boundary.** An open pillow surface ends on
  edges of the outer boundary of the block structure. It never stops at an
  edge with blocks all the way round it: a layer cannot end in the middle of
  the material.

Anything else is refused before a node moves, naming the face or the edge at
fault.

## What happens to the classification

The rule is that **pillowing never takes a neighbouring cell off what it is
on**. Only the faces of the pillow surface and the cells the layer creates
change.

The layer leaves a copy of each node exactly where it was, and draws the
original in. **The copy is the one on the geometry**, so it is the copy that
keeps what the node was classified on — and its freeze with it, a frozen
node having been frozen where it stood rather than wherever it is pushed to.

The node drawn in is still attached to the cells beside the pillow surface:
the faces and edges that are not part of it and did not go to the copy. They
say where it may go. On a wall, it slides along that wall; on two walls, along
the curve where they meet; with nothing classified beside it, it is free where
the step put it. It takes what it lands on. Draw in the top of a box and its
four corners slide down the four uprights, each still on its curve, and the
four walls keep their classification.

When the cells beside it leave a node only a point, or leave it no way to
move away from its copy — the step would take it straight off the surface
they are on, or within 10° of that — the pillowing is **refused** and the
node is named. That is the pillow surface that stops in the middle of a wall
of the model: the node at its edge would have to leave the wall, and drag the
faces beside it off with it. A curve between two surfaces that meet within
10° of flat counts as the middle of a wall.

When the blocks behind the pillow surface would pass instead, the refusal
says so and lists their faces: the faces you picked, and every face of those
blocks with another block on its other side. Pillowing those draws the layer
round the blocks, and it comes out on the walls, where its nodes can slide.
On the page those faces are selected for you, so pressing `P` again takes
them; a script reads them from the message.

Getting this wrong is invisible until you mesh: a node pinned to a curve it is
no longer anywhere near drags the mesh along that curve out through the
boundary, and a face taken off its wall leaves a dent in the mesh where the
model is flat.

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

The two sides are told apart **round each node** of the pillow surface, not
round the whole structure. A wall across a ring of blocks — the blocks round a
hole, from the hole's wall to the outer wall — works: the blocks on either
side of it still reach each other the other way round the ring, but at the
wall they are apart. What is refused is two blocks that also meet along
another face **at a node of the pillow surface**: there the node stays
shared, and there is nowhere for the layer to go.

## What it will not do

A node on the edge of the pillow surface is shared with blocks outside it, and
moving it takes them with it. The mesh stays conforming and those blocks are
deformed a little, along the wall the node slides on. That is the honest
consequence of a shared node being shared, and a smoothing pass is the answer
to it rather than a special case here.

!!! note "Why this one is not dart surgery"
    A [sheet cut](sheets.md#cutting) had to rewire the map by hand, because
    nothing in the map library splits a cell. A pillow does not: moving
    nodes, adding a block and gluing it are three operations that already
    exist and already have tests behind them, and the topology that comes out
    is the same. The surgery would have been the more impressive way to lose
    a day.
