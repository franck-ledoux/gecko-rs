-- The reason for having a language at all: a loop and a conditional
-- driving the kernel.

gecko.load_geometry(FIXTURES .. "/cylinder.msh")
gecko.box_geometry{ margin = 0 }

-- The four corners of each end onto the rim they are meant to follow.
for _, rim in ipairs({ { z = 0, tag = 201 }, { z = 2, tag = 202 } }) do
  local four = {}
  for _, node in ipairs(gecko.nodes()) do
    if math.abs(node.pos[3] - rim.z) < 0.5 then
      four[#four + 1] = node.id
    end
  end
  assert(#four == 4, "four corners at each end")
  gecko.classify_node(four, 1, rim.tag)
end

assert(gecko.stats().classified.nodes == 8, "all eight on the rims")

-- Every node that ended up on a curve is out at the radius, which is what
-- the projection did.
for _, node in ipairs(gecko.nodes()) do
  if node.on and node.on.dim == 1 then
    local r = math.sqrt(node.pos[1] ^ 2 + node.pos[2] ^ 2)
    assert(math.abs(r - 1) < 1e-9, "a node on the rim is not on the circle")
  end
end
