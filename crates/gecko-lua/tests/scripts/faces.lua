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

-- And a patch is raised into blocks, all of it at once.
gecko.undo()           -- the cube
gecko.face_on_surface(1)
local face = gecko.faces()[1]
gecko.split_sheet(gecko.edges()[1].id, 0.5)
assert(gecko.stats().faces == 2, "two faces in one patch")

face = gecko.faces()[1]
gecko.extrude_patch(face.id, 0.5)
s = gecko.stats()
assert(s.blocks == 2, "a block from each face")
assert(s.patches == 0, "and the sheet is gone")
assert(s.nodes == 12, "glued to each other: twelve corners, not sixteen")

-- Either way: a sheet has no inside, so neither of its sides is the wrong
-- one. Said without naming a direction, because which way the quadrangle
-- is wound came from the triangles under it: the two answers are mirror
-- images of each other about the sheet.
local function centroid()
  local c, n = { 0, 0, 0 }, 0
  for _, node in ipairs(gecko.nodes()) do
    for k = 1, 3 do c[k] = c[k] + node.pos[k] end
    n = n + 1
  end
  for k = 1, 3 do c[k] = c[k] / n end
  return c
end

gecko.undo()
assert(gecko.stats().patches == 1, "the sheet is back")
local flat = centroid()
face = gecko.faces()[1]

gecko.extrude_patch(face.id, 0.5)
assert(gecko.stats().blocks == 2, "two blocks, one way")
local up = centroid()

gecko.undo()
gecko.extrude_patch(face.id, -0.5)
assert(gecko.stats().blocks == 2, "two blocks, the other way")
local down = centroid()

local apart = 0
for k = 1, 3 do
  assert(
    math.abs((up[k] - flat[k]) + (down[k] - flat[k])) < 1e-9,
    "the two directions are not mirror images about the sheet"
  )
  apart = apart + math.abs(up[k] - down[k])
end
assert(apart > 0.1, "both extrusions went the same way")
