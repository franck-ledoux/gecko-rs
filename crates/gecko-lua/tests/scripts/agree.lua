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
