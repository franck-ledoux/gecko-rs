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

## Laying a quadrangle on four points

`Q` takes a whole surface's own corners. `⇧Q` takes four you click, which is
the way to pave a surface whose corners are no use — a disc, a long strip, a
boundary you want five blocks along.

Press `⇧Q` and click four times. Each click puts a corner where the pointer
is:

- **on an existing handle**, if there is one there — which is how a
  quadrangle is laid *against* another and welded to it. The handle reached
  is the one **on the geometry**: a block resting on a surface has a node
  inside the material right behind each of the ones on it, a fraction of a
  pixel away on screen, and that one is never the corner you meant.
- otherwise **on the geometry**, corners before curves before surfaces. That
  order is the tolerance: a corner is a small ball and a curve a thin ribbon,
  so a click only reaches one by being near it.

What is on screen says how far along you are — a mark, then a side, then two,
and at the fourth the quadrangle. `Esc` gives up, `⌫` takes back the last
corner.

```lua
gecko.add_face({ {0,0,0}, {1,0,0}, {1,1,0}, {0,1,0} }, {
  on = { { dim = 0, tag = 3 }, { dim = 1, tag = 8 }, false, false },
  surface = 9,
})
```

The four corners come back **already classified**, from the clicks. A corner
that follows nothing is written `false` rather than left out, so that the
list stays one entry per corner.

**The quadrangle itself**, and its sides, are settled the way everything
else is: the settling that runs after every command carries a
classification up from a cell's boundary, so four corners all on one
surface leave the face on that surface without anybody saying so.

Say it when that cannot work — corners on curves of two different surfaces,
or on nothing. Pick the surface first — `M`, click it — and `⇧Q` lays the
face on it, which the status line repeats back.

!!! note "A face, not a thin block"
    This used to lay a block of small thickness, and that was a mistake worth
    recording. A block has an inside, so laying one by hand means saying
    which side the material is on and how thick it is — and four clicks say
    neither. Laid along a boundary, the winding alternated from one to the
    next, so consecutive blocks ended up on opposite sides of the surface;
    the thickness, a share of each quadrangle's own diagonal, came out
    different for every one of them; and the handles inside the material got
    in the way of the clicks. A sheet has no inside. The height comes later,
    once, for the whole strip.

### Welded to what it was laid against

Lay a second quadrangle on two of the first one's corners and the two share
the side between them. Paving a surface therefore builds **one** patch, which
one drag raises into blocks.

A side is welded when the two points it was laid on were both already nodes
of the model, an edge joins them, and that edge has **no face on its other
side**. Clicking two of a neighbour's handles makes all three true; clicking
the geometry twice at the same places does not, and is not meant to.

There is no question of *which* face to weld to — a side of an open sheet
carries one face, so a free edge between two given points is the answer or
there is none. Where two sheets lie coincident and unwelded there are two
free edges between the same two points, and then the side is left free rather
than welded to a guess.

**Which way round you click does not matter.** β₂ joins two darts running
opposite ways along one edge, so a quadrangle wound the other way from its
neighbour could not be welded to it as it stands — and a sheet has no inside,
so which way round it is says nothing anybody meant. Each side that has a
neighbour votes, and the quadrangle is turned to agree with them. The corners
still come back in the order you clicked them.

The neighbour does not move: the quadrangle being laid is the one that gives
way, and the node and the side that survive keep the neighbour's name, class
and freeze. The status line says how many sides were welded, so one you
expected to join on and did not is visible at once.

### A thin block, in the language

`gecko.add_slab` lays a block of small thickness on four points, welded to
what it is laid against along any side whose two points were already nodes
joined by an edge. It has no key: a lone slab is a reasonable thing to script
and an unreasonable thing to click, for the reasons in the note above.

```lua
gecko.add_slab({ {0,0,0}, {1,0,0}, {1,1,0}, {0,1,0} }, { thickness = -0.1 })
```

The thickness is signed — the sign picks which side the material is on — and
without one it is a tenth of the quadrangle's own shortest diagonal. Which
side of an edge to weld to is a real question for a slab, since its edge
carries both the broad face it was laid on and the thin side: they are told
apart by which way they lean, from the middle of the shared edge toward the
middle of their other two corners.

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
