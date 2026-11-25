# Meshing

A blocking is a coarse description of a shape. **Meshing** turns it into the
thing a solver reads: every block cut into a structured grid of hexahedra,
by transfinite interpolation, following whatever geometry the blocking was
classified on.

|                         |                                                          |
| ----------------------- | -------------------------------------------------------- |
| `E`, or the grid button | discretise every block — and again to take the mesh away |
| `⇧E`                    | write the mesh out, as VTK                               |
| `=` `-`                 | one more segment along the lit sheet, or one fewer       |

## How many segments

A number per **sheet**, not per edge. Every edge of a sheet has to carry the
same number or the mesh does not match across the faces between them, so
setting one sets them all — conformity comes out of where the number lives
rather than out of a check afterwards.

Click an edge to light its sheet; the readout says how many segments it is
in, and `=` and `-` change it. **segments** under Operations in the settings
panel is what a sheet nobody has set gets: ten by default.

Cutting a sheet gives both halves the number the whole had, so a cut doubles
the nodes along that direction — which is what cutting it is for. A sheet
made by gluing two that were set differently answers with the **larger** of
the two, that being the one that can represent what either side was asking
for.

## What transfinite means here

Three passes, by dimension, and each is read by the one above it **after**
its projection:

1. **The edges** are divided into their segments, along the straight line
   between their two ends — and then, if the edge is classified on a curve,
   every point is moved onto that curve.
1. **The faces** are filled from their four edges by a Coons patch: the two
   pairs of opposite sides interpolated, less the corners they both carry.
   A face classified on a surface then has its inside moved onto it.
1. **The blocks** are filled from their six faces the same way, one
   dimension up.

So a face is filled from edges that already lie on their curves, and a block
from faces that already lie on their surfaces. That is what makes the mesh
*follow* the model rather than merely have its shape, and it is why
[classification](classification.md) is worth the trouble.

!!! note "Nodes belong to the cell they lie in"
    Two blocks glued along a face must share the mesh nodes on that face, or
    the mesh has a seam down it that nothing would show until a solver choked
    on it. Rather than mesh each block and weld what lands in the same place
    — which needs a tolerance, and it is always the wrong tolerance for
    somebody's model — every mesh node is **named by the cell of the blocking
    it lies in**: a corner, a point along an edge, a point inside a face, or
    a point inside a block.

    Cells are shared by construction — two glued blocks have one face
    between them, not two — so the sharing of nodes falls out of the
    naming. There is no
    tolerance anywhere in the mesher.

## Looking at it

What is drawn is the mesh's **skin**: the quadrangles belonging to one
hexahedron rather than two. The inside of a mesh is the overwhelming
majority of it and is never seen — a hundred thousand hexahedra are six
hundred thousand quadrangles of which perhaps twenty thousand are on the
outside — so the rest is never sent to the browser at all.

**Mesh** in the display panel folds like any other group: shown or not, one
color or one per block, and how far through it you can see. The element
grid is drawn over it, which is the half of the picture that says what the
discretisation actually did.

A mesh is **dropped the moment the blocking changes**. A mesh of a model you
have since edited is a lie, and a lie that looks right; press `E` again.

`E` on a model that already has one **takes the mesh away**. Two ways of
getting rid of it, and they are not the same: this one throws it out, and
the **show** box in the display panel hides it and keeps it. Before, editing
the blocking was the only thing that got rid of a mesh, which made it
something you could put on screen and not take off.

## How good it is

Every hexahedron is measured by its **scaled Jacobian**: at each corner, the
volume of the three unit edges leaving it — one at a right angle, nought where
the corner has folded flat, below nought where it has folded through — and the
cell's figure is the worst of its eight corners. It is the number solvers and
other meshers quote, so a figure read here can be compared with one read
anywhere else.

Measured on the **mesh**, not on the blocks. A block whose corners are all
square can still hold cells turned inside out, where a face of it follows a
curve that bulges further than the block next to it is thick, and that is
exactly the case worth catching before a solver does.

Meshing says the worst of it straight away:

```text
meshed — 37347 nodes, 33198 hexahedra; worst cell 0.42
meshed — 35034 nodes, 31034 hexahedra; worst cell -1.00, 95 inverted
```

The **Mesh** section of the display panel has the rest:

- the worst cell, the mean, and how many are inverted;
- a histogram, from inverted on the left to perfect on the right — on a
  logarithmic scale, since a good mesh has nearly every cell in the last bin
  and the handful in the first ones are what it is for;
- **show cells below** a threshold: every cell worse than it, drawn whole
  wherever it is — most bad cells are inside the mesh, where its skin never
  shows them — colored by its quality, with the skin made nearly clear around
  them;
- **Go to the worst cell**, which flies the camera there and says which block
  it is in;
- and **quality** among the ways to color the mesh: green for a cube,
  through yellow and orange to red at nought, and a darker red for a cell
  turned inside out.

From a script:

```lua
gecko.mesh{ segments = 10 }
local q = gecko.quality()      -- nil with no mesh
print(q.min, q.mean, q.below["0"], q.below["0.2"])
print(q.worst.block, q.worst.quality, q.worst.center[1])
for _, b in ipairs(q.blocks) do   -- the worst blocks, worst first
  print(b.block, b.worst, b.inverted)
end
```

`q.histogram[i]` counts the cells from `q.bins[i]` up to the next bin's start,
the last running to one.

## How faithfully it follows the geometry

Quality says the cells are well shaped; this says the mesh is the right
**shape**. A mesh can score perfectly by quality with a hole never cut, a
notch left filled or a sharp edge rounded off.

**Check against the geometry**, under the quality figures, measures three
things:

- **the mesh on the geometry**: how far each node of the mesh's skin is from
  the nearest surface. Skin floating off the part — a face classified on
  nothing, a node stranded between two surfaces — shows here. With every
  boundary cell classified it is nought.
- **the geometry on the mesh**: how far the geometry is from the skin, looked
  at all over every surface, about a cell apart. The direction that catches
  what is **missing**: a feature the blocking never made leaves geometry far
  from any mesh.
- **curves and corners**: whether every curve has mesh edges running **along**
  it — within 30° of its direction, not merely the skin passing near — and
  every corner a mesh node on it. That is what aligned with the boundary
  means.

A curve counts if the model still has it as a curve: itself, or a merge of
curves. A seam a merge of surfaces took inside it, such as the joint of a
cylinder's two halves, is not a feature, and is not checked.

Each gap is held against a **tolerance**, a share of the element size there —
a quarter by default, set with the slider. A mesh that does follow a curve
still cuts across it between its nodes, by about an eighth of the element size
where the cells are as long as the curve's radius, and more across the corners
of a curve given as a polygon; a feature it misses leaves gaps a cell across or
more. A tenth flagged the circles of a part meshed correctly.

**Show what is missed** draws it in red: pieces of curves no edge runs along,
crosses on corners and on points of surfaces away from the mesh, and whole
triangles where nothing of them is near it. **Go to the worst gap** flies
there.

It is asked for rather than worked out with every mesh, being a nearest-point
search from every node of the skin and all over the geometry — a few hundredths
of a second for a part meshed with thirty thousand cells. A new mesh drops the
last check.

```lua
local f = gecko.fidelity{ tolerance = 0.25 }   -- nil without mesh and geometry
print(f.skin.max, f.geometry.max)
print(f.curves.followed, f.curves.checked)
for _, c in ipairs(f.curves.missed) do print("not followed", c.tag) end
for _, c in ipairs(f.corners.missed) do print("no node on", c.tag) end
if f.worst then print(f.worst.what, f.worst.distance) end
```

## Writing it out

`⇧E` writes a VTK legacy ASCII file, named for the moment it was taken —
`gecko-2026-10-15-183012.vtk` — and meshes first if nothing has been meshed
since the last edit, so the file is always of the blocking as it now stands.

```lua
gecko.export_vtk("mesh.vtk")
```

An unstructured grid of cells of type 12, `VTK_HEXAHEDRON`, in the usual VTK
corner order. Each cell carries the name of the block it came out of as cell
data, so a reader can color the mesh by block:

```text
CELL_DATA 1000
SCALARS block int 1
LOOKUP_TABLE default
```

## Groups in the file

Every hexahedron already carries the name of the block it came out of, as
the cell variable `block`. A [group](display.md#groups-of-blocks) marked
**written** adds one more: a variable of its own, named after the group,
holding 1 for the cells of the blocks in it and 0 for the rest.

One variable per group rather than one number per cell, because a block can
be in several groups at once and a single column could only name one of
them. A group that is not marked written stays out of the file: it is a way
of looking at part of the model, not part of the model.

A name VTK will not take — a space, a slash — is written with underscores
in their place. The name in the band is the one you typed; the one in the
file is the one a reader can parse.

## What it does not do yet

Nothing fits a curved edge onto its curve *between* its ends: a classified
edge of the blocking is still a straight line, and it is the mesh points
along it that are projected. For a coarse discretisation of a strongly
curved edge that is visible — the chords are short, but they are chords.

There is no grading either: the segments along an edge are evenly spaced.
A boundary layer wants them bunched at one end, and that is a number per
sheet the same way this one is.
