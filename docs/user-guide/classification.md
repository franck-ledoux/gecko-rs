# Classification

A block structure is meant to follow a geometry. **Classification** is the
statement of which part of the geometry each part of the blocking follows: a
corner of a block on a corner of the model, an edge along a curve, a node
free to slide over a surface.

Once a cell is classified, it stays there. Moving a node classified on a
surface slides it over that surface; moving one classified on a corner does
not move it at all, its entity being a single point.

## With the mouse

|                           |                                          |
| ------------------------- | ---------------------------------------- |
| `K`, or the magnet button | put the whole blocking on the geometry   |
| click a node              | pick it — the readout says what it is on |
| `L`                       | lock the picked node, or let it go       |
| `U`                       | take the picked node off the geometry    |

Picking a node is new: until now a node was something you dragged and
nothing else. Locking and unclassifying are things you *say* about a node,
so one has to be nameable without being moved. A press and release on a
handle without moving it is a pick; anything further is still a drag.

### Reading it off the screen

A classified node, edge or face **wears the colour of what it follows**, under
that entity group's own setting in the drawer. With surfaces coloured *by
id*, a node on surface 12 comes out exactly the colour of surface 12 — the
pairing reads at a glance rather than by clicking things. Unclassified, a
cell keeps the blocking's own colour.

A **frozen** node is grey, whatever it is classified on: it is not a kind of
classification, it is a cell out of play, and that is the more urgent thing
to see. It cannot be dragged at all — letting it be dragged and having the
kernel refuse would show it moving and snapping back, which reads as a bug
rather than as a locked node.

The readout counts how many nodes are on the model, which is the thing you
are working towards.

## Putting the blocking on the model

The usual order is: [box the geometry](geometry.md#a-box-round-it), pull the
box roughly onto the shape, then classify.

```python
s.load_geometry("part.msh")
s.box_geometry()      # one block, standing clear of the shape
...                   # drag its corners onto the model
s.classify()          # and pin what is near enough
```

`classify(tolerance=0.02)` does the whole blocking at once. The tolerance is
a **share of the geometry's own size**, not a length: a figure in millimetres
is right for exactly one model.

There is a second way in, and it is the one you use with the mouse: **a node
dropped near the model takes hold of it**. Drag a corner onto a surface, let
go, and it is classified on that surface and pulled onto it. The two together
are `classify()` and `snap_node()` in the C++ GECKO, and they are here for
the same reasons.

## How a node is classified

By **proximity**, dimension by dimension, stopping at the lowest dimension
with anything inside the tolerance. A node near a corner of the model is put
on the corner — not on the three curves through it, nor on the surfaces, all
of which are just as near.

Then it is **pulled onto** what it was classified on. Saying a node is on a
corner and leaving it a millimetre away would be a label rather than a fact.

## How an edge is classified

Never by proximity. An edge goes on the **lowest-dimensional entity
containing both its ends' classifications**:

| Its two ends            | The edge   |
| ----------------------- | ---------- |
| on one curve            | that curve |
| on two curves of a face | that face  |
| sharing nothing         | nothing    |
| either one unclassified | nothing    |

This is what keeps an edge coherent with its own corners, which proximity
cannot. Sampling one point of an edge can land it on a curve neither of its
endpoints touches — a diagonal across a face passes nowhere near any curve,
and a search from its midpoint would say "surface" for the right reason by
luck and "curve" for the wrong reason just as often.

It costs nothing to keep up to date, so it is redone after **anything** that
touches the model: two table lookups and an intersection per edge, and no
geometry is measured.

!!! note "What contains what is read, not measured"
    A corner is on a curve when its own node *is* one of the curve's nodes; a
    curve is on a surface when every one of its nodes is one of the
    surface's. Integer set membership — no tolerance, nothing ambiguous.

    `$Elements` states nowhere which curve bounds which surface, so this has
    to be recovered, and it is recovered the way the C++ does it. It assumes
    the faceting is **conformal**: a curve's nodes are the same node numbers
    as its bounding surface's. True of any `.msh` gmsh wrote from one CAD
    model; false of one assembled from parts meshed separately, where it
    reports fewer containments and never wrong ones.

## Freezing

A frozen node does not move. Not for a drag, not for a classification, not
for anything, until it is thawed.

```python
s.freeze_node(n)      # and s.thaw_node(n)
```

A move of a frozen node is **refused** rather than quietly ignored: a caller
told nothing would go on believing it happened. `classify()` leaves frozen
nodes out altogether — classification is followed by a projection, and a
projection is a move.

## Coming off the model

Dragging a classified node away does not free it. Freeing it is something you
say:

```python
s.unclassify_node(n)         # it stays where it is, and may now go anywhere
s.classify_node(n, 1, 205)   # or put it somewhere by hand: a curve, tag 205
```

A constraint that gave way when pulled hard enough would be a constraint you
could not rely on.

## What is not classified

Blocks. The rule extends to them unchanged — a block on the
lowest-dimensional entity containing all six of its faces' — though with
only volumes at that dimension there is little to say until the geometry
has any. Nothing fits a curved edge onto its curve yet either: a classified
edge is still a straight line between its two ends.
