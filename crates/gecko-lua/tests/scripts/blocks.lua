-- Making blocks, joining them, taking them away.

assert(gecko.stats().blocks == 0, "a session starts empty")

gecko.create_cube()
gecko.create_cube{ origin = {3, 0, 0}, size = 2 }
local s = gecko.stats()
assert(s.blocks == 2, "two blocks")
assert(s.nodes == 16, "and sixteen corners between them")

-- Every block is listed with its centre, which is how a script names one
-- without having a mouse.
local blocks = gecko.blocks()
assert(#blocks == 2)
assert(math.abs(blocks[1].centre[1] - 0.5) < 1e-9, "the first cube's centre")

gecko.remove_block(blocks[2].id)
assert(gecko.stats().blocks == 1, "and one of them taken away")

-- A refusal stops the script where it went wrong rather than letting it
-- carry on over a mistake.
local ok, why = pcall(gecko.remove_block, 999)
assert(not ok, "a block that does not exist was removed")
assert(tostring(why):find("999"), "the message says which: " .. tostring(why))

-- Nothing is nothing: a cell on no entity has `on == nil`, which is how
-- anybody would write it. A sentinel that is not nil but prints like it
-- would be a trap.
for _, edge in ipairs(gecko.edges()) do
  assert(edge.on == nil, "an unclassified edge is not on nil")
end
