# Sheets

A **sheet** is a layer of hexahedra running through the model. It is the thing
a sheet cut will split, so before there is a cut there is this: click an edge
and the whole sheet through it lights up.

## What the closure is

The sheet through an edge is the transitive closure of *the opposite edge in a
quadrangle*. On a lone block that is the four edges parallel to the one you
named, and cutting it would halve the block. Across glued blocks it runs on
through them.

Two glued cubes show both cases at once. An edge lying **in the seam** has a
sheet of six: four in each block, sharing the two that lie in the seam itself.
An edge **across** the seam has a sheet of four — the shared face is
perpendicular to it and has no edge parallel to it, so the sheet stays in the
block it started in.

## Why the direction matters

Each edge of a sheet comes back with a direction, and they all agree.

That is not decoration. A sheet is cut at one parameter, and the same `t` has
to mean the same side of the sheet on every edge of it. In a quadrangle
A→B→C→D the edge opposite AB is CD — but the corners that correspond are A↔D
and B↔C, so AB traversed A→B matches CD traversed **D→C**, the other way
round the cycle.

Getting that wrong does not fail. It cuts alternate cells at `t` and `1 − t`,
and the sheet comes out as a concertina: something you notice by counting, not
by looking. Which is why the direction is carried through the walk, why it is
computed by a function of four numbers with no map in sight, and why it is
tested on its own.

A sheet that comes back to an edge it has already seen and disagrees about
which way round it runs is not orientable — it meets itself. That is refused,
and said, rather than cut badly.

## Clicking

Clicking priority is **node, then edge, then face**: smallest target first,
each being the thing you had to aim at to hit it. Lines are one pixel wide, so
an edge is picked within a few pixels of one, a tolerance kept constant on
screen however far away you are.

The sheet is drawn **thickened**, and over everything else rather than behind
the surfaces. A sheet runs *through* the model, and where it comes out the
far side is the part worth seeing.

Thick is not a line. WebGL caps `lineWidth` at one pixel whatever is asked of
it, so the lit sheet is a ribbon built as geometry rather than a line drawn
fatter. Its width is a share of the screen, not of the model, so a sheet
reads the same whether you are up against it or looking at the whole thing
from across the room.

Clicking the same edge again puts it out, as does `Esc`.

## Not yet

Cutting. This is the half that could be got wrong invisibly, so it is the half
that went first: if the sheet lighting up is not the set of cells you expect,
that is worth knowing before the surgery is written rather than after.
