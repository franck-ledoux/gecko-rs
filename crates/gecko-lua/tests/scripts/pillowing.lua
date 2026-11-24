-- A pillow surface given as faces alone, and one given as blocks.

-- Two cubes stacked and glued: one face between them.
local function two_stacked()
  gecko.reset()
  gecko.create_cube{ origin = {0, 0, 0}, size = 1 }
  gecko.create_cube{ origin = {0, 0, 1}, size = 1 }
  local pair = {}
  for _, face in ipairs(gecko.faces()) do
    if math.abs(face.center[3] - 1) < 1e-9 then
      pair[#pair + 1] = face.id
    end
  end
  gecko.glue_faces(pair[1], pair[2])
  for _, face in ipairs(gecko.faces()) do
    if math.abs(face.center[3] - 1) < 1e-9 then
      return face.id
    end
  end
end

-- The face between them, with no block: the kernel finds the side.
local between = two_stacked()
gecko.pillow({ between })
assert(gecko.stats().blocks == 3, "a layer between the two")
assert(gecko.script():find("gecko.pillow({" .. between .. "})", 1, true),
  "and the history says the face alone: " .. gecko.script())

-- The bottom cube as a block: the pillow surface goes round it, outer
-- faces included, and the layer goes towards it.
two_stacked()
local low = gecko.blocks()[1].id
gecko.pillow_blocks({ low })
assert(gecko.stats().blocks == 2 + 6, "round the six faces of the cube")

-- Without the outer faces, just the face between the two.
two_stacked()
low = gecko.blocks()[1].id
gecko.pillow_blocks({ low }, { outer = false })
assert(gecko.stats().blocks == 3, "the face between them only")
assert(gecko.script():find("gecko.pillow_blocks({" .. low .. "}, { outer = false })", 1, true),
  gecko.script())

-- Both blocks away, as one step.
two_stacked()
local ids = {}
for _, block in ipairs(gecko.blocks()) do ids[#ids + 1] = block.id end
gecko.remove_blocks(ids)
assert(gecko.stats().blocks == 0, "both gone")
gecko.undo()
assert(gecko.stats().blocks == 2, "and back in one undo")
