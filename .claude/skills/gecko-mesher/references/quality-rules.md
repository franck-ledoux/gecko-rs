# What a good mesh is

> **These are defaults.** The user has rules of their own for what a good
> mesh is; when they give them, they replace this file, and
> `scripts/check.py` reads the same numbers from `rules.json` beside it.
> Until then, use these and say in your summary that you did.

## Hard rules — a mesh that breaks one is not done

| rule | threshold | read from |
| ---- | --------- | --------- |
| The script runs to the end | `ok` is true | report |
| No inverted cell | `quality.scaled_jacobian.inverted == 0` | report |
| Minimum scaled Jacobian | ≥ 0.30 | `quality.scaled_jacobian.min` |
| Every real curve followed | `fidelity.curves.missed` holds only known CAD defects | report |
| Every real corner on a node | `fidelity.corners.missed` holds only known CAD defects | report |
| The skin on the part | `fidelity.skin.max` ≤ the fidelity tolerance (a quarter of the local element size) — i.e. `fidelity.worst` absent or a known defect | report |
| Every outer face attached | no `outer_face_on_nothing` in `blocking.issues` | report |
| **The blocking is fundamental** | every geometric surface captured by one fun sheet, every curve by fundamental chords — below | `scripts/fundamental.py report.json` |

## The blocking is fundamental

Boundary alignment is more than nodes lying on curves. A mesh can follow
every curve and corner and still have its worst-shaped layers along the
boundary, where the analyst needs the best ones: what decides that is how
the **layers** of hexahedra meet the geometry. This is the theory of
fundamental meshes (Ledoux & Shepherd, *Topological and geometrical
properties of hexahedral meshes*, Eng. with Comput. 2010; Kowalski, Ledoux,
Staten & Owen, *Fun sheet matching*, Eng. with Comput. 2012), applied to the
**block structure**: a blocking is a coarse hex mesh, its sheets of blocks
are primal sheets and its columns of blocks primal chords, and cutting every
block into a grid keeps each of them one — so a fundamental blocking gives a
fundamental mesh, and it is checked on the blocks, before meshing.

For a geometric surface s, let H_s be the blocks with a face on s.

- **s is captured** when one sheet of blocks contains all of H_s, and each
  block of H_s meets s through one face only — the sheet runs along the
  surface once, without folding or crossing itself there (Def. 9). "One layer
  of blocks per surface."
- The capturing sheet is a **level 1 fun sheet** when all its blocks lie on
  the surfaces it captures (it is wholly on the boundary), and a **level 2**
  fun sheet when some of its blocks are inside (Defs. 11, 12). Both are fine.

For a geometric curve c between surfaces s1 and s2:

- c has a **fundamental chord** for s1 when the blocks with a face on s1
  along c form a single column of blocks running along c, each once (Def. 10)
  — and the same for s2. "One or more columns of blocks per curve."
- Its **type** is how many distinct columns that takes: 1 when one column
  holds it from both sides (a convex edge of a box), 2 when the two sides are
  different columns (a pillowed edge), 3 when there are more (Fig. 13).

The blocking is **fundamental** (Def. 14) when every surface and every curve
is captured. Geometric corners have no rule of their own, but an n-valent
corner needs at least n − 2 blocks round it, and what it needs propagates
along its curves.

What usually breaks it, and the fix:

| symptom (`fundamental.py` says) | usual cause | fix |
| -------------------------------- | ----------- | --- |
| a surface's blocks lie in several sheets | an O-grid box, or any block arrangement, whose sheets end on the surface instead of running along it | a layer of blocks along that surface: carry the box's sides inside the part, or pillow the surface (`patterns.md` §4, §11) |
| a block meets a surface through two faces | one block wraps a corner of the surface, or both walls of a thin part are one surface | split the block, or treat the walls as two surfaces |
| a curve's blocks lie in several columns | the same, seen along the curve | follows from fixing the surfaces |
| no block face on one of a curve's surfaces | an outer face classified on nothing | see `outer_face_on_nothing` |

Known CAD defects (a zero-length curve, a duplicated corner) are exempt, as
everywhere: `fundamental.py --known curves=… surfaces=…`.

## Targets — what "good" means, in order

1. Minimum scaled Jacobian **≥ 0.5**, and better is better.
2. Fewer than **1 %** of cells below 0.7.
3. Mean scaled Jacobian **≥ 0.9**.
4. The **fewest blocks** that reach 1–3. An extra block has to buy a
   measurable gain in 1–3 or in alignment; say what it bought.
5. Maximum aspect ratio ≤ 10, maximum skewness ≤ 0.8.

## Trade-offs

When targets conflict, prefer in this order: no inverted cells, alignment,
a fundamental blocking, minimum SJ, block count, mean SJ. A fundamental
blocking usually costs blocks — a layer along each surface an O-grid used to
touch — and that is the price of good layers on the boundary; say what it
cost. A block count lower by one is not worth a
minimum SJ lower by more than ~0.05; offer both variants when it is close.

## Element size

Unless the user says otherwise: a target size h of about 1/25 of the part's
span (`gecko.geometry().span`), at least 3 cells through any wall, at least
4 along any block edge on a curve.

## Known CAD defects

Zero-length curves, duplicated curves and coincident corners cannot be
followed. List them (tag and why) in the summary and pass them to
`check.py --known`; never count them as the mesh's failure, and never hide a
real miss among them.
