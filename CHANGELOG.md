# Changelog

Versions are what a reader can download, and this is what they get. Dates are
the day a tag was cut.

## Unreleased

- **A halo under the pointer.** The cell a click would take is ringed in
  cyan before the click, and the readout names it —
  `picking blocks — node 42`, `picking geometry — curve 7`. A ball around a
  point, a ribbon along a line, a ring round the sides of a face or the
  border of a surface; a ring rather than a tint, so it never covers what
  you are looking at. For the two operations where picking is the hard part:
  classifying, where a cell of the blocking and the cell of the model it goes
  onto are at the same pixels, and laying a quadrangle, where each of the four
  clicks may land on a point, a curve or a handle to reuse. Laying has its own
  preference for a handle that is on the geometry, and the halo follows it
  rather than the plain one. Switched off in **Display → Colors**.

  It is drawn from the same answer the click acts on, so it refuses whatever
  the click refuses: nothing behind an opaque model, nothing a group filter is
  holding back, nothing on a layer that is off. The resolution order used to
  be written three times over in three listeners; it is one function now, and
  a test says the ring and the click take the same cell at every pixel it
  probes.

## 0.2.1

Three things that were wrong in 0.2.0, each found by using it. A filter that
could empty the window and never said so, a neighbour that came away from an
o-grid, and a pointer that reached through a solid model.

- **A solid model hides what is behind it from the pointer.** With the faces
  drawn opaque, a click still reached the handles and the edges on the far
  side and inside: the pick asked only the handles and the wireframe, never
  the surface, so the ray went straight through a shell you could not see
  through.

  A pick is now stopped by whatever is drawn **opaque**, tested against the
  meshes actually on screen. That is what makes it follow the picture rather
  than the model: shrinking the blocks opens gaps and the ray goes through
  them, hiding a layer or filtering a group takes the surface out of the
  scene and out of this with it, and a translucent shell stops nothing —
  what you can see you may still pick. The discretised mesh goes on letting
  clicks through, by the `isPickable` it already carried.

  A mark drawn **on** the shell is kept: handles and the wireframe are sized
  in pixels, so a handle on a front corner stands proud of the surface by a
  world amount that depends on the distance and on which camera is looking,
  and an edge runs exactly along the face it borders. That slack is worked
  out for both kinds of camera, the quad panes being orthographic.

  The band is deliberately untouched: a lasso means everything inside it,
  the far side included, which is what you want when the corners you are
  after are the ones at the back.

  Measured, by taking it away: all twelve edges of a cube answer instead of
  ten, and all four corners behind an opaque face answer instead of none.

- **A block beside a pillowed one stays welded to the layer.** Grow a cube
  out of one side of another and run an o-grid along the first, and four
  edges of the neighbour ran from its own far corners into the middle of
  the pillowed block, crossing the surface they should have stopped at.

  Pillowing a face another block is behind lets go of the far side and
  leaves it a copy of every node of the patch to hold on to, because the
  nodes it was holding are about to be pushed inwards. The far side's
  blocks and its faces were moved onto those copies; its **edges** were
  not. Only the patch's own edges were copied, and an edge running *away*
  from the patch is a copy of nothing — so it kept naming a node that then
  moved, and the faces and the edges gave two accounts of the same model.

  Edges wholly on the far side now follow their blocks and faces, by the
  same "wholly on the far side" rule the faces already used. Measured, by
  taking it away: the new test names the four edges, and nothing else in
  the suite moves.

- **A group filter no longer outlives the session, and admits to itself
  while it lives.** Decook "blocks in no group" once and every later visit
  to that address opened blind: the readout counted one block, eight nodes,
  six faces and twelve edges, and the window stayed empty. Nothing on screen
  said why — the only witness was a checkbox in a drawer that is shut by
  default — and `localStorage` is per origin and per browser, so the same
  gecko drew on one and not on the next, which is how it was found.

  `hiddenGroups` is a list of group **names**, and a name belongs to a
  model, not to a browser. The page always opens without a model, so a
  stored name could only ever be applied to a *different* one than the one
  it was written for: hide a `fluid` in one model and the next model's
  `fluid` arrived hidden. It is now dropped on the way in and scrubbed on
  the way out, and the readout carries the filter in amber for as long as
  it is holding something back — plainly, when it is hiding all of it.

  Measured, by putting the persistence back: the two new tests fall and the
  six around them pass.

## 0.2.0

The block structure stops being a combinatorial map and becomes an explicit
complex of cells: the same model in 25 times less memory, a redraw 190 times
faster, and `honeycomb` out of the lock file. Not a line of the test suite
changed. [The design note](developer-guide/cellular.md) says why, with the
measurements that decided it — and with the three of its own predictions
that did not hold.

And two new ways onto a surface: a quadrangle laid on four corners you
click, welded to what it was laid against, and several faces that grow into
blocks together rather than one at a time.

- **The block structure is an explicit complex of cells, and `honeycomb` is
  out of the lock file.** The same model in **25 times less memory**:
  19 903 bytes a block became 802, measured by the same counting allocator
  over the same grid, with the cells alone 19 414 against 550.

  The cause was never the combinatorial map as an idea. It was that **a cell
  attribute is paid for by the dart**: ten `fast-stm` transactional
  variables per dart, twenty-four darts a block, about eighty-four bytes and
  two allocations each to hold a `u32`. The representation worked against
  the one extension this project is heading for, which is attaching a curve
  to every edge.

  Four arenas now, and nothing else is state. A node, an edge, a face and a
  block are plain data; everything that climbs — a node's edges, an edge's
  faces, "does this edge exist already" — is derived and cached, and a cache
  is not state, so `snapshot` is a derive where it was seventy lines of
  hand-written dart copying. Three facts carry the rest:

  - **a face has none, one or two sides**, and its corners are wound as side
    0 sees them, outward. Side 1 reads the same list backwards, and that is
    the whole of the orientation question — no twist byte, nothing stored
    that can come to disagree with the structure it describes.
  - **a name is a slot and the generation it was packed with**, so nothing
    is ever renamed, and a name handed out before a slot was reused cannot
    come back designating the cell that took its place.
  - **welding two faces leaves one face that both blocks name.** Conformity
    stops being something maintained and becomes something structural: there
    is nothing to keep in agreement because there is nothing in two places.

  Not a line of the test suite changed. That was the point of confining
  honeycomb first: the suite spoke only of named cells, so it described the
  structure being written as well as the one being replaced, and it is what
  the nine stages were measured against — 43 of its tests passing at the
  start, 330 of 330 at the end. A differential bench ran both engines on the
  same scripts and compared every answer after every step, which caught five
  divergences no test had thought to ask about.

  **And it is faster by a long way, which was not the point but is the
  thing anybody will notice.** The page's whole scene buffer, at five
  hundred blocks: **73.1 ms to 0.38 ms**. The light buffer that goes out on
  every pointer move, which is why dragging a node felt slow: **6.9 ms to
  18 µs**. Measured by the same bench over both engines, on models that
  match byte for byte, and none of it is a change to the drawing code — it
  is the same buffers read out of cells instead of out of darts.

  Taking a layer of blocks out was the one operation that still scaled
  badly, because a weld rewrote every reference to the corners it merges by
  sweeping the whole structure, and a layer welds a seam per block: at eight
  thousand blocks, 111 ms of the 124 that took. An index of who holds each
  node and each edge, built once for the operation and carried through the
  welds, brings it to 22.9 ms — and every operation is now linear in the
  blocks it touches.

  **What this costs anyone outside the kernel:** `agree.lua`'s three pinned
  numbers moved, and one of them is the mesh itself — 45 hexahedra where it
  had 36. That script asks for whichever edge the structure lists first and
  sets its sheet to two segments, and that is a different edge in an arena
  from what it was in a map, so the sheet runs a different way. The mesh a
  blocking produces is unchanged; what moved is a choice the script left to
  the representation. [The design note](developer-guide/cellular.md) has the
  measurements, the two conventions that had to be settled, and the three of
  its own predictions that did not hold.

- **The combinatorial map is reachable from nowhere but the one file that owns
  it.** Not a change to what the program does: a change to what the rest of it
  is allowed to know, and the precondition for replacing the representation
  without touching anything above it.

  `honeycomb` now appears in `blocking.rs`, in the trait implementations
  `attributes.rs` owes it, and in the two test files that are **about** the
  representation and have moved into `src/` to say so. Nowhere else, and **no
  public signature mentions a map identifier**. The accessors that spoke the
  map's language are `pub(crate)` and suffixed `_of`; the names they occupied
  take a `CellId`, and cells are listed by name. `Hex` moved in beside the
  tables it reads, so the mesher no longer walks beta functions to recover a
  hexahedron's frame — it reads the block's own corners. `BlockFace` gave up
  the volume identifier it carried for speed and carries the two answers that
  were asked of it instead. `BlockingError` reports the map's failures as words
  rather than as its types, so honeycomb is not in the signature of everything
  that can fail.

  The three thousand lines of tests now speak only of named cells, which is
  what makes them an oracle for the structure to come rather than a description
  of the one being replaced. The mesh file is unchanged byte for byte.

- **A quadrangle is laid on four corners you click, welded to what it was
  laid against.** `⇧Q`, beside the `Q` that lays one on a whole surface's own
  corners. Lay them one after another along a surface and what grows is one
  patch, which a single drag raises into blocks of one height — one step per
  node along the mitre, welded along every shared edge, which
  `extrude_patch` already did.

  A side is welded when the two points it was laid on were both already
  nodes of the model, an edge joins them, and that edge has no face on its
  other side. There is no question of *which* face: a side of an open sheet
  carries one, so a free edge between two given points is the answer or
  there is none. Two coincident sheets put two free edges between the same
  two points, and then the side is left free rather than welded to a guess.

  **Which way round you click does not matter.** β₂ joins two darts running
  opposite ways along one edge, so a quadrangle wound the other way from its
  neighbour could not be welded to it as it stands — and a sheet has no
  inside, so which way round it is says nothing anybody meant. Each side
  with a neighbour votes and the quadrangle is turned to agree. The corners
  still come back in the order they were clicked.

  The neighbour does not move, and the node and the side that survive keep
  its name, its class and its freeze. The corners are classified from the
  clicks in the same command. The quadrangle itself and its sides are
  settled the way everything else is, from their boundary — four corners all
  on one surface leave the face on it with nobody saying so — and a surface
  picked before the laying starts is how to say it when that cannot work,
  the corners being on curves of two different surfaces or on nothing.

  The handle a click reaches while laying is the one **on the geometry**. A
  block resting on a surface has a node inside the material a fraction of a
  pixel behind each of the ones on it, and that one is never the corner
  anybody meant.

  What the mode draws, after trying it on a real model: a mark twice the
  size of a handle rather than a sphere a world unit across, which on a
  twenty-unit model was the size of a block; a thinner side from the last
  corner placed to wherever the pointer is, running on from there back to
  the first once three are down, so the quadrangle is whole before the click
  that closes it; and the pointer aimed at the geometry for the duration, so
  that no edge of a block lights up under it offering a cut the mode cannot
  take. The pointer is handed back as it was found.

- **A thin block laid by hand was the wrong primitive, and `⇧Q` no longer
  lays one.** Worth recording rather than quietly replacing, because the
  design note had decided the opposite.

  Five of them laid along the boundary of a surface, by hand, do not work.
  A block has an inside, so placing one means saying which side the material
  is on and how thick it is, and four clicks say neither: along the boundary
  the winding alternated from one to the next, so consecutive blocks sat on
  opposite sides of the surface and were correctly refused a weld; the
  thickness, a share of each quadrangle's own diagonal, came out between
  0.35 and 0.61 on a model whose features are two units; and two clicks in
  five reached the node inside the material instead of the one on the
  surface, landing a corner a thickness off the model, classified on
  nothing.

  `gecko.add_slab` stays in the language, welding included — a lone slab is
  a reasonable thing to script. [The design note](developer-guide/cellular.md)
  says what overturning its first decision costs the structure to come.

- **A thin block is laid on four corners you click.** `⇧Q`, beside the `Q`
  that lays one on a whole surface's own corners.

  Each click puts a corner where the pointer is: on an existing handle if it
  is near one, which is how a slab comes to be laid *against* another;
  otherwise on the geometry, corners before curves before surfaces — which is
  what the tolerance is made of, a corner being a small ball and a curve a
  thin ribbon, so a click only reaches one by being near it. What is on
  screen says how far along it is: a mark, then a side, then two, and at the
  fourth the block. `Esc` gives up and `⌫` takes back the last corner.

  A block and not a face, because a face on its own has no inside and
  everything that follows wants one. The thickness is signed — the sign picks
  which side the material is on — and defaults to a tenth of the quadrangle's
  own shortest diagonal, so "thin" means the same thing whatever the model is
  measured in.

  The four corners on the geometry arrive **already classified**, in the same
  command, rather than in a second step somebody could forget. The four inside
  the material follow nothing, and the session is told so: each is next to a
  classified corner, and the settling that puts fresh nodes on the model would
  otherwise pull them onto the surface and leave the slab no thickness at all.

  **And it is welded to what it was laid against**, so that paving a surface
  builds one blocking and not a heap of blocks in the same place. A side is
  welded when the two points it was laid on were both already nodes of the
  model and an edge joins them — which clicking two of a neighbour's handles
  makes true. A slab's edge has two faces of its own, the broad face it was
  laid on and the thin side, and they are told apart by **which way they
  lean**: from the middle of the shared edge toward the middle of their other
  two corners. The thin side leans into the material, the way the new side
  does; the broad face leans across. The neighbour does not move — the new
  slab comes onto the old face and adopts its thickness, because two slabs a
  tenth of their own diagonal thick are not equally thick and the one being
  laid is the one with no claim. Where two faces lean the same way the side
  is left free rather than welded to a guess, and so is a slab thickened the
  other way: back to back is not the same as side by side.

- **A recorded slab replays.** The script writer spells a corner that
  follows nothing `false` — a `nil` inside a Lua table literal would end the
  list there and make the fourth corner the third — and the reader was
  refusing it: `invalid type: boolean false, expected struct Spot`, on the
  page's own script, at the first corner laid on nothing. A script that
  cannot be replayed is a record of nothing. `true` is still refused, because
  reading it as "nothing" would be reading a mistake as a meaning.

- **Several faces grow into blocks at once, welded where they meet.** Drag
  any one of the selected faces and they all go up, in one command.

  Not a loop over one-at-a-time, and the difference is the point. Two faces
  that meet along an edge raise that edge **once**: the corners it holds are
  shared, so the places they go to are shared, and the blocks they become
  have a face in common rather than two faces in the same place. That face is
  then welded. Raised one after the other, the same two faces leave 16 nodes
  and 16 faces, two slabs leaning on each other; raised together, 14 and 15.

  Each face grows along **its own** normal, which is what an extrusion means,
  and a corner on several of them goes along the mitre so that it ends up the
  full height clear of every face it was on. On a flat patch that is the
  plain normal; on a right-angled crease it is the diagonal, which is what
  makes the two copies meet in the first place.

  Three chosen faces on one edge are refused rather than welded arbitrarily:
  which two of the three blocks meet is not a question with an answer.

  `gecko.extrude_face` takes either shape now — `(face, block, height)` for
  one, or `({{f, b}, …}, height)` for several — and a session that grew one
  block still writes itself out as one face.

- **A selected face can be slid instead of grown from.** Hold `⌥` and drag
  it: the corners move, and no block is built.

  `⇧` was the obvious modifier and is the wrong one — it bands, and held
  over a face it lights that face's chord. `⌥` is free by construction here:
  the shortcut table refuses every press that carries it.

  A slide is a **translation**, which is why one normal serves however many
  faces are selected: a node two selected faces share would otherwise have
  two places to go, and a drag is one gesture with one direction in it. It
  is previewed by moving the nodes in the working copy, as dragging a handle
  is, so what you aim at is the model with the face where you are putting
  it rather than a ghost of something else — and it is committed as one
  `move_node` carrying the rest, so one gesture is one step of undo and each
  corner still does what its own classification allows.

  A drag may also now begin from **any** of the selected faces rather than
  the first: several are selected precisely so one gesture can act on all of
  them, and which one the pointer is over is not a choice anybody made.

- **The mesher no longer keeps a table the size of the mesh it builds.**
  Every mesh node used to be an entry in a `HashMap<Site, u32>`, which is
  what made two blocks share the nodes on the face between them. It was also
  the one table here that grows with the mesh rather than with the blocking:
  measured at a hundred million cells it is gigabytes, and it is why a mesh
  that size could not be built in a tab at all.

  A node's number is arithmetic now. Each cell of the blocking owns a
  contiguous run of numbers — one for a corner, `n - 1` along an edge,
  `(na-1)(nb-1)` across a face, `(ni-1)(nj-1)(nk-1)` through a block — and the
  runs are laid end to end. Two blocks sharing a face reach the same number
  for a shared point without consulting anything, because they reach the same
  cell and count inside it the same way. The tables that remain are sized by
  the blocking: a hundred thousand entries to number a billion nodes.

  Measured on 64 blocks, at ten and at twenty segments an axis:

  |        | held      | at the peak    |
  | ------ | --------- | -------------- |
  | before | 86 B/cell | **139 B/cell** |
  | after  | 62 B/cell | **62 B/cell**  |

  The peak and what is held are now the same number, the transient table
  having gone. Extrapolated to a hundred million cells, 13.8 GB of peak
  becomes 6.2 GB — still too much to hold, which the streaming mesher is
  for, but no longer twice too much before it starts.

  The file is unchanged, byte for byte: the numbering was deliberately laid
  out in the order the three passes already placed their points in, so
  `agree.lua` passes on the digests it always had.

  Each face's lattice and each block's frame are also worked out once and
  kept, rather than asked for again by the pass that follows — six walks of
  the map per block, for lattices that cannot change while a mesh is being
  built.

- **Four tests the coverage figure asked for.** The light node buffer — the one that goes out on every
  pointer move — was exercised only by the browser, and it holds the same
  eight lines of node encoding as the full view; two tests now hold the two
  encoders against each other, on an origin neither would have chosen and
  on a blocking where the group masks are not nought.

  The other two are about smoothing a **patch**: faces laid on a surface
  with no block under them yet. There the measure that refuses a move
  cannot be the corner of a hexahedron, because there is no hexahedron —
  it is the folding of a quadrangle. Every node of an open patch is free to
  walk inwards, and taking that measure away collapses a two-by-two patch
  into a point in three rounds. Measured, by taking it away.

## 0.1.6

Two faults in the shaded hollows that 0.1.5 shipped: a face of the boundary
drawn black, and four views that drew it in the wrong place.

- **A face of the boundary came out pitch black under the shading.**
  Reported with a script of seven lines: one quadrangle of the outside,
  black, where everything round it was shaded properly.

  A face shared by two blocks is drawn once, under one of them, so a block
  can end up contributing a **single** face to what is drawn, its
  neighbours having claimed the other five. The normals were turned
  outward by asking the drawn quadrangles which way was out — and a lone
  flat quadrangle cannot answer that: its own middle lies in its own
  plane, so the question came out exactly zero and the kernel's winding,
  which points into the block, was kept. A hemisphere aimed into the solid
  finds the solid.

  The block's own center answers it, and the surface builder has had it
  all along — it is what a shrink pulls towards, computed from all six of
  a block's faces whether they are drawn or not. Each quadrangle is now
  turned to face away from that, one at a time, so there is no vote left
  to be ambiguous.

- **The shaded hollows are no longer wrong when the window is split.**
  Reported of the four-view split: no shading in the X, Y and Z panes, and
  the 3-D one wrong. It was wrong because the shading is computed over the
  whole canvas while that camera owns a quarter of it, so the occlusion of
  the model was drawn in the empty space beside it.

  Neither way round works, and both were measured: attached to all four
  cameras, each pane's output wipes the one before it and the window goes
  black; told to respect the panes, every one of them draws itself at a
  quarter size in a corner. The shading waits for a single view now. The
  switch stays on and the faces stay opaque while you are in four views —
  what you asked for is what you get back the moment the window is one
  again — and the readout says so rather than leaving a ticked box and a
  flat picture to explain each other.

## 0.1.5

A sheet that comes out without leaving a hole, one layer of blocks to stand
in and walk, and a model whose shape can be read rather than inferred.

- **Taking a sheet out could leave a hole.** Reported of a model built by
  cutting: one layer of sixteen could not be removed, and the refusal cost
  seven blocks.

  Closing a seam merges pairs of nodes by averaging them, so a corner two
  seams share had already moved when the second seam was asked — and the
  map measures the two faces before it joins them, to refuse a pair that
  does not run opposite ways. The pair at fault passed that test at -0.0123
  when it was read and failed it at +0.0028 once its neighbor had pulled
  its corners half way. By then the layer was gone.

  The seam is settled **before** the first sew now rather than by the sews:
  every pair of nodes they will merge is read off the map, grouped, and each
  group written to its own average. The two faces of a seam then coincide
  exactly and no later seam can pull them apart. The measurement that stood
  in for this is gone with it — which face meets which is read off the map,
  as it always should have been.

- **A command that fails leaves the model alone.** The state before it was
  already copied for the undo; it is copied even with the history switched
  off, and put back when the command gives up. An operation that stops half
  way would otherwise leave the model with a hole in it and the reply saying
  nothing happened.

- **A sheet can be taken out without closing behind it.** `N` with a sheet
  lit, or `gecko.remove_sheet(edge, false)`: the two walls the layer was
  between stay where they are, unglued, so the model keeps its shape and
  gains a slot. A layer closed over is a layer you have to cut again to get
  one.

- **A lit sheet stops where the picture does.** A sheet runs through the
  whole model, and a filter — a group hidden, or one layer being looked at
  — shows part of it. Lit over the part that is off screen, it was a thread
  hanging in the air beside the thing you were working on. It is lit over
  the edges the wireframe draws, by the same reckoning, and the marks a cut
  is aimed with go with it.

- **The arrow keys no longer turn the view.** Babylon hands an arc-rotate
  camera the arrows of its own accord: a binding this application never
  asked for, which its own table of shortcuts does not print and could not
  have shown you. It went unnoticed until the arrows were given a job —
  every step from one layer to the next turned the view as well. The view
  moves with the pointer, and with the keys that say they move it.

- **One layer of blocks, on its own.** `J` with a sheet lit narrows the view
  to that layer — its blocks, faces, edges and corners and nothing else —
  and `←` and `→` step to the layer next door; `Esc` comes back out. A face
  in the middle of a hundred blocks cannot be clicked on through the
  ninety-nine in front of it, and taking them off the screen is the whole of
  the remedy, so this is a place to stand rather than a mode to be in: every
  edit works in here exactly as it does outside, and a layer an edit takes
  away puts you back out.

  A layer is named by the **least** of its sheet's edges. It has as many
  names as it has edges, and which one you are handed depends on where you
  were pointing — so without that, the one beside you is not recognisable as
  the one you came from, and the third step of a walk goes back the way it
  came.

- **A lit sheet is no longer drawn over everything.** It runs through the
  model, so a sheet at the back of a hundred blocks read exactly like one at
  the front. It is drawn with the blocking now — hidden by what is in front
  of it, tinted by the faces it lies behind — and pulled a hair towards the
  camera, because its lines are edges of the model drawn a second time and
  the depth test between two things at the same depth decides nothing.

- **Shaded hollows.** A switch in the drawer's **Colors** section shades each
  pixel of a surface by how enclosed it is — ambient occlusion, computed in
  screen space — so a pocket, a notch or a re-entrant corner reads as one.
  It makes the faces opaque while it is on and hands the transparency back
  when it goes off: there is nothing to shade on a surface you can see
  through.

  Its reach is a share of the model's own diagonal rather than a fixed
  distance, which is what lets it read the same on a unit cube and on a part
  in millimetres. Below full opacity it does nothing at all: the buffer
  it reads holds one depth and one normal per pixel, and a surface you can
  see through puts several surfaces on that pixel. The wireframe and the node handles are kept out of it: left
  in, a line lying along an edge of the model shades the faces either side of
  it, and on a grid of twenty-seven blocks that alone darkens three quarters
  as many pixels again.

- **The mark a cut is aimed with is hidden by the model too.** The lit
  sheet stopped being drawn over everything in this version; the mark along
  it did not. It is drawn on every edge of the sheet at once, so a cut aimed
  at the far side of the model came out in front of the blocks it runs
  behind — a lot of red in the wrong place. It is drawn with the blocking
  now, a little in front of the sheet it lies along, which it has to win
  against: the two are the same edges at the same depth.

- **A second light, from one side.** A hemispheric light shades a surface by
  how far it faces *up*, and the four walls of a block all face sideways — so
  they came out exactly the same color and a block read as a flat outline
  with a lid. A dim directional light, fixed in the world rather than carried
  by the camera, is what makes the corner between two walls visible.

- **The surfaces' normals point out of the model.** They are computed from
  the winding of the kernel's quadrangles, which points in. Nothing had ever
  read them — a two-sided material flips a back-facing normal before it
  lights it — until the shading above read them out of a geometry buffer,
  pointed its hemisphere into the solid, and found the solid everywhere.

## 0.1.4

Groups of blocks, the display settled around them, and editing that keeps
up with a model of a hundred blocks.

- **Editing is no longer quadratic in the size of the model.** Reported as
  "every operation is far too slow past a hundred blocks", and measured:
  at a hundred and twenty-five blocks the view buffer took 3.2 seconds, a
  read of the faces 3.1, and a cut 0.6. They take 60 ms, 63 ms and 125 ms
  now. On B10, a cut at forty blocks went from 181 ms to 40.

  Then, reported again of pillowing: a chord pillowed on a
  hundred-and-fifty-block grid took **7.5 seconds**, and takes **57 ms**.
  Laying that grid out in the kernel went from 9 seconds to 44 ms.

  - **Walking the map was the whole of it.** This map answers "which cell
    is this dart in?" by walking the dart's orbit, and its own iterators
    ask that **once per dart** — so listing the nodes walked every node's
    orbit once for each dart of it. The kernel goes up the darts once now
    and marks each orbit off as it walks it, which is one walk per cell;
    and it does so inside a single transaction, because a single β lookup
    in this map is a transaction of its own and that, rather than the
    walking, was most of the cost. Naming the cells of a model went from
    27 ms to 2.9.
  - **Finding the face of a block at four given corners** read every face
    in the map. A pillow does that a few hundred times — once per wall
    between two layers it has laid — so it is read from the one block now.
  - **Laying a block and gluing a face each named the cells of the whole
    map** afterwards, to catch whatever the operation left unnamed. A block
    laid can only have left its own cells unnamed, and a face sewn its own
    face, edges and nodes; so each looks only there. The names come out in
    the same order either way, which matters: a name is what a script says,
    and renumbering would make every script written before mean something
    else. There is a test with the names in it.
  - Finding a cell **by name** walked the whole map, and the walks were
    inside loops: once per quadrangle to ask which groups its block is in
    and whether it is a patch, once per node a cut makes to put it back on
    the geometry. A face of a block now carries the volume it belongs to,
    which the caller already had in hand, and a name is looked up from a
    hint — where that name was last seen — checked against the cell's own
    tag, falling back to the walk.
  - The **geometry's bounding box** was worked out again after every edit,
    walking every point of the model to find a number that cannot have
    changed. It is worked out once per geometry now.

- American spelling throughout — `color`, `center`, `neighbor`, `behavior`,
  `gray` — in the code, the interface, the documentation and the keys a
  script reads. **A script that reads `face.centre` must now read
  `face.center`.**

- **Groups of blocks.** A group is a part of the model you have named: a set
  of blocks, and with them everything they are made of — a face, an edge or
  a node is in a group when a block it bounds is. A block can be in several
  at once, and sixty-four groups is what a session holds.

- A **Groups** band in the display drawer has a row each: show it, rename it
  in place, put the selected blocks in or take them out, and forget it. The
  last row is the blocks in no group, which is how the unnamed part of a
  model is hidden.

- A group marked **written** becomes a variable of the mesh: one cell array
  per group, named after it, holding 1 for the cells of its blocks. One
  array per group rather than one number per cell, because a cell can be in
  several and a column of numbers could only name one.

- A fourth **coloring**, `group`, for nodes, edges, faces and blocks alike —
  and the colorings are a list now rather than a row of boxes, since four
  of them do not fit across the drawer.

- Membership lives on the block itself, one bit each, so the model carries
  it: cutting a block in two leaves both halves in whatever it was in, and
  welding two gives the result both their groups. Nothing recomputes it and
  nothing can go stale.

- **The local server says how long its answers may be kept**: never for the
  page, for ever for the bundle under `assets/`, whose name carries a hash
  of what is in it. A reply with no freshness on it is one a browser may
  guess about, and guessing wrong here means rebuilding and seeing the
  application you had before.

- A cell is drawn when **any** block it bounds is drawn, so hiding a group
  leaves the blocks beside it whole — a shared edge stays because the block
  still on screen still needs it.

- `gecko.group`, `gecko.ungroup`, `gecko.rename_group`, `gecko.drop_group`
  and the `gecko.groups()` read, so a session's groups are in its script and
  replay with it.

## 0.1.3

Three things from a session with it, and one of them took the measuring.

- **A session no longer bleeds classification.** A node an operation makes
  among nodes that are on the model is now put on the model too, when it is
  near enough — the same question `classify` asks, asked at the moment the
  node appears. Before, a node made where there was nothing to inherit
  landed on the geometry claiming nothing, every edge through it was
  cleared, and anything cut out of *those* inherited nothing in turn.
  Measured on one session: twenty of seventy-two nodes sat on the model
  claiming nothing and a fifth of the edges had been cleared because of
  them; the same session now ends with none and with thirty more edges
  classified.
- The reach it is judged by is the one the **last `classify` was given**,
  not a number in the kernel: somebody working at five percent means five
  percent.
- And the limit, which is the other half of it: a block somebody has just
  **made** is never pinned behind their back, however exactly it sits on
  the geometry. Dropping a cube on a model is not a statement that the cube
  follows it.
- **An edge keeps what nothing contradicts.** It used to be cleared the
  moment either end stopped being classified — so a pillow, which rightly
  lets go of the four nodes it draws into the material, wiped every edge
  that touched one of them though none had moved. Now the end that is still
  classified has to disagree before the edge gives anything up, and an edge
  with nothing at either end — the inside of the model — still goes.
  Nothing is handed to an edge from one end alone. Faces follow the same
  rule, one dimension up.
- **The handles can be hidden**, like edges, faces and blocks — and hidden,
  they are out of the pointer's way as well: a handle you cannot see but
  can still grab moves the model with nothing on screen to say what you took
  hold of.
- The mesh's controls sit directly in their band: the **skin** fold inside
  it is gone. A mesh has no groups to tell apart, so the fold was a step to
  open before reaching three controls.

## 0.1.2

Smoothing, a session you can replay, and a handful of things from using it.

- **`R` smooths the blocking**: every node towards the middle of its
  neighbors, as far as its classification allows. A node on a curve slides
  along the curve, one on a surface over the surface, one on a geometric
  corner stays, and so does one on the outside of the blocking that is
  classified on nothing — it has no surface to be kept on, and moving it
  would pull the blocking off the model. How many passes and how far each
  one goes are two sliders under *Operations*.
- **Smart**, in the sense the word has in the literature as well as in the
  slider: a move is taken only if it does not make the worst corner of the
  cells round that node any worse, measured by the scaled Jacobian. A plain
  Laplacian on a concave boundary turns cells inside out; this does not. On
  a blocking that is already as good as it gets it does nothing, and says
  so.
- **Dragging a node still moves only that node** (and whatever was picked
  with it). Smoothing outwards from a drag, ring by ring, was built for this
  release and taken out again before it: what you want of a drag is the node
  under the pointer, and a neighborhood rearranging itself around it is a
  surprise rather than a help.
- A **frozen** node is drawn as a **cube** rather than a ball, in whatever
  color the coloring gives it. It was gray of its own until now, which hid
  the two things the coloring is for — which node it is, and what it
  follows. Being out of play is a thing about the cell, so it is said with
  the shape. The color panel has one fewer swatch for it.
- **Open takes a `.lua`**, and replays it from an empty session: blocks,
  geometry and mesh go first, because a script describes a session from
  nothing and run on top of a model it would describe something else. The
  console opens, since that is where a script talks. One step of undo takes
  the reset and the whole replay back together. `gecko.reset()` is the same
  thing from a script. Dropping a `.lua` on the window works too — and
  dropping the `.msh` beside it is how a script that opens a geometry by
  name finds it.
- **`E` takes the mesh away again.** Before, the only thing that got rid of
  a mesh was editing the blocking, so a mesh was something you could put on
  screen and not take off. The **show** box in the display panel is still
  the other thing: that one hides it and keeps it.
- **`⇧Esc` drops the last thing you picked** and leaves the rest. `Esc`
  clears the whole selection, which is a lot to pay for one wrong click in a
  selection that took a while to build. It knows which of the selections
  your last click went to, so mixing nodes, faces, edges and geometric
  entities does not confuse it.
- **The classification colors are told apart.** They were three violets of
  one hue and a corner looked like a curve: measured in OKLab, the nearest
  pair was 11 units in the dark theme and 6 in the light. They are a ramp
  now — pink, magenta, violet, the lightness falling and the hue turning the
  same way — at 22 and 20, and each stays clear of the color an
  unclassified cell wears, which the old deep violet was 7 from. The
  measurement is a test, so they cannot be quietly harmonized back together.
- **How big the blocking is drawn is a setting**: *handles* and *edges*
  under *Drawing*, in pixels on the screen. The wireframe is a ribbon rather
  than a GL line to make the second of those possible at all — a GL line is
  one pixel wide whatever anybody asks for. Measured on a blocking of 1500
  edges, the ribbon costs about 2% more to rebuild and 1.5% more per frame
  than the line it replaces at the default width, and 10% more per frame at
  the widest setting the panel offers.
- **Nodes, edges and faces start colored by their classification.** Putting
  the blocking onto the model is what a session is for, so what each cell is
  attached to is the thing worth seeing first. Blocks keep their own
  coloring, being classified on nothing.
- **A bounding box hugs the model**: 2% of each half-side rather than 10%,
  in the kernel's default as well as in the drawer, because pulling its
  faces onto the surfaces nearby is the first thing anybody does with a box.
- **"follows" is gone** — this codebase's word for a cell's classification,
  which meant nothing to anybody else. The drawer says `class`, the fold
  line says `classification`, and the help card and the guide say "what a
  cell is classified on".
- **Fixed**: the blocking was drawn an origin-shift away from the model
  after an edit that took a lot of it out — a removed sheet, say. Everything
  in the scene is sent rebased on one origin so that f32 coordinates stay
  small, and the blocking's own view was choosing an origin of its own, over
  the blocking alone: a smaller world than the one the session settles over,
  and so one that walks past its own threshold first. The mesh and the
  geometry never heard about the new center, so they stayed where they were
  and the block structure jumped. The view now takes the origin it is given.
  The page also fetches the geometry and the mesh again when the origin does
  move, which it had no reason to do before: neither buffer had changed, only
  the middle of the scene they are drawn around.
- **Fixed**: `gecko.move_node` dropped its `smooth` option without a word,
  so a script — and the session's own replayed history — got the plain move
  where a drag had rippled. The reference had documented the option all
  along.
- **Fixed**: a table returned to the console printed only eight of its
  fields, in hash order, so `gecko.stats()` hid `blocks` behind an ellipsis
  as soon as `patches` joined the table. Twenty-four fields print now, list
  entries in index order and named ones in name order, so adding a field no
  longer moves the others.

## 0.1.1

A readable help card, and a window that says which gecko it is.

- The card of shortcuts **fits the window**. It had grown past the bottom of
  the screen — at 1280×800 it wanted 1087 pixels and had 670 — because one
  group had reached twenty shortcuts. The keys that act on the geometry and
  the ones about meshing are groups of their own now, and the card lays them
  out in columns the browser balances rather than in a grid that aligns its
  rows. Measured at three ordinary window sizes: nothing to scroll at any of
  them.
- The **name and version** are on the card's title and at the left of the
  readout along the bottom, taken from the kernel itself — the same number a
  release is tagged with.

## 0.1.0

The first release. A blocking toolkit that runs in a browser tab, scripted in
Lua, with a kernel on a 3-dimensional combinatorial map.

### The application

- **One page, carrying everything.** The kernel compiled to wasm and a Lua
  5.4 interpreter beside it, so the mouse and a script drive the same session
  with nothing in between — and a static host is all it needs.
- **A script console**, with the session's own history in it: every
  interactive operation writes the Lua line that would repeat it, and the
  whole session saves as a script that runs.
- `gecko` on this machine: serves the page, opens it, and gives a script real
  files to read and write. `gecko run script.lua` runs one with no browser.

### Blocks

- Blocks made, glued, removed and extruded; **sheets** cut and removed;
  **chords** removed, and wrapped in an **o-grid** along their whole length.
- Gluing welds each pair of corners **at the point between them**, with
  classification overriding it: a frozen corner does not move, and a welded
  corner lands on the more constrained of the two entities — the curve where
  two surfaces meet, rather than either of them.

### Blocking a surface first

- A **quadrangle laid on a geometric surface**: pick it, press `Q`, and the
  four corners the boundary turns a right angle at become a face already
  classified — corners on corners, face on the surface, each side on the
  curves between two chosen corners.
- A side that runs along several curves is one entity, a **composite**, so a
  node slides along the whole of it and the mesher follows it.
- Those faces are cut, removed and welded like anything else, and a whole
  connected sheet of them is **extruded into blocks** in one go — either
  way, since a sheet has no inside — with the blocks glued to each other
  along the faces the shared edges were raised into.
- **Pillowing** a patch of faces, which draws the picked blocks in and lays a
  layer into the space, leaving the model's surface where it was.
- Five steps of **undo**, and the depth is a setting.

### Geometry and meshing

- A gmsh surface mesh as the geometry, with corners, curves and surfaces
  recovered from it, and a bounding box built round the whole model or round
  the surfaces you choose.
- **Classification** by proximity for nodes and by inference for edges and
  faces, projection onto whatever a cell is pinned to, and nodes frozen
  against being moved at all.
- **Transfinite meshing** of every block — Coons patches, then Gordon volumes,
  each projected before the level above reads it — with the segment count a
  property of the sheet, and export as a VTK legacy file.

### Files

- In a terminal, a name is a name. In a page **drop the file on the window**
  or open it with the toolbar, and a script opens it by name from then on: a
  browser cannot be given a path, so the file goes the other way.
- `gecko.files()` says what can be opened by name wherever the script runs.

### The interface

- Four views at once (3D, X, Y, Z), a lasso over the node handles, drag
  several nodes together, and every operation on the keyboard as well as in
  the toolbar.
- Colors, shrink, visibility and the geometry's groups all adjustable and
  remembered; a picture saved at the size you ask for.
