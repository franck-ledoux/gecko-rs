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

## Cutting

**Press an edge and drag along it.** The sheet lights up as you press, the
cut follows the pointer, and letting go cuts.

While you drag, every edge of the sheet is drawn in two colours: from its
start up to the cut in one, beyond it in the other. The join between the two
*is* the plane. One edge would not be enough — what the parameter does to the
rest of the sheet is the thing worth seeing before committing to it.

Pressing and letting go without moving selects the sheet rather than cutting
it. A cut cannot be undone, and a click is cheap to make by accident.

The parameter is clamped just short of either end: a cut at 0 or 1 is not a
cut, and the kernel refuses it.

From Python:

```python
s.create_cube([0, 0, 0], 1.0)
edge = ...                  # a name from s.sheet(...)
s.split_sheet(edge, 0.25)
s.stats     # {'nodes': 12, 'edges': 20, 'faces': 11, 'blocks': 2}
```

## Taking a sheet out

The bin button, or `Del`, with a sheet lit: the whole layer of blocks goes,
and what was glued to either end of it ends up glued to the other.

The seam lands **half way** between where the two faces were, so the gap
closes from both sides rather than one being dragged onto the other. A cap
with nothing behind it is a cap on the model's surface — there is nothing to
close against, so the model simply loses that layer.

From Python, `s.remove_sheet(edge)`.

A sheet that comes back round onto itself is refused: one of its own blocks
is on the far side of a cap, and there is no outside to close against.

## What a cut does to names

Everything the sheet touches is replaced. Each split edge becomes two new
edges, each crossed face two new faces, each crossed block two new blocks —
and none of them inherits the name of what it came from. That is what a split
means here: what comes out is new cells, not a cell and a claimant to an old
name.

Cells the sheet does not touch keep their names. The corner keys survive
throughout, so the cells that had a corner go on recognising it.
