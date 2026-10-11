# Geometry

A block structure is meant to **follow** something. That something is a
geometry: a boundary representation read from a gmsh surface mesh, drawn
alongside the blocks so you can see what you are meshing.

## Opening one

The folder button, or `O`, opens a `.msh` file. From Python:

```python
import gecko

s = gecko.Session()
s.load_geometry("sphere.msh")
# 'sphere.msh loaded — 2 corners, 1 curve, 1 surface (2986 triangles),
#  18220 elements of other kinds ignored'
```

The browser has no filesystem to name, so what travels is the **contents** of
the file, not a path. `load_geometry` reads the file and builds the same
command the folder button does — which is why a geometry opened in either
place turns up in the other, like everything else in a station session.

## What a file has to say

**MSH 2.2, ASCII.** What the C++ GECKO reads, and what every `.msh` in its
test data is. Modern gmsh writes 4.1 by default, where the same information
lives somewhere else entirely, so a 4.1 file is refused by name rather than
misread — re-export it with `-format msh2`.

Every element in `$Elements` carries two tags, and it is the **second** — the
elementary entity tag — that says which geometric entity it belongs to:

| In the file               | Becomes          |
| ------------------------- | ---------------- |
| triangles sharing a tag   | one **surface**  |
| lines sharing a tag       | one **curve**    |
| a point element (type 15) | a **corner**     |
| anything with tag 0       | nothing; dropped |

A corner is **read, not deduced**. Which node of a faceting is a corner of the
model is a statement about the model, and no amount of looking at triangles
recovers it — so it has to be in the file, as a point element.

The other tag, the `$PhysicalNames` group, is read and kept but **groups
nothing**. A physical group is a name for a set of entities and routinely
spans several of them; and in every `.msh` GECKO's own test data holds, it is
0 on every element while the entity tag carries the whole structure.

!!! note "Tetrahedra are skipped, and counted"
    A gmsh file of a meshed CAD model usually carries the volume mesh and its
    boundary together. The volume elements are not read — a geometry here is a
    boundary representation — and the message says how many were passed over,
    so a file you expected to be a surface and was not says so rather than
    loading quietly.

    Their interior nodes go with them. On `sphere.msh` that is the difference
    between 1495 nodes and several thousand that nothing draws.

A file with no triangles at all is refused: that is not a surface mesh. So is
one whose triangles carry no entity tag, which would otherwise load as an
empty model and look like a bug in the viewer rather than a fact about the
file.

## Looking at it

Three groups in the drawer — **surfaces**, **curves**, **corners** — and
`4`, `5`, `6` to show and hide each. Each takes the same three controls as
faces and blocks:

| Control     | What it does                                          |
| ----------- | ----------------------------------------------------- |
| **show**    | draw this group at all                                |
| **uniform** | one colour for the whole group, taken from its swatch |
| **by id**   | a colour per entity, from its gmsh tag                |
| **opacity** | transparency, per group                               |

There is no **shrink**. A geometry is not a decomposition you can explode: its
surfaces meet along the very curves also on screen, and pulling them apart
would only break that.

Surfaces come up **by id** and translucent by default, because telling one CAD
patch from the next is the whole reason the file was read as a model rather
than as a pile of triangles — and because what goes inside it is the point.
The geometry's colours are a family of their own, green against the blocking's
blues and browns, so that at a glance you can tell the model from the thing it
is meant to follow.

A geometric corner is drawn larger than a node handle. The two will often sit
on top of each other — that is what the geometry is there for — so they are
told apart by size as well as by colour.

`F` frames the geometry along with the blocks: a geometry with nothing on it
yet is still what there is to see.

## Selecting surfaces

`M`, or the hexagon button, aims the mouse at the **geometry** instead of at
the blocks. The button wears the mode it is in — the hexagon cut into
quadrilaterals for the blocks, whole for the geometry — and the readout
along the bottom names it, because a toggle that looks the same either way
is one you have to click to find out about. Clicking a surface then picks it, clicking it again puts it out,
and the background or `Esc` lets go of the lot. A picked surface turns the
selection colour, exactly as a picked face does.

It is a mode rather than a priority, because the two really do overlap: a
geometric surface and the block faces inside it are at the same pixels, and
whichever answered the click first would make the other unreachable. In
geometry mode nothing of the block structure answers at all — not the node
handles either.

The mode is **not remembered** between sessions. A mode you left on last
week and have forgotten is a mystery rather than a preference.

Curves and corners are not selectable. A surface carries triangles, so it
has an area and a set of points, which is what the one thing you can do with
a selection needs.

## A box round it

`B`, or the box button, adds **one block**: a box containing the geometry, or
containing just the surfaces selected.

It is almost always the first block of a session: something that contains the
shape, to be cut and fitted onto it afterwards. The C++ GECKO has the same
button, and the same margin — each half-side is grown by **10%**, so the box
stands clear of the geometry rather than resting on it. From Python,
`s.box_geometry(margin=0)` for one that rests on it exactly.

The box is square to the world. A version along the shape's own axes of
inertia was built and taken out again: a shape with a round or square
cross-section has two equal eigenvalues and so no preferred way up at all,
and the box turns about the shape's length as it pleases. The useful form of
that idea is the *smallest* box, which is a different and much longer answer.

A selection that is **flat** — one planar surface, say — is refused: a box
round it would have no thickness, and a block with no volume is not a block.
Pick something with some depth to it, or add a surface facing another way.

## What it does not do yet

Nothing is **classified** onto it. A box round the geometry is a block that
*contains* the shape; no cell of it knows which surface it is supposed to lie
on, and nothing snaps. That is the next thing this exists for.

There is also no incidence: which curve bounds which surface is not stated
anywhere in `$Elements`, and the C++ recovers it from shared mesh nodes
because classification needs it. Nothing needs it yet, and a table nobody
reads is a table nobody notices is wrong.

## Dropping it

`s.clear_geometry()` from Python. The blocks are untouched either way: loading
and clearing a geometry never changes a single cell of the block structure.
