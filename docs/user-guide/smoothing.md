# Smoothing

A blocking that follows a model is rarely a *tidy* blocking: edges end up
long in one place and short in another, and corners end up less square than
they could be. Press `R` and every node moves towards the middle of its
neighbours — as far as what it follows allows.

```lua
gecko.smooth{ rounds = 5, share = 0.5 }
```

## Smart, and what that buys

Two halves to the name, and both of them earn their keep.

**Laplacian**: a node goes a *share* of the way to the average of the nodes
an edge joins it to. All of the way is what makes a Laplacian swing from one
side of a crease to the other and never settle; half of it lets you watch
what is happening to your model. The share and the number of passes are the
two sliders under *Operations*.

**Smart**: a move is taken only if it does not make the worst corner of the
cells round that node any worse. A plain Laplacian on a concave boundary
happily turns cells inside out; this is what stops it. The measure is the
**scaled Jacobian** at each corner of each cell the node belongs to — the
volume the three edges leaving a corner span, over the product of their
lengths, which is 1 at a right angle and negative where the cell is inside
out.

The consequence worth knowing: on a blocking that is already as good as it
gets, smoothing does **nothing**, and says so. A cube's corners are right
angles, and there is no move that improves on a right angle.

## What the classification decides

| the node                                                       | what it does                     |
| -------------------------------------------------------------- | -------------------------------- |
| frozen                                                         | stays                            |
| on a geometric corner                                          | stays — its entity is a point    |
| on a curve, or a [side](faces.md#sides-a-curve-made-of-curves) | moves, then is projected onto it |
| on a surface                                                   | moves, then is projected onto it |
| on the **boundary** of the blocking, following nothing         | stays                            |
| inside the blocking, following nothing                         | moves freely                     |

The last two are the pair that matters, and the second of them is the rule
people are surprised by until they think about it. An interior node has
blocks all round it and nothing to fall off. A node on the outside of the
blocking that is classified on nothing has **no surface to be kept on** — so
moving it would pull the blocking away from the model it is meant to follow.
Classify it, and it joins in.

A node is projected onto what it follows *before* the move is weighed, so
what gets judged is the point the node would actually end up at. And a
classification the node cannot satisfy without spoiling the cells round it
leaves it where it is: the smoother will not tear a blocking to satisfy a
curve.

## Dragging a node: the ripple

Drag a node by hand and the smoothing follows it outwards. The nodes an edge
joins to it are smoothed one pass, then the ring beyond those, then the next
— and it **stops when a ring barely moves**, where barely is a fraction of
how far you dragged. A nudge stays local; a long drag is allowed to tell its
neighbours about it.

Each ring goes a **shorter way** than the one inside it: a fifth of the way
to the middle of its neighbours for the ring next to the node you dragged, a
tenth for the one beyond it, a fifteenth for the next — a fifth over how far
out the ring is. A fifth rather than the half an asked-for smoothing takes,
and falling off with distance, because this smoothing is not something you
asked for: it comes with every drag, and a gesture that rearranged its whole
neighbourhood by half would be a gesture nobody could aim. With the stopping
rule on top of it, what you see is the model giving way where you pulled it
and settling down a few edges out.

The node you dragged is **held**: smoothing it would undo the gesture that
asked for it. So are the others, when you have several picked and drag them
together.

It is one command, so one gesture is still one step of undo. And it is a
property of the **gesture**, not of the command: a script that puts a node
at a point gets the plain move, because a script placing nodes one by one is
not asking for the ones around them to be rearranged.

```lua
gecko.move_node(id, { x, y, z }, { smooth = true })   -- what a drag sends
```

!!! note "Why it is the same code as the button"
    The ripple is the same smoothing, asked for one ring at a time with one
    pass each and the dragged node held. Nodes are visited **by name**, and
    each move is applied before the next is worked out, so the answer does
    not depend on how the map happened to lay its darts out — which is
    checked by running the same script in both the terminal and the page and
    comparing the bytes.
