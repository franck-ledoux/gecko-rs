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

!!! note "A geometry is named, not carried"
    `gecko.load_geometry("cylinder.msh")` sends the *contents* of the file
    to the kernel, because a browser has no path to give. The script keeps
    the **name**, so the `.msh` has to be beside the script when it runs
    again. The header the file is written with says so.

## Writing it

Required arguments are positional; optional ones live in a table at the
end. That is why `gecko.mesh{ segments = 8 }` has no brackets — Lua's own
shorthand for a call with one table.

|                                                              |                                            |
| ------------------------------------------------------------ | ------------------------------------------ |
| `gecko.create_cube{ origin = {0,0,0}, size = 1 }`            | and `add_block{ … }` with eight corners    |
| `gecko.face_on_surface(tag)`                                 | a quadrangle on a geometric surface        |
| `gecko.add_face{ {x,y,z}, … }`                               | one by hand, with four corners             |
| `gecko.remove_face(id)`                                      | one face of a patch                        |
| `gecko.extrude_patch(face, 0.5)`                             | the whole sheet, into blocks; ± either way |
| `gecko.remove_block(id)`, `gecko.glue_faces(a, b)`           |                                            |
| `gecko.extrude_face(face, block, height)`                    |                                            |
| `gecko.pillow({ {face, block}, … }, { tolerance = 0.02 })`   |                                            |
| `gecko.remove_chord(face, block)`                            | the whole thread through a face            |
| `gecko.pillow_chord(face, block, { tolerance = 0.02 })`      | an o-grid along it                         |
| `gecko.split_sheet(edge, 0.5)`, `gecko.remove_sheet(edge)`   |                                            |
| `gecko.set_segments(edge, 8)`                                | on the whole sheet                         |
| `gecko.move_node(id, {x,y,z}, { carrying = {…}, snap = 0 })` |                                            |
| `gecko.classify_node(ids, dim, tag)`, `unclassify_node(ids)` | one id or a list                           |
| `gecko.freeze_node(ids)`, `gecko.thaw_node(ids)`             |                                            |
| `gecko.load_geometry(path)`, `gecko.clear_geometry()`        |                                            |
| `gecko.box_geometry{ surfaces = {…}, margin = 0.1 }`         |                                            |
| `gecko.classify{ tolerance = 0.02 }`                         |                                            |
| `gecko.mesh{ segments = 10 }`, `gecko.clear_mesh()`          |                                            |
| `gecko.export_vtk(path)`                                     | meshes first if it has to                  |
| `gecko.undo()`, `gecko.set_history(n)`                       |                                            |

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

A script reaches files only where there are files to reach.

| where the page came from                  | what a path means                                            |
| ----------------------------------------- | ------------------------------------------------------------ |
| the terminal (`gecko run`)                | the working directory; absolute paths in full                |
| served by `gecko` on your machine         | the folder `gecko` was started in; absolute paths in full    |
| a static host, such as the published page | **there are no files**, and no path of any shape reaches one |

So `gecko.load_geometry("cylinder.msh")` opens the file beside the one you
launched `gecko` in, and `gecko.export_vtk("mesh.vtk")` writes next to it.
From the published page the same two lines find nothing and hand you a
**download** respectively — the geometry is opened with the toolbar's own
button there, which reads the file through the browser rather than through
a path.

!!! note "A read that fails is not a path that was not absolute enough"
    It is the first thing anyone tries, so the message says which of the
    two it was: no such file *here*, or no files at all. A browser cannot
    open `/home/you/cylinder.msh` by name and should not be able to; what
    `gecko` adds when it serves the page is exactly that door, and only
    onto the folder you started it in.
