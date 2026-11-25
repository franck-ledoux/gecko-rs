-- A geometry, a box round it, and the blocking put on it.

gecko.load_geometry(FIXTURES .. "/cube.msh")
local g = gecko.stats().geometry
assert(g.surfaces == 6 and g.curves == 12 and g.corners == 8, "the cube's entities")

gecko.box_geometry{ margin = 0 }
assert(gecko.stats().blocks == 1, "one block boxes it")

gecko.classify()
local on = gecko.stats().classified
assert(on.nodes == 8, "every corner on a corner")
assert(on.edges == 12 and on.faces == 6)

-- And every entity is listed, which is the only way to find out what the
-- file called things.
local corners = 0
for _, e in ipairs(gecko.entities()) do
  if e.dim == 0 then corners = corners + 1 end
end
assert(corners == 8)

-- How faithfully a mesh follows it: nothing to say before there is a mesh,
-- and nothing missed once the box put on the cube is meshed.
assert(gecko.fidelity() == nil, "no mesh, nothing to check")
gecko.mesh{ segments = 3 }
local f = gecko.fidelity()
assert(f.share == 0.25, "a quarter of the element size by default")
assert(f.curves.checked == 12 and f.curves.followed == 12, "every curve followed")
assert(f.corners.checked == 8 and #f.corners.missed == 0, "every corner on a node")
assert(f.skin.beyond == 0 and f.geometry.beyond == 0 and f.worst == nil)
assert(gecko.fidelity{ tolerance = 0.5 }.share == 0.5, "a tolerance said is the one used")
