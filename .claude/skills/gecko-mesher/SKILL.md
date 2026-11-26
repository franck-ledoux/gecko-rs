---
name: gecko-mesher
description: Build block-structured hexahedral meshes with gecko-rs (Gecko) — plan a blocking for a CAD part, script it in Gecko's Lua API, mesh it, and drive the scaled Jacobian up, the block count down and the mesh onto every curve and corner of the part. Use this whenever the user wants to hex-mesh, block, or mesh a model with Gecko or gecko-rs, mentions a Mambo model (B0…B99, M…, S…), a .msh/.step part to mesh, scaled Jacobian, O-grids, sheets, blocking or boundary alignment, asks why a Gecko mesh is bad or does not follow the geometry, or wants feature ideas for Gecko drawn from meshing real parts — even if they do not say "skill" or name the API.
---

# Meshing with Gecko

Gecko (gecko-rs) builds a mesh the way an analyst does by hand: a handful of
blocks put round the part, each attached to the surfaces, curves and corners
it should follow, each cut into a structured grid of hexahedra. Your job is to
choose those blocks, place them, attach them, and keep improving them until
the mesh meets the quality rules — and to write down every place where Gecko
made that harder than it should be, because that list is how Gecko improves.

Four things are judged, and they pull against each other:

1. **Quality** — the scaled Jacobian (SJ) of every cell: 1 is a perfect cube,
   below 0 is inverted. The minimum matters most; the mean and the share of
   cells below 0.5 or 0.7 next.
2. **Few blocks** — a blocking a person could read and edit. Every block you
   add has to buy quality or alignment.
3. **Boundary alignment** — every curve of the part has mesh edges running
   along it, every corner has a node on it, and the skin lies on the surfaces.
4. **A fundamental blocking** — the boundary captured by the *layers*: one
   sheet of blocks running along each geometric surface, one or more columns
   of blocks along each curve (the user's theory of fundamental sheets, in
   `references/quality-rules.md`). This is what puts well-shaped layers of
   cells along the boundary, where the analyst needs them, and it is decided
   by the topology — so it is planned in step 2, not repaired at the end.

The rules the user holds a mesh to are in `references/quality-rules.md`.
Read it before you start; it says what "done" means.

## Gecko first: ask for the feature, do not write the script

When Gecko cannot do something the job needs, the user wants to hear it as
**a feature request for Gecko**, not to receive a Python script that does it
on the side. A helper that measures the mesh, searches vertex positions or
re-derives what the kernel knows solves one part once and hides what Gecko
should do for every part. So before writing any such helper: look for a way
inside Gecko, write the request (the command or read you want, with its
arguments), and only if the part cannot be finished otherwise write the
smallest stopgap — named as one, pointing to its request.
`references/feature-requests.md` says how, and lists the requests the
skill's own scripts already stand for: `fundamental.py`, `optimize.py`,
`check.py` and parts of `blocklib.lua` are stopgaps too, to use while the
features do not exist and to say so when you do.

## Before anything: the tools

- The binary: `gecko` on the PATH, or `<gecko-rs>/target/release/gecko`.
  Build it with `cargo build --release -p gecko-cli` in the gecko-rs checkout
  if it is missing **or older than the source** — a stale binary silently
  lacks reads this skill relies on (`gecko.blocking()`, `gecko.geometry()`,
  `gecko.render`, `--json`). `gecko run x.lua --json` failing on an unknown
  function means rebuild.
- The geometry has to be a gmsh **MSH 2.2** surface mesh. For a STEP file:
  `python3 scripts/step_to_msh.py part.step part.msh [size_factor]`.
- Work in a folder of its own per part, with the `.msh` beside the script:
  `gecko run` opens files relative to where it runs. Copy
  `scripts/blocklib.lua` there too.

Paths in this skill (`scripts/…`, `references/…`) are relative to this
skill's folder.

## The workflow

Each step says what to produce before going on. Skipping the early ones is
what makes the late ones slow: a topology chosen without understanding the
part cannot be rescued by moving vertices.

### 1. Understand the part

```lua
gecko.load_geometry("part.msh")
print(gecko.merge_smooth{ angle = 10 })   -- merges smooth seams; read what it says
local g = gecko.geometry()                  -- every entity: size, incidence, shape
gecko.render("part-iso.png")                -- labelled with every tag
gecko.render("part-top.png", { view = "top" })
```

Run it, read the printed facts **and look at the pictures** (Read the PNGs).
Then write down, in a few lines:

- **What the part is** — e.g. "an S-bent wall 2 thick and 10 high, a Ø4 hole
  through its middle, the left end trimmed by an R5 cylinder". Shapes come
  from `g.surfaces[i].shape`: a `cylinder` that is not `convex` is a hole or
  an inner wall; parallel planes or concentric cylinders a fixed distance
  apart are a **wall** and that distance is its thickness.
- **The merged entities** and their tags (negative numbers): you will classify
  on those, not on the pieces.
- **CAD defects**: curves of zero length, two curves with the same ends and
  length (duplicates), corners at the same point. They make "curve not
  followed" / "corner not held" reports that no blocking can clear, and
  merges that leave faces on nothing. Converting from STEP with
  `step_to_msh.py` heals them (the default); with only a `.msh`, name them
  now so you do not chase them later. Log them either way.

### 2. Plan the topology before writing code

Decide the blocks on paper — in words or a small table — before any Lua.
`references/patterns.md` has the patterns that work and why: one block
through a thin wall, the O-grid round a hole, splitting at an inflection,
handling a concave fillet, where irregular vertices must go. For a swept part
(a wall bent along a path), lay it out **developed**: arc length along the
path × height, and block that 2D picture; every quad becomes one hex through
the thickness.

**Design the sheets first** (`references/patterns.md` §11): for every
surface, say which sheet of blocks will run along it, and check that no
sheet ends on a surface without a layer of blocks between — an O-grid box
reaching the boundary is the usual way to break it.

For each block, know its 8 corners and what each corner is attached to: a
geometric corner (dim 0), a curve (dim 1), a surface (dim 2), or nothing when
it is inside the part. A corner of the part must be a corner of some block,
and every curve of the part must be run along by block edges — that is where
boundary alignment is won or lost, not in the mesher.

Estimate the block count and say it. If a cheaper topology is plausible,
note it as a variant to try in step 7.

### 3. Build it as one script

Use `scripts/blocklib.lua` (read its header). The pattern:

```lua
local L = dofile("blocklib.lua")
gecko.load_geometry("part.msh"); gecko.merge_smooth{ angle = 10 }
L.point("TL_A", {-3, 0, 7.5}, 0, 13)        -- name, position, dim, tag
L.point("M_A",  {x, y, z},     1, -10)       -- on a curve: it will be projected
L.point("R1_A", {x, y, z},     2, -3)        -- on a surface
L.point("C",    {x, y, z})                   -- inside: attached to nothing
L.block{ "a0","a1","a2","a3","b0","b1","b2","b3" }   -- bottom quad, then top quad
L.weld()        -- glue every pair of coincident faces
L.attach()      -- classify each node on what its point says
```

Explicit attachment beats `gecko.classify{ tolerance }` on anything but a
box: proximity picks the wrong entity wherever two are close (a thin wall,
a hole near an edge). Keep every position a **parameter** at the top of the
script, in one table of defaults that a global `PARAMS` overrides key by key
(`for k, v in pairs(PARAMS or {}) do P[k] = v end`): step 7 tunes them so.

### 4. Check the blocking before meshing

```bash
gecko run part.lua --json > report.json
```

Read `report.json` → `blocking`: `summary.issues`, then each `issues[i].message`.
Before meshing, the only issues left should be the CAD defects from step 1.
An `outer_face_on_nothing` names what each of its corners is on — the one
that shares no surface with the others is the culprit. `inverted_block` or a
`worst_block.jacobian` below ~0.2 means fix the layout now; a mesh will not
be better than its blocks.

Then check the layers: `python3 scripts/fundamental.py report.json`. It says
whether the blocking is fundamental, and for each surface or curve that is
not, which sheets or columns its blocks fall into. Fix those now too — it
is a question of topology, and nothing later in the workflow changes it.

### 5. Size, mesh, measure

Give the mesh a target element size rather than one segment count for all:
`L.size(h)` sets each sheet from its longest edge. Thin directions want at
least 2–4 cells. Then `gecko.mesh()` and run with `--json` again. Read:

- `quality.scaled_jacobian`: `min`, `mean`, `inverted`, `histogram`,
  `blocks` (the worst blocks), `worst.center`;
- `quality.skewness` and `quality.aspect_ratio`;
- `fidelity`: `curves.followed/checked`, `curves.missed`, `corners.missed`,
  `skin.max`, `geometry.max`.

Then `python3 scripts/check.py report.json [--known curves=27 corners=19]`
applies the rules and says pass or fail, rule by rule.

### 6. Diagnose what is bad

Look before you guess:

```lua
gecko.render("bad.png", { below = 0.7, view = "front", color = "block" })
gecko.render("close.png", { center = q.worst.center, radius = 2, below = 0.5 })
```

The red cells say *where*; `quality.scaled_jacobian.blocks` says *which
blocks*. Then match the symptom to its cause — `references/patterns.md`,
"Reading bad cells", lists the ones met so far (45° O-grid corners, shear
across an inflection, stretch between inner and outer walls, a block edge
spanning too much arc). Fix topology problems with topology, placement
problems with placement.

### 7. Optimise placement

Once the topology is right, the vertex positions are worth tuning. Gecko
has no placement optimiser yet (`references/feature-requests.md`); until it
does, with the positions behind `PARAMS` as in step 3, run the stopgap
`scripts/optimize.py` (read its
header) with a `params.json` naming what may move: a random
coordinate search on min SJ (plus a little mean), each evaluation a
`gecko run --json`. Optimise at a **coarse** size (fast), then **verify at
the final size** — the minimum moves when the cell aspect changes (on B1:
0.69 at 6 segments, 0.64 at the final size).

`gecko.smooth{}` moves block corners towards cubes. Try it, but compare
before and after: on B1 it inverted cells and pulled nodes off a hole rim.
Keep whichever is better.

Iterate between 6 and 7. When the bottleneck stops moving, the topology is
the limit: try the variant from step 2, or add the one sheet that relieves
the worst corner (on B1, one extra cut through the hole's right half took min
SJ from 0.58 to 0.69 for two more blocks). Say what each extra block bought.

### 8. Deliver

In the part's folder:

- `part.lua` — **self-contained**: the final values in its defaults, `blocklib.lua`
  beside it, rerunnable with `gecko run part.lua`, writing `part.vtk`;
- `part.vtk`, `report.json`, and pictures: the blocking, the mesh by quality,
  and the bad cells;
- a short summary for the user: block count, hexes, SJ min/mean/inverted,
  share below 0.5 and 0.7, curves followed, whether the blocking is
  fundamental (and the fun-sheet levels and curve types), CAD defects, what was tried and
  what each step bought, and what would be tried next.

Report numbers as the report gives them; if a rule failed, say which and by
how much rather than rounding it into a pass.

### 9. Log the frictions and feature requests — this is half the job

Every time Gecko made you work around something — a read that was missing,
a number you had to compute outside, a command that did the wrong thing, a
defect it did not catch, a step you did by hand that a tool could do — append
one line to `frictions.jsonl` in the part's folder, as you go, not at the end:

```json
{"part":"B1","step":"plan","kind":"feature-request","need":"…","workaround":"…","cost":"…","idea":"gecko.…{ … }","severity":3}
```

Every script you wrote to make up for Gecko, and every stopgap of the skill
you used, is a `feature-request` line. `references/feature-log.md` has the
fields and examples. At the end, `python3 scripts/frictions.py <folders…>`
ranks them across parts — that is the feature list the user is after; give
the requests first in your summary. A run with no frictions logged is
suspicious: say so if there truly were none.

## Reference files

- `references/patterns.md` — blocking patterns, the angle bookkeeping behind
  them, and how to read bad cells. Read in step 2, and again in step 6.
- `references/api.md` — the Lua API you need, with the pitfalls that cost
  time (merged tags, classification inference, Lua nils, smoothing). Read
  before writing the first script.
- `references/quality-rules.md` — what "done" means. Read first.
- `references/feature-log.md` — the friction-log format.
- `references/feature-requests.md` — asking Gecko for features instead of
  scripting around them; the requests the skill's own stopgaps stand for,
  and those found while testing. Read before writing any helper script.
- `examples/B1.lua` — a complete worked example: Mambo B1, 13 blocks, min
  SJ 0.64, with its own known defect noted in the header.
