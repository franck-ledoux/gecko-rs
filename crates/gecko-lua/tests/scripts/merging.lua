-- Pieces of the geometry run together, and a face laid down on it.

gecko.reset()
gecko.load_geometry(FIXTURES .. "/cube.msh")

-- The faces of a cube meet at 90°: nothing smooth to merge.
local said = gecko.merge_smooth()
assert(tostring(said):find("nothing meets within 10"), tostring(said))

-- Merged by hand, the bottom and the front: the curve between them goes.
gecko.merge_surfaces({1, 3})
local ok, err = pcall(gecko.merge_surfaces, {3, 4})
assert(not ok and tostring(err):find("surface 3 is merged already"), tostring(err))

-- A box standing clear, its bottom face laid down on the top surface: the
-- face's corners go onto the top's corners.
gecko.box_geometry()
local top, high = nil, -math.huge
for _, f in ipairs(gecko.faces()) do
  if f.center[3] > high then top, high = f.id, f.center[3] end
end
gecko.classify_face(top, 2, 2, { down = true })
local on = 0
for _, n in ipairs(gecko.nodes()) do
  if n.on and n.on.dim == 0 then on = on + 1 end
end
assert(on == 4, "four corners on corners, not " .. on)

local script = gecko.script()
for _, line in ipairs({
  "gecko.merge_smooth()",
  "gecko.merge_surfaces({1, 3})",
  "gecko.classify_face({" .. top .. "}, 2, 2, { down = true })",
}) do
  assert(script:find(line, 1, true), "the history says " .. line .. ":\n" .. script)
end
