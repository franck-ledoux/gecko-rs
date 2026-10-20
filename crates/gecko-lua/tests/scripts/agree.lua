-- The same script, run by `gecko run` and in the page's console, has to
-- come to the same bytes. Two hosts answer the four primitives — one holds
-- the session, the other reaches it through JavaScript — and nothing above
-- them is allowed to differ.
--
-- This file is run by the Rust suite **and** by a Playwright test that
-- feeds it to the console. The assertion below is the agreement: if either
-- host drifts, one of the two fails.

gecko.create_cube()
gecko.create_cube{ origin = {1, 0, 0} }

-- Glue them, found by looking rather than by knowing: the two faces at
-- x = 1, one from each block.
local pair = {}
for _, face in ipairs(gecko.faces()) do
  if math.abs(face.centre[1] - 1) < 1e-9 then pair[#pair + 1] = face.id end
end
assert(#pair == 2, "two faces meet at x = 1")
gecko.glue_faces(pair[1], pair[2])

gecko.set_segments(gecko.edges()[1].id, 2)
gecko.mesh{ segments = 3 }

local text = gecko.vtk()
assert(text, "there is a mesh to write out")
local sum = 0
for i = 1, #text do
  sum = (sum * 31 + text:byte(i)) % 1000003
end

local m = gecko.stats().mesh
print(string.format("%d nodes, %d hexes, digest %d", m.nodes, m.hexes, sum))
assert(m.hexes == 36, "the mesh is not the one this script makes: " .. m.hexes)
assert(sum == 392026, "the two hosts do not agree on the bytes: " .. sum)

-- And again for **smoothing**, which has an order of its own to agree
-- about: nodes are visited by name and each move is applied before the
-- next is worked out, so a host that walked them in some other order
-- would come to different positions from the same model.
--
-- The two cubes above are all boundary and follow nothing, so none of
-- their nodes may move. Three by three by three has **eight** nodes
-- inside it, and they are neighbours of each other — which is what makes
-- the order they are visited in observable at all: each move is applied
-- before the next is worked out, so a host walking them another way round
-- would reach different positions from the same model.
for _ = 1, 20 do gecko.undo() end
assert(gecko.stats().blocks == 0, "back to nothing")

for i = 0, 2 do
  for j = 0, 2 do
    for k = 0, 2 do
      gecko.create_cube{ origin = {i, j, k} }
    end
  end
end

-- Every pair of faces sharing a centre, read once: gluing one pair leaves
-- the others' names alone.
local seen, pairs_to_glue = {}, {}
for _, face in ipairs(gecko.faces()) do
  local key = string.format("%.4f/%.4f/%.4f", face.centre[1], face.centre[2], face.centre[3])
  if seen[key] then
    pairs_to_glue[#pairs_to_glue + 1] = { seen[key], face.id }
  else
    seen[key] = face.id
  end
end
for _, both in ipairs(pairs_to_glue) do
  gecko.glue_faces(both[1], both[2])
end
assert(gecko.stats().blocks == 27, "twenty-seven blocks")

-- The node in the middle, dragged off centre and then smoothed back by
-- hand: rounds and a share, both said out loud.
local middle
for _, node in ipairs(gecko.nodes()) do
  if math.abs(node.pos[1] - 1) < 1e-9
    and math.abs(node.pos[2] - 1) < 1e-9
    and math.abs(node.pos[3] - 1) < 1e-9 then
    middle = node.id
  end
end
assert(middle, "the node in the middle")

gecko.move_node(middle, {1.6, 1.4, 0.7})
gecko.smooth{ rounds = 2, share = 0.6 }

-- And once more as the interface does it: a drag that ripples outwards,
-- ring by ring, each ring going a shorter way than the one inside it,
-- until a ring barely moves.
--
-- The message is read rather than thrown away: an option the binding
-- quietly dropped would leave the digests below agreeing on a model that
-- was never smoothed, which is the one way this file could pass while
-- saying nothing.
local rippled = gecko.move_node(middle, {1.3, 1.2, 1.1}, { smooth = true })
assert(rippled:find("ring"), "the drag rippled: " .. rippled)

gecko.mesh{ segments = 2 }
local after = gecko.vtk()
assert(after, "there is a mesh to write out")
local digest = 0
for i = 1, #after do
  digest = (digest * 31 + after:byte(i)) % 1000003
end

local grid = gecko.stats().mesh
print(string.format("smoothed: %d nodes, %d hexes, digest %d", grid.nodes, grid.hexes, digest))
assert(grid.hexes == 216, "two segments each way over twenty-seven blocks: " .. grid.hexes)
assert(digest == 197086, "the two hosts do not agree on the smoothed bytes: " .. digest)
