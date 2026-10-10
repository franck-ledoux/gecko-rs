"""Startup file for `./dev`: a session, already being watched.

Run through `python -i`, so everything here is in scope at the prompt.
"""

import os
import time
import urllib.error
import urllib.request
import webbrowser

import gecko


def forwards_here(view, tries=20, pause=0.25):
    """Is the page at `view` really served in front of *this* station?

    Worth asking, because the page cannot. A dev server left over from an
    earlier `./view` serves the page perfectly and forwards nothing, and
    "there is no `mode` route here" is exactly what a genuinely static host
    looks like — so the tab starts a kernel of its own and then ignores
    everything typed at this prompt, while looking entirely healthy.

    This process is the one that knows a station exists, so it is the one that
    can check.
    """
    for _ in range(tries):
        try:
            with urllib.request.urlopen(view + "mode", timeout=2) as answer:
                if answer.read().decode().strip() == "station":
                    return True
        except (urllib.error.URLError, OSError):
            pass
        time.sleep(pause)
    return False


# Empty, deliberately. A session starts with nothing in it, the same way
# the static page does, so the first thing on screen is whatever you asked
# for rather than a cube somebody else put there.
s = gecko.Session()

# The station listens on the port the dev server is told to forward to. The
# page is served by Vite, not from here, which is what makes a TypeScript edit
# reload the view without touching this session.
s.show(port=int(os.environ["GECKO_PORT"]), open=False)

# Opened here rather than by `./dev`, because here is where the station is
# known to be listening. Vite's address, not the station's: the page has to
# come from the dev server for a TypeScript edit to reload it.
view = os.environ["GECKO_VIEW"]
if forwards_here(view):
    webbrowser.open(view)
else:
    # The station serves its own copy of the viewer, so there is somewhere to
    # go. What is lost is hot reload, not the session.
    print(f"\n  {view} is not forwarding to this session; using the station's own page.")
    view = s.url
    webbrowser.open(view)

print(f"""
  gecko — session ready as `s`, and empty.

      s.create_cube([0, 0, 0], 1.0)
      s.load_geometry("../gecko/test_data/B48.msh")
      s.box_geometry()                    # one block round the lot

  The view is at {view} and follows every command.
  Drag a node there and `s.nodes` will say so.
""")
