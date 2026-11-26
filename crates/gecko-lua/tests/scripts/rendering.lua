-- A picture of the session, drawn without a browser.

local function png(path)
  local f = assert(io.open(path, "rb"))
  local head = f:read(24)
  f:close()
  assert(head:sub(1, 8) == "\137PNG\r\n\26\n", "a PNG signature")
  -- Width and height, big-endian, in the header chunk.
  local function be(s) local a, b, c, d = s:byte(1, 4); return ((a * 256 + b) * 256 + c) * 256 + d end
  return be(head:sub(17, 20)), be(head:sub(21, 24))
end

-- The geometry alone: labelled, from the default view.
gecko.load_geometry(FIXTURES .. "/cylinder.msh")
local said = gecko.render(SCRATCH .. "/geometry.png", { width = 320, height = 240 })
assert(said:find("geometry") and said:find("labels"), said)
local w, h = png(SCRATCH .. "/geometry.png")
assert(w == 320 and h == 240)

-- Blocks round it, from above; then the mesh, with the bad cells over it.
gecko.box_geometry{ margin = 0 }
gecko.classify()
said = gecko.render(SCRATCH .. "/blocks.png", { view = "top", width = 200, height = 200 })
assert(said:find("blocks") and said:find("geometry"), said)
gecko.mesh{ segments = 4 }
said = gecko.render(SCRATCH .. "/mesh.png", { width = 200, height = 150, below = 2, color = "block" })
assert(said:find("mesh"), said)
assert(said:find("cells below the threshold"), "every cell is below 2: " .. said)
png(SCRATCH .. "/mesh.png")

-- A close-up: framed on a ball rather than on everything.
gecko.render(SCRATCH .. "/close.png", { center = { 1, 0, 1 }, radius = 0.3, view = { 1, 0, 0 } })

-- Options that make no sense are refused, saying what would do.
local ok, err = pcall(gecko.render, SCRATCH .. "/x.png", { view = "sideways" })
assert(not ok and tostring(err):find("iso"), tostring(err))
ok, err = pcall(gecko.render, SCRATCH .. "/x.png", { show = { "blocs" } })
assert(not ok and tostring(err):find("blocks"), tostring(err))
