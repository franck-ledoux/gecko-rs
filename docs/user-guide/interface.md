# The interface

The canvas owns the window. Everything else floats over it and can be put
away.

## Three layers

**A toolbar**, top left, carrying the verbs you use constantly: create a cube,
glue, remove, recentre, save a picture, settings, shortcuts. Icons with a
tooltip each, and a key each.

**A settings drawer**, shut unless asked for — `Tab`, or the sliders button.
Colours, cells, and how a saved picture comes out. Its state is remembered.

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
| `L`         | lock the picked node, or let it go            |
| `U`         | take the picked node off the geometry         |
| `G`         | glue the two selected faces                   |
| `Del` `⌫`   | remove the selection: a block, or a lit sheet |
| `P`         | pillow the selected faces                     |
| `Esc`       | clear the selection                           |
| `F`         | frame everything                              |
| `X` `Y` `Z` | look down an axis, `⇧` for the negative side  |
| `T`         | the X/Y/Z triad                               |
| `D`         | dark or light                                 |
| `1` `2` `3` | faces, blocks, edges                          |
| `4` `5` `6` | geometric surfaces, curves, corners           |
| `I`         | internal faces                                |
| `Tab`       | settings                                      |
| `⇧Tab`      | hide everything                               |
| `S`         | save a picture                                |
| `H` `?`     | the card                                      |

Clicking the background clears the selection too — with nothing under the
pointer there is nothing to pick, so the click means *that one*.

Two things are deliberately left alone. Anything with ctrl, meta or alt
belongs to the browser or the system: a shortcut that swallowed ⌘S would be a
bug. And anything typed into a field is typing — the colour inputs and the
sliders both take characters and arrow keys, and `Tab` inside the open drawer
walks its fields rather than shutting it.

!!! note "Why the digits are read by position"
    Letters are matched by the character they produce, so the key you press
    is the one printed on it whatever your layout. Digits cannot be: on a
    French keyboard the digit row is shifted, and the key printed `1`
    produces `&`. Those three are matched by physical position instead, so
    the card stays true.

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
