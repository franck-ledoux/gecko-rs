-- One blocking of one part, faceted and exact: the cylinder of radius 1
-- and height 2, as gmsh meshed it (cylinder.msh) and as OpenCascade has it
-- (cylinder.gcad, NURBS). The two are blocked the same way; the exact one
-- puts the mesh on the cylinder itself, and loses nothing for it.

local GCAD = FIXTURES .. "/../../../gecko-geom/tests/data/cylinder.gcad"

local function run(load, file)
  gecko.reset()
  load(file)
  gecko.box_geometry{ margin = 0 }
  -- The four corners at each end onto its rim, found by where it is: the
  -- two files number their entities differently.
  for _, z in ipairs({ 0, 2 }) do
    local rim = gecko.closest({ 1, 0, z }).curve
    local four = {}
    for _, node in ipairs(gecko.nodes()) do
      if math.abs(node.pos[3] - z) < 0.5 then four[#four + 1] = node.id end
    end
    gecko.classify_node(four, 1, rim.tag)
  end
  gecko.classify()
  gecko.mesh{ segments = 6 }
  -- How far the points of the side of the mesh are from the cylinder.
  local worst = 0
  local points = gecko.vtk():match("POINTS %d+ double\n(.-)\nCELLS")
  for x, y, z in points:gmatch("(%S+) (%S+) (%S+)") do
    x, y, z = tonumber(x), tonumber(y), tonumber(z)
    local r = math.sqrt(x * x + y * y)
    if math.abs(r - 1) < 0.01 and z > 1e-9 and z < 2 - 1e-9 then
      worst = math.max(worst, math.abs(r - 1))
    end
  end
  return gecko.stats().classified, worst, gecko.quality().min, gecko.fidelity()
end

local on_msh, off_msh, q_msh = run(gecko.load_geometry, FIXTURES .. "/cylinder.msh")
local on_cad, off_cad, q_cad, f_cad = run(gecko.load_cad, GCAD)

assert(on_cad.nodes == on_msh.nodes and on_cad.edges == on_msh.edges and on_cad.faces == on_msh.faces,
  "the same blocking, classified the same")
assert(off_cad < 1e-12, "the exact mesh is on the cylinder: " .. off_cad)
assert(off_cad < off_msh, "and nearer it than the faceted one: " .. off_cad .. " against " .. off_msh)
-- At least as good: the points of the rims and the side are where the
-- shape is, not on a polygon a little inside it.
assert(q_cad >= q_msh - 0.01 * q_msh, "as good a mesh: " .. q_cad .. " against " .. q_msh)
assert(f_cad.skin.worst.distance < 1e-12, "and its skin measured on the exact surfaces")
