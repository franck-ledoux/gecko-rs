#!/usr/bin/env python3
"""Generate a curved (P2) faceted CAD surface: the unit sphere as 8 six-node
triangles, one per octant.

Eight facets is deliberately absurd for a sphere -- that is the point. Rendered
as flat P1 triangles it is an octahedron; evaluated through the quadratic map it
is a sphere. The gap between those two pictures is what a high-order import buys,
and it is what the tessellation-level slider shows.

The model decomposition is complete: 6 model vertices (the poles), 12 model
curves (quarter great circles), 8 model surfaces (the octants) -- so block
corners have all three dimensions to classify onto.
"""
import itertools, math

def unit(p):
    n = math.sqrt(sum(c * c for c in p))
    return tuple(c / n for c in p)

def mid(a, b):
    return unit(tuple((x + y) / 2 for x, y in zip(a, b)))

poles = [(1,0,0), (-1,0,0), (0,1,0), (0,-1,0), (0,0,1), (0,0,-1)]

nodes, index = [], {}
def node(p):
    key = tuple(round(c, 12) for c in p)
    if key not in index:
        nodes.append(key)
        index[key] = len(nodes)          # gmsh ids are 1-based
    return index[key]

for p in poles:
    node(p)

# Octants: one face per sign combination, oriented outward.
faces, edges = [], {}
for sx, sy, sz in itertools.product((1, -1), repeat=3):
    a, b, c = (sx,0,0), (0,sy,0), (0,0,sz)
    if sx * sy * sz < 0:                  # keep consistent outward winding
        b, c = c, b
    faces.append((a, b, c))
    for u, v in ((a,b), (b,c), (c,a)):
        edges.setdefault(frozenset((u, v)), (u, v))

edge_id = {k: i + 1 for i, k in enumerate(edges)}

elements = []
for i, p in enumerate(poles):
    elements.append((15, i + 1, [node(p)]))               # model vertex
for k, (u, v) in edges.items():
    elements.append((8, edge_id[k], [node(u), node(v), node(mid(u, v))]))   # curve
for i, (a, b, c) in enumerate(faces):
    elements.append((9, i + 1, [node(a), node(b), node(c),
                                node(mid(a,b)), node(mid(b,c)), node(mid(c,a))]))

out = ["$MeshFormat", "2.2 0 8", "$EndMeshFormat", "$Nodes", str(len(nodes))]
out += [f"{i+1} {p[0]!r} {p[1]!r} {p[2]!r}" for i, p in enumerate(nodes)]
out += ["$EndNodes", "$Elements", str(len(elements))]
for i, (ty, tag, ns) in enumerate(elements):
    out.append(" ".join(str(x) for x in [i + 1, ty, 2, tag, tag] + ns))
out += ["$EndElements", ""]

open("sphere_p2.msh", "w").write("\n".join(out))
print(f"sphere_p2.msh: {len(nodes)} nodes, {len(elements)} elements "
      f"({len(poles)} vertices, {len(edges)} curves, {len(faces)} surfaces)")
