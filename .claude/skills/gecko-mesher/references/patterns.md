# Blocking patterns

What works, why it works, and what it costs. Each pattern was met on a real
part; the B1 numbers are from `examples/B1.lua`.

## Contents

1. Angle bookkeeping — the one idea behind all the rest
2. Thin walls: one block through the thickness
3. Swept parts: block the developed picture
4. A hole through a wall: the O-grid
5. A concave fillet or trim
6. Inflections: cut there
7. Inner and outer walls of different radius
8. Long curved spans
9. Reading bad cells
10. Variants worth trying when stuck
11. Fundamental sheets: design the layers first

## 1. Angle bookkeeping

A block corner wants 90°. Where the part does not offer 90°, some block
corners must be smaller or larger, and the worst of them sets the minimum SJ
of the cells there (a corner of angle θ gives SJ ≈ sin θ: 45° → 0.71,
60° → 0.87, 72° → 0.95).

- A convex 90° corner of the part takes **one** block corner.
- A straight boundary point takes **two** (90 + 90). If three blocks meet
  there, they share 180°: 60° each at best.
- An interior vertex with **five** blocks round it has 72° each at best.
- A curved boundary owes angle: a concave arc that turns by 90° (a hole's
  quarter, a fillet) has to be paid for by **90° of deficit** somewhere —
  e.g. two corners at 45°, three at 60° (a point where 3 blocks meet on a
  straight boundary) or five at 72° (an interior valence-5 vertex).

So the question for every curved feature is *where does its deficit go*, and
the answer that gives the best minimum spreads it over as many corners as
possible. A hole (360° owed) inside a plain rectangular O-grid box puts
8 corners at 45° → min SJ ≈ 0.71 in 2D, worse in 3D.

## 2. Thin walls: one block through the thickness

A wall of thickness t much smaller than its other dimensions: one block
layer through t, the blocking drawn on the mid-surface. Cells through the
thickness come from the sheet's segment count (2–4 is usually plenty for
SJ; the user's rules may ask for more).

Each block then has one face on each wall surface: both are classified, so
the mesh follows both walls exactly.

## 3. Swept parts: block the developed picture

A wall bent along a path (an S-bend, a ring, a bracket) is a rectangle once
developed: s = arc length along the path, z = height. Block that rectangle
in 2D, then map each 2D vertex to its two 3D nodes (one per wall surface) by
walking the path. In code: a function `at(s, side, z)` returning the 3D point
on side A or B (see `examples/B1.lua`).

Pick the s of each vertex so that the blocks on curved stretches do not span
too much arc (section 8).

## 4. A hole through a wall: the O-grid

Four blocks round the hole, each with one face on the hole's cylinder, the
outer side of the ring a box. The ring is one sheet that captures the hole
as a level 1 fun sheet — the O-grid is fundamental for the hole by
construction. What it does to the **other** surfaces depends on the box:
a box whose sides end on the part's boundary cuts every surface they reach
into several sheets (§11). Its quality is decided by the **box corners**:

- Box corners **on the part's boundary** (box top on the wall's top edge):
  3 blocks meet there; with a plain neighbour beside the box, the neighbour
  takes 90° and the two O-grid blocks 45° each → min SJ ≈ 0.6–0.7.
- **Let the neighbours share the deficit**: a cut through the neighbour
  region that meets the box side (and so splits one O-grid block, its chord
  running into the hole) lets the box corners open towards 60°. On B1 this
  took min SJ from 0.58 to 0.69 for +2 blocks.
- Box corners as **interior valence-5 vertices** (a ring of blocks round the
  box) are best for quality (≈72°) but cost the most blocks — and they are
  the arrangement that keeps the blocking **fundamental**: the layer of
  blocks between the box and each outer surface is the sheet that captures
  it. Prefer it whenever the box would otherwise reach a surface (§11).

Rim vertices: put them where the diagonals from the box corners meet the
rim, not at fixed 45° — the hole is rarely centred in its box. Let the
optimiser move them (B1 ended at 33°, 123°, 200°, −124°, −54°).

## 5. A concave fillet or trim

A material boundary that is a concave arc turning by 90° (a part trimmed by
a cylinder, an inside fillet) owes 90°. Two good options:

- **A vertex on the arc where 3 blocks meet** (k = 3): the arc is split into
  two block edges, and two rays from the split point go into the part. Angles
  ≈ 60/75/105 after placement. B1's left end: 3 blocks (Q1, Q2, Q3).
- A quarter-O-grid: one block along the whole arc, with 45° corners where its
  far side meets the boundary. Fewer blocks, worse minimum.

A tangent continuation of the arc (a short flat face carrying the arc on to
the next boundary, B1's S6) belongs to the same block edge as the arc's end:
merge it (`merge_smooth` usually does) so the edge runs along one curve.

## 6. Inflections: cut there

Where a swept wall changes direction (B1's S-bend joint), the two sides of
the wall swap radii: the side that was inner becomes outer. A block that
spans the inflection gets its mesh points distributed by different arc
lengths on its two sides, and its through-thickness lines **shear** (B1:
146°/34° corners, SJ 0.56). **Put a block boundary at the inflection.** On
B1 that is two extra blocks — the O-grid top and bottom blocks split at the
joint — and it removed the shear entirely.

## 7. Inner and outer walls of different radius

On a curved wall, the outer side is longer than the inner (r5 vs r3: 1.67×).
Cells are stretched on one side and squeezed on the other, which turns a 45°
O-grid corner into ~34° on one side. Give each 2D vertex a **per-side
offset** (the A node at s + d, the B node at s − d) and let the optimiser
trade that skew against shear through the thickness. On B1 it bought
0.56 → 0.58; small, but cheap.

## 8. Long curved spans

Gecko meshes a block edge as a straight chord whose points are then projected
onto the curve. Fine up to ~110–120° of arc on concentric circles (the
spacing grows towards the middle but cells stay square); beyond that, and on
**any span that crosses an inflection**, split the block. A merged curve made
of two arcs bending opposite ways is an inflection.

## 9. Reading bad cells

Render them: `gecko.render("bad.png", { below = 0.7, color = "block" })`,
then a close-up on `quality.scaled_jacobian.worst.center`. Then:

| what you see                                                         | cause                                   | fix                                       |
| -------------------------------------------------------------------- | --------------------------------------- | ----------------------------------------- |
| Bad cells at the corners of an O-grid box, angles ~35–45°            | the hole's deficit lands on 8 corners   | §4: a cut so neighbours share it          |
| A band of skewed cells through the thickness, two corners ~145°/35°  | shear across an inflection              | §6: a block boundary at the inflection    |
| Bad cells on one side of a curved wall only                          | inner/outer stretch                     | §7: per-side offsets                      |
| Cells bunched or fanned along a long curved block edge               | chord projection over too much arc      | §8: split the block                       |
| Inverted cells next to a curved face, the block itself fine          | the face bulges past the block opposite | thicker block there, or one more layer    |
| All cells of one block mediocre                                      | the block's own corners (see `blocking.blocks[i].jacobian`) | move its vertices; optimise |

The optimiser balancing several blocks at the same minimum (B1: six blocks
within 0.0002 of 0.694) is the sign the topology is the limit, not the
placement.

## 10. Variants worth trying when stuck

- Move the O-grid box corners off the boundary (one more layer of blocks).
- Merge two blocks along a straight run where nothing changes (B1: one block
  for the right bend instead of two cost nothing in min SJ).
- Re-cut the fillet: a k = 3 split vs a quarter O-grid.
- Change which side of a feature a sheet passes.

Record each variant tried with its block count and min SJ; the comparison
is part of what the user gets.

## 11. Fundamental sheets: design the layers first

The quality rules ask for a **fundamental** blocking
(`quality-rules.md`): every geometric surface captured by **one sheet** of
blocks running along it, every curve by **columns** of blocks along it.
That is decided by the topology, so plan it before placing anything:

1. **List the surfaces** (after `merge_smooth`: the merged ones count as one)
   and, for each, say which sheet of blocks will run along it. In a thin
   wall the sheet through the thickness runs along both walls at once — a
   level 1 fun sheet capturing two surfaces, which is ideal.
2. **Check what ends on a surface.** Any sheet that *meets* a surface
   instead of running along it splits that surface between several sheets,
   unless a layer of blocks lies between them. The usual culprit is an
   O-grid box whose sides run out to the boundary: B1's box reaches the top
   and bottom of the wall, so S8 and S9 are each spread over 3 sheets and the
   blocking is not fundamental, though its min SJ is 0.64.
3. **Give such surfaces their layer.** Two ways:
   - carry the structure **inside** — the box's corners become interior
     vertices with a strip of blocks between the box and the surface
     (§4, valence-5 corners): more blocks, better corners;
   - **pillow** the blocks along the surface,
     `gecko.pillow({ {face, block}, … })` on the faces on that surface only,
     or `gecko.pillow_blocks(ids, { outer = true })` on a region. Never pillow
     every block of a part that is only one block thick: on B1 that inverted
     26 blocks (min SJ −0.998).
4. **Count the columns along each curve.** A convex edge between two
   surfaces captured by two different sheets is held by one column (Type 1).
   Pillowed edges take two (Type 2); concave edges usually three (Type 3).
   An n-valent geometric corner needs at least n − 2 blocks.
5. **Check it on the blocks**, before meshing:
   `python3 scripts/fundamental.py report.json`. It names each surface whose
   blocks lie in several sheets (and which), and each curve whose blocks lie
   in several columns. Fix the surfaces first; the curves usually follow.

Being fundamental and having a good minimum SJ pull the same way more often
than not: the strip of blocks that captures a surface is also what moves an
O-grid's 45° corners off the boundary.

