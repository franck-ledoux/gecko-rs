# The interface

The canvas owns the window. Everything else floats over it and can be put
away.

## Three layers

**A toolbar**, top left, carrying the verbs you use constantly: create a cube,
glue, remove, recentre, save a picture, settings, shortcuts. Icons with a
tooltip each, and a key each.

**Two panels**, shut unless asked for, and never both at once — they are
drawn in the same place. The sliders button, or `Tab`, opens **Display**:
colours, cells, geometry, and how a saved picture comes out. The cog, or `,`,
opens **Settings**: what the operations do. Both remember their state.

**A readout**, bottom left: the counts, which kind of thing the mouse is
aimed at, and whatever just happened.

`⇧Tab` puts all three away and brings them back. Which is cosmetic rather
than necessary — see [Pictures](#pictures) below.

## Keyboard

`H` or `?` shows the card, which is generated from the same table the keys are
read from: it cannot describe a key that does not exist, or miss one that
does. Any key dismisses it.

|             |                                               |
| ----------- | --------------------------------------------- |
| `C`         | create a cube                                 |
| `O`         | open a geometry (`.msh`)                      |
| `M`         | aim at the geometry, or back at the blocks    |
| `B`         | box the geometry, or the selected surfaces    |
| `K`         | put the blocking on the geometry              |
| `A`         | put the selected nodes on the selected entity |
| shift-drag  | catch every node handle inside the band       |
| `L`         | lock the picked node, or let it go            |
| `U`         | take the picked node off the geometry         |
| `G`         | glue the two selected faces                   |
| `Del` `⌫`   | remove the selection: a block, or a lit sheet |
| `P`         | pillow the selected faces                     |
| `⌘Z`        | take back the last change (ctrl `Z` on a PC)  |
| `Esc`       | clear the selection                           |
| `F`         | frame everything                              |
| `X` `Y` `Z` | look down an axis, `⇧` for the negative side  |
| `V`         | split into four views: 3D, X, Y and Z         |
| `T`         | the X/Y/Z triad                               |
| `D`         | dark or light                                 |
| `1` `2` `3` | faces, blocks, edges                          |
| `4` `5` `6` | geometric surfaces, curves, corners           |
| `I`         | internal faces                                |
| `Tab`       | display: colours, cells, geometry             |
| `,`         | settings: what the operations do              |
| `⇧Tab`      | hide everything                               |
| `S`         | save a picture                                |
| `H` `?`     | the card                                      |

Clicking the background clears the selection too — with nothing under the
pointer there is nothing to pick, so the click means *that one*.

Two things are deliberately left alone. A combination the table does not
claim belongs to the browser or the system: a shortcut that swallowed ⌘S
would be a bug, and alt is left out altogether, being how several keyboards
type the characters above the digits. And anything typed into a field is
typing — the colour inputs and the sliders both take characters and arrow
keys, and `Tab` inside the open drawer walks its fields rather than shutting
it.

Undo is the one shortcut that takes a modifier, because it is the one
everybody already knows. Bare `Z` is a view down the Z axis and stays one.

!!! note "Why the digits are read by position"
    Letters are matched by the character they produce, so the key you press
    is the one printed on it whatever your layout. Digits cannot be: on a
    French keyboard the digit row is shifted, and the key printed `1`
    produces `&`. Those three are matched by physical position instead, so
    the card stays true.

## The settings panel

The cog, or `,`. Three numbers, and each one is an **argument to the command
it belongs to** rather than something the kernel is holding: the kernel's own
constant is what you get when nobody says, and these are somebody saying.

| Setting        | What it changes                                                                                                                                                                                       |
| -------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **box margin** | how much room a bounding box leaves round the geometry, as a share of each half-side. 0 rests it exactly on the model, 10% is the default and the C++'s                                               |
| **snap**       | how near a node has to come to a geometric entity to be taken by it, as a share of the geometry's own size. Used by `K` and by a node you drop by hand; 0 means a dropped node stays where you put it |
| **steps**      | how far back `⌘Z` can go — see below                                                                                                                                                                  |

Both shares are of the model rather than lengths, because a tolerance in
millimetres is right for exactly one geometry.

## Going back

`⌘Z`, or the arrow in the toolbar, puts the block structure back the way it
was before the last thing that changed it. The button is greyed out when
there is nothing to go back to, so the history is something you can see
rather than something you find out by pressing.

Five steps by default, and **steps** under **History** in the settings panel
sets it anywhere from 0 to 20. Shortening it throws the oldest states away
there and then.

What it covers is everything that changes the model: blocks created, boxed,
glued, removed, extruded and pillowed; nodes moved, frozen, classified and
unclassified; sheets cut and taken out; and loading or clearing a geometry,
which comes back with the blocks. What it does not cover is how the model is
*drawn* — colours, opacity, which groups are showing, where the camera is.
Those are not changes to anything, and they are already a key away.

!!! note "What a step costs"
    Each one is a copy of the whole block structure, which is why there is a
    number in the drawer at all rather than an unbounded stack. On a model
    of a size anyone edits by hand that is a few hundred kilobytes and about
    a tenth of what the operation itself costs — a cut of fifty blocks takes
    some eighty milliseconds, and the copy taken before it about nine. The
    geometry is **shared** rather than copied: it is read-only once loaded,
    so five steps hold five block structures and no triangles.

    What you get for that is the thing a cheaper undo cannot give: the cells
    answer to the same numbers afterwards. See
    [the architecture](../developer-guide/architecture.md) for why a replay
    of the commands would not.

## Pictures

`S`, or the camera button, saves a PNG named for the moment it was taken —
`gecko-2026-10-09-183012.png` — so a folder of them sorts the way they were
taken.

Two settings, under **Picture** in the drawer:

- **size**, as a multiple of what the canvas renders. The default is **2×**,
  which is what a figure in a paper wants; a window has fewer pixels than a
  page. On a high-density display the canvas is already twice its CSS size,
  so 2× there is a generously large image.
- **transparent background**, for dropping a figure onto a page whose colour
  you do not know yet.

**The interface is never in the picture.** The scene is rendered to an
offscreen target; the toolbar and the drawer are DOM over the canvas and this
does not go near the canvas at all. Hiding them before a capture is not
something you have to remember.

The triad is not in the picture either: the capture renders through the main
camera, and the triad is a second camera on a corner viewport.

!!! note "Why the handles do not shrink at 2×"
    Node handles are sized in **pixels**, from the canvas height. Rendered
    into a target twice that size they would be worked out against the
    target's height instead, come out half as big in the model, and the
    picture would not match the screen. They are sized against the canvas
    explicitly, so the capture is the same picture with more pixels in it.
