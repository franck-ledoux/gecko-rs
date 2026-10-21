# Scripting

The console is in the page: `;`, or the chevron in the toolbar. It is a
real Lua 5.4 interpreter, running in a WebAssembly module beside the
kernel's, so loops and conditionals drive the model the same way the mouse
does.

```lua
for i = 1, 3 do
  gecko.create_cube{ origin = {i * 1.5, 0, 0} }
end
```

Enter runs what you have typed; shift-enter makes a new line. The arrows
walk back through what you have run. `Esc` puts the console away — and a
key pressed into it is **typing**, console or not, so the application's own
shortcuts are asleep while it has the focus. Click the model to wake them.

## The session writes itself down

Every command the interface sends appears in the console as the line of Lua
that would have made it:

```
gecko.create_cube{ origin = {0, 0, 0}, size = 1 }   ← the button
> gecko.box_geometry{ margin = 0 }                   ← typed
```

So the history is one list whichever door you used. `gecko.script()` reads
it back, and **save** writes it out as a `.lua` you can run again.

An undo takes its line **off** rather than writing `gecko.undo()`: what
comes out is a recipe for where the session now is, not a diary of
everything anybody tried.

## Running one again

**Open** takes a `.lua` as well as a `.msh` — the same button, since "open"
is the same question whether the file is a model to work on or a session to
play back. Dropping a script on the window does the same.

The session is **emptied first**. A script describes a session from nothing
— it starts by making a cube or opening a geometry — so run on top of a
model it would describe something else. Blocks, geometry and mesh all go,
and the console says so before the script's own output.

It is **one step of undo**: the reset and the whole replay come back off
together if it was the wrong file.

A script that opens a geometry by name needs that file within reach. In the
terminal that is the folder you ran `gecko` in; in the page, **drop the
`.msh` and the `.lua` together** — a dropped file is kept by name, and the
script finds it there.

!!! note "A geometry is named, not carried"
    `gecko.load_geometry("cylinder.msh")` sends the *contents* of the file
    to the kernel, because a browser has no path to give. The script keeps
    the **name**, so the `.msh` has to be beside the script when it runs
    again. The header the file is written with says so.

## Writing it

Required arguments are positional; optional ones live in a table at the
end. That is why `gecko.mesh{ segments = 8 }` has no brackets — Lua's own
shorthand for a call with one table.

|                                                                             |                                            |
| --------------------------------------------------------------------------- | ------------------------------------------ |
| `gecko.create_cube{ origin = {0,0,0}, size = 1 }`                           | and `add_block{ … }` with eight corners    |
| `gecko.face_on_surface(tag)`                                                | a quadrangle on a geometric surface        |
| `gecko.add_face{ {x,y,z}, … }`                                              | one by hand, with four corners             |
| `gecko.remove_face(id)`                                                     | one face of a patch                        |
| `gecko.extrude_patch(face, 0.5)`                                            | the whole sheet, into blocks; ± either way |
| `gecko.smooth{ rounds = 5, share = 0.5 }`                                   | a smart Laplacian, geometry respected      |
| `gecko.remove_block(id)`, `gecko.glue_faces(a, b)`                          |                                            |
| `gecko.extrude_face(face, block, height)`                                   |                                            |
| `gecko.pillow({ {face, block}, … }, { tolerance = 0.02 })`                  |                                            |
| `gecko.remove_chord(face, block)`                                           | the whole thread through a face            |
| `gecko.pillow_chord(face, block, { tolerance = 0.02 })`                     | an o-grid along it                         |
| `gecko.split_sheet(edge, 0.5)`, `gecko.remove_sheet(edge)`                  |                                            |
| `gecko.set_segments(edge, 8)`                                               | on the whole sheet                         |
| `gecko.move_node(id, {x,y,z}, { carrying = {…}, snap = 0 })`                | moves those nodes and nothing else         |
| `gecko.classify_node(ids, dim, tag)`, `unclassify_node(ids)`                | one id or a list                           |
| `gecko.freeze_node(ids)`, `gecko.thaw_node(ids)`                            |                                            |
| `gecko.load_geometry(path)`, `gecko.clear_geometry()`                       |                                            |
| `gecko.files()`                                                             | what can be opened by name here            |
| `gecko.box_geometry{ surfaces = {…}, margin = 0.02 }`                       | 2% round each half-side by default         |
| `gecko.classify{ tolerance = 0.02 }`                                        |                                            |
| `gecko.mesh{ segments = 10 }`, `gecko.clear_mesh()`                         |                                            |
| `gecko.export_vtk(path)`                                                    | meshes first if it has to                  |
| `gecko.undo()`, `gecko.set_history(n)`                                      |                                            |
| `gecko.reset()`                                                             | an empty session: no blocks, no geometry, no mesh — one step of undo |

And the reads, each a plain table: `nodes()`, `edges()`, `faces()`,
`blocks()`, `entities()`, `stats()`, `sheet(edge)`, `chord(face, block)`,
`vtk()`, `script()`.

```lua
for _, node in ipairs(gecko.nodes()) do
  if node.on == nil then              -- on nothing: nil, not a sentinel
    gecko.freeze_node(node.id)
  end
end
```

A refusal from the kernel is raised as a Lua error, so a chunk stops where
it went wrong rather than carrying on over a mistake. `pcall` catches it if
that is what you want.

## Without a browser

The same API, the same scripts, from a terminal:

```bash
gecko run sweep.lua
```

```lua
gecko.load_geometry("cylinder.msh")
gecko.box_geometry{ margin = 0 }
for _, n in ipairs({ 2, 4, 8 }) do
  gecko.mesh{ segments = n }
  local m = gecko.stats().mesh
  print(string.format("%d segments: %d hexahedra", n, m.hexes))
  gecko.export_vtk(string.format("mesh-%d.vtk", n))
end
```

`gecko run` and the page are **two sessions**, not one: a script run in the
terminal shows nothing, and a script run in the console is the one you
watch. What they are not is two implementations — the binding is one trait
four methods wide, and a test runs the same script through both hosts and
checks the bytes it produces agree.

## Files

A script opens a file **by name**. What a name reaches depends on where the
page came from:

| where it came from                        | what a name reaches                                       |
| ----------------------------------------- | --------------------------------------------------------- |
| the terminal (`gecko run`)                | the working directory; absolute paths in full             |
| served by `gecko` on your machine         | the folder `gecko` was started in; absolute paths in full |
| a static host, such as the published page | the files you handed the page                             |

`gecko.files()` lists them, wherever you are.

### On the published page: hand it the file first

**Drop a file on the window**, or open it with the toolbar's button, and
from then on a script opens it by name:

```lua
gecko.load_geometry("cylinder.msh")   -- dropped a moment ago
gecko.box_geometry{ margin = 0.05 }
gecko.classify()
```

A folder written in front of the name is ignored — `data/cylinder.msh`
finds the same file — because on a page there is no folder for it to be
wrong about.

!!! note "Why it has to be handed over, rather than asked for"
    A browser cannot open `/home/you/cylinder.msh` by name, and should not
    be able to: that is a boundary, not a gap. The picker that *could* ask
    the reader for one cannot be used here — it is **asynchronous**, and
    Lua asks for a file in the middle of a loop with no way to hand the
    event loop back. A file already in the page needs no waiting, which is
    why what you hand over is kept rather than read and thrown away.

Writing is the mirror of it: `gecko.export_vtk("mesh.vtk")` writes a file
when you launched the app yourself, and hands the browser a **download**
when you are using the published page.

!!! note "A read that fails is not a path that was not absolute enough"
    It is the first thing anyone tries, so the message says what to do
    instead, and `gecko.files()` says what the page is holding.
