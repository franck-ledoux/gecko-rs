---
tags: [gecko-mesher]
max_turns: 120
timeout_seconds: 3600
allowed_tools: [Read, Glob, Grep, Skill, Write, Edit, Bash]
---

Ok, I want you to use gecko to mesh the B1 model. I want a good scaled jacobian, with a low number of blocks, and boundary alignment in the final mesh. Gecko is built from the gecko-rs checkout; put everything you produce in a folder called b1-mesh.

Input files: B1.step
