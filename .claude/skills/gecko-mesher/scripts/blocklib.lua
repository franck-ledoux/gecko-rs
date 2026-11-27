-- blocklib: the bookkeeping every Gecko blocking script repeats.
--
-- Copy this file beside your script and load it:
--
--   local L = dofile("blocklib.lua")
--
-- Then:
--   L.point(name, {x,y,z} [, dim, tag])  register a block vertex: where it is
--                                         and what it is attached to (nothing
--                                         when dim is nil: inside the part)
--   L.block{ n0..n7 }                     a block from 8 point names: bottom
--                                         quad n0-n3, then n4-n7 above them;
--                                         turned right-handed if it is not
--   L.weld()                              glue every pair of coincident faces
--   L.attach()                            classify each node on its point's
--                                         entity (and so project it there)
--   L.size(h [, {min=2, stretch=1.1}])    segments per sheet from a target
--                                         cell size: the longest edge of the
--                                         sheet / h, times `stretch` for arcs
--   L.issues([most])                      print the blocking's problems;
--                                         returns gecko.blocking()
--   L.summary()                           print quality and fidelity in a line
--                                         each; returns quality, fidelity
--   L.tuned{ gecko =, min_sj =, curves = } what the positions were tuned on
--                                         and what they gave; return it from
--                                         the script (`return { tuned = … }`)
--                                         so check.py can tell when Gecko has
--                                         moved under them
--
-- Order matters: build every block, weld, then attach. Welding matches faces
-- by their centres, and attaching moves nodes onto the geometry.

local L = { points = {}, order = {} }

local function sub(a, b) return { a[1] - b[1], a[2] - b[2], a[3] - b[3] } end
local function dot(a, b) return a[1] * b[1] + a[2] * b[2] + a[3] * b[3] end
local function cross(a, b)
  return { a[2] * b[3] - a[3] * b[2], a[3] * b[1] - a[1] * b[3], a[1] * b[2] - a[2] * b[1] }
end

--- Register a vertex. Registering the same name twice replaces it.
function L.point(name, pos, dim, tag)
  assert(type(pos) == "table" and #pos == 3, "point " .. tostring(name) .. ": a position is three numbers")
  if (dim == nil) ~= (tag == nil) then error("point " .. name .. ": give both dim and tag, or neither") end
  if not L.points[name] then L.order[#L.order + 1] = name end
  L.points[name] = { pos = { pos[1], pos[2], pos[3] }, dim = dim, tag = tag }
end

function L.pos(name)
  local p = L.points[name]
  assert(p, "no point called " .. tostring(name))
  return p.pos
end

--- A block from eight point names. Returns the corner positions it used.
function L.block(names)
  assert(#names == 8, "a block has eight corners")
  local c = {}
  for i, n in ipairs(names) do c[i] = L.pos(n) end
  local vol = dot(cross(sub(c[2], c[1]), sub(c[4], c[1])), sub(c[5], c[1]))
  if vol == 0 then error("block " .. table.concat(names, ",") .. " is flat at its first corner") end
  if vol < 0 then c = { c[1], c[4], c[3], c[2], c[5], c[8], c[7], c[6] } end
  gecko.add_block(c)
  return c
end

--- Glue every pair of faces with the same centre. Returns how many.
function L.weld()
  local seen, glued = {}, 0
  for _, f in ipairs(gecko.faces()) do
    local k = string.format("%.6f,%.6f,%.6f", f.center[1], f.center[2], f.center[3])
    if seen[k] and seen[k] ~= f.id then
      gecko.glue_faces(seen[k], f.id)
      glued = glued + 1
    else
      seen[k] = f.id
    end
  end
  return glued
end

--- Classify every node that sits on a registered point with dim and tag.
--- Returns how many nodes were classified, and the names of points no node
--- was found at (a block that was never built, a typo in a position).
function L.attach(tolerance)
  local tol2 = (tolerance or 1e-6) ^ 2
  local done, found = 0, {}
  for _, n in ipairs(gecko.nodes()) do
    for _, name in ipairs(L.order) do
      local p = L.points[name]
      local d = sub(n.pos, p.pos)
      if dot(d, d) < tol2 then
        found[name] = true
        if p.dim then
          gecko.classify_node(n.id, p.dim, p.tag)
          done = done + 1
        end
        break
      end
    end
  end
  local unused = {}
  for _, name in ipairs(L.order) do
    if not found[name] then unused[#unused + 1] = name end
  end
  return done, unused
end

--- Set every sheet's segments from a target cell size.
function L.size(h, options)
  options = options or {}
  local least, stretch = options.min or 2, options.stretch or 1.1
  local pos = {}
  for _, n in ipairs(gecko.nodes()) do pos[n.id] = n.pos end
  local edges = gecko.edges()
  local function len(e)
    local d = sub(pos[e.nodes[1]], pos[e.nodes[2]])
    return math.sqrt(dot(d, d))
  end
  table.sort(edges, function(a, b) return len(a) > len(b) end)
  local done, sheets = {}, 0
  for _, e in ipairs(edges) do
    if not done[e.id] then
      gecko.set_segments(e.id, math.max(least, math.ceil(stretch * len(e) / h)))
      sheets = sheets + 1
      -- A sheet's edges are tables, not ids.
      for _, x in ipairs(gecko.sheet(e.id).edges) do done[x.id] = true end
    end
  end
  return sheets
end

--- Print the blocking's problems. Returns the whole of gecko.blocking().
function L.issues(most)
  local b = gecko.blocking()
  local s = b.summary
  local worst = s.worst_block and string.format("%.3f (block %d)", s.worst_block.jacobian, s.worst_block.block) or "-"
  print(string.format("blocking: %d blocks, %d inverted, worst block %s, %d issues",
    s.blocks, s.inverted_blocks, worst, #b.issues))
  for i, issue in ipairs(b.issues) do
    if i > (most or 12) then
      print(string.format("  … and %d more", #b.issues - (most or 12)))
      break
    end
    print("  - " .. issue.message)
  end
  return b
end

--- Print the mesh's quality and fidelity. Returns both.
function L.summary()
  local q = gecko.quality()
  if not q then print("no mesh") return end
  local below = 0
  for i, t in ipairs(q.bins) do
    if t < 0.7 then below = below + q.histogram[i] end
  end
  print(string.format("mesh: %d hexes, scaled Jacobian min %.3f mean %.3f, %d inverted, %d below 0.7",
    gecko.stats().mesh.hexes, q.min, q.mean, q.inverted or 0, below))
  local f = gecko.fidelity()
  if f then
    local missed = {}
    for _, c in ipairs(f.curves.missed) do missed[#missed + 1] = "curve " .. c.tag end
    for _, c in ipairs(f.corners.missed) do missed[#missed + 1] = "corner " .. c.tag end
    print(string.format("fidelity: curves %d/%d, skin max %.3f%s", f.curves.followed, f.curves.checked,
      f.skin.max, #missed > 0 and (", missed: " .. table.concat(missed, ", ")) or ""))
  end
  return q, f
end

--- What the vertex positions were tuned on, and what they gave there.
---
--- Tuned positions hold for the Gecko they were tuned with, not for every
--- Gecko: a change in how nodes are projected moves every node on a curve,
--- and positions tuned against the old ones are no longer the best — on B1,
--- 0.2.10's smooth projection took a placement tuned at 0.644 to 0.467, and
--- a retune brought it back. Lua cannot ask Gecko's version yet
--- (references/feature-requests.md), so the script states it, returns it,
--- and `check.py` compares it with the `gecko` field of the report.
function L.tuned(stamp)
  assert(type(stamp) == "table" and stamp.gecko and stamp.min_sj,
    "L.tuned{ gecko = \"0.2.10\", min_sj = 0.644, curves = \"17/17\" }")
  L.stamp = stamp
  return stamp
end

return L
