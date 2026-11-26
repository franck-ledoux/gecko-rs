-- The geometry, described: what a script plans a blocking from when it
-- cannot look at the part.

gecko.load_geometry(FIXTURES .. "/cylinder.msh")
local g = gecko.geometry()
assert(#g.surfaces == 3 and #g.curves == 2 and #g.corners == 0)
assert(math.abs(g.bounds.max[3] - 2) < 1e-9, "the box round it")

local by = {}
for _, s in ipairs(g.surfaces) do by[s.tag] = s end
-- The two ends are planes, facing out of the part.
assert(by[1].shape.kind == "plane" and by[1].shape.normal[3] == -1)
assert(by[2].shape.kind == "plane" and math.abs(by[2].shape.offset - 2) < 1e-9)
-- The side is a whole cylinder of radius 1 up z, with the material inside.
local side = by[3].shape
assert(side.kind == "cylinder", side.kind)
assert(math.abs(side.radius - 1) < 1e-6 and side.sweep == 360 and side.convex)
assert(math.abs(math.abs(side.axis[3]) - 1) < 1e-9 and math.abs(side.length - 2) < 1e-9)
-- And it is bounded by both rims, which are circles that close.
table.sort(by[3].curves)
assert(by[3].curves[1] == 201 and by[3].curves[2] == 202)
for _, c in ipairs(g.curves) do
  assert(c.closed and c.shape.kind == "circle" and math.abs(c.shape.radius - 1) < 1e-6)
  assert(#c.surfaces == 2, "a rim is between the side and an end")
end

-- A rim's nodes, in order, and back to the first.
local rim = gecko.polyline(201)
assert(#rim.runs == 1 and rim.closed)
local run = rim.runs[1]
assert(run[1][1] == run[#run][1] and run[1][2] == run[#run][2])
assert(gecko.polyline(999) == nil, "no such curve")

-- The nearest of each kind of entity, and of one in particular.
local near = gecko.closest({ 0, 0, 1.9 })
assert(near.corner == nil, "this part has no corners")
assert(near.surface.tag == 2 and math.abs(near.surface.distance - 0.1) < 1e-9)
assert(near.curve.tag == 202)
local on = gecko.closest({ 3, 0, 1 }, { dim = 2, tag = 3 })
assert(on.tag == 3 and math.abs(on.distance - 2) < 0.02, "onto the side, from outside it")

-- The cube: six planes, twelve straight edges, each between two corners.
gecko.reset()
gecko.load_geometry(FIXTURES .. "/cube.msh")
g = gecko.geometry()
for _, s in ipairs(g.surfaces) do assert(s.shape.kind == "plane" and math.abs(s.area - 1) < 1e-9) end
for _, c in ipairs(g.curves) do
  assert(c.shape.kind == "line" and #c.ends == 2 and math.abs(c.length - 1) < 1e-9)
end
for _, p in ipairs(g.corners) do assert(#p.curves == 3, "three edges at a corner of a cube") end

-- A merge is described as the one entity it is, and its pieces as theirs.
gecko.merge_surfaces({ 1, 2 })
g = gecko.geometry()
local merged
for _, s in ipairs(g.surfaces) do if s.tag < 0 then merged = s end end
assert(merged and #merged.members == 2 and #merged.pieces == 2)
assert(merged.shape.kind == "other", "two faces of a cube are no one plane")
assert(merged.pieces[1].shape.kind == "plane")

gecko.reset()
assert(gecko.geometry() == nil, "nothing to describe")
