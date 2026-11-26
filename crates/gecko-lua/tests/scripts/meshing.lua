-- Discretising, and writing the mesh out.

gecko.create_cube()
assert(gecko.stats().mesh.generation == 0, "nothing has been meshed")

gecko.mesh{ segments = 3 }
local m = gecko.stats().mesh
assert(m.hexes == 27 and m.nodes == 64, "three segments a side")

-- A sheet's number, which every edge of it has to share.
local edge = gecko.edges()[1].id
gecko.set_segments(edge, 2)
local sheet = gecko.sheet(edge)
assert(sheet.segments == 2, "the sheet says how fine it is")
assert(#sheet.edges == 4, "and which edges are on it")

gecko.mesh{ segments = 4 }
assert(gecko.stats().mesh.hexes == 2 * 4 * 4)

local path = SCRATCH .. "/mesh.vtk"
gecko.export_vtk(path)
local file = io.open(path):read("a")
assert(file:find("DATASET UNSTRUCTURED_GRID"), "a VTK legacy file")
assert(file:find("CELL_TYPES 32"), "with the cells it said")

-- How good it is: every cell of an untouched cube is a cube.
local q = gecko.quality()
assert(q.cells == 2 * 4 * 4, "one figure a cell")
assert(q.metric == "scaled_jacobian", "the scaled Jacobian unless asked")
assert(math.abs(q.min - 1) < 1e-12 and q.inverted == 0, "and every one perfect")
assert(#q.histogram == #q.bins, "a bin a count")
-- The other two, by name.
local skew = gecko.quality("skewness")
assert(skew.label == "skewness" and skew.max < 1e-12, "no skew in a cube")
-- Two segments one way and four the others: every cell twice as long as
-- it is wide.
local ratio = gecko.quality("aspect_ratio")
assert(math.abs(ratio.min - 2) < 1e-12 and math.abs(ratio.max - 2) < 1e-12, "half as fine one way")
local ok, err = pcall(gecko.quality, "jacobian")
assert(not ok and tostring(err):find("skewness"), "a name that is no metric: " .. tostring(err))
gecko.clear_mesh()
assert(gecko.quality() == nil, "no mesh, no figures")
