# Meshing

A blocking is a coarse description of a shape. **Meshing** turns it into the
thing a solver reads: every block cut into a structured grid of hexahedra,
by transfinite interpolation, following whatever geometry the blocking was
classified on.

|                         |                                                    |
| ----------------------- | -------------------------------------------------- |
| `E`, or the grid button | discretise every block                             |
| `⇧E`                    | write the mesh out, as VTK                         |
| `=` `-`                 | one more segment along the lit sheet, or one fewer |

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

    Cells are shared by construction, that being what the combinatorial map
    is for, so the sharing of nodes falls out of the naming. There is no
    tolerance anywhere in the mesher.

## Looking at it

What is drawn is the mesh's **skin**: the quadrangles belonging to one
hexahedron rather than two. The inside of a mesh is the overwhelming
majority of it and is never seen — a hundred thousand hexahedra are six
hundred thousand quadrangles of which perhaps twenty thousand are on the
outside — so the rest is never sent to the browser at all.

**Mesh** in the display panel folds like any other group: shown or not, one
colour or one per block, and how far through it you can see. The element
grid is drawn over it, which is the half of the picture that says what the
discretisation actually did.

A mesh is **dropped the moment the blocking changes**. A mesh of a model you
have since edited is a lie, and a lie that looks right; press `E` again.

## Writing it out

`⇧E` writes a VTK legacy ASCII file, named for the moment it was taken —
`gecko-2026-10-15-183012.vtk` — and meshes first if nothing has been meshed
since the last edit, so the file is always of the blocking as it now stands.

```lua
gecko.export_vtk("mesh.vtk")
```

An unstructured grid of cells of type 12, `VTK_HEXAHEDRON`, in the usual VTK
corner order. Each cell carries the name of the block it came out of as cell
data, so a reader can colour the mesh by block:

```text
CELL_DATA 1000
SCALARS block int 1
LOOKUP_TABLE default
```

## What it does not do yet

Nothing fits a curved edge onto its curve *between* its ends: a classified
edge of the blocking is still a straight line, and it is the mesh points
along it that are projected. For a coarse discretisation of a strongly
curved edge that is visible — the chords are short, but they are chords.

There is no grading either: the segments along an edge are evenly spaced.
A boundary layer wants them bunched at one end, and that is a number per
sheet the same way this one is.
