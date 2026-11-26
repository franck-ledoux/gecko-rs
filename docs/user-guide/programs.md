# Scripts run by programs

A person running a script reads what it prints and looks at the page. A
program running one — a test bench working through a set of parts, an agent
planning a blocking — can do neither, and has to be told in a form it can
read. Three things are for that reader: a run written up as JSON, the
geometry described in numbers, and pictures drawn without a browser.

All three work from a terminal, with `gecko run`. The geometry reads work in
the page's console too.

## A run, as JSON

```bash
gecko run part.lua --json
```

prints **one JSON object** on stdout, and nothing else: what the script
printed is captured into it rather than mixed with it.

| field      | what it holds                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------- |
| `ok`       | whether the script got to the end                                                              |
| `error`    | why not, with the file and line, or `null`                                                     |
| `printed`  | the lines `print` and `io.write` wrote, in order                                               |
| `returned` | what the script returned, as JSON — `return { blocks = 13 }` arrives as an object              |
| `stats`    | `gecko.stats()` as the run left it                                                             |
| `blocking` | the block structure, whole, and what looks wrong with it: `gecko.blocking()`, below            |
| `quality`  | the mesh under each metric — `scaled_jacobian`, `skewness`, `aspect_ratio` — or `null`         |
| `fidelity` | the mesh held against the geometry, as `gecko.fidelity()` gives it less its drawing, or `null` |
| `seconds`  | how long it took                                                                               |

The report is written **when the script fails as well**: `ok` is false, and
the counts, the blocking, the quality and the fidelity are of the session as
it was at the error, which is the state worth having. The exit status still says which, so a shell
can branch on it without parsing anything.

```bash
gecko run part.lua --json | jq '.quality.scaled_jacobian.min'
```

## The block structure, written up

`gecko.blocking()` — and `blocking` in the report — is the whole block
structure, with what looks wrong with it found before anything is meshed:

| field     | what it holds                                                                                                      |
| --------- | ------------------------------------------------------------------------------------------------------------------ |
| `summary` | how many blocks, how many inverted, the worst block and its figure, how many problems of each kind, `not_followed` |
| `issues`  | each problem: its `kind`, the cells it is about, and a `message` that says why                                     |
| `blocks`  | `id`, its eight `corners` in VTK order, `center`, and `jacobian`: the scaled Jacobian of its eight corners         |
| `nodes`   | `id`, `pos`, `on` (`{dim, tag}` or nothing), `frozen`, `outer`: whether it is on the outside of the blocking       |
| `edges`   | `id`, its two `nodes`, `on`, and the `sheet` it is in                                                              |
| `faces`   | `id`, the `blocks` it bounds — one when it is `outer` — its four `nodes` and `on`                                  |
| `sheets`  | `id`, the `segments` it is cut into and whether somebody `set` them, how many `blocks`, its `edges`                |

The problems it looks for:

| `kind`                  | what it means                                                                                            |
| ----------------------- | -------------------------------------------------------------------------------------------------------- |
| `inverted_block`        | a block turned inside out: some corner's scaled Jacobian is below nought                                 |
| `poor_block`            | a block whose worst corner is below 0.2 — it rarely meshes into cells any better                         |
| `outer_face_on_nothing` | a face on the outside follows no surface; `corners_on` says what each corner is on, which is usually why |
| `outer_node_on_nothing` | a node on the outside follows nothing, so it neither stays on the part nor moves when smoothed           |
| `node_off_its_entity`   | a node further from what it is classified on than rounding                                               |
| `corner_not_held`       | a corner of the geometry with no node on it                                                              |
| `curve_not_followed`    | a curve of the geometry no edge is classified on                                                         |
| `surface_not_covered`   | a surface of the geometry no face is classified on                                                       |

The last three are **boundary alignment failing**, said before the mesh is
built rather than measured after it by `gecko.fidelity()`; `summary.not_followed`
lists their tags. A part with a hole that never got its own blocks shows up as
the hole's surface and rims there.

## The geometry, described

A blocking is planned from facts about the part — the hole is a cylinder of
radius 2 along x, this face is the plane z = 7.5 — and a faceting states none
of them. `gecko.geometry()` works them out:

```lua
gecko.load_geometry("B1.msh")
gecko.merge_smooth()
local g = gecko.geometry()
for _, s in ipairs(g.surfaces) do
  print(s.tag, s.shape.kind, s.shape.radius, s.area)
end
```

It answers for the model **as it now stands**: a merge is one entity, with
its `members` and each of its `pieces` described on its own, and what a merge
swallowed is left out — the way the page draws it.

| entity    | fields                                                                                                       |
| --------- | ------------------------------------------------------------------------------------------------------------ |
| the whole | `bounds = {min, max}`, `span`                                                                                |
| a corner  | `tag`, `point`, `curves` through it                                                                          |
| a curve   | `tag`, `length`, `bounds`, `ends` (the corners at its ends), `closed`, `runs`, `surfaces` it bounds, `shape` |
| a surface | `tag`, `area`, `bounds`, `normal` (the mean, facing out), `curves` round it, `shape`                         |

and a `shape` is one of:

| `kind`     | for     | with                                                                                              |
| ---------- | ------- | ------------------------------------------------------------------------------------------------- |
| `line`     | curve   | `direction`                                                                                       |
| `circle`   | curve   | `center`, `normal`, `radius`, `sweep` in degrees — 360 for a whole one                            |
| `plane`    | surface | `normal`, `offset` — every point p of it has `normal · p = offset`                                |
| `cylinder` | surface | `axis`, `point` on it level with the middle, `radius`, `sweep`, `length`, `convex`                |
| `sphere`   | surface | `center`, `radius`, `convex`                                                                      |
| `other`    | either  | nothing: it is none of these — an intersection of two cylinders, or two different surfaces merged |

A shape is only claimed when **every node** is on it, within a thousandth of
the entity's own size: gmsh puts its nodes on the exact surface, so a real
cylinder fits to rounding and anything else misses by far more.

`convex` says which way the surface turns, as its triangles' winding tells it
— gmsh writes that outward from the volume. A convex cylinder has the
material inside it, an outer wall or a boss; one that is not is a hole, or an
inner wall.

Two more reads go with it:

```lua
gecko.polyline(5)          -- a curve's nodes in order: { runs = {{p1, p2, …}}, closed, ends }
gecko.closest({4, 0, 3})   -- the nearest corner, curve and surface, each with
                           -- its tag, the point on it and how far
gecko.closest({4, 0, 3}, { dim = 2, tag = -1 })   -- onto one entity
```

## Pictures without a browser

```lua
gecko.render("part.png")
gecko.render("worst.png", { below = 0.5, view = "front" })
```

draws the session in software and writes a PNG. What is drawn by default
depends on what there is: the **mesh**, coloured by quality, when there is
one; the **blocks** over a faded geometry when there are blocks; and the
**geometry**, every entity labelled with its tag, when that is all.

| option             | what it does                                                                                                          | default                 |
| ------------------ | --------------------------------------------------------------------------------------------------------------------- | ----------------------- |
| `width`, `height`  | the picture's size, 16 to 4096 pixels                                                                                 | 1200 × 900              |
| `view`             | `"iso"`, `"front"`, `"back"`, `"left"`, `"right"`, `"top"`, `"bottom"`, or a direction `{x, y, z}` towards the viewer | `"iso"`                 |
| `up`               | the direction up the picture                                                                                          | z, or y looking along z |
| `show`             | any of `"geometry"`, `"curves"`, `"corners"`, `"blocks"`, `"mesh"`, `"labels"`                                        | what there is, as above |
| `color`            | the mesh by `"quality"` or by `"block"`                                                                               | `"quality"`             |
| `below`            | cells whose scaled Jacobian is below this, drawn red over everything, wherever they are                               | none                    |
| `center`, `radius` | frame on this ball rather than on everything drawn — a close-up                                                       | everything              |

Labels read `S` for a surface, `C` for a curve, `P` for a corner and `B` for
a block, then the tag: the same numbers `gecko.geometry()` and
`gecko.blocks()` give, so a reader can name what it sees.

The projection is orthographic and the lighting a headlight. It is a
picture to look at, not a rendering to publish: the page's camera button
saves those.

!!! note "Only in a terminal"
    In the page, `gecko.render` says so: a picture is bytes, and the page's
    script host passes text. The camera button in the toolbar saves one there.
