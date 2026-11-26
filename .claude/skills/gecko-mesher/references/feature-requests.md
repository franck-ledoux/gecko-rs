# Asking Gecko for what is missing

The user builds Gecko, and the point of meshing parts with it is as much to
find what Gecko lacks as to get meshes. So when a job needs something Gecko
does not do — a number it cannot read, a step it cannot take, a check it
cannot make — the answer the user wants is **a feature request for Gecko**,
not a script that does it on the side. A Python helper that measures the
mesh, a Lua loop that searches vertex positions, a parser that re-derives
what the kernel already knows: each solves the problem once, for one part,
and hides from the user that Gecko should solve it for every part.

## What to do when Gecko falls short

1. **Stop before writing the helper**, and write the request: what you
   needed, the Gecko command or read that would give it — named and with its
   arguments, as it would appear in `docs/user-guide/scripting.md` — and
   what it costs not to have it. It goes in `frictions.jsonl` with
   `"kind": "feature-request"` (`references/feature-log.md`).
2. **Look for a way inside Gecko first**: another read, another command,
   another order of operations. Log the request even if you find one, when
   the way was roundabout.
3. **Only if the part cannot be finished otherwise**, write the smallest
   stopgap that finishes it, name it as a stopgap in the summary, and point
   to the request it stands in for. Never present a stopgap as part of the
   method.
4. In the **summary to the user**, list the feature requests first among the
   frictions, each with the API you propose and what it would have saved.

The skill's own scripts are stopgaps of this kind, and each already stands
for a request below. Use them while the features do not exist, and say so.

## Standing requests — the skill's stopgaps

| stopgap | what it does | the Gecko feature that replaces it |
| ------- | ------------ | ---------------------------------- |
| `scripts/fundamental.py` | rebuilds block sheets and chords from the report to check fun sheets and fundamental chords | `gecko.fundamental()` — for each surface the sheet capturing it and its level, for each curve its fundamental chords and type, and the verdict; `gecko.render{ show = {"sheets"} }` colouring the capturing sheets |
| `scripts/optimize.py` | random search over vertex positions, one `gecko run` per try | `gecko.optimize{ nodes = …, objective = "min_sj", size = h }` — block-corner placement against the **mesh** quality, nodes kept on their entities; or a `gecko.smooth` that judges its steps by the mesh |
| `scripts/check.py` | holds a report to rules | `gecko.check{ rules }` — the hard rules and targets of `quality-rules.md` as a read, so the page can show them too |
| `blocklib.size(h)` | segments per sheet from a target size | `gecko.mesh{ size = h }` |
| `blocklib.weld()` | glues every pair of coincident faces | `gecko.glue_coincident()`, or `add_block` welding onto faces already there |
| `blocklib.attach()` | classifies nodes by matching positions | `gecko.add_block{ corners, on = { … } }` taking each corner's entity |
| `scripts/step_to_msh.py` | STEP to MSH 2.2 through gmsh, healing duplicates | `gecko.load_geometry("part.step")`, healing coincident corners and duplicated curves on load |

When one of these features lands in Gecko, use it, and say in the friction
log that the stopgap is obsolete.

## Requests found by running the skill

Met while testing the skill on Mambo B1; log them again when a part hits
them, with what it cost there.

- **Duplicated curves and coincident corners are not detected.** A CAD export
  can carry one seam twice (B1's curves 23 and 26, corners 17 and 19), and
  nothing says so until faces end up on nothing. Wanted: a warning on load,
  and `gecko.heal()` to stitch them.
- **`merge_curves` builds a curve no surface contains without a word.**
  Wanted: a warning, or a refusal saying which piece lies on which surface.
- **The fidelity check never samples a surface no face is on**, so a whole
  surface the mesh misses scores as followed. Wanted: `fidelity` measuring
  every surface, and naming the uncovered ones.
- **Mesh nodes are projected onto the facets**, not the smooth surface, so a
  coarse `.msh` caps the quality (B1: min SJ rose from 0.614 to 0.630 just
  from a finer faceting). Wanted: projection onto the surface's fitted shape
  where `gecko.geometry()` recognises one.
- **Pillowing drops a classification on several curves taken as one.**
