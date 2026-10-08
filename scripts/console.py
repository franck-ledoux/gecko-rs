"""Startup file for `./dev`: a session, already being watched.

Run through `python -i`, so everything here is in scope at the prompt.
"""

import os
import webbrowser

import gecko

s = gecko.Session()
s.create_cube([0, 0, 0], 1.0)

# The station listens on the port the dev server is told to forward to. The
# page is served by Vite, not from here, which is what makes a TypeScript edit
# reload the view without touching this session.
s.show(port=int(os.environ["GECKO_PORT"]), open=False)

# Opened here rather than by `./dev`, because here is where the station is
# known to be listening. Vite's address, not the station's: the page has to
# come from the dev server for a TypeScript edit to reload it.
webbrowser.open(os.environ["GECKO_VIEW"])

print(f"""
  gecko — session ready as `s`, already holding one cube.

      s.create_cube([2, 0, 0], 0.5)
      n = s.nodes[0]
      s.move_node(n["id"], [0.5, -0.2, 0])

  The view is at {os.environ["GECKO_VIEW"]} and follows every command.
  Drag a node there and `s.nodes` will say so.
""")
