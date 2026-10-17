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
