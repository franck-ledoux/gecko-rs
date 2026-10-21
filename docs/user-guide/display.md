# Appearance and the triad

Both modes share one viewer, so everything here applies to the static page and
to one served by `gecko` alike.

## Two panels

The toolbar carries two, and they answer two different questions:

- the **sliders** icon, or `Tab`, opens **Display** — everything on this
  page: what is drawn, in what colour, how far through it you can see;
- the **cog**, or `,`, opens **Settings** — what the operations *do*: the
  margin a bounding box leaves, how near a node has to come to be taken by
  an entity, how many steps of undo. Those are in
  [The interface](interface.md#the-settings-panel).

Only one is open at a time: they are drawn in the same place.

## Folding

Every section of the display panel folds, and so does every group inside
**Cells**, **Geometry** and **Mesh**. A folded group is one line carrying what you
reach for without thinking — its **show** box, its name, and a digest of how
it is drawn right now (`classification · 35%`, or `hidden`). Open it for the rest.

Within a band only one group is open at a time, so the panel stays the height
of one group however many there are. **Cells** and **Geometry** start
unfolded, **Colours** and **Picture** put away, and what you leave open is
remembered like every other preference.

## Colours

The drawer's **Colours** section has one swatch per colourable thing —
background, nodes, edges, faces, blocks, and the geometry's corners, curves
and surfaces — and two presets, **Dark** and
**Light** (`D` swaps them). The face and block swatches are what the
**uniform** mode below uses; in **id** mode they are ignored. The light
preset
exists because "change the background" nearly always means "I need a light one
for a figure", and setting three swatches by hand to get there is tedious.

The panel is generated from the theme's own keys, so a colour added later needs
no new markup.

!!! note "Your colours, not the session's"
    A theme is **view state, not kernel state**. It never goes through
    `Session::apply`, and it is remembered by your browser rather than by
    the session — so two tabs may legitimately look nothing alike. The kernel
    says what exists; it has no business knowing what colour you like it.

    The flip side: clearing site data forgets them, and they do not follow you
    to another machine.

## Faces and blocks

Each has its own box in the panel, and the two are independent: either, both,
or neither.

| Control     | What it does                                                                                                                                 |
| ----------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| **show**    | draw this group at all                                                                                                                       |
| **uniform** | one colour for the whole group, taken from its swatch                                                                                        |
| **id**      | a colour per cell, from its stable name                                                                                                      |
| **class**   | a colour per kind of geometric entity, by what the cell is classified on — see [Classification](classification.md#reading-it-off-the-screen) |
| **opacity** | transparency, per group                                                                                                                      |
| **shrink**  | pull each cell towards its own centre                                                                                                        |

Nodes and edges take the three colourings too; nodes are always drawn, so
that is all their box carries. **Blocks** take only the first two: a block
is classified on nothing, a surface mesh having no volumes to put it on.

**They are the same surfaces.** A block is bounded by its faces, so with both
on at full size you are looking at one shape coloured two ways. Shrinking is
what makes the pair worth having: pull the blocks in and they become separate
solids floating inside a translucent skin of faces.

Each kind of cell shrinks towards **its own** centre, which is why the three
sliders do visibly different things. A face pulls towards the middle of that
face, so it shrinks within its own plane and the model keeps its outline. A
block pulls towards the middle of the block, so the whole thing narrows. An
edge pulls towards its own midpoint, leaving a gap at each corner.

!!! note "Which pass each thing is drawn in"
    Transparency is sorted **per mesh**, back to front by how far the
    mesh's middle is from the camera, and that is a guess. A geometry
    surface and the block face following it have their middles in almost
    the same place, so which was drawn first flipped as the view turned.

    The geometry and the blocking are drawn in separate passes now — the
    geometry first — which settles it. They share one depth buffer, so
    what is genuinely in front still wins. A lit sheet and an extrusion
    preview are drawn over everything, on purpose: both run through the
    model rather than sitting in it.

    Within one pass the per-mesh guess is still a guess. Two translucent
    things of about the same size in about the same place cannot be sorted
    correctly by their middles, and the remedy is to turn one of them down
    or off.

!!! note "One mesh per block"
    Transparency is drawn back to front, and Babylon sorts **meshes** — it
    does nothing about the triangles inside one. Every cell in a single mesh
    left it nothing to sort: a block behind another could be drawn over it,
    and a block's own far side over its near side. Each block is its own
    mesh now, and each is drawn inside-out before outside-in, which settles
    the other half. The cost is a draw call per block, which is the trade to
    revisit when there are thousands of them.

### Colour by id

The colour comes from the cell's **stable name**, and three things follow from
that being the rule rather than a position in a list:

- a cell keeps its colour for as long as it exists;
- adding a block recolours nothing — the hue is a function of the name alone,
  not of how many cells there are;
- a face and a block that happen to share a number do not share a colour.

Names are never reused, so over a long session the hues drift and two distant
names can land near each other. That is the trade for stability; the
alternative is a palette that renumbers behind you.

### Edges

The wireframe has a **show** box of its own and a shrink slider, and `3`
switches it. Shrinking it to a tenth leaves a dotted outline, which is a
different thing from no wireframe at all — hence two controls rather than
one standing in for the other.

### Internal faces

**internal faces** (`I`) decides whether faces with a block on each side are
drawn.
Blocks are unaffected: each keeps its own six, or shrinking one would open it.

A face is internal when it turns up twice in the surface, once per block —
derived from the model rather than declared, so it cannot disagree with it.
Nothing is internal until two blocks are [glued](gluing.md).

!!! note "Dragging a slider does not rebuild the scene"
    Moving opacity, shrink or colouring rewrites the mesh's buffers in place.
    Only a change that alters the *shape* — showing or hiding a group, hiding
    internal faces, a new model — builds a new mesh. Rebuilding on every
    mouse-move is what makes a slider stutter and the model blink while you
    drag.

!!! note "Where the wireframe went"
    The edges run exactly along the faces, so at full opacity the depth test
    has nothing to separate them and the wireframe disappears into the
    surface. The surfaces are drawn with a small depth bias to stop that. If
    you ever see edges flicker in and out, that bias is the thing to look at.

## The mesh

**Mesh** holds one group, the skin of the last mesh built — shown or not,
one colour or one per block, and how far through it you can see. The element
grid is drawn over it in the edge colour, which is the half of the picture
that says what the discretisation did. See [Meshing](meshing.md).

## The geometry

The three groups a [geometry](geometry.md) is drawn in — surfaces, curves and
corners — take the same **show / uniform / id / opacity** controls, and no
shrink. They are on that page rather than this one, because what the tags mean
is the larger half of the story.

## Node handles

The spheres marking nodes keep a **constant size on screen**, whatever the zoom
and however many blocks there are.

That is less obvious than it sounds. Sizing them in the model's own units is
wrong in both directions: tied to the model's extent they balloon as blocks are
added, until a corner marker is half the width of its own cube; tied to a
block's edge they vanish once the model is big enough to be framed from far
away. A handle is something you aim at, so the thing that has to stay fixed is
how big it is where you are aiming.

## Zoom

The wheel changes the distance by a **share of what is left**, not by a fixed
number of model units.

A fixed step is the same distance however near you are, which is wrong at both
ends: far out it barely moves, and close in one notch is a leap straight
through the model and out the far side. The camera also stops a little short
of the model rather than reaching the point it orbits — an orbit camera that
arrives at its own centre has nothing left to turn around, and the view flips.

How near and how far are both derived from the model's size, and they follow
it: build a block from a script and the limits move with it, without the camera
being yanked anywhere.

## Recentring

**Recentre view** (`F`) brings the whole model into view, and **leaves the
direction alone**. Where the model is and how far away it has to be come
from its own bounds; which way round you are looking at it from is yours, and
the triad is what changes that. A recentring that also swung the view back to
some default angle would mean re-aiming after every use.

It **glides** rather than cutting, eased in and out, for the same reason the
triad swings: a camera that teleports leaves you working out which way round
the model ended up. A second recentring replaces the first rather than racing
it, and any inertia left over from a gesture is stopped first — otherwise it
would go on nudging the camera while the glide moved it, and the two would
land somewhere neither meant.

It is the only control that moves the camera on purpose. Edits do not, except
for creating a cube, which glides too.

## Four views

`V`, or the grid button, splits the window into four: the 3-D view top
left, and beside and below it three views looking straight down **X**, **Y**
and **Z**.

They are four cameras on **one scene**. Moving a node moves it in all four
because it is the same node — there is nothing keeping them in step, and so
nothing that can fall out of step. Only the cameras differ.

The three axis views are **orthographic**: no perspective, so two things the
same size are drawn the same size wherever they are. That is what makes them
worth having for lining anything up, and it is why their zoom moves the
extent they show rather than a distance.

### What each view answers for

The pane the pointer is over is the one that answers. Clicks, drags and
every verb work in all four — the pick goes through that pane's own camera —
and the camera controls are attached to that pane alone, so a drag turns one
view rather than all of them.

Each pane zooms and pans on its own, as in any CAD package: frame one
closely while another still holds the whole model. `F` reframes all four at
once.

### Dragging in the plane

In an axis view a node is dragged **in the plane of the screen**: two
coordinates move and the third cannot. Placing a corner becomes a matter of
looking at it rather than of guessing how far away it ended up.

!!! note "Said, not inherited"
    An orthographic camera locked to an axis already faces that plane, so
    leaving the constraint out would look the same today. It is stated
    anyway, which makes it a property of the pane rather than a coincidence
    of where the camera happens to be pointing.

The 3-D view is unchanged: a drag there follows the camera's own facing
plane, which is oblique to the axes, and all three coordinates move.

!!! note "One mesh, four zooms"
    A node handle is one sphere shown in four views, so it cannot be the
    right number of pixels across in all of them at once — but it can be
    resized **between** them. It is rescaled once per camera rather than
    once per frame, each camera's render happening after its own turn to
    adjust it.

    Sizing it for whichever pane the pointer was in left it the size of a
    block in a pane zoomed in further, which is what it looked like.

## The orientation triad

Bottom right, and switched on and off with **X/Y/Z triad** in the drawer, or
`T`.

It is not a picture of the axes — it is a second camera on a corner viewport,
sharing the main view's rotation exactly. Which is also what makes it
clickable.

**Clicking an axis puts the camera on it**, so you end up looking at the plane
opposite: click X and you see the YZ plane, Y gives XZ, Z gives XY. All six
directions are reachable — the large ball on each axis is the positive end, the
small dim one the negative.

The view **swings** into place rather than jumping. A jump is faster and leaves
you working out which way round the model ended up.

Hiding the triad also makes it unpickable, so it is not quietly in the way of
clicks in that corner.

!!! warning "One known rough edge"
    The corner viewport is drawn over the model. A node that happens to sit
    behind it can still be grabbed and dragged, because the drag is handled by
    the main camera's own picking. The corner is small and usually over empty
    space, so this is noted rather than worked around.
