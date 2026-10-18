# Faces on their own

A blocking of a volume can start as a blocking of its **surface**:
quadrangles laid on the geometry, cut into a grid, and then extruded into
blocks. So a face has to be able to exist without a block around it.

One does. It is drawn, picked, cut, classified and undone like anything
else, and the only thing it cannot do is be mistaken for a block.

## A patch is a connected component of faces

Faces welded edge to edge make a **patch**. That is not a list anybody
keeps: in the map a face is welded to its neighbour by β₂, and β₂ joins the
faces *of one volume*, so a connected component of faces already **is** one
volume — with a name of its own, which a pick carries like any other.

What tells a patch from a block is whether the shell closes. A block has a
neighbour across every edge of every one of its faces; a patch has a
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
