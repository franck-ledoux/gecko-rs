-- Undo, from a script.

gecko.create_cube()
gecko.create_cube{ origin = {3, 0, 0} }
assert(gecko.stats().blocks == 2)

gecko.undo()
assert(gecko.stats().blocks == 1, "one step back")

-- The bottom of the stack is an answer, not an error.
gecko.undo()
assert(gecko.stats().blocks == 0)
assert(gecko.undo() == "nothing to undo")
