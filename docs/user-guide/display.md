# Appearance and the triad

Both modes share one viewer, so everything here applies to the static page and
to a station tab alike.

## Colours

The **Affichage** panel has one swatch per colourable thing — background,
nodes, edges — and two presets, **Sombre** and **Clair**. The light preset
exists because "change the background" nearly always means "I need a light one
for a figure", and setting three swatches by hand to get there is tedious.

The panel is generated from the theme's own keys, so a colour added later needs
no new markup.

!!! note "Your colours, not the session's"
    A theme is **view state, not kernel state**. It never goes through
    `Session::apply`, it is remembered by your browser, and in station mode two
    tabs on the same session may legitimately look nothing alike. The kernel
    says what exists; it has no business knowing what colour you like it.

    The flip side: clearing site data forgets them, and they do not follow you
    to another machine.

## Node handles

The spheres marking nodes keep a **constant size on screen**, whatever the zoom
and however many blocks there are.

That is less obvious than it sounds. Sizing them in the model's own units is
wrong in both directions: tied to the model's extent they balloon as blocks are
added, until a corner marker is half the width of its own cube; tied to a
block's edge they vanish once the model is big enough to be framed from far
away. A handle is something you aim at, so the thing that has to stay fixed is
how big it is where you are aiming.

## The orientation triad

Bottom right, and switched on and off with **trièdre X/Y/Z**.

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
