# Geometry

A block structure is meant to **follow** something. That something is a
geometry: a boundary representation read from a gmsh surface mesh, drawn
alongside the blocks so you can see what you are meshing.

## Opening one

The folder button, or `O`, opens a `.msh` file — and so does **dropping one
on the window**. From a script:

```lua
gecko.load_geometry("sphere.msh")
-- 'sphere.msh loaded — 2 corners, 1 curve, 1 surface (2986 triangles),
--  18220 elements of other kinds ignored'
```

On the published page that name is one you handed over first: dropping a
file on the window, or opening it with the button, keeps it under its own
name so a script can open it afterwards — see
[Files](scripting.md#files). A browser cannot be given a path, so what
travels is the **contents** of the file, never a path. `load_geometry` reads the file and builds the same
command the folder button does — which is why a geometry opened in either
place turns up in the other, the console and the mouse being one session.

## What a file has to say

**MSH 2.2, ASCII.** Modern gmsh writes 4.1 by default, where the same information
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
spans several of them, and it is often 0 on every element while the entity tag
carries the whole structure.

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

| Control     | What it does                                         |
| ----------- | ---------------------------------------------------- |
| **show**    | draw this group at all                               |
| **uniform** | one color for the whole group, taken from its swatch |
| **id**      | a color per entity, from its gmsh tag                |
| **opacity** | transparency, per group                              |

There is no **shrink**. A geometry is not a decomposition you can explode: its
surfaces meet along the very curves also on screen, and pulling them apart
would only break that.

Surfaces come up colored **by id** and translucent by default, because telling one CAD
patch from the next is the whole reason the file was read as a model rather
than as a pile of triangles — and because what goes inside it is the point.
The geometry's colors are a family of their own, green against the blocking's
blues and browns, so that at a glance you can tell the model from the thing it
is meant to follow.

A geometric corner is drawn larger than a node handle. The two will often sit
on top of each other — that is what the geometry is there for — so they are
told apart by size as well as by color.

`F` frames the geometry along with the blocks: a geometry with nothing on it
yet is still what there is to see.

## Selecting entities

`M`, or the hexagon button, aims the mouse at the **geometry** instead of at
the blocks. The button wears the mode it is in — the hexagon cut into
quadrilaterals for the blocks, whole for the geometry — and the readout
along the bottom names it, because a toggle that looks the same either way
is one you have to click to find out about. Clicking an entity then picks
it, clicking it again puts it out, and the background or `Esc` lets go of
the lot. A picked entity turns the selection color, exactly as a picked
face does.

Corners, curves and surfaces are all pickable, **smallest target first**: a
curve lies exactly on the surfaces it bounds, so whichever answered first
would hide the other — and it is the curve you had to aim at. That is the
same rule as for the blocking's own handles, lines and faces.

It is a mode rather than a priority, because the two really do overlap: a
geometric surface and the block faces inside it are at the same pixels, and
whichever answered the click first would make the other unreachable. In
geometry mode nothing of the block structure answers at all — not the node
handles either.

The mode is **not remembered** between sessions. A mode you left on last
week and have forgotten is a mystery rather than a preference.

The block selection and the geometry selection are **held at the same
time**: they are made in different modes and used together — a handful of
nodes, and the entity to put them on. See
[Classification](classification.md#the-fast-way-onto-a-model).

A box is built round the **surfaces** in the selection, a curve having no
thickness to contain anything.

## Merging pieces of it

A CAD export cuts a closed curve in two at a pair of corners, and a closed
surface in two along a seam. The blocking does not care: a rim is one rim,
a skin one skin. Left as they are, the pieces make every cell on them be told
by hand that it spans two of them.

So say it once, of the geometry:

```lua
gecko.merge_curves({2, 4})      -- one curve, curve 2+4
gecko.merge_surfaces({2, 3})    -- one surface, surface 2+3
gecko.merge_smooth()            -- or find them: what meets smoothly
```

A merge **stands in for its pieces** from then on. Proximity finds it rather
than them, so `classify` puts the blocking on it; picking one of its pieces
picks all of it; and the joins between the pieces it swallows — the seam
inside the two surfaces, the corners where the two curves meet — stop being
anywhere a node can be. What was already on a piece, or on a join, is moved
onto the merge, which contains it.

`merge_smooth` merges surfaces across a curve where they meet within 10° of
flat, then curves at a corner where they are the only two left, run along
the same surfaces, and meet within 10° of straight — **beyond** how much
each turns at its next node anyway, since a circle drawn with 22 segments
turns 16° at every node and is no less smooth for it.

A crease is measured the same way, against how much each surface folds on
its own between its triangles: a cylinder drawn with 24 facets folds 15°
at every edge along it. So the **seam down a cylinder** — a curve with the
one surface on both sides of it — goes into the surface, and a corner left
with nothing through it but a closed curve, such as where that seam met the
rim, goes onto the curve. It says what it merged
and the nearest miss, so a join that was nearly smooth enough is a number
to raise rather than a mystery: `gecko.merge_smooth{ angle = 20 }`.

With the mouse: `⇧K`, or the button beside the magnet, for the smooth ones;
`M`, click the pieces, `⇧M` for any others. How far from smooth still counts
is **merge within** in the settings panel, in degrees, and it is what `⇧K`
sends.

A merge is **drawn as one entity**: one color, one click, named by its
pieces — `surface 2+3` — and the joins it swallowed are no longer drawn.

A piece belongs to one merge. Undo takes a merge back like anything else.

## What a node is kept on

A node, an edge point or a face point classified on a curve or a surface is
kept on it by projection — and not onto the facets themselves. A circle drawn
with 31 segments is a polygon, and a node held on the polygon is held on its
flats and kinks: its distance from the centre rises and falls as it slides
round, and a smoothing reads every side as a shallow dip to settle in. The
result depends on where it started rather than on the part — a symmetric part
came out of smoothing visibly lopsided.

So the facets are a sampling of something smooth, reconstructed when the
geometry is read, in the spirit of P. Frey's surface simplification within a
Hausdorff envelope:

- at every node of a **curve**, the quadratic through it and its two
  neighbours along the curve — at an end, through it and the next two;
- at every node of a **surface**, a normal from the triangles round it and the
  quadratic height over its tangent plane that best fits its neighbours.

A point is projected onto the facets as before, and that foot is lifted onto
the quadratics of the segment's two ends, or the triangle's three corners,
blended by where the foot lies between them. A node of the faceting stays
where it is, and on a flat face or a straight curve nothing changes at all.
Each entity is fitted on its own triangles and segments, so a crease between
two surfaces stays a crease.

What it is not: the exact nearest point of the reconstruction, only a point
of it near the facets' nearest point; and the corners of a curve that bends
*within* one entity are rounded off, since nothing marks them.

## A box round it

`B`, or the box button, adds **one block**: a box containing the geometry, or
containing just the surfaces selected.

It is almost always the first block of a session: something that contains the
shape, to be cut and fitted onto it afterwards. Each half-side is grown by
**2%**, so the box
stands clear of the geometry rather than resting on it. From a script,
`gecko.box_geometry{ margin = 0 }` for one that rests on it exactly.

The box is square to the world. A version along the shape's own axes of
inertia was built and taken out again: a shape with a round or square
cross-section has two equal eigenvalues and so no preferred way up at all,
and the box turns about the shape's length as it pleases. The useful form of
that idea is the *smallest* box, which is a different and much longer answer.

A selection that is **flat** — one planar surface, say — is refused: a box
round it would have no thickness, and a block with no volume is not a block.
Pick something with some depth to it, or add a surface facing another way.

## What it does not do yet

**No CAD in a session yet.** A geometry a session loads is a faceting and
only a faceting: there is no analytic surface behind it, so how closely the
blocking can follow a curved part is bounded by how finely the part was
meshed — the reconstruction under the facets narrows that, and does not
remove it. An exact model, with NURBS curves and surfaces, exists in the
library (see [the architecture](../developer-guide/architecture.md#an-exact-geometry));
reading one from a file and loading it into a session come next.

**A surface's reconstruction stops at its facets.** A flat end of a coarse
cylinder is reconstructed as the octagon it was given as, while the rim round
it is reconstructed as a circle that bulges past the octagon's sides. A mesh
node between the two is on the true disc and reads as a little off it — a
few hundredths on a cylinder of radius 1 faceted in eight.

**Only MSH 2.2, ASCII.** A 4.1 file or a binary one is refused by name; gmsh
writes 2.2 with `-format msh2`.

**Incidence is read from shared nodes.** Which curves bound which surface,
and which corners end which curve, is stated nowhere in `$Elements`, so it is
recovered from the node numbers the entities share — what
[classification](classification.md) uses to put an edge on what both of its
ends agree about. That assumes a **conformal** faceting, the curves' nodes
being the surfaces' own: true of any file gmsh writes from one CAD model, and
not of one assembled from parts meshed separately, where fewer containments
are found and never wrong ones.

**No volumes.** Tetrahedra are skipped and counted; the inside of the part is
never read.

## Dropping it

`gecko.clear_geometry()` from a script. The blocks are untouched either way: loading
and clearing a geometry never changes a single cell of the block structure.
