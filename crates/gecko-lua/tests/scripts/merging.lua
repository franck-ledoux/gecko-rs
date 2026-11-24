-- Pieces of the geometry run together.

gecko.reset()
gecko.load_geometry(FIXTURES .. "/cube.msh")

-- Merged by hand, the bottom and the front: the curve between them goes.
gecko.merge_surfaces({1, 3})
local ok, err = pcall(gecko.merge_surfaces, {3, 4})
assert(not ok and tostring(err):find("surface 3 is merged already"), tostring(err))

local script = gecko.script()
for _, line in ipairs({
  "gecko.merge_surfaces({1, 3})",
}) do
  assert(script:find(line, 1, true), "the history says " .. line .. ":\n" .. script)
end
