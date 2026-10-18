-- What a script can open by name.
--
-- In a terminal that is the working directory; in a page it is whatever
-- the reader handed over — chosen with the button or dropped on the
-- window, since a browser cannot be given a path at all. One question,
-- answered by whichever host is underneath.

local here = gecko.files()
assert(type(here) == "table", "a list of names")

-- This suite runs with the crate as its working directory, so the crate's
-- own manifest is there to be found. Names, not paths, and no directories:
-- a script opens files.
local found = false
for _, name in ipairs(here) do
  assert(not name:find("/"), "a name, not a path: " .. name)
  if name == "Cargo.toml" then found = true end
end
assert(found, "the manifest should be among the " .. #here .. " files listed")

-- And opening one works the same way it does anywhere: by name where the
-- host has files under a folder, and by full path otherwise.
gecko.load_geometry(FIXTURES .. "/cube.msh")
assert(gecko.stats().geometry.surfaces == 6, "the cube came in")
