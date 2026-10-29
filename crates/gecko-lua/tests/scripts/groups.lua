-- Groups of blocks, from a script: made by naming them, and carried by the
-- model through its own operations.
gecko.create_cube()
gecko.create_cube{ origin = {2, 0, 0} }

local all = {}
for _, b in ipairs(gecko.blocks()) do all[#all + 1] = b.id end
assert(#all == 2, "two blocks")

gecko.group("fluid", all[1])
local groups = gecko.groups()
assert(#groups == 1, "one group")
assert(groups[1].name == "fluid", groups[1].name)
assert(groups[1].blocks == 1, groups[1].blocks)
assert(groups[1].written, "written into the mesh unless it says otherwise")

-- The same name is the same group, and a block can be in two at once.
gecko.group("fluid", all[2])
gecko.group("looking", all[1], { written = false })
local by_name = {}
for _, g in ipairs(gecko.groups()) do by_name[g.name] = g end
assert(by_name.fluid.blocks == 2, by_name.fluid.blocks)
assert(by_name.looking.blocks == 1)
assert(not by_name.looking.written, "a temporary group stays out of the mesh")

-- Cutting a block of the fluid gives two blocks of the fluid: the model
-- carries the membership, nothing recomputes it.
local edge
for _, e in ipairs(gecko.edges()) do if not edge then edge = e.id end end
gecko.split_sheet(edge, 0.5)
assert(gecko.stats().blocks == 3, "one of them became two")
for _, g in ipairs(gecko.groups()) do
  if g.name == "fluid" then assert(g.blocks == 3, g.blocks) end
end

-- Out of a group, and the group stays; the group goes, and the blocks stay.
gecko.ungroup("fluid", all[2])
for _, g in ipairs(gecko.groups()) do
  if g.name == "fluid" then assert(g.blocks == 2, g.blocks) end
end
gecko.rename_group("fluid", "water")
gecko.drop_group("looking")
local left = gecko.groups()
assert(#left == 1 and left[1].name == "water", "one group, renamed")
assert(gecko.stats().blocks == 3, "the blocks are all still there")

-- And the mesh carries a column for it.
gecko.mesh{ segments = 2 }
local file = gecko.vtk()
assert(file:find("SCALARS water int 1", 1, true), "the group is a variable of the mesh")
assert(not file:find("looking", 1, true), "a group that was dropped is not")
print("groups: " .. #left .. " left, " .. gecko.stats().blocks .. " blocks")
