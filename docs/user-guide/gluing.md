# Joining and removing blocks

Two blocks that merely touch are two objects. Gluing a face of one to a face
of the other **welds** them: four pairs of nodes become four nodes, four pairs
of edges four edges, and the two faces one. From then on moving a node of the
seam moves it for both.

## In the viewer

Click a face, then another, then **Coller les faces**.

The two are highlighted differently, and the difference matters: **the first
one is the anchor**. The second face is brought onto it, and the seam ends up
where the first face was. The panel says which way round before you commit to
it.

A third click starts the selection again rather than being ignored, and
clicking a selected face unpicks it. Clicking empty space clears everything.

!!! note "Why a button, and not the second click"
    Gluing cannot be undone — there is no undo in this project yet — and a
    misplaced click is cheap to make. The button is the confirmation.

Dragging from a face turns the view, as it always did; only a click that does
not move selects. And a node handle still wins wherever the two overlap, even
with an opaque face in front of it.

## From Python

A script has no mouse, so it names a face from the listing:

```python
import gecko

s = gecko.Session()
s.create_cube([0, 0, 0], 1.0)
s.create_cube([1.5, 0, 0], 1.0)

blocks = sorted({f["block"] for f in s.faces})
first = next(f for f in s.faces if f["block"] == blocks[0] and f["centre"][0] == 1.0)
second = next(f for f in s.faces if f["block"] == blocks[1] and f["centre"][0] == 1.5)

s.glue_faces(first["id"], second["id"])
s.stats    # {'nodes': 12, 'edges': 20, 'faces': 11, 'blocks': 2}
```

`s.faces` gives each face its stable name, the block it bounds, and its
centre. A face with a block on each side is listed once per block, as the
surface is.

## What moves, and what does not

**Only the four corners of the second face move.** The rest of that block
stays where it is, so gluing two blocks that are far apart deforms one of them
rather than carrying it over. For blocks already side by side — the usual case
— nothing is distorted at all.

The corners are paired by the quarter turn that moves them least. A glue is
necessarily orientation-reversing, which leaves exactly four candidates, and
the closest wins. Two blocks turned against each other have no right answer
here, only a least bad quarter turn.

!!! note "Why the first face wins, and why it has to be said"
    The map merges two vertices by **averaging** them. Left to it, a glue
    across a gap would leave the seam half way between the two faces. The
    second face is therefore moved onto the first *before* the sewing, so the
    average is of two points that already coincide — settling the question
    where the rule lives rather than reaching into a merge policy that
    belongs to the map.

## What is refused

- a face glued to itself;
- a face that is already glued to another block;
- a name no face answers to.

A refused glue changes nothing at all, and says why.

## Removing a block

Click any face of it and press **Supprimer le bloc**. One click is enough:
the quadrangles are listed per (face, block) pair, so a pick knows which
block's side it landed on — even for a face that has one on each. The status
line names the block before you commit to it.

From Python, `s.remove_block(id)`, with the names from `s.blocks`.

**Whatever it was glued to keeps its own cells.** They are split off on the
way out, not deleted with it: the neighbour comes out of the operation a
closed block again, with the face that was internal back on its boundary and
free to be glued to something else. Removing the middle of three glued blocks
leaves two closed ones.

The names of the neighbour's split cells are **cleared** and reissued, which
is exactly the case the two identities were designed around: nothing was cut
in two, so the corner keys are copied across and the neighbour goes on
recognising its own corners. See
[Architecture](../developer-guide/architecture.md#naming-a-cell).

## The camera stays where you put it

Neither gluing nor removing moves the view. Both act on something you are
already looking at, and a camera that jumps on every edit is a camera you
have to re-aim after every edit.

Creating a cube does reframe, because it puts a new block down beside the
others and a model growing past the edge of the window is no use. For
everything else there is **Recadrer la vue**, which frames the whole model
without touching anything else.
