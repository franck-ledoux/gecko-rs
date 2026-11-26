-- Mambo B1, meshed with Gecko: a worked example of the whole workflow.
--
--   gecko run B1.lua           (B1.msh and blocklib.lua beside it)
--
-- The part: a wall 2 thick and 10 high bent into an S — two half-annuli of
-- radii 3 to 5, centred (0,0) below y = 0 and (8,0) above it, meeting at
-- x in [3,5] — with a hole of radius 2 drilled along x through the joint at
-- z = 3, and the left end trimmed from below by a cylinder of radius 5 along
-- y through (x,z) = (-5,-2).
--
-- The blocking: one block through the wall, quads laid out in the developed
-- (s, z) plane, s the arc length at mid-radius (s < 0 on the left bend):
--
--   Q1 Q2 Q3      the trimmed left end: a vertex M on the trim arc where three
--                 blocks meet takes the arc's 90° (patterns.md §5)
--   OLU OLL       the hole's O-grid, left half, split at the hole's west point
--   OTL OTR       its top, split at the joint s = 0 — the S-bend's inflection,
--   OBL OBR       and its bottom likewise (patterns.md §6)
--   ORu ORl       its right half, split at the hole's east point, and that cut
--   Ru Rl         carried through the right bend to the end face, so the box
--                 corners there open towards 60° (patterns.md §4)
--
-- 13 blocks, 25 242 hexes at H = 0.4: scaled Jacobian min 0.640, mean 0.929,
-- none inverted; 17/18 curves followed.
--
-- NOT FUNDAMENTAL (scripts/fundamental.py): the O-grid box's sides reach the
-- top and bottom of the wall, so S9 and S8 are each spread over 3 sheets of
-- blocks instead of one running along them, and the curves along them over
-- several columns. The side walls (one sheet through the thickness, level 1),
-- the hole (the O-grid ring, level 1) and the end faces are captured. The fix
-- is a strip of blocks between the box and the top and bottom faces
-- (patterns.md §4, §11) — more blocks, and likely better box corners.
--
-- Known defects, of the CAD file: curve 27 has no length and corner 19 sits
-- on corner 17; curve 26 duplicates curve 23 but lies on S7 where 23 lies on
-- S6. Merging 20+23+26 (below) clears corner 16 but leaves a curve no surface
-- contains, so 5 outer faces along the trim follow nothing — blocking.issues
-- shows them. The mesh still lies within 0.11 of the part, but check.py
-- fails it: 39 skin nodes past the tolerance, and the trim never measured.
-- The cure is upstream: scripts/step_to_msh.py heals the file (the duplicate
-- seam and the zero-length curve are fused), after which the plain merges
-- 20+23 follow both surfaces — the tags change, so the script needs
-- retagging. The fix within this file is in the diagnose-trim-faces case.
--
-- The vertex positions were tuned with scripts/optimize.py (min SJ 0.41 for
-- the first placement); P holds the result. A global PARAMS set before this
-- script runs overrides any of them — that is how optimize.py tries values.

local L = dofile("blocklib.lua")
local P = {
  aE = 8.5847, zZ = 3.7469, dY = -0.1332, zY = 3.8098, sY = 6.9053, dM = -0.2628,
  dT1 = 0.0836, dG = -0.0105, dF = 0.0071, dR1 = -0.0557, dD = 0.1216, dE = 0.0781,
  sE = -3.3412, sD = -3.4939, sF = 4.5212, sG = 4.3646, sR = -5.8216, sT = -6.6381,
  sM = -7.6490, zR = 1.5831, aNE = 32.9313, aNW = 123.2028, aW = 199.6043,
  aSW = -123.9877, aSE = -53.5532,
}
for k, v in pairs(PARAMS or {}) do P[k] = v end   -- what the optimiser is trying
local H = H or 0.4

gecko.load_geometry("B1.msh")
gecko.merge_smooth{ angle = 10 }
gecko.merge_curves({ 20, 23, 26 })

-- The merges, by the tags they got (read back from gecko.geometry()).
local SIDE_B, SIDE_A = -2, -3                  -- the wall's two faces: through x = 5 and x = 3 at the joint
local RIM_A, RIM_B = -5, -6                    -- the hole's rims on each face
local TOP_B, BOT_B, BOT_A, TOP_A = -7, -8, -9, -11
local TRIM_A, TRIM_B = -10, -12                -- the trim cylinder's edges on each face

-- Developed (s, z) to 3D, on side A or B.
local PI = math.pi
local function at(s, side, z, d)
  s = s + (d or 0) * (side == "A" and 1 or -1)  -- per-side offset (patterns.md §7)
  if s <= 0 then
    local t, r = 2 * PI + s / 4, side == "A" and 3 or 5
    return { r * math.cos(t), r * math.sin(t), z }
  end
  local f, r = PI - s / 4, side == "A" and 5 or 3
  return { 8 + r * math.cos(f), r * math.sin(f), z }
end
local function trimz(x) return -2 + math.sqrt(math.max(0, 25 - (x + 5) ^ 2)) end
local function hole(deg, side)
  local a = math.rad(deg)
  local y, z = 2 * math.cos(a), 3 + 2 * math.sin(a)
  local x
  if y >= 0 then x = side == "A" and 8 - math.sqrt(25 - y * y) or 8 - math.sqrt(9 - y * y)
  else x = side == "A" and math.sqrt(9 - y * y) or math.sqrt(25 - y * y) end
  return { x, y, z }
end

-- Each 2D vertex is two points, one per face of the wall.
local function pair(name, A, B, onA, onB)
  L.point(name .. "_A", A, onA and onA[1], onA and onA[2])
  L.point(name .. "_B", B, onB and onB[1], onB and onB[2])
end
local function along(name, s, z, d, onA, onB) pair(name, at(s, "A", z, d), at(s, "B", z, d), onA, onB) end
local function rim(name, deg) pair(name, hole(deg, "A"), hole(deg, "B"), { 1, RIM_A }, { 1, RIM_B }) end

pair("TL", { -3, 0, 7.5 }, { -5, 0, 7.5 }, { 0, 13 }, { 0, 18 })
pair("BL", { -3, 0, trimz(-3) }, { -5, 0, 3 }, { 0, 12 }, { 0, 15 })
do
  local a, b = at(P.sM, "A", 0, P.dM), at(P.sM, "B", 0, P.dM)
  a[3], b[3] = trimz(a[1]), trimz(b[1])
  pair("M", a, b, { 1, TRIM_A }, { 1, TRIM_B })
end
pair("BR", { -0.025, -3, -2.5 }, { -0.025, -5, -2.5 }, { 0, 10 }, { 0, 17 })
along("T1", P.sT, 7.5, P.dT1, { 1, TOP_A }, { 1, TOP_B })
along("E", P.sE, 7.5, P.dE, { 1, TOP_A }, { 1, TOP_B })
along("D", P.sD, -2.5, P.dD, { 1, BOT_A }, { 1, BOT_B })
along("R1", P.sR, P.zR, P.dR1, { 2, SIDE_A }, { 2, SIDE_B })
along("F", P.sF, 7.5, P.dF, { 1, TOP_A }, { 1, TOP_B })
along("G", P.sG, -2.5, P.dG, { 1, BOT_A }, { 1, BOT_B })
along("Y", P.sY, P.zY, P.dY, { 2, SIDE_A }, { 2, SIDE_B })
pair("ZE", { 13, 0, P.zZ }, { 11, 0, P.zZ }, { 1, 34 }, { 1, 9 })
pair("TR", { 13, 0, 7.5 }, { 11, 0, 7.5 }, { 0, 21 }, { 0, 6 })
pair("BRR", { 13, 0, -2.5 }, { 11, 0, -2.5 }, { 0, 20 }, { 0, 7 })
pair("TM", { 3, 0, 7.5 }, { 5, 0, 7.5 }, { 1, TOP_A }, { 1, TOP_B })
pair("BM", { 3, 0, -2.5 }, { 5, 0, -2.5 }, { 1, BOT_A }, { 1, BOT_B })
pair("HN", { 3, 0, 5 }, { 5, 0, 5 }, { 1, RIM_A }, { 1, RIM_B })
pair("HS", { 3, 0, 1 }, { 5, 0, 1 }, { 1, RIM_A }, { 1, RIM_B })
rim("HNE", P.aNE); rim("HNW", P.aNW); rim("HW", P.aW)
rim("HE", P.aE); rim("HSW", P.aSW); rim("HSE", P.aSE)

-- The blocks, as quads of the developed plane; each is one hex through the wall.
local quads = {
  { "BL", "M", "T1", "TL" }, { "M", "R1", "E", "T1" }, { "M", "BR", "D", "R1" },
  { "R1", "HW", "HNW", "E" }, { "D", "HSW", "HW", "R1" },
  { "E", "HNW", "HN", "TM" }, { "TM", "HN", "HNE", "F" },
  { "HE", "Y", "F", "HNE" }, { "HSE", "G", "Y", "HE" },
  { "D", "BM", "HS", "HSW" }, { "BM", "G", "HSE", "HS" },
  { "Y", "ZE", "TR", "F" }, { "G", "BRR", "ZE", "Y" },
}
for _, q in ipairs(quads) do
  L.block{ q[1] .. "_A", q[2] .. "_A", q[3] .. "_A", q[4] .. "_A",
           q[1] .. "_B", q[2] .. "_B", q[3] .. "_B", q[4] .. "_B" }
end
L.weld()
local _, unused = L.attach()
assert(#unused == 0, "points no block uses: " .. table.concat(unused, ", "))

L.issues(6)
L.size(H)
gecko.mesh()
L.summary()
if not NO_EXPORT then gecko.export_vtk("B1.vtk") end
