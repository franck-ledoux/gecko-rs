-- An edge along two curves, a face across two surfaces: several entities
-- taken as one.

gecko.reset()
gecko.load_geometry(FIXTURES .. "/cube.msh")
gecko.box_geometry({ margin = 0 })
gecko.classify()

local function node_at(x, y, z)
  for _, n in ipairs(gecko.nodes()) do
    if math.abs(n.pos[1] - x) + math.abs(n.pos[2] - y) + math.abs(n.pos[3] - z) < 1e-9 then
      return n.id
    end
  end
end

local function edge_between(a, b)
  for _, e in ipairs(gecko.edges()) do
    if (e.nodes[1] == a and e.nodes[2] == b) or (e.nodes[1] == b and e.nodes[2] == a) then
      return e
    end
  end
end

local a, b = node_at(0, 0, 0), node_at(1, 0, 0)
gecko.classify_edge(edge_between(a, b).id, 1, {201, 202})
local on = edge_between(a, b).on
assert(on.dim == 1 and on.tag < 0, "on a composite of curves")

-- One tag is still one tag, and a list of nodes still a list.
gecko.classify_edge({ edge_between(a, b).id }, 1, 201)
assert(edge_between(a, b).on.tag == 201, "back on curve 201")

-- The bottom face, across surfaces 1 and 3.
local bottom
for _, f in ipairs(gecko.faces()) do
  if math.abs(f.center[3]) < 1e-9 then bottom = f.id end
end
gecko.classify_face(bottom, 2, {1, 3})

-- And a node, along two curves past the corner where they meet.
gecko.classify_node(b, 1, {201, 202})

-- Refused, and nothing changed: the top ring reaches neither curve.
local top = edge_between(node_at(0, 0, 1), node_at(1, 0, 1))
local ok, err = pcall(gecko.classify_edge, top.id, 1, {201, 202})
assert(not ok and tostring(err):find("corner 105"), tostring(err))
assert(edge_between(node_at(0, 0, 1), node_at(1, 0, 1)).on.tag == top.on.tag)

local script = gecko.script()
for _, line in ipairs({
  "gecko.classify_edge({" .. edge_between(a, b).id .. "}, 1, {201, 202})",
  "gecko.classify_face({" .. bottom .. "}, 2, {1, 3})",
  "gecko.classify_node({" .. b .. "}, 1, {201, 202})",
}) do
  assert(script:find(line, 1, true), "the history says " .. line .. ":\n" .. script)
end
