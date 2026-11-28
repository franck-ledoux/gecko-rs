-- agree.lua's question for an exact geometry: the same script, run by
-- `gecko run` and in the page's console, comes to the same bytes. NURBS
-- projections are where two hosts could most easily part — iterations,
-- square roots, a last digit rounded one way — and the mesh below has every
-- point of its skin projected onto a cylinder, its rims and its discs.
--
-- Run by the Rust suite, which finds the file beside its fixtures, and by a
-- Playwright test, which hands the page the file first.

local GCAD = FIXTURES and (FIXTURES .. "/../../../gecko-geom/tests/data/cylinder.gcad") or "cylinder.gcad"

gecko.reset()
gecko.load_cad(GCAD)
gecko.box_geometry{ margin = 0 }
for _, z in ipairs({ 0, 2 }) do
  local rim = gecko.closest({ 1, 0, z }).curve
  local four = {}
  for _, node in ipairs(gecko.nodes()) do
    if math.abs(node.pos[3] - z) < 0.5 then four[#four + 1] = node.id end
  end
  gecko.classify_node(four, 1, rim.tag)
end
gecko.classify()
gecko.smooth{ rounds = 5 }
gecko.mesh{ segments = 4 }

local text = gecko.vtk()
local sum = 0
for i = 1, #text do
  sum = (sum * 31 + text:byte(i)) % 1000003
end
local m = gecko.stats().mesh
print(string.format("exact: %d nodes, %d hexes, digest %d", m.nodes, m.hexes, sum))
assert(m.hexes == 64, "the mesh is not the one this script makes: " .. m.hexes)
assert(sum == 809104, "the two hosts do not agree on the bytes: " .. sum)
