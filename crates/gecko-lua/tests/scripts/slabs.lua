-- A block of small thickness, laid on four points.
--
-- What `Q` does from a surface's own corners, done from four points somebody
-- placed. The points are the block's outer face; it is thickened away from
-- them, and the sign of the thickness says which way.

gecko.add_slab({ {0,0,0}, {1,0,0}, {1,1,0}, {0,1,0} })
local one = gecko.stats()
assert(one.blocks == 1, "one block: " .. one.blocks)
assert(one.nodes == 8 and one.faces == 6, one.nodes .. " nodes, " .. one.faces .. " faces")

-- Thin by default: a tenth of the quadrangle's shortest diagonal, so the
-- far corners sit just off the plane the points are in.
local high = 0
for _, n in ipairs(gecko.nodes()) do
  if n.pos[3] > high then high = n.pos[3] end
end
assert(math.abs(high - 0.1 * math.sqrt(2)) < 1e-9, "default thickness: " .. high)

gecko.undo()
assert(gecko.stats().blocks == 0, "back to nothing")

-- Said out loud, and the other way: the sign picks the side.
gecko.add_slab({ {0,0,0}, {1,0,0}, {1,1,0}, {0,1,0} }, { thickness = -0.25 })
local low = 0
for _, n in ipairs(gecko.nodes()) do
  if n.pos[3] < low then low = n.pos[3] end
end
assert(math.abs(low + 0.25) < 1e-9, "the other way: " .. low)
print(string.format("%d blocks, %d nodes", gecko.stats().blocks, gecko.stats().nodes))
