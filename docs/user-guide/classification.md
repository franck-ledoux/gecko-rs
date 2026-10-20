# Classification

A block structure is meant to follow a geometry. **Classification** is the
statement of which part of the geometry each part of the blocking follows: a
corner of a block on a corner of the model, an edge along a curve, a node
free to slide over a surface.

Once a cell is classified, it stays there. Moving a node classified on a
surface slides it over that surface; moving one classified on a corner does
not move it at all, its entity being a single point.

## With the mouse

|                           |                                               |
| ------------------------- | --------------------------------------------- |
| click a node              | pick it, or put it out again                  |
| hold `W` and drag         | catch every handle inside the band            |
| drag a picked handle      | move every picked node with it                |
| `A`                       | put the picked nodes on the picked entity     |
| `K`, or the magnet button | put the whole blocking on whatever is nearest |
| `L`                       | lock the picked nodes, or let them go         |
| `U`                       | take the picked nodes off the geometry        |

A press and release on a handle without moving it is a pick; anything
further is still a drag. Locking and unclassifying are things you *say*
about a node, so one has to be nameable without being moved.

### The fast way onto a model

`K` puts the whole blocking on whatever is nearest, which is right until it
is not: the corners of a box round a cylinder are nearer the tube than the
circle they are meant to follow. `A` is how you say where they go.

For a cylinder, per circle:

1. **Catch the four corners.** Hold `W` — or shift — and drag a band round
   them. It takes everything *inside* it, including the handles on the far
   side of the model, which is what you want when the four you are after
   are the four at the bottom. Clicking them one by one does the same
   thing, and either adds to what is already picked.

   The band is read through the camera of the pane it was **begun** in, so
   it works in the X, Y and Z views as well as in the 3-D one — and in
   those, where the model is seen flat on, it is usually easier to see
   which four you are catching.

1. **Pick the circle.** `M` to aim at the geometry, then click it. Curves
   are pickable now, and a curve wins over the surfaces it bounds.

1. **`A`.** All four go on that curve and are **projected onto it** — each
   to its own nearest point, which for the corners of a box is the four
   points at 45°, 135°, 225° and 315°.

Both selections are let go afterwards, because the next four corners and
the next curve are the next gesture. It is one command and so **one step of
undo**, and the edges and faces above those nodes settle once rather than
four times.

The two selections are held at the same time and survive `M`: they are made
in different modes and used together. `Esc` is how you let go of both.

### Reading it off the screen

Nodes, edges and faces each take one of **three colourings**, from their own
box in the drawer:

|             |                                                  |
| ----------- | ------------------------------------------------ |
| **uniform** | one colour for the group, from its swatch        |
| **id**      | a colour per cell, from its stable name          |
| **follows** | a colour per *kind* of thing it is classified on |

The third is the classification one, and it is four colours rather than one
per entity:

|              |                           |
| ------------ | ------------------------- |
| on a corner  | one violet                |
| on a curve   | another                   |
| on a surface | a third                   |
| on nothing   | the group's own colour    |

`H` shows them, read off the theme so the card cannot come to disagree with
the screen.

!!! note "Why not the colour of the entity itself"
    It was, and it was a mistake. A block face painted the colour of the
    very surface it follows, and then drawn on top of that surface, is two
    things nobody can tell apart — and on a cylinder, where every face of
    the bounding box follows some surface, the whole blocking disappeared
    into the geometry.

    What you read off this colouring is whether the blocking has taken
    hold of the geometry, and that is a question with four answers.

A **frozen** node is drawn as a **cube** rather than a ball, in whatever
colour the colouring gives it. Being out of play is not a kind of
classification — it is a thing about the cell — so it is said with the
shape of the handle, which leaves the colour free to go on saying what the
node follows or which node it is. It was a grey of its own until 0.1.2, and
grey hid the two things the colouring is there for. It cannot be dragged at
all — letting it be dragged and having the
kernel refuse would show it moving and snapping back, which reads as a bug
rather than as a locked node.

The readout counts how many nodes are on the model, which is the thing you
are working towards.

## Putting the blocking on the model

The usual order is: [box the geometry](geometry.md#a-box-round-it), pull the
box roughly onto the shape, then classify.

```lua
gecko.load_geometry("part.msh")
gecko.box_geometry()   -- one block, standing clear of the shape
...                    -- drag its corners onto the model
gecko.classify()       -- and pin what is near enough
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

## Cutting something that is classified

Cut an edge lying on a curve and you get **a node and two edges, all three
on that curve** — and the node is *pulled onto* it, not merely labelled with
it. On a curve that bends between the edge's two ends, the straight middle
of the edge is nowhere near the curve, and leaving the node there would be
the same empty label a classification without a projection always is.

Cutting a face does the same one dimension up: the edge and the two faces
that replace it are on whatever the face was on.

!!! note "Written down for the node and the new edge"
    Those two are the ones nothing else could work out. A node has no
    boundary to infer from at all. A new edge across a face has both ends
    on that face's boundary, and a boundary shared with whatever else it
    bounds says nothing about which side of it the edge is on — which is
    exactly the cylinder's end disc.

    The halves follow from the rule already there: they have the inherited
    cell at one end and an old corner at the other, and those agree.

Where the boundary is **not** fully classified the inference says so rather
than guessing: cut a face with an unclassified edge and the pieces are on
nothing. A face whose boundary is not on the model is not on the model.

## Moving them together

Dragging a handle that is picked moves **every** picked node by the same
offset; dragging one that is not moves it alone and leaves the selection
where it is.

What each node does with that offset is its own business, exactly as for a
drag of one: a node on a curve slides along that curve, a node on a surface
over that surface, a node on nothing goes where it is put and looks around
when it lands. A frozen node in the selection stays where it is — not
moving is what frozen means — and the readout says how many were left.

The offset is the one **asked for**, read before anything moves: a dragged
node that is itself on a curve lands short of the pointer, and the others
must not then move by that shortfall.

It is one command, so it is one step of undo for one gesture.

## Freezing

A frozen node does not move. Not for a drag, not for a classification, not
for anything, until it is thawed.

```lua
gecko.freeze_node(n)          -- and gecko.thaw_node(n)
gecko.freeze_node{a, b, c}    -- one node or several, here as everywhere
```

With the mouse: pick the nodes and press `L`. A mixed selection is frozen
rather than thawed — the key makes them agree, and agree the way most of a
gesture is meant.

A move of a frozen node is **refused** rather than quietly ignored: a caller
told nothing would go on believing it happened. `classify()` leaves frozen
nodes out altogether — classification is followed by a projection, and a
projection is a move.

## Coming off the model

Dragging a classified node away does not free it. Freeing it is something you
say:

```lua
gecko.unclassify_node(n)        -- it stays where it is, and may go anywhere
gecko.classify_node(n, 1, 205)  -- or put it by hand: a curve, tag 205
gecko.classify_node({a, b, c, d}, 1, 205)   -- or four of them, in one act
```

A constraint that gave way when pulled hard enough would be a constraint you
could not rely on.

With the mouse: pick the nodes and press `U`. They stay where they are and
may now go anywhere — and when you drop one somewhere else it takes hold of
whatever is nearest *there*, which is the point of freeing it when the
nearest thing here was the wrong one. How near it has to come is **snap**
in the settings panel.

## When a block moves under it

Two operations move nodes and so have to decide what they are on again.

**A cut** hands the pieces what was cut: the node in the middle of a cut
edge is on what that edge was on, and is projected onto it. See
[Cutting something that is classified](#cutting-something-that-is-classified).

**Pillowing** leaves a copy of each node where it was and draws the original
in. The copy keeps the classification, being the one still on the geometry;
the node drawn in is looked at again by proximity, and lands on whatever it
is on *now* — see [Pillowing](pillowing.md#what-happens-to-the-classification).

## What is not classified

Blocks. The rule extends to them unchanged — a block on the
lowest-dimensional entity containing all six of its faces' — though with
only volumes at that dimension there is little to say until the geometry
has any. Nothing fits a curved edge onto its curve yet either: a classified
edge is still a straight line between its two ends.
