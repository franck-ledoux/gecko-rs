# Smoothing

A blocking attached to a model is rarely a *tidy* blocking: edges end up
long in one place and short in another, and corners end up less square than
they could be. Press `R` and every node moves to where the blocks round it
are as near cubes as they can be — as far as its classification allows.

```lua
gecko.smooth{ rounds = 5, share = 0.5 }
```

## What it aims at

Every block is scored, and the smoothing lowers the sum of the scores. Two
parts to a block's score, each of them 1 for a cube of any size and growing
from there:

- **Square corners.** At each of the eight corners, how far the three edges
  leaving it are from square, measured on the edges' directions alone. It
  grows without bound as a corner folds flat, so a cell is not turned inside
  out on the way to anywhere.
- **Even spacing.** The mean squared edge over the volume to the two
  thirds. Square corners alone say nothing about where a straight cut
  through a row of blocks sits — every place makes the same rectangles — and
  this does: two blocks between fixed ends score best at the same size.

Neither part changes when a block is scaled, so nothing is gained by
shrinking: a blocking free to slide over a surface does not walk into a
point, as a Laplacian's would. A quadrangle of a patch is scored the same
way, one dimension down.

## Symmetric, and safe

Every node's move is worked out **from the same positions**, and they are
taken together: the answer does not depend on which node was looked at
first, so a configuration that is symmetric stays symmetric, and a cut put
down off the middle of a symmetric one goes to the middle.

Each node's move is a Newton step on its own score, carrying part of the
last round's direction with it so that a correction keeps travelling along
a row of nodes. All the moves are then scaled by **one** length — `share`
of the way for the first round, and from then on twice what was last taken
— halved until the whole scores better and no corner has fallen below a
scaled Jacobian of 0.2 (or, if one was already below that, until none is
worse than it was). The score's wall alone is not enough: a step can jump
clean over it.

The consequence worth knowing: on a blocking that is already as good as it
gets, smoothing does **nothing**, and says so. A cube's corners are right
angles and its edges even, and there is no move that improves on that.

## Untangling first

A block **turned inside out** — a corner folded through, the way projecting a
rough blocking onto curved geometry can leave one — has no score worth
lowering: the score grows without bound as a corner folds flat, from either
side, so a corner already through is walled in there. One such block used to
stop the whole smoothing.

So a round that starts with anything tangled **untangles** instead: every
corner is charged for how far its volume falls short of a tenth of what a cube
of the block's size would have — nothing for a corner already past that, and
more the further it has folded, with no wall anywhere. The floor on the worst
corner is set aside while it does, since mending a broken block takes room
from the ones round it. Once nothing is tangled, the rounds go back to
squaring blocks up.

Which way a block's corners ought to turn is read from how it is joined to its
neighbours, not from its own volume: a block turned inside out as a whole
looks, by its volume alone, like a sound one numbered the other way round.

What it cannot mend is a fold **inside** a block: a face projected onto a
curve that bulges further than the block next to it is thick. The blocks'
corners are all a smoothing moves.

It takes rounds. A blocking far from where it settles may need twenty or
thirty — the rounds slider goes to 30, and `R` again picks up where the last
left off.

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
    Every move is worked out from the same positions, so the answer does
    not depend on how the cells happen to be stored. The score uses nothing
    but the four operations and square roots — its one cube root is worked
    out by hand — so the terminal and the page round alike: the same script
    is run in both and the bytes it produces are compared.
