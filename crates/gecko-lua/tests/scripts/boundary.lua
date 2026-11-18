-- One face of the boundary came out pitch black under the shading, and this
-- is the model it was reported on. Recorded from a session, and written by
-- **where each cell is** rather than by the names that session saw: a name
-- is the structure's to choose, and the names moved when the block
-- structure stopped being a combinatorial map. The shape is the same, and
-- the shape is what the bug was about.

local function near(p, q)
  return math.abs(p[1] - q[1]) < 1e-6
     and math.abs(p[2] - q[2]) < 1e-6
     and math.abs(p[3] - q[3]) < 1e-6
end

local function place(id)
  for _, node in ipairs(gecko.nodes()) do
    if node.id == id then return node.pos end
  end
end

-- The face whose centre is at a point, and the block it belongs to.
local function face_at(at)
  for _, face in ipairs(gecko.faces()) do
    if near(face.center, at) then return face.id, face.block end
  end
  error("no face centred at " .. table.concat(at, ", "))
end

-- Cut the sheet through the edge between two points, at `t` **measured from
-- the first of them**: which way round an edge runs is the structure's to
-- choose, so the script says which end it means.
local function cut(from, to, t)
  for _, e in ipairs(gecko.edges()) do
    local a, b = place(e.nodes[1]), place(e.nodes[2])
    if near(a, from) and near(b, to) then
      gecko.split_sheet(e.id, t)
      return
    end
    if near(a, to) and near(b, from) then
      gecko.split_sheet(e.id, 1 - t)
      return
    end
  end
  error("no edge between those two points")
end

gecko.create_cube{ origin = {0, 0, 0}, size = 1 }

-- Out at y = 1, then down at z = 0.
local face, block = face_at({0.5, 1, 0.5})
gecko.extrude_face(face, block, 0.4182381969483314)
face, block = face_at({0.5, 0.5, 0})
gecko.extrude_face(face, block, 0.4151222583184677)

-- Four cuts across the piece that grew out at y = 1.4182.
cut({0, 1.4182381969483314, 1}, {1, 1.4182381969483314, 1}, 0.4839949382283461)
cut({1, 1.4182381969483314, 1}, {0.4839949382283461, 1.4182381969483314, 1}, 0.263662443881934)
cut({1, 1.4182381969483314, 1}, {1, 1.4182381969483314, 0}, 0.28041743818831727)
cut({1, 1.4182381969483314, 0}, {1, 1.4182381969483314, 0.7195825618116827}, 0.5251268070619435)

local s = gecko.stats()
print(string.format("%d blocks, %d nodes, %d faces, %d edges", s.blocks, s.nodes, s.faces, s.edges))
-- The shape, pinned: written by position rather than by name, these four
-- numbers came out the same over the combinatorial map and over the cells
-- that replaced it, which is what says the model is the reported one and
-- not merely a model of about that size.
assert(s.blocks == 21, "twenty-one blocks: " .. s.blocks)
assert(s.nodes == 56, "fifty-six nodes: " .. s.nodes)
assert(s.faces == 88, "eighty-eight faces: " .. s.faces)
assert(s.edges == 122, "a hundred and twenty-two edges: " .. s.edges)
