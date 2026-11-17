-- Growing blocks out of faces, one at a time and several at once.
--
-- The point of the second kind is the weld: two faces that meet raise the
-- edge they share **once**, so the blocks they become have a face in common
-- rather than two faces in the same place.

gecko.create_cube()

-- Both shapes of the call reach the same command. One face, named the way a
-- click names it.
local function face_near(x, y, z)
  local best, far = nil, math.huge
  for _, f in ipairs(gecko.faces()) do
    local d = (f.center[1] - x) ^ 2 + (f.center[2] - y) ^ 2 + (f.center[3] - z) ^ 2
    if d < far then best, far = f, d end
  end
  return best
end

local top = face_near(0.5, 0.5, 1)
gecko.extrude_face(top.id, top.block, 1)
local one = gecko.stats()
assert(one.blocks == 2, "one block grown: " .. one.blocks)
assert(one.nodes == 12 and one.faces == 11, one.nodes .. " nodes, " .. one.faces .. " faces")

for _ = 1, 10 do gecko.undo() end
assert(gecko.stats().blocks == 0, "back to nothing")

-- And two that meet, in one call: a list of {face, block} pairs.
gecko.create_cube()
local a = face_near(0.5, 0.5, 1)
local b = face_near(1, 0.5, 0.5)
gecko.extrude_face({ { a.id, a.block }, { b.id, b.block } }, 1)

local two = gecko.stats()
assert(two.blocks == 3, "three blocks: " .. two.blocks)
-- Raised one after the other these would be 16 nodes and 16 faces: each
-- block would bring its own copy of the edge they share.
assert(two.nodes == 14, "the raised edge is one edge: " .. two.nodes)
assert(two.faces == 15, "and the two blocks share a face: " .. two.faces)
print(string.format("%d blocks, %d nodes, %d faces", two.blocks, two.nodes, two.faces))
