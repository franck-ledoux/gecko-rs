-- Faces that belong to no block: the beginning of a surface blocking.

assert(gecko.stats().blocks == 0)

gecko.add_face{ {0, 0, 0}, {1, 0, 0}, {1, 1, 0}, {0, 1, 0} }
local s = gecko.stats()
assert(s.faces == 1, "one face")
assert(s.nodes == 4 and s.edges == 4, "four corners and four sides")
assert(s.blocks == 0, "and it is not a block")
assert(s.patches == 1, "it is a patch")

-- A face of a patch says so, so a script can tell what it is holding.
local faces = gecko.faces()
assert(#faces == 1)
assert(faces[1].patch == true)

-- What needs an inside is refused, and says which name is the trouble.
local ok, why = pcall(gecko.remove_chord, faces[1].id, faces[1].block)
assert(not ok, "a chord was walked through an open sheet")
assert(tostring(why):find("patch"), "the message says why: " .. tostring(why))

-- Cutting works in two dimensions: the sheet through one side of a lone
-- quadrangle is that side and the one opposite, and cutting it leaves two
-- faces welded to each other — still one component, still no block.
local along = nil
for _, edge in ipairs(gecko.edges()) do
  along = along or edge.id
end
gecko.split_sheet(along, 0.5)
s = gecko.stats()
assert(s.faces == 2, "the quadrangle is two now")
assert(s.blocks == 0 and s.patches == 1, "one patch, no block")

-- A quadrangle laid on a geometric surface, which is where a surface
-- blocking really starts: the kernel chooses the four corners of the
-- surface the boundary turns a right angle at.
gecko.undo()
gecko.undo()
assert(gecko.stats().faces == 0, "back to nothing")

gecko.load_geometry(FIXTURES .. "/cube.msh")
gecko.face_on_surface(1)
s = gecko.stats()
assert(s.faces == 1 and s.patches == 1, "one quadrangle, one patch")
assert(s.classified.nodes == 4, "its corners are on the geometry's")
assert(s.classified.edges == 4 and s.classified.faces == 1)

-- A face of a cube is bounded by four curves, so every side is one curve
-- and no composite is needed to name any of them.
for _, edge in ipairs(gecko.edges()) do
  assert(edge.on.dim == 1 and edge.on.tag > 0, "a plain curve, not a side")
end

-- And a surface that does not exist stops the script.
local ok, why = pcall(gecko.face_on_surface, 99)
assert(not ok and tostring(why):find("no surface"), tostring(why))

-- And a face of a patch comes away on its own, leaving whatever it was
-- welded to whole.
local face = gecko.faces()[1]
gecko.remove_face(face.id)
assert(gecko.stats().faces == 0, "the quadrangle is gone")

-- A side of a block is not a face to take away: that is the block.
gecko.create_cube()
local side = gecko.faces()[1]
local ok, why = pcall(gecko.remove_face, side.id)
assert(not ok and tostring(why):find("belongs to a block"), tostring(why))
