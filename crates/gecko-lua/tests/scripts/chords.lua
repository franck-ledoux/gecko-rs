-- The thread through a face: taking one out, and wrapping one in an o-grid.

-- Three slabs stacked and glued, so one chord runs through all three.
local function three_slabs()
  for z = 0, 2 do
    gecko.create_cube{ origin = {0, 0, z}, size = 1 }
  end
  for z = 1, 2 do
    local pair = {}
    for _, face in ipairs(gecko.faces()) do
      if math.abs(face.centre[3] - z) < 1e-9 then
        pair[#pair + 1] = face.id
      end
    end
    assert(#pair == 2, "two faces meet at z = " .. z)
    gecko.glue_faces(pair[1], pair[2])
  end
end

-- The (face, block) pair a click gives: for a chord the side does not
-- matter, but the kernel is told which one anyway.
local function flat_at(z)
  for _, face in ipairs(gecko.faces()) do
    if math.abs(face.centre[3] - z) < 1e-9 then
      return face.id, face.block
    end
  end
  error("no face at z = " .. z)
end

three_slabs()
assert(gecko.stats().blocks == 3)

local face, block = flat_at(0)
gecko.pillow_chord(face, block)
assert(gecko.stats().blocks == 15, "five blocks where each one was")

-- The ends of the tube stayed where they were: the chord runs from z = 0
-- to z = 3 and nothing was pushed past either end.
local low, high = math.huge, -math.huge
for _, node in ipairs(gecko.nodes()) do
  low = math.min(low, node.pos[3])
  high = math.max(high, node.pos[3])
end
assert(math.abs(low) < 1e-9 and math.abs(high - 3) < 1e-9, "the ends moved")

-- Undo, and then take the chord away instead.
gecko.undo()
assert(gecko.stats().blocks == 3)

face, block = flat_at(0)
gecko.remove_chord(face, block)
assert(gecko.stats().blocks == 0, "the stack was the chord")

-- A face nobody answers to stops the script rather than letting it carry on.
local ok, why = pcall(gecko.remove_chord, 4242, 0)
assert(not ok, "a chord through a face that does not exist was removed")
assert(tostring(why):find("no face"), "the message says why: " .. tostring(why))
