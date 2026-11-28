# /// script
# requires-python = ">=3.11,<3.14"
# dependencies = ["cadquery-ocp==8.0.1.1.0"]
# ///
"""A STEP file to a .gcad file: the exact geometry gecko reads.

    uv run scripts/step_to_gcad.py part.step part.gcad [--deflection D] [--sew]

uv builds the environment this needs on first use — OpenCascade's Python
bindings, cadquery-ocp — and keeps it cached: nothing is installed in the
repository or the system Python. gecko itself never needs OpenCascade; only
this script does.

What it does, in order:

1. Reads the STEP file. Lengths come out in millimetres, OpenCascade's
   default, whatever unit the file was written in.
2. Heals it (ShapeFix_Shape), and sews its faces together if asked to
   (`--sew`): for a file whose faces were written apart.
3. Converts every curve and surface to a NURBS (BRepBuilderAPI_NurbsConvert):
   planes, cylinders, cones, spheres, tori, extrusions and revolutions
   become rational B-splines, exactly.
4. Writes out the topology — vertices, edges, faces, the loops bounding each
   face — with every curve and surface as degree, knots, control points and
   weights, and every edge's curve in each face's parameters (its p-curve).
5. Writes a tessellation beside it, for drawing and for a first guess:
   triangles per face and a polyline per edge, within `--deflection` of the
   exact shapes (by default a thousandth of the part's diagonal). An edge's
   polyline is the nodes its faces' triangles have along it, so the faces
   meet node for node, as in a gmsh mesh.

Tags are positions in OpenCascade's maps of the shape's vertices, edges and
faces, from one: the same file imports to the same tags every time.

The format is described in docs/developer-guide/architecture.md, "The .gcad
format". Everything is JSON, so that two imports can be diffed.
"""

from __future__ import annotations

import argparse
import json
import math
import sys
from pathlib import Path

from OCP.Bnd import Bnd_Box
from OCP.BRep import BRep_Tool
from OCP.BRepAdaptor import BRepAdaptor_Curve
from OCP.BRepBndLib import BRepBndLib
from OCP.BRepBuilderAPI import BRepBuilderAPI_NurbsConvert, BRepBuilderAPI_Sewing
from OCP.BRepMesh import BRepMesh_IncrementalMesh
from OCP.BRepTools import BRepTools, BRepTools_WireExplorer
from OCP.GCPnts import GCPnts_QuasiUniformDeflection
from OCP.Geom import Geom_TrimmedCurve
from OCP.Geom2d import Geom2d_TrimmedCurve
from OCP.Geom2dConvert import Geom2dConvert
from OCP.GeomConvert import GeomConvert
from OCP.IFSelect import IFSelect_RetDone
from OCP.ShapeFix import ShapeFix_Shape
from OCP.STEPControl import STEPControl_Reader
from OCP.TopAbs import TopAbs_EDGE, TopAbs_FACE, TopAbs_FORWARD, TopAbs_REVERSED, TopAbs_VERTEX, TopAbs_WIRE
from OCP.TopExp import TopExp, TopExp_Explorer
from OCP.TopLoc import TopLoc_Location
from OCP.TopoDS import TopoDS
from OCP.collections import IndexedMap_TopoDS_Shape_TopTools_ShapeMapHasher as ShapeMap

FORMAT = "gcad"
VERSION = 1


def read_step(path: Path):
    reader = STEPControl_Reader()
    if reader.ReadFile(str(path)) != IFSelect_RetDone:
        sys.exit(f"{path}: not a STEP file OpenCascade can read")
    reader.TransferRoots()
    shape = reader.OneShape()
    if shape.IsNull():
        sys.exit(f"{path}: no shape in it")
    return shape


def heal(shape, sew: bool):
    fix = ShapeFix_Shape(shape)
    fix.Perform()
    shape = fix.Shape()
    if sew:
        sewing = BRepBuilderAPI_Sewing(1e-6)
        sewing.Add(shape)
        sewing.Perform()
        shape = sewing.SewedShape()
    return shape


def diagonal(shape) -> float:
    box = Bnd_Box()
    BRepBndLib.Add_s(shape, box)
    lo, hi = box.CornerMin(), box.CornerMax()
    return lo.Distance(hi)


# --- curves and surfaces -----------------------------------------------------------


def expanded(knots, mults):
    out = []
    for k, m in zip(knots, mults):
        out.extend([k] * m)
    return out


def curve_json(bs, dims: int):
    """A B-spline curve, OpenCascade's 3D or 2D, as gecko writes one."""
    if bs.IsPeriodic():
        bs.SetNotPeriodic()
    n = bs.NbPoles()
    knots = expanded(
        [bs.Knot(i) for i in range(1, bs.NbKnots() + 1)],
        [bs.Multiplicity(i) for i in range(1, bs.NbKnots() + 1)],
    )
    points = []
    for i in range(1, n + 1):
        p = bs.Pole(i)
        points.append([p.X(), p.Y(), p.Z()] if dims == 3 else [p.X(), p.Y()])
    weights = [bs.Weight(i) for i in range(1, n + 1)]
    return {"degree": bs.Degree(), "knots": knots, "points": points, "weights": weights}


def surface_json(bs):
    if bs.IsUPeriodic():
        bs.SetUNotPeriodic()
    if bs.IsVPeriodic():
        bs.SetVNotPeriodic()
    nu, nv = bs.NbUPoles(), bs.NbVPoles()
    ku = expanded(
        [bs.UKnot(i) for i in range(1, bs.NbUKnots() + 1)],
        [bs.UMultiplicity(i) for i in range(1, bs.NbUKnots() + 1)],
    )
    kv = expanded(
        [bs.VKnot(i) for i in range(1, bs.NbVKnots() + 1)],
        [bs.VMultiplicity(i) for i in range(1, bs.NbVKnots() + 1)],
    )
    points, weights = [], []
    for i in range(1, nu + 1):
        row, wrow = [], []
        for j in range(1, nv + 1):
            p = bs.Pole(i, j)
            row.append([p.X(), p.Y(), p.Z()])
            wrow.append(bs.Weight(i, j))
        points.append(row)
        weights.append(wrow)
    return {
        "degree": [bs.UDegree(), bs.VDegree()],
        "knots": [ku, kv],
        "points": points,
        "weights": weights,
    }


def edge_curve(edge):
    """The edge's curve, cut to its range, or nothing for a degenerate edge."""
    if BRep_Tool.Degenerated_s(edge):
        return None
    first, last = BRep_Tool.Range_s(edge)
    curve = BRep_Tool.Curve_s(edge, first, last)
    if curve is None:
        return None
    return GeomConvert.CurveToBSplineCurve_s(Geom_TrimmedCurve(curve, first, last, True, True))


def p_curve(edge, face):
    """The edge's curve in the face's parameters, cut to its range."""
    first, last = BRep_Tool.Range_s(edge, face)
    curve = BRep_Tool.CurveOnSurface_s(edge, face, first, last)
    if curve is None:
        sys.exit("an edge has no curve in the parameters of a face it bounds")
    return Geom2dConvert.CurveToBSplineCurve_s(Geom2d_TrimmedCurve(curve, first, last, True, True))


# --- the whole of it ---------------------------------------------------------------


def mapped(shape, kind):
    m = ShapeMap()
    TopExp.MapShapes_s(shape, kind, m)
    return m


def gcad(shape, deflection: float | None = None, source: str = "") -> dict:
    """The .gcad document of a shape, healed or not: see the module."""
    shape = BRepBuilderAPI_NurbsConvert(shape, True).Shape()
    span = diagonal(shape)
    deflection = deflection or span / 1000.0
    BRepMesh_IncrementalMesh(shape, deflection, False, 0.5, True)

    vertices, edges, faces = (mapped(shape, k) for k in (TopAbs_VERTEX, TopAbs_EDGE, TopAbs_FACE))
    tolerance = 0.0

    out_vertices = []
    for i in range(1, vertices.Extent() + 1):
        v = TopoDS.Vertex(vertices.FindKey(i))
        p = BRep_Tool.Pnt_s(v)
        tolerance = max(tolerance, BRep_Tool.Tolerance_s(v))
        out_vertices.append({"tag": i, "point": [p.X(), p.Y(), p.Z()]})

    out_edges = []
    for i in range(1, edges.Extent() + 1):
        e = TopoDS.Edge(edges.FindKey(i).Oriented(TopAbs_FORWARD))
        tolerance = max(tolerance, BRep_Tool.Tolerance_s(e))
        ends = [
            vertices.FindIndex(TopExp.FirstVertex_s(e)),
            vertices.FindIndex(TopExp.LastVertex_s(e)),
        ]
        bs = edge_curve(e)
        # Filled in from the faces' triangles below: the points a face's
        # triangles have along the edge, so that the faceting is conformal.
        polyline = []
        out_edges.append(
            {
                "tag": i,
                "ends": ends,
                "curve": None if bs is None else curve_json(bs, 3),
                "polyline": polyline,
                "params": [],
            }
        )

    out_faces = []
    for i in range(1, faces.Extent() + 1):
        f = TopoDS.Face(faces.FindKey(i))
        tolerance = max(tolerance, BRep_Tool.Tolerance_s(f))
        reversed_ = f.Orientation() == TopAbs_REVERSED
        surface = GeomConvert.SurfaceToBSplineSurface_s(BRep_Tool.Surface_s(f))
        loops, bounding = [], []
        wires = TopExp_Explorer(f, TopAbs_WIRE)
        while wires.More():
            wire = TopoDS.Wire(wires.Current())
            uses = []
            walk = BRepTools_WireExplorer(wire, f)
            while walk.More():
                e = walk.Current()
                tag = edges.FindIndex(e)
                if tag not in bounding:
                    bounding.append(tag)
                uses.append(
                    {
                        "edge": tag,
                        "reversed": e.Orientation() == TopAbs_REVERSED,
                        "pcurve": curve_json(p_curve(e, f), 2),
                    }
                )
                walk.Next()
            loops.append(uses)
            wires.Next()

        loc = TopLoc_Location()
        tri = BRep_Tool.Triangulation_s(f, loc)
        nodes, uvs, triangles = [], [], []
        if tri is not None:
            moved = loc.Transformation()
            for k in range(1, tri.NbNodes() + 1):
                p = tri.Node(k).Transformed(moved)
                nodes.append([p.X(), p.Y(), p.Z()])
                if tri.HasUVNodes():
                    q = tri.UVNode(k)
                    uvs.append([q.X(), q.Y()])
            for k in range(1, tri.NbTriangles() + 1):
                t = tri.Triangle(k)
                a, b, c = (t.Value(1) - 1, t.Value(2) - 1, t.Value(3) - 1)
                triangles.append([a, c, b] if reversed_ else [a, b, c])
            # Each edge's polyline from the first face it bounds: the very
            # nodes the triangles have along it, which every face along the
            # edge shares — what lets gecko stitch the faces' triangles into
            # one faceting, as a gmsh mesh is.
            for tag in bounding:
                edge_out = out_edges[tag - 1]
                if edge_out["polyline"] or edge_out["curve"] is None:
                    continue
                e = TopoDS.Edge(edges.FindKey(tag))
                polygon = BRep_Tool.PolygonOnTriangulation_s(e, tri, loc)
                if polygon is None:
                    continue
                ks = range(1, polygon.NbNodes() + 1)
                points = [nodes[polygon.Node(k) - 1] for k in ks]
                params = [polygon.Parameter(k) for k in ks] if polygon.HasParameters() else []
                start = out_vertices[edge_out["ends"][0] - 1]["point"]
                if math.dist(points[-1], start) < math.dist(points[0], start):
                    points.reverse()
                    params.reverse()
                edge_out["polyline"] = points
                edge_out["params"] = params
        u0, u1, v0, v1 = BRepTools.UVBounds_s(f)
        out_faces.append(
            {
                "tag": i,
                "reversed": reversed_,
                "surface": surface_json(surface),
                "uv": [[u0, u1], [v0, v1]],
                "edges": bounding,
                "loops": loops,
                "triangulation": {"nodes": nodes, "uv": uvs, "triangles": triangles},
            }
        )

    # An edge no face's triangles run along — a wire on its own — sampled
    # within the deflection instead.
    for edge_out in out_edges:
        if edge_out["polyline"] or edge_out["curve"] is None:
            continue
        e = TopoDS.Edge(edges.FindKey(edge_out["tag"]).Oriented(TopAbs_FORWARD))
        sampler = GCPnts_QuasiUniformDeflection(BRepAdaptor_Curve(e), deflection)
        ks = range(1, sampler.NbPoints() + 1)
        edge_out["polyline"] = [[p.X(), p.Y(), p.Z()] for p in (sampler.Value(k) for k in ks)]
        edge_out["params"] = [sampler.Parameter(k) for k in ks]

    return {
        "format": FORMAT,
        "version": VERSION,
        "source": source,
        "units": "mm",
        "deflection": deflection,
        "tolerance": tolerance,
        "vertices": out_vertices,
        "edges": out_edges,
        "faces": out_faces,
    }


def write(doc: dict, path: Path):
    # One entity to a line, so that two imports diff entity by entity.
    with path.open("w") as out:
        out.write("{\n")
        head = {k: v for k, v in doc.items() if k not in ("vertices", "edges", "faces")}
        for k, v in head.items():
            out.write(f"  {json.dumps(k)}: {json.dumps(v)},\n")
        for n, key in enumerate(("vertices", "edges", "faces")):
            items = doc[key]
            out.write(f"  {json.dumps(key)}: [\n")
            for i, item in enumerate(items):
                comma = "," if i + 1 < len(items) else ""
                out.write(f"    {json.dumps(item, separators=(',', ':'))}{comma}\n")
            out.write("  ]" + (",\n" if n < 2 else "\n"))
        out.write("}\n")


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("step", type=Path)
    ap.add_argument("gcad", type=Path)
    ap.add_argument("--deflection", type=float, help="of the tessellation, in mm")
    ap.add_argument("--sew", action="store_true", help="sew faces written apart")
    args = ap.parse_args()
    shape = heal(read_step(args.step), args.sew)
    doc = gcad(shape, args.deflection, source=args.step.name)
    write(doc, args.gcad)
    print(
        f"{args.gcad}: {len(doc['vertices'])} vertices, {len(doc['edges'])} edges, "
        f"{len(doc['faces'])} faces; deflection {doc['deflection']:.3g} mm, "
        f"tolerance {doc['tolerance']:.3g} mm"
    )


if __name__ == "__main__":
    main()
