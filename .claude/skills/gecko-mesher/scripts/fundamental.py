#!/usr/bin/env python3
"""Is a block structure fundamental? Fun sheets and fundamental chords, on blocks.

    python3 fundamental.py report.json [--known surfaces=… curves=…] [--json]

Reads the `blocking` section of a `gecko run --json` report and applies the
theory of fundamental meshes (Ledoux & Shepherd, Eng. with Comput. 2010;
Kowalski, Ledoux, Staten & Owen, "Fun sheet matching", Eng. with Comput.
2012) to the **block structure**. A blocking is a coarse hexahedral mesh:
its block sheets are primal sheets and its columns of blocks primal chords,
and refining every block into a grid keeps each of them a sheet or a chord,
so a fundamental blocking gives a fundamental mesh.

For each geometric surface s, H_s is the set of blocks with a face on s:

- s is **captured** (Def. 9) when one block sheet P contains all of H_s, each
  block of H_s meeting s through one face only — the sheet runs along the
  surface once, as a 2-manifold, rather than folding or crossing itself there;
- that sheet is a **level 1** fun sheet (Def. 11) when every block of P has a
  face on one of the surfaces P captures — wholly on the boundary — and a
  **level 2** fun sheet (Def. 12) when some of its blocks are inside.

For each geometric curve c between surfaces s1 and s2, F_i is the set of
block faces on s_i that have an edge on c:

- c has a **fundamental chord** for s_i (Def. 10) when the blocks of F_i lie
  in one column of blocks running along c, each once;
- c is captured when it has one for each of its surfaces. The number of
  distinct fundamental chords gives its type: 1 for a convex edge held by one
  column (two level 1 sheets), 2 or 3 when it takes more (Fig. 13).

The blocking is **fundamental** (Def. 14) when every surface and every curve
that blocks touch is captured. An n-valent geometric corner needs at least
n − 2 blocks round it; that is reported, not judged.

Prints a summary, the failures with the reason, and exits 1 if the blocking
is not fundamental. --json prints the whole analysis instead.
"""
import argparse
import json
import sys
from collections import defaultdict

# A block's corners in VTK order; its edges in three directions of four
# parallel edges, and its faces with the direction of their normal.
AXIS_EDGES = [
    [(0, 1), (3, 2), (4, 5), (7, 6)],
    [(0, 3), (1, 2), (4, 7), (5, 6)],
    [(0, 4), (1, 5), (2, 6), (3, 7)],
]
FACES = [  # (corners, the axis of the edges leaving it)
    ((0, 3, 2, 1), 2),
    ((4, 5, 6, 7), 2),
    ((0, 1, 5, 4), 1),
    ((1, 2, 6, 5), 0),
    ((2, 3, 7, 6), 1),
    ((3, 0, 4, 7), 0),
]


def label(on):
    return None if on is None else (on["dim"], on["tag"])


class Blocking:
    def __init__(self, b):
        self.blocks = {blk["id"]: blk["corners"] for blk in b["blocks"]}
        self.edge_sheet = {}
        self.edge_on = {}
        for e in b["edges"]:
            key = frozenset(e["nodes"])
            self.edge_sheet[key] = e.get("sheet")
            self.edge_on[key] = label(e.get("on"))
        self.face_on = {frozenset(f["nodes"]): label(f.get("on")) for f in b["faces"]}
        self.node_on = {n["id"]: label(n.get("on")) for n in b["nodes"]}

    def sheet(self, block, axis):
        """The block sheet through `block` across `axis`: the sheet of its
        edges running along it."""
        c = self.blocks[block]
        a, b = AXIS_EDGES[axis][0]
        return self.edge_sheet.get(frozenset((c[a], c[b])))

    def faces(self, block):
        """Each face of the block: its node set, the axis of its normal, and
        what it is on."""
        c = self.blocks[block]
        for corners, axis in FACES:
            nodes = frozenset(c[k] for k in corners)
            yield nodes, axis, self.face_on.get(nodes)

    def face_edges(self, nodes, block):
        """The four edges of a face of `block`, each with its axis."""
        c = self.blocks[block]
        for axis, edges in enumerate(AXIS_EDGES):
            for a, b in edges:
                if c[a] in nodes and c[b] in nodes:
                    yield frozenset((c[a], c[b])), axis

    def chords(self):
        """Columns of blocks: (block, axis) joined to the block across each
        face square to that axis, at the axis square to the shared face
        there. Returns a component id for each (block, axis)."""
        parent = {}

        def find(x):
            parent.setdefault(x, x)
            while parent[x] != x:
                parent[x] = parent[parent[x]]
                x = parent[x]
            return x

        def union(x, y):
            parent[find(x)] = find(y)

        owners = defaultdict(list)
        for blk in self.blocks:
            for nodes, axis, _ in self.faces(blk):
                owners[nodes].append((blk, axis))
        for pair in owners.values():
            for blk, axis in pair:
                find((blk, axis))
            if len(pair) == 2:
                union(pair[0], pair[1])
        return {key: find(key) for key in list(parent)}


def analyse(report, known=None):
    known = known or {"surfaces": set(), "curves": set()}
    b = report.get("blocking")
    if not b or not b.get("blocks"):
        return {"fundamental": False, "reason": "no blocking in the report"}
    m = Blocking(b)
    chord_of = m.chords()

    # --- surfaces ---------------------------------------------------------------
    touching = defaultdict(list)  # surface -> [(block, axis, face nodes)]
    for blk in m.blocks:
        for nodes, axis, on in m.faces(blk):
            if on and on[0] == 2:
                touching[on[1]].append((blk, axis, nodes))

    surfaces = {}
    sheet_blocks = defaultdict(set)
    for blk in m.blocks:
        for axis in range(3):
            sheet_blocks[m.sheet(blk, axis)].add(blk)
    captured_by = {}
    for tag, items in touching.items():
        sheets = sorted({m.sheet(blk, axis) for blk, axis, _ in items}, key=str)
        per_block = defaultdict(int)
        for blk, _, _ in items:
            per_block[blk] += 1
        twice = sorted(blk for blk, n in per_block.items() if n > 1)
        entry = {"blocks": sorted(per_block), "sheets": sheets}
        if len(sheets) == 1 and not twice:
            entry["captured"] = True
            captured_by[tag] = sheets[0]
        else:
            entry["captured"] = False
            why = []
            if len(sheets) > 1:
                why.append(f"its {len(per_block)} blocks lie in {len(sheets)} different sheets {sheets}, not one")
            if twice:
                why.append(f"blocks {twice} meet it through more than one face, so the sheet folds or crosses itself there")
            entry["why"] = "; ".join(why)
        surfaces[tag] = entry

    # Levels: a capturing sheet is level 1 when all its blocks are on the
    # surfaces it captures, level 2 otherwise.
    captures = defaultdict(list)
    for tag, sheet in captured_by.items():
        captures[sheet].append(tag)
    sheets = {}
    for sheet, tags in captures.items():
        on_them = set()
        for tag in tags:
            on_them |= {blk for blk, _, _ in touching[tag]}
        inside = sorted(sheet_blocks[sheet] - on_them)
        level = 1 if not inside else 2
        sheets[sheet] = {"captures": sorted(tags), "level": level, "blocks": len(sheet_blocks[sheet]), "inside": inside}
        for tag in tags:
            surfaces[tag]["level"] = level

    # --- curves -----------------------------------------------------------------
    sides = defaultdict(lambda: defaultdict(list))  # curve -> surface -> [(block, chord)]
    for blk in m.blocks:
        for nodes, _, on in m.faces(blk):
            if not on or on[0] != 2:
                continue
            for edge, axis in m.face_edges(nodes, blk):
                e_on = m.edge_on.get(edge)
                if e_on and e_on[0] == 1:
                    sides[e_on[1]][on[1]].append((blk, chord_of[(blk, axis)]))
    curves = {}
    for tag, by_surface in sides.items():
        entry = {"surfaces": sorted(by_surface), "chords": {}}
        fails = []
        for surface, items in by_surface.items():
            chords = {c for _, c in items}
            per_block = defaultdict(int)
            for blk, _ in items:
                per_block[blk] += 1
            twice = sorted(blk for blk, n in per_block.items() if n > 1)
            ok = len(chords) == 1 and not twice
            entry["chords"][surface] = {"blocks": sorted(per_block), "fundamental": ok}
            if not ok:
                if len(chords) > 1:
                    fails.append(f"along surface {surface} its blocks lie in {len(chords)} columns, not one")
                if twice:
                    fails.append(f"along surface {surface} blocks {twice} touch it twice")
        distinct = {c for items in by_surface.values() for _, c in items}
        entry["captured"] = not fails and len(by_surface) >= 1
        if len(by_surface) < 2:
            fails.append("no block face on one of its two surfaces runs along it")
            entry["captured"] = False
        entry["type"] = len(distinct) if entry["captured"] else None
        if fails:
            entry["why"] = "; ".join(fails)
        curves[tag] = entry

    # Curves of the geometry no block edge is on: from the blocking's own
    # report of what nothing follows.
    unfollowed = (b.get("summary", {}).get("not_followed") or {})
    for tag in unfollowed.get("curves", []):
        curves.setdefault(tag, {"captured": False, "type": None, "why": "no block edge runs along it"})
    for tag in unfollowed.get("surfaces", []):
        surfaces.setdefault(tag, {"captured": False, "why": "no block face is on it"})

    # --- corners: at least n - 2 blocks round an n-valent corner -------------
    corner_blocks = defaultdict(set)
    for blk, corners in m.blocks.items():
        for n in corners:
            on = m.node_on.get(n)
            if on and on[0] == 0:
                corner_blocks[on[1]].add(blk)

    bad_surfaces = {t: s for t, s in surfaces.items() if not s["captured"] and t not in known["surfaces"]}
    bad_curves = {t: c for t, c in curves.items() if not c["captured"] and t not in known["curves"]}
    return {
        "fundamental": not bad_surfaces and not bad_curves,
        "surfaces": surfaces,
        "curves": curves,
        "sheets": {str(k): v for k, v in sheets.items()},
        "corners": {t: len(bs) for t, bs in corner_blocks.items()},
        "failed": {"surfaces": sorted(bad_surfaces), "curves": sorted(bad_curves)},
        "levels": {
            "1": sum(1 for s in sheets.values() if s["level"] == 1),
            "2": sum(1 for s in sheets.values() if s["level"] == 2),
        },
    }


def known_from(items):
    known = {"surfaces": set(), "curves": set()}
    for item in items or []:
        kind, _, tags = item.partition("=")
        if kind in known:
            known[kind].update(int(t) for t in tags.split(",") if t)
    return known


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("report")
    ap.add_argument("--known", nargs="*", default=[])
    ap.add_argument("--json", action="store_true")
    a = ap.parse_args()
    with open(a.report) as f:
        report = json.load(f)
    known = known_from(a.known)
    r = analyse(report, known)
    if a.json:
        print(json.dumps(r, indent=1, default=str))
        sys.exit(0 if r["fundamental"] else 1)
    if "surfaces" not in r:
        sys.exit(r.get("reason"))
    s, c = r["surfaces"], r["curves"]
    print(f"surfaces captured: {sum(1 for x in s.values() if x['captured'])}/{len(s)}"
          f"  (level 1 sheets: {r['levels']['1']}, level 2: {r['levels']['2']})")
    print(f"curves captured:   {sum(1 for x in c.values() if x['captured'])}/{len(c)}")
    types = defaultdict(int)
    for x in c.values():
        if x.get("type"):
            types[x["type"]] += 1
    if types:
        print("curve types:       " + ", ".join(f"{n} of type {t}" for t, n in sorted(types.items())))
    for tag in sorted(s, key=str):
        if not s[tag]["captured"]:
            mark = "known" if tag in known["surfaces"] else "FAIL"
            print(f"  {mark} surface {tag}: {s[tag]['why']}")
    for tag in sorted(c, key=str):
        if not c[tag]["captured"]:
            mark = "known" if tag in known["curves"] else "FAIL"
            print(f"  {mark} curve {tag}: {c[tag]['why']}")
    print("\nFUNDAMENTAL" if r["fundamental"] else "\nNOT FUNDAMENTAL")
    sys.exit(0 if r["fundamental"] else 1)


if __name__ == "__main__":
    main()
