-- The block structure written up, and what looks wrong with it.

local function kinds(b)
  local n = {}
  for _, i in ipairs(b.issues) do n[i.kind] = (n[i.kind] or 0) + 1 end
  return n
end

-- A cube boxed and classified: nothing wrong, and all of it listed.
gecko.load_geometry(FIXTURES .. "/cube.msh")
gecko.box_geometry{ margin = 0 }
gecko.classify()
local b = gecko.blocking()
assert(#b.issues == 0, b.issues[1] and b.issues[1].message)
assert(b.summary.blocks == 1 and b.summary.inverted_blocks == 0 and b.summary.outer_faces == 6)
assert(#b.blocks == 1 and #b.blocks[1].corners == 8 and math.abs(b.blocks[1].jacobian - 1) < 1e-9)
assert(#b.nodes == 8 and #b.edges == 12 and #b.faces == 6 and #b.sheets == 3)
for _, n in ipairs(b.nodes) do assert(n.outer and n.on.dim == 0, "every corner on a corner") end
for _, e in ipairs(b.edges) do assert(e.on.dim == 1 and e.sheet ~= nil) end
assert(#b.summary.not_followed.curves == 0 and #b.summary.not_followed.surfaces == 0)

-- Let one corner go: it follows nothing now, and the geometric corner has
-- no node on it. Its faces keep what they were told, which stands until
-- something contradicts it.
local corner = b.nodes[1].id
gecko.unclassify_node(corner)
b = gecko.blocking()
local k = kinds(b)
assert(k.outer_node_on_nothing == 1, "the corner let go")
assert(k.corner_not_held == 1, "and the geometric corner nothing is on now")
assert(k.outer_face_on_nothing == nil, "its faces keep their surfaces")

-- A box never classified: every outer face follows nothing, and says why.
gecko.reset()
gecko.load_geometry(FIXTURES .. "/cube.msh")
gecko.box_geometry{ margin = 0.1 }
b = gecko.blocking()
k = kinds(b)
assert(k.outer_face_on_nothing == 6 and k.outer_node_on_nothing == 8)
assert(k.surface_not_covered == 6 and k.curve_not_followed == 12 and k.corner_not_held == 8)
for _, i in ipairs(b.issues) do
  if i.kind == "outer_face_on_nothing" then
    assert(#i.corners_on == 4 and i.corners_on[1].node and i.corners_on[1].on == nil)
    assert(i.message:find("nor are its corners"), i.message)
  end
end

-- Fold a block through itself: it is turned inside out before any mesh.
gecko.reset()
gecko.create_cube{ origin = { 0, 0, 0 }, size = 1 }
local far
for _, n in ipairs(gecko.nodes()) do
  if n.pos[1] == 1 and n.pos[2] == 1 and n.pos[3] == 1 then far = n.id end
end
gecko.move_node(far, { -0.5, -0.5, -0.5 })
b = gecko.blocking()
assert(b.summary.inverted_blocks == 1 and kinds(b).inverted_block == 1)
assert(b.summary.worst_block.jacobian < 0)
-- No geometry, so nothing to follow and nothing said about it.
assert(b.summary.not_followed == nil)
