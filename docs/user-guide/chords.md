# Chords

A **sheet** is a layer; a **chord** is a thread. Both are dual to the blocks:
a sheet is what a family of parallel edges sweeps out, and a chord is the
curve that goes in at one face of a block, out at the face opposite, and on
into whatever is glued there.

Exactly one chord runs through any face — a face has one pair of opposite
faces across its own block — which is why **one click names one**. Click a
face and hold `⇧`, and the chord through it lights up.

## What the walk is

In at a face, out at the face opposite, across β₃ into the next block, and
again. It stops in two ways:

- at the **boundary**, where nothing is glued on to go to. The walk goes both
  ways from the face it was asked about, so a chord ending at two boundaries
  comes back whole however it was clicked;
- when it **closes into a ring**, coming back to the face it started from.

Which block a face was clicked on does not change its chord. An internal face
has one on each side and the thread crosses it once either way — a property of
walking both ways, not a coincidence.

!!! note "A chord that meets itself is refused"
    A chord can come back through a block it has already crossed, by a
    different pair of faces. Removing that would take one block away twice,
    and laying a layer along it would ask one block to shrink in directions
    it has already shrunk in. Both are refused, with the face named.

    A **ring** is not this: it crosses each of its blocks once and comes back
    to the start. Rings are fine.

## Removing a chord

Select a face and press `⇧Del`. Every block the chord crosses goes, one
[removal](gluing.md#removing-a-block) after another, so whatever each of them
was glued to comes out closed again. A chord taken out of the middle of a
blocking leaves a **tunnel** with sound walls, not a hole needing repair.

From a script:

```lua
local faces = gecko.faces()
gecko.remove_chord(faces[1].id, faces[1].block)
```

The status line says how many blocks went, before and after.

## An o-grid along a chord

Select a face and press `⇧P`. What is pillowed is the chord's **side**: the
four faces of each of its blocks that the chord does not cross. Each of those
blocks draws its four side faces in and a ring of four new blocks fills the
space, so a cross-section of the chord goes from one block to five.

That is the o-grid — the pattern for putting a boundary layer round a hole, or
for getting a decent mesh out of a cylinder.

```lua
gecko.pillow_chord(face, block)              -- or
gecko.pillow_chord(face, block, { tolerance = 0.02 })
```

**The two ends stay where they are.** The faces at the ends of the chord are
deliberately left out of the patch, so the tube is open at both ends. A node
on an end face belongs to two of the side faces and to nothing else, so it
moves along the mitre of those two: inwards across the chord, and not one bit
along it. A chord ending on the model's surface keeps its ends flush with it.

Pillowing moves nodes, so where the nodes that moved are classified is decided
again afterwards, exactly as for [pillowing a patch](pillowing.md). The copy
left behind keeps what the node was pinned to; the one drawn in is looked at
afresh.

## Chords and sheets together

|                     | sheet                                    | chord                                   |
| ------------------- | ---------------------------------------- | --------------------------------------- |
| named by            | an edge                                  | a face, and the block it was clicked on |
| what it is          | a layer of blocks                        | a thread of blocks                      |
| closure             | the opposite **edge** in a quadrangle    | the opposite **face** in a block        |
| removing it         | the two sides end up glued to each other | leaves a tunnel                         |
| the other operation | [cut it in two](sheets.md)               | wrap it in an o-grid                    |

A chord is the intersection of two sheets, which is worth knowing when a
blocking surprises you: three chords run through every block, one per pair of
opposite faces.
