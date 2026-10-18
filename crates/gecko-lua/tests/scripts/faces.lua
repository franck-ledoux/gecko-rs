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
