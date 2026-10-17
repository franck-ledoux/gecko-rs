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
