# Joining and removing blocks

Two blocks that merely touch are two objects. Gluing a face of one to a face
of the other **welds** them: four pairs of nodes become four nodes, four pairs
of edges four edges, and the two faces one. From then on moving a node of the
seam moves it for both.

## In the viewer

Click a face, then another, then the glue button — or `G`.

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

## From a script

A script has no mouse, so it names a face from the listing:

```lua
gecko.create_cube{ origin = {0, 0, 0}, size = 1 }
gecko.create_cube{ origin = {1.5, 0, 0}, size = 1 }

-- The two faces that look at each other, found by where they are.
local pair = {}
for _, face in ipairs(gecko.faces()) do
  if math.abs(face.centre[1] - 1) < 1e-9 or math.abs(face.centre[1] - 1.5) < 1e-9 then
    pair[#pair + 1] = face.id
  end
end

gecko.glue_faces(pair[1], pair[2])
gecko.stats()   -- {nodes = 12, edges = 20, faces = 11, blocks = 2}
```

`gecko.faces()` gives each face its stable name, the block it bounds, and its
centre. A face with a block on each side is listed once per block, as the
surface is.

## Where the seam ends up

**The two faces meet in the middle.** Each pair of corners is welded at the
point between them, so both blocks give way by half and neither is dragged the
whole distance onto the other. Only those four corners move: the rest of each
block stays where it is, so gluing two blocks that are far apart deforms both
of them a little rather than carrying one over. For blocks already side by
side — the usual case — nothing moves at all.

The corners are paired by the quarter turn that moves them least. A glue is
necessarily orientation-reversing, which leaves exactly four candidates, and
the closest wins. Two blocks turned against each other have no right answer
here, only a least bad quarter turn.

### What the classification does to that

A node pinned to the geometry is pinned for a reason the glue does not know,
so classification comes first. Per pair of corners:

| the two corners                | where the weld goes                               |
| ------------------------------ | ------------------------------------------------- |
| neither frozen nor classified  | half way                                          |
| one of them **frozen**         | it does not move, and the other comes to it       |
| both frozen, in the same place | there, and nothing to decide                      |
| both frozen, apart             | **refused** — there is no legal answer            |
| **classified**                 | half way, then projected onto what the weld is on |

and the welded node is left on the **more constrained** of the two
classifications:

- one entity contains the other — the contained one survives. A node on a
  circle welded to a node on the cylinder it bounds is on the circle, which is
  the stronger statement and implies the other.
- the two entities meet — what they share survives, and it is often neither of
  them. Welding a node on one surface to a node on another puts it on the
  curve where the two meet.
- they share nothing — the lower dimension survives, and the status line says
  which claim was dropped: `node 7 kept curve 3 and left surface 5`.

Two surfaces can meet along more than one curve, and then the position settles
it: the nearest of the shared entities wins. A weld right at the end of a seam
lands on the corner there, which is nearer than the rest of the curve only
when it really is the end.

!!! note "Why the position has to be settled here"
    The map merges two vertices by **averaging** them, so a glue across a gap
    lands half way whether anybody meant it to or not. Rather than let a merge
    policy that belongs to the map decide where a seam goes, both corners are
    written to the chosen point *before* the sewing — and then the average is
    of two points that already coincide. The midpoint is the default because
    it is the honest reading of "weld these two", not because it is what the
    map would have done.

!!! note "Pillowing and extrusion glue differently"
    Both build a block against an existing face and then glue it there, so
    their corners coincide already and the midpoint rule cannot be told from
    any other. They use a glue of their own that says the second face moves
    onto the first, which is what a caller that has already placed both faces
    wants.

## What is refused

- a face glued to itself;
- a face that is already glued to another block;
- a name no face answers to;
- a pair of corners that are both frozen and not in the same place.

A refused glue changes nothing at all, and says why.

## Removing a block

Click any face of it and press the bin button, or `Del`. One click is
enough:
the quadrangles are listed per (face, block) pair, so a pick knows which
block's side it landed on — even for a face that has one on each. The status
line names the block before you commit to it.

From a script, `gecko.remove_block(id)`, with the names from `gecko.blocks()`.

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
everything else there is
[**Recentre view**](display.md#recentring) — `F` — which brings the whole model
into view without changing the direction you are looking from.

## Growing a block out of a face

Click a face to select it, then **drag that same face**: a new block grows
out of it, along the normal at its centre, as far as you pull. Let go and it
is built, welded to the face it came from.

Selecting first is what arms the gesture, and it is not ceremony. A face
fills the screen, and dragging anywhere on the model is how the view is
turned — one drag cannot be both. Every face but the selected one still
orbits; press and release on the selected one without moving and that is a
click, which lets go of it as usual.

While you drag, the block that would be built is drawn as a translucent
solid with its edges over it — rebuilt in place on every pointer move
rather than thrown away and made again, which is what a drag of several
events per frame costs otherwise.

The face **does not move**. The new block's four near corners are built on
it and welded to it, so the weld averages each pair of coincident points and
lands back where it started.

!!! note "Two operations you already have"
    An extrusion is [`add_block`](../developer-guide/api.md) followed by a
    glue. Nothing new is cut or sewn, which is the point: the dart surgery
    in this project is where the bugs have been, and this needed none of it.

    Which of the new block's six faces to weld is known because the block
    was built here — it is the one its first dart is on. Not the one nearest
    the old face: for a thin extrusion the far face is just as near, and
    matching by position is exactly what the sheet removal got wrong.

A face inside the model is refused. It has a block on each side already, and
nothing left to be glued to.

From a script, `gecko.extrude_face(face, block, height)`, with the `(face, block)`
pair `gecko.faces()` reports.
