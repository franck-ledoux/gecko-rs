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

The sheet is drawn **thickened**, and with the blocking rather than over it:
a sheet runs *through* the model, so what is in front of it hides it and the
faces it lies behind tint it. That is what tells you where it runs. Drawn
over everything — which is what it used to be — a sheet at the back of a
hundred blocks reads exactly like one at the front.

It also stops where the picture does. A sheet runs through the whole model
and a filter — a group hidden, or one layer being looked at — shows part of
it; lit over the part that is off screen, it is a thread hanging in the air
beside what you are working on. It is lit over the edges the wireframe
draws, and nowhere else, and the marks a cut is aimed with go with it.

The marks go with it for depth, too: they are drawn with the blocking, a
little in front of the sheet they lie along, so a cut aimed at the far side
of the model reads as the far side.

Thick is not a line. WebGL caps `lineWidth` at one pixel whatever is asked of
it, so the lit sheet is a ribbon built as geometry rather than a line drawn
fatter. Its width is a share of the screen, not of the model, so a sheet
reads the same whether you are up against it or looking at the whole thing
from across the room.

!!! note "Which kind of width, and the name that gets it backwards"
    Babylon's `sizeAttenuation` is the switch, and it reads the opposite way
    round from what it sounds like: **off** means the width is in *scene
    units*, so the ribbon shrinks into the distance like the model does, and
    **on** means a width in pixels. A sheet is something you are being shown
    rather than part of the model, so it wants pixels — off, the ribbon's
    thickness is a reading of how far away the camera happens to be, and
    since `F` moves the camera, every recentring changed it.

The readout says how far the sheet reaches: how many edges it runs along,
and **how many blocks** it covers. Only the edges are lit, and a sheet that
turns a corner through a glued neighbor can reach further than it looks —
the count is there to be read before removing anything.

Clicking the same edge again puts it out, as does `Esc`, as does clicking the
background: with nothing at all under the pointer, a click lets go of
whatever was held, sheet or faces alike.

## Cutting

**Press an edge and drag along it.** The sheet lights up as you press, the
cut follows the pointer, and letting go cuts.

While you drag, every edge of the sheet is drawn in two colors: from its
start up to the cut in one, beyond it in the other. The join between the two
*is* the plane. One edge would not be enough — what the parameter does to the
rest of the sheet is the thing worth seeing before committing to it.

Pressing and letting go without moving selects the sheet rather than cutting
it. A cut cannot be undone, and a click is cheap to make by accident.

The parameter is clamped just short of either end: a cut at 0 or 1 is not a
cut, and the kernel refuses it.

From a script:

```lua
gecko.create_cube()
local edge = gecko.edges()[1].id
gecko.split_sheet(edge, 0.25)
gecko.stats()   -- {nodes = 12, edges = 20, faces = 11, blocks = 2}
```

## Taking a sheet out

The bin button, or `Del`, with a sheet lit: the whole layer of blocks goes,
and what was glued to either end of it ends up glued to the other.

The seam lands **half way** between where the two faces were, so the gap
closes from both sides rather than one being dragged onto the other. A cap
with nothing behind it is a cap on the model's surface — there is nothing to
close against, so the model simply loses that layer.

Which face meets which is a question about the map, not about where
anything is. From a dart of one cap, crossing into the lateral face beside
it and out the far side lands on the cap at the other end, on the matching
edge; the β₃ of both is the pair the neighbors are sewn along, read before
the layer goes.

The seam is then **settled before it is sewn**: every pair of nodes the
sews will merge is read off the map, grouped — a corner where four seams
meet is in four pairs — and each group written to its own average. The two
faces of a seam coincide exactly after that, and what one seam does no
longer depends on what the one before it did.

Letting the sews do that positioning is what the earlier versions did, and
it is what left holes. A sew merges as it goes, so a corner two seams share
has already moved when the second one is asked; measure the two faces then
— as the map does before it joins them — and a seam that was fine when it
was read is refused for where its neighbor left it. By then the layer is
gone. Measuring them earlier instead only moves the disagreement: the
answer was read before the move and acted on after it.

From a script, `gecko.remove_sheet(edge)`.

## Leaving the gap

`N`, with a sheet lit, or `gecko.remove_sheet(edge, false)`: the layer goes
and **nothing closes behind it**. The two walls it was between stay exactly
where they are, unglued, and the model keeps its shape and gains a slot —
which is where a crack, a seal or a second part goes. A layer closed over
is a layer you have to cut again to get one.

It also asks less of the sheet, since nothing has to meet anything: the
caps are counted, to say that the sheet really is a layer, and what lies
behind them is not looked at at all.

A sheet that comes back round onto itself is refused: one of its own blocks
is on the far side of a cap, and there is no outside to close against. That
is checked for the whole sheet **before** anything is removed, so a refusal
leaves the model exactly as it was rather than part-way through.

## Standing in one layer

`J`, with a sheet lit: the view narrows to that layer of blocks and nothing
else — its blocks, its faces, its edges, its corners. `←` and `→` step to
the layer next door, `Esc` comes back out.

What it is for: a face in the middle of a hundred blocks cannot be clicked
on through the ninety-nine in front of it, and taking them off the screen is
the whole of the remedy. So it is a place to stand rather than a mode to be
in — picking, dragging and every edit work in here exactly as they do
outside, and what changes is only what is on screen.

An edit can take the layer away, or give its cells other names. The layer is
read again after every change, and one that has gone puts you back out
rather than leaving you looking at nothing.

!!! note "Which layer is next door, and how it knows where it came from"
    A layer is capped at either end by a face its sheet does not cross.
    Through a cap is the block next door, and the edge of **that** block
    running out of the face they share names the layer it belongs to. There
    are two such layers and neither of them is "left": the one to step to is
    whichever is not the one you came from, and that is how an arrow keeps
    going the same way.

    Which means a layer has to be recognisable. It has as many names as it
    has edges, and which one you are handed depends on where you were
    pointing — so a layer is named by the **least** of its sheet's edges,
    and two readings of the same layer agree. Named by the edge that
    happened to cross the block next door, the third step of a walk goes
    back the way it came.

## What a cut does to names

Everything the sheet touches is replaced. Each split edge becomes two new
edges, each crossed face two new faces, each crossed block two new blocks —
and none of them inherits the name of what it came from. That is what a split
means here: what comes out is new cells, not a cell and a claimant to an old
name.

Cells the sheet does not touch keep their names. The corner keys survive
throughout, so the cells that had a corner go on recognizing it.

## And the other dual thing

A sheet is a layer, and the thread crosswise through the blocks is a
**chord** — named by a face rather than an edge, and with two operations of
its own. See [Chords](chords.md).
