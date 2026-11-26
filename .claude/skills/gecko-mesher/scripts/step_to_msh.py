#!/usr/bin/env python3
"""Turn a STEP (or BREP) part into the MSH 2.2 surface mesh Gecko reads.

    python3 step_to_msh.py part.step part.msh [size_factor] [--no-heal]

The part is **healed** first: OCC's coherence pass fuses corners that sit on
one point and curves or surfaces given twice — what a CAD export leaves at a
badly closed seam (Mambo B1 has a corner twice and one edge twice). Healed,
every curve can be followed and nothing has to be exempted; `--no-heal`
keeps the file as it is. Gecko cannot heal on load yet
(references/feature-requests.md): this is a stopgap.

`size_factor` scales gmsh's element size (default 0.5). Finer facets make
`gecko.geometry()`'s shape fitting and the pictures more exact, and the
projection of mesh nodes onto curved surfaces more accurate; the blocking
does not care. Corners, curves and surfaces keep gmsh's entity tags, which
are the tags you will classify on.

gmsh's Python module is looked for on the path, then where Homebrew puts it.
"""
import glob
import os
import sys


def import_gmsh():
    try:
        import gmsh  # noqa: F401
        return gmsh
    except ImportError:
        pass
    candidates = sorted(glob.glob("/opt/homebrew/Cellar/gmsh/*/lib"), reverse=True)
    candidates += ["/opt/homebrew/opt/gmsh/lib", "/usr/local/opt/gmsh/lib", "/usr/lib/python3/dist-packages"]
    for d in candidates:
        if os.path.exists(os.path.join(d, "gmsh.py")):
            sys.path.insert(0, d)
            import gmsh
            return gmsh
    sys.exit("gmsh's Python module was not found: `pip install gmsh`, or `brew install gmsh`")


def convert(source, target, size_factor=0.5, heal=True):
    gmsh = import_gmsh()
    gmsh.initialize()
    try:
        gmsh.option.setNumber("General.Terminal", 0)
        gmsh.model.add("part")
        gmsh.model.occ.importShapes(source)
        if heal:
            gmsh.model.occ.removeAllDuplicates()
        gmsh.model.occ.synchronize()
        gmsh.option.setNumber("Mesh.CharacteristicLengthFactor", size_factor)
        gmsh.model.mesh.generate(2)
        gmsh.option.setNumber("Mesh.MshFileVersion", 2.2)
        gmsh.option.setNumber("Mesh.Binary", 0)
        # Every entity, with its tag: without physical groups gmsh would
        # otherwise be free to leave the corners out.
        gmsh.option.setNumber("Mesh.SaveAll", 1)
        gmsh.write(target)
        counts = {d: len(gmsh.model.getEntities(d)) for d in (0, 1, 2, 3)}
    finally:
        gmsh.finalize()
    return counts


if __name__ == "__main__":
    args = [a for a in sys.argv[1:] if a != "--no-heal"]
    heal = "--no-heal" not in sys.argv
    if len(args) not in (2, 3):
        sys.exit(__doc__)
    source, target = args[0], args[1]
    if not source.lower().endswith((".step", ".stp", ".brep")):
        sys.exit("the part has to be a STEP (.step, .stp) or BREP (.brep) file")
    factor = float(args[2]) if len(args) == 3 else 0.5
    if not 0 < factor <= 10:
        sys.exit("size_factor is a positive scale, typically 0.1 to 1")
    c = convert(source, target, factor, heal)
    print(f"{target}: {c[0]} corners, {c[1]} curves, {c[2]} surfaces, {c[3]} volumes" + ("" if heal else " (not healed)"))
