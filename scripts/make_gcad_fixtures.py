# /// script
# requires-python = ">=3.11,<3.14"
# dependencies = ["cadquery-ocp==8.0.1.1.0"]
# ///
"""The .gcad files gecko's tests read, in crates/gecko-geom/tests/data/.

    uv run scripts/make_gcad_fixtures.py [path/to/manbo-leg]

Made with OpenCascade, so that the tests themselves never need it: four
primitives built here, and two parts of the Mambo benchmark imported from
their STEP files when the path to a copy of it is given (Apache-2.0,
https://gitlab.com/franck.ledoux/mambo).

- plate.gcad: a 4 × 4 × 1 box with a hole of radius 1 through it — planar
  faces trimmed by a circle, and the cylinder of the hole with its seam;
- cylinder.gcad: radius 1, height 2 — a seam, two discs;
- sphere.gcad: radius 1.5 — a seam, and two degenerate edges at its poles;
- cone.gcad: radius 1 to a point, height 2 — an apex;
- B10.gcad, B47.gcad: Mambo parts.

Each is tessellated within a two-hundredth of its diagonal: coarse, to keep
the files small, and fine enough to test that the triangles stay within it.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import step_to_gcad as g  # noqa: E402

from OCP.BRepAlgoAPI import BRepAlgoAPI_Cut  # noqa: E402
from OCP.BRepPrimAPI import (  # noqa: E402
    BRepPrimAPI_MakeBox,
    BRepPrimAPI_MakeCone,
    BRepPrimAPI_MakeCylinder,
    BRepPrimAPI_MakeSphere,
)
from OCP.gp import gp_Ax2, gp_Dir, gp_Pnt  # noqa: E402

OUT = Path(__file__).parent.parent / "crates" / "gecko-geom" / "tests" / "data"


def plate():
    box = BRepPrimAPI_MakeBox(gp_Pnt(-2, -2, 0), 4, 4, 1).Shape()
    hole = BRepPrimAPI_MakeCylinder(gp_Ax2(gp_Pnt(0, 0, -1), gp_Dir(0, 0, 1)), 1, 3).Shape()
    cut = BRepAlgoAPI_Cut(box, hole)
    cut.Build()
    return cut.Shape()


def fixture(name: str, shape, source: str):
    doc = g.gcad(shape, g.diagonal(shape) / 200.0, source=source)
    g.write(doc, OUT / f"{name}.gcad")
    print(f"{name}: {len(doc['vertices'])} vertices, {len(doc['edges'])} edges, {len(doc['faces'])} faces")


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    fixture("plate", plate(), "a box with a hole, made by make_gcad_fixtures.py")
    fixture("cylinder", BRepPrimAPI_MakeCylinder(1, 2).Shape(), "made by make_gcad_fixtures.py")
    fixture("sphere", BRepPrimAPI_MakeSphere(1.5).Shape(), "made by make_gcad_fixtures.py")
    fixture("cone", BRepPrimAPI_MakeCone(1, 0, 2).Shape(), "made by make_gcad_fixtures.py")
    if len(sys.argv) > 1:
        mambo = Path(sys.argv[1])
        for part in ("B10", "B47"):
            step = mambo / "Basic" / f"{part}.step"
            fixture(part, g.heal(g.read_step(step), False), f"Mambo {part}.step")


if __name__ == "__main__":
    main()
