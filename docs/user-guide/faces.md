# Faces on their own

A blocking of a volume can start as a blocking of its **surface**:
quadrangles laid on the geometry, cut into a grid, and then extruded into
blocks. So a face has to be able to exist without a block around it.

One does. It is drawn, picked, cut, classified and undone like anything
else, and the only thing it cannot do is be mistaken for a block.

## A patch is a connected component of faces

Faces welded edge to edge make a **patch**. That is not a list anybody
keeps: in the map a face is welded to its neighbor by β₂, and β₂ joins the
faces *of one volume*, so a connected component of faces already **is** one
volume — with a name of its own, which a pick carries like any other.

What tells a patch from a block is whether the shell closes. A block has a
neighbor across every edge of every one of its faces; a patch has a
boundary, which is exactly where that is missing. Nothing is flagged and
nothing is declared: six faces welded into a closed box are a block, because
they are one.

The status line says which you have picked, the drawer counts them apart,
and the operations that need an inside — a chord, a pillow — say so rather
than doing something indefensible.

!!! note "A patch is not meshed"
    `E` discretises blocks. A patch has no inside to fill, so it is passed
    over in silence; extrude it first.

## Laying a quadrangle on a surface

Press `M` to aim at the geometry, click a surface, and press `Q`.

The four corners of the quadrangle are the geometric corners of that
surface's boundary **whose interior angle is nearest a right angle**, taken
in boundary order. A corner the boundary barely turns at is a point along a
side, not a corner of a block; one that turns through two right angles is a
spike nobody wants a block corner on.

The angle is measured on the triangles — the interior angle at a node is the
sum of the angles its surface's triangles make there — because a faceted
curve has no derivative worth the name.

What comes back is already classified:

- its **corners** on the geometric corners;
- its **face** on the surface;
- each of its four **sides** on the curves between two chosen corners.

```lua
gecko.face_on_surface(1)
```

A surface with fewer than four corners is refused: there is nothing to
choose from, and inventing a corner out of the facets would be inventing
geometry. For a cylinder, box the geometry as before.

### Sides: a curve made of curves

A side between two chosen corners often runs along **several** curves —
through the corners that were not chosen. A cell is classified on one
entity, so such a side is itself one entity: a **composite**, named once
over the curves it is made of.

It behaves like any other curve. A node on it slides along the whole of it,
across the junctions, and the mesher follows it the same way. The status
line prints it as `side 1 (curves 11, 12)` rather than by its own number,
which is an implementation detail.

Three things are worth knowing about them:

- **A curve belongs to as many sides as there are sides using it.** Two
  surfaces that touch share boundary curves, so this is the usual case, not
  a corner one. Sides overlap, nest and repeat, and nothing partitions
  anything.
- **The same run of curves is always the same side.** Asking twice gives one
  name, which is what makes the operation repeatable and a script replayable.
- **A side is never guessed at.** Proximity does not return one, and
  inference from a cell's boundary does not reach for one — the diagonal of a
  face has the same two ends as the side beside it. A cell *keeps* a side it
  was deliberately put on, and inherits one by being cut from a cell that
  was, and that is all.

The same machinery takes surfaces rather than curves, for a face of a
blocking that spans several geometric surfaces. Nothing more is needed when
that day comes: it is one notion with a dimension on it.

## Taking one away

Pick a face and press `Del`.

One key, two meanings, and the kernel says which: a face of a **patch** is a
face, and a face of a **block** is how you point at the block. The status
line says which you are holding before you press anything.

```lua
gecko.remove_face(id)
```

Whatever the face was welded to keeps its own cells — they are split off on
the way out, not deleted with it — so a face taken from the middle of a
patch leaves the ones around it whole, with the edges that were shared back
on their own boundary and free to be welded to something else.

## Raising it into blocks

Drag a face of a patch, as you would drag a face of a block to grow one —
and **the whole patch comes up**, one block per face.

```lua
gecko.extrude_patch(face, 0.5)
```

**Either way.** Drag towards yourself and the blocks go that way instead.
A sheet has no inside, so neither of its sides is the wrong one — and which
way its quadrangles are wound came from the triangles of the surface they
were laid on, which is nothing anybody can read off the screen. A face of a
**block** is the other case, and grows one way only: the other side of it
is the block.

All of it or none of it, and that is not a convenience. β₂ joins the faces
of one volume, so a block welded to an open sheet of faces would have to be
the same volume as the sheet and a different one at once: half a patch
cannot be raised.

Three things make the blocks fit together:

- **One step per node**, not per face. A node moves along the mitre of the
  normals of the faces meeting there, so two blocks raised from two faces
  that share an edge raise that edge to the *same* two corners instead of to
  two pairs that happen to be close. Over a fold, the ridge rises straight
  up the fold's own plane of symmetry.
- **A glue along every shared edge**, so the blocks end up sharing the face
  that edge was raised into. What comes out is one object, not a heap of
  boxes: two faces give two blocks with twelve corners between them, not
  sixteen.
- **The patch is consumed.** Its faces are the bottoms of the blocks now,
  carrying what they were classified on — the surface, the sides, the
  corners, and any freeze — which is what makes the blocking go on
  following the geometry once it is a volume.

From there it is an ordinary blocking: cut its sheets, pillow it, mesh it.

## Pillowing, and what a node keeps

Pillowing draws the blocks behind a patch of faces inwards and lays a layer
into the space. The nodes that move **let go of the geometry**: a node
inside the material pinned to a curve it is no longer near drags the mesh
out through the boundary. The copy the layer leaves on the surface takes
what it was following, freeze included.

A node on the **rim** of the pillowed patch is the exception that matters
here: it has often only slid *along* what it was on, and it is still on it.
So the operation hands back what each node was following, and the settling
keeps a **side** that still holds — proximity cannot answer "side", since a
composite is never a candidate, and narrowing a node to whichever of the
side's curves it now happens to be over would lose what somebody
deliberately put there. Between two entities of the model, proximity's
answer is the better one and is taken: a corner drawn in off the rim of a
disc really has left the rim.

## Cutting a sheet of faces

Exactly as for blocks: drag along an edge, and the whole sheet is cut. In
two dimensions the sheet through an edge is that edge, the one opposite, and
on through whatever is welded there — and cutting it leaves two faces welded
along the cut, still one patch.

```lua
gecko.split_sheet(edge, 0.5)
```

A node the cut makes inherits what the edge it came from was on, which is
how a cut along a side leaves its new node on that side rather than on one
of its curves.
