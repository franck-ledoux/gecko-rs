# Smoothing

A blocking attached to a model is rarely a *tidy* blocking: edges end up
long in one place and short in another, and corners end up less square than
they could be. Press `R` and every node moves towards the middle of its
neighbors — as far as its classification allows.

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

A node is projected onto what it is classified on *before* the move is weighed, so
what gets judged is the point the node would actually end up at. And a
classification the node cannot satisfy without spoiling the cells round it
leaves it where it is: the smoother will not tear a blocking to satisfy a
curve.

## Dragging a node does not smooth

A drag moves **the node you have hold of**, and the ones you had picked with
it, and nothing else. For a while it also smoothed outwards from it, ring by
ring; that came out again. What you want of a drag is the node under the
pointer, and a neighborhood rearranging itself around it is a surprise
rather than a help.

Smoothing is something you ask for: `R`, or `gecko.smooth`, when the
blocking is where you want it and you want it tidied.

!!! note "Determinism, and how it is checked"
    Nodes are visited **by name** and each move is applied before the next
    is worked out, so the answer does not depend on how the map happened to
    lay its darts out. The same script is run in the terminal and in the
    page and the bytes it produces are compared.
