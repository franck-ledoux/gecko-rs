"""The Python façade, used the way a session would use it.

Nothing here tests geometry: that is the kernel's own suite, in Rust, where it
runs in microseconds. These check that the binding exposes it faithfully —
argument shapes, error mapping, and the station it can start.
"""

import json
import time
import urllib.error
import urllib.request

import pytest

import gecko
from conftest import CUBE, cells, shifted


def test_a_session_starts_empty():
    s = gecko.Session()
    assert s.stats == {
        "nodes": 0,
        "edges": 0,
        "faces": 0,
        "blocks": 0,
        "geometry": {"generation": 0},
        "history": {"steps": 5, "back": 0},
        "mesh": {"generation": 0},
        "classified": {"nodes": 0, "edges": 0, "faces": 0, "frozen": 0},
    }
    assert s.url is None
    assert "gecko.Session" in repr(s)


def test_adding_a_block_gives_cells_at_every_dimension():
    s = gecko.Session()
    message = s.add_block(CUBE)
    assert "12 edges" in message
    assert cells(s) == {"nodes": 8, "edges": 12, "faces": 6, "blocks": 1}

    # A second block, standing apart, is a second component.
    s.add_block(shifted(dx=3))
    assert cells(s) == {"nodes": 16, "edges": 24, "faces": 12, "blocks": 2}


def test_the_raw_door_is_the_same_door():
    typed = gecko.Session()
    raw = gecko.Session()
    typed.add_block(CUBE)
    raw.apply(json.dumps({"op": "add_block", "corners": CUBE}))
    assert bytes(typed.view_buffer) == bytes(raw.view_buffer)
    assert typed.stats == raw.stats


def test_a_refused_command_raises_and_changes_nothing():
    s = gecko.Session()
    s.add_block(CUBE)
    before = bytes(s.view_buffer)

    for bad in ["{}", '{"op":"nonesuch"}', '{"op":"add_block"}']:
        with pytest.raises(ValueError):
            s.apply(bad)
    with pytest.raises(ValueError):
        s.apply("not json")

    assert bytes(s.view_buffer) == before


def test_the_view_buffer_is_the_wire_format():
    s = gecko.Session()
    s.add_block(CUBE)
    buf = bytes(s.view_buffer)

    assert buf[:4] == b"GKB1"
    assert int.from_bytes(buf[4:8], "little") == 6
    n_nodes = int.from_bytes(buf[32:36], "little")
    n_edges = int.from_bytes(buf[36:40], "little")
    n_quads = int.from_bytes(buf[40:44], "little")
    assert (n_nodes, n_edges, n_quads) == (8, 12, 6)
    # Per node: three f32 of position, one u32 of name, and three more for
    # what it is classified on and whether it is frozen. Per edge: two node
    # indices, a name, and two for its classification. Per quadrangle: four
    # corner indices, the face and block names it can be coloured by, and
    # two for its classification.
    assert len(buf) == 44 + n_nodes * 28 + n_edges * 20 + n_quads * 32


def test_show_serves_the_viewer_and_is_idempotent():
    s = gecko.Session()
    s.add_block(CUBE)
    url = s.show(open=False)
    assert url.startswith("http://127.0.0.1:")
    # Asking twice must not start a second server on the same session.
    assert s.show(open=False) == url
    assert s.url == url

    with urllib.request.urlopen(url + "mode", timeout=5) as r:
        assert r.read().decode().strip() == "station"
    with urllib.request.urlopen(url, timeout=5) as r:
        assert r.status == 200
        assert b"<canvas" in r.read()
    with pytest.raises(urllib.error.HTTPError):
        urllib.request.urlopen(url + "nonesuch", timeout=5)


def test_two_sessions_do_not_fight_over_a_port():
    # Port 0 asks the system for a free one; hard-coding is how the second
    # session fails to start, or silently attaches to the first.
    a, b = gecko.Session(), gecko.Session()
    assert a.show(open=False) != b.show(open=False)


def test_creating_a_cube_is_the_same_as_its_eight_corners():
    by_cube = gecko.Session()
    by_corners = gecko.Session()
    by_cube.create_cube([0, 0, 0], 1.0)
    by_corners.add_block(CUBE)
    assert bytes(by_cube.view_buffer) == bytes(by_corners.view_buffer)

    with pytest.raises(ValueError, match="strictly positive"):
        gecko.Session().create_cube([0, 0, 0], 0.0)


def test_nodes_are_listed_with_a_name_to_move_them_by():
    s = gecko.Session()
    s.create_cube([0, 0, 0], 2.0)
    nodes = s.nodes
    assert len(nodes) == 8
    assert {tuple(n["pos"]) for n in nodes} == {
        (x, y, z) for x in (0.0, 2.0) for y in (0.0, 2.0) for z in (0.0, 2.0)
    }
    # Distinct names, or two spheres would answer to one.
    assert len({n["id"] for n in nodes}) == 8


def test_moving_a_node_moves_exactly_that_one():
    s = gecko.Session()
    s.create_cube([0, 0, 0], 1.0)
    before = {n["id"]: tuple(n["pos"]) for n in s.nodes}
    target = sorted(before)[3]

    s.move_node(target, [9.0, 8.0, 7.0])

    after = {n["id"]: tuple(n["pos"]) for n in s.nodes}
    assert after.keys() == before.keys(), "a move must not rename anything"
    assert after[target] == (9.0, 8.0, 7.0)
    for name, was in before.items():
        if name != target:
            assert after[name] == was, f"node {name} moved too"
    # Not a topological change.
    assert cells(s) == {"nodes": 8, "edges": 12, "faces": 6, "blocks": 1}


def test_a_name_still_means_the_same_node_after_another_edit():
    # The reason nodes are named by a stable id and not by a position in the
    # list: the list is reordered by edits, the name is not.
    s = gecko.Session()
    s.create_cube([0, 0, 0], 1.0)
    target = sorted(n["id"] for n in s.nodes)[0]

    s.create_cube([5, 0, 0], 1.0)
    s.move_node(target, [-3.0, -3.0, -3.0])

    moved = [n for n in s.nodes if n["id"] == target]
    assert len(moved) == 1
    assert tuple(moved[0]["pos"]) == (-3.0, -3.0, -3.0)
    # The second cube is untouched.
    others = [n["pos"] for n in s.nodes if n["id"] != target]
    assert len(others) == 15


def test_moving_a_node_that_does_not_exist_raises():
    s = gecko.Session()
    s.create_cube([0, 0, 0], 1.0)
    before = bytes(s.view_buffer)
    with pytest.raises(ValueError, match="4242"):
        s.move_node(4242, [1.0, 2.0, 3.0])
    assert bytes(s.view_buffer) == before


def test_gluing_two_blocks_through_the_binding():
    s = gecko.Session()
    s.create_cube([0.0, 0.0, 0.0], 1.0)
    s.create_cube([1.5, 0.0, 0.0], 1.0)

    # A script has no mouse, so it names a face from the listing.
    faces = s.faces
    assert len(faces) == 12
    blocks = sorted({f["block"] for f in faces})
    first = next(f for f in faces if f["block"] == blocks[0] and f["centre"][0] == 1.0)
    second = next(f for f in faces if f["block"] == blocks[1] and f["centre"][0] == 1.5)

    message = s.glue_faces(first["id"], second["id"])
    assert "glued" in message
    assert cells(s) == {"nodes": 12, "edges": 20, "faces": 11, "blocks": 2}

    # The rule: the seam is where the *first* face was, not half way. The map
    # merges two vertices by averaging them, so this would be 1.25 if the
    # faces were not brought together before being sewn.
    seam = [n for n in s.nodes if abs(n["pos"][0] - 1.0) < 1e-9]
    assert len(seam) == 4
    assert not [n for n in s.nodes if abs(n["pos"][0] - 1.25) < 1e-9]
    # Only the face moved: the far side of the second block stayed put.
    assert [n for n in s.nodes if abs(n["pos"][0] - 2.5) < 1e-9]


def test_a_refused_glue_raises_and_changes_nothing():
    s = gecko.Session()
    s.create_cube([0.0, 0.0, 0.0], 1.0)
    before = bytes(s.view_buffer)

    face = s.faces[0]["id"]
    with pytest.raises(ValueError, match="itself"):
        s.glue_faces(face, face)
    with pytest.raises(ValueError, match="no face"):
        s.glue_faces(face, 9999)
    assert bytes(s.view_buffer) == before


def test_removing_a_block_through_the_binding():
    s = gecko.Session()
    for x in (0.0, 1.0, 2.0):
        s.create_cube([x, 0.0, 0.0], 1.0)

    # A script has no mouse, so it names a block from the listing.
    blocks = s.blocks
    assert len(blocks) == 3
    assert [b["centre"][0] for b in blocks] == [0.5, 1.5, 2.5]

    message = s.remove_block(blocks[1]["id"])
    assert "removed" in message
    assert cells(s) == {"nodes": 16, "edges": 24, "faces": 12, "blocks": 2}
    assert [b["centre"][0] for b in s.blocks] == [0.5, 2.5]


def test_removing_a_glued_block_leaves_the_neighbour_whole():
    s = gecko.Session()
    s.create_cube([0.0, 0.0, 0.0], 1.0)
    s.create_cube([1.0, 0.0, 0.0], 1.0)
    blocks = [b["id"] for b in s.blocks]
    seam = [f for f in s.faces if abs(f["centre"][0] - 1.0) < 1e-9]
    s.glue_faces(*[f["id"] for f in seam])
    assert cells(s) == {"nodes": 12, "edges": 20, "faces": 11, "blocks": 2}

    s.remove_block(blocks[1])
    # A closed hexahedron again: the shared cells were split off on the way
    # out rather than going with it.
    assert cells(s) == {"nodes": 8, "edges": 12, "faces": 6, "blocks": 1}
    assert sorted({round(n["pos"][0], 9) for n in s.nodes}) == [0.0, 1.0]


def test_a_refused_removal_raises_and_changes_nothing():
    s = gecko.Session()
    s.create_cube([0.0, 0.0, 0.0], 1.0)
    before = bytes(s.view_buffer)
    with pytest.raises(ValueError, match="no block"):
        s.remove_block(4242)
    assert bytes(s.view_buffer) == before


def test_the_sheet_through_an_edge():
    s = gecko.Session()
    s.create_cube([0.0, 0.0, 0.0], 1.0)
    s.create_cube([1.0, 0.0, 0.0], 1.0)
    blocks = [b["id"] for b in s.blocks]
    seam = [f["id"] for f in s.faces if abs(f["centre"][0] - 1.0) < 1e-9]
    s.glue_faces(*seam)

    # An edge of the shared face: its sheet runs on into the second block.
    nodes = {n["id"]: n["pos"] for n in s.nodes}
    on_seam = [i for i, p in nodes.items() if abs(p[0] - 1.0) < 1e-9]
    assert len(on_seam) == 4

    # Any edge parallel to y, found through the sheet's own report.
    sheets = {}
    for edge in range(40):
        try:
            sheets[edge] = s.sheet(edge)
        except ValueError:
            pass
    sizes = sorted({len(v) for v in sheets.values()})
    # Four for a sheet that stays in one block, six for one that crosses.
    assert sizes == [4, 6], sizes
    assert blocks  # two blocks, welded


def test_a_sheet_asked_for_by_a_name_no_edge_has():
    s = gecko.Session()
    s.create_cube([0.0, 0.0, 0.0], 1.0)
    with pytest.raises(ValueError, match="no edge"):
        s.sheet(4242)


def _sheet(session, edge):
    """The sheet through an edge, or None if no edge answers to that name."""
    try:
        return session.sheet(edge)
    except ValueError:
        return None


def _edge_along(session, axis, among=200):
    """An edge whose sheet runs along this axis.

    The sheet reports where a cut would run from and to on each of its
    edges, so the direction is in the answer rather than worked out from
    the geometry by hand.
    """
    for edge in range(among):
        sheet = _sheet(session, edge)
        if not sheet:
            continue
        step = [abs(sheet[0]["to"][k] - sheet[0]["from"][k]) for k in range(3)]
        if max(range(3), key=lambda k: step[k]) == axis:
            return edge
    raise AssertionError(f"no edge along axis {axis}")


def test_cutting_a_sheet_from_python():
    s = gecko.Session()
    s.create_cube([0.0, 0.0, 0.0], 1.0)

    edge = _edge_along(s, 2)
    assert len(s.sheet(edge)) == 4

    message = s.split_sheet(edge, 0.25)
    assert "cut at 0.250" in message
    assert cells(s) == {"nodes": 12, "edges": 20, "faces": 11, "blocks": 2}

    # One plane, not two: the parameter runs the same way round on every
    # edge of the sheet, so all four new nodes land together.
    heights = sorted({round(n["pos"][2], 9) for n in s.nodes})
    assert heights in ([0.0, 0.25, 1.0], [0.0, 0.75, 1.0]), heights
    assert sum(1 for n in s.nodes if abs(n["pos"][2] - heights[1]) < 1e-9) == 4


def test_a_cut_can_be_cut_again():
    # One cut per direction gives a 2x2x2 grid — the strongest sign that
    # what comes out is a mesh and not merely the right counts.
    s = gecko.Session()
    s.create_cube([0.0, 0.0, 0.0], 1.0)
    for axis in (2, 0, 1):
        s.split_sheet(_edge_along(s, axis), 0.5)
    assert cells(s) == {"nodes": 27, "edges": 54, "faces": 36, "blocks": 8}


def test_a_parameter_outside_the_edge_is_refused():
    s = gecko.Session()
    s.create_cube([0.0, 0.0, 0.0], 1.0)
    before = bytes(s.view_buffer)
    edge = _edge_along(s, 2)
    for at in (0.0, 1.0, -0.5, 2.0):
        with pytest.raises(ValueError, match="strictly inside"):
            s.split_sheet(edge, at)
    assert bytes(s.view_buffer) == before


def _face_at(session, axis, value):
    """The (face, block) pair whose four corners all sit at this value."""
    nodes = {n["id"]: n["pos"] for n in session.nodes}
    for face in session.faces:
        if abs(face["centre"][axis] - value) < 1e-9:
            return face["id"], face["block"]
    raise AssertionError(f"no face at {axis} = {value}; nodes {nodes}")


def test_pillowing_a_face_from_python():
    s = gecko.Session()
    s.create_cube([0.0, 0.0, 0.0], 1.0)
    message = s.pillow([_face_at(s, 2, 1.0)])
    assert "pillowed 1 face" in message
    assert cells(s) == {"nodes": 12, "edges": 20, "faces": 11, "blocks": 2}

    # The surface has not moved: four nodes still on it, four still on the
    # far side, and one new layer a third of the way in between.
    heights = sorted({round(n["pos"][2], 9) for n in s.nodes})
    assert heights == [0.0, round(1.0 - 1.0 / 3.0, 9), 1.0], heights


def test_pillowing_the_whole_surface():
    s = gecko.Session()
    s.create_cube([0.0, 0.0, 0.0], 1.0)
    s.pillow([(f["id"], f["block"]) for f in s.faces])
    # The cube, drawn in, inside a shell of six slabs welded together.
    assert s.stats["blocks"] == 7
    assert s.stats["nodes"] == 16
    corners = [n for n in s.nodes if all(abs(x) < 1e-9 or abs(x - 1.0) < 1e-9 for x in n["pos"])]
    assert len(corners) == 8, "the original corners are still the original corners"


def test_pillowing_inside_the_model():
    s = gecko.Session()
    s.create_cube([0.0, 0.0, 0.0], 1.0)
    s.pillow([_face_at(s, 2, 1.0)])

    # The face between the cube and its layer has a block on each side.
    seen = {}
    for f in s.faces:
        seen.setdefault(f["id"], []).append(f["block"])
    inside = next(i for i, blocks in seen.items() if len(blocks) == 2)
    # Pick it on the upper block, so the layer goes between the two.
    s.pillow([(inside, seen[inside][0])])
    assert s.stats["blocks"] == 3
    assert s.stats["nodes"] == 16


def test_an_empty_pillow_is_refused():
    s = gecko.Session()
    s.create_cube([0.0, 0.0, 0.0], 1.0)
    with pytest.raises(ValueError, match="nothing was selected"):
        s.pillow([])


def test_removing_a_sheet_closes_the_gap():
    # Three slabs stacked and glued; take the middle one out.
    s = gecko.Session()
    for z in (0.0, 1.0, 2.0):
        s.create_cube([0.0, 0.0, z], 1.0)
    for z in (1.0, 2.0):
        pair = [f["id"] for f in s.faces if abs(f["centre"][2] - z) < 1e-9]
        assert len(pair) == 2
        s.glue_faces(*pair)
    assert cells(s) == {"nodes": 16, "edges": 28, "faces": 16, "blocks": 3}

    middle = _edge_between(s, 1.0, 2.0)
    message = s.remove_sheet(middle)
    assert "removed" in message
    assert cells(s) == {"nodes": 12, "edges": 20, "faces": 11, "blocks": 2}

    # The seam sits half way between where the two faces were.
    heights = sorted({round(n["pos"][2], 9) for n in s.nodes})
    assert heights == [0.0, 1.5, 3.0], heights


def _edge_between(session, low, high):
    """An edge whose sheet runs from one height to the other."""
    for edge in range(200):
        sheet = _sheet(session, edge)
        if not sheet:
            continue
        ends = sorted([sheet[0]["from"][2], sheet[0]["to"][2]])
        if abs(ends[0] - low) < 1e-9 and abs(ends[1] - high) < 1e-9:
            return edge
    raise AssertionError(f"no edge from {low} to {high}")


def test_removing_a_sheet_at_the_surface_just_loses_it():
    s = gecko.Session()
    for z in (0.0, 1.0):
        s.create_cube([0.0, 0.0, z], 1.0)
    pair = [f["id"] for f in s.faces if abs(f["centre"][2] - 1.0) < 1e-9]
    s.glue_faces(*pair)

    s.remove_sheet(_edge_between(s, 0.0, 1.0))
    # One slab left, where it was: there was nothing below to close against.
    assert s.stats["blocks"] == 1
    assert sorted({round(n["pos"][2], 9) for n in s.nodes}) == [1.0, 2.0]


def test_removing_a_sheet_by_a_name_no_edge_has():
    s = gecko.Session()
    s.create_cube([0.0, 0.0, 0.0], 1.0)
    before = bytes(s.view_buffer)
    with pytest.raises(ValueError, match="no edge"):
        s.remove_sheet(4242)
    assert bytes(s.view_buffer) == before


def test_a_sheet_says_how_many_blocks_it_runs_through():
    s = gecko.Session()
    for z in (0.0, 1.0):
        s.create_cube([0.0, 0.0, z], 1.0)
    pair = [f["id"] for f in s.faces if abs(f["centre"][2] - 1.0) < 1e-9]
    s.glue_faces(*pair)

    # Across the seam: one block. Along it: both.
    across = _edge_between(s, 0.0, 1.0)
    assert len(s.sheet(across)) == 4


SQUARE_MSH = """$MeshFormat
2.2 0 8
$EndMeshFormat
$Nodes
4
1 0 0 0
2 1 0 0
3 1 1 0
4 0 1 0
$EndNodes
$Elements
10
1 15 2 0 21 1
2 15 2 0 22 2
3 15 2 0 23 3
4 15 2 0 24 4
5 1 2 0 11 1 2
6 1 2 0 12 2 3
7 1 2 0 13 3 4
8 1 2 0 14 4 1
9 2 2 0 1 1 2 3
10 2 2 0 1 1 3 4
$EndElements
"""


def _square(tmp_path, text=SQUARE_MSH):
    path = tmp_path / "square.msh"
    path.write_text(text)
    return str(path)


def test_a_geometry_is_read_from_a_file_on_disk(tmp_path):
    # The one convenience the Python façade has that the browser cannot: a
    # path. It reads the file and sends the contents, so the command itself
    # stays the one both modes can carry.
    s = gecko.Session()
    message = s.load_geometry(_square(tmp_path))
    assert "square.msh" in message
    assert "4 corners" in message
    assert "4 curves" in message
    assert "1 surface" in message

    assert s.stats["geometry"] == {
        "generation": 1,
        "corners": 4,
        "curves": 4,
        "surfaces": 1,
        "segments": 4,
        "triangles": 2,
    }
    # And the blocks are untouched by any of it.
    assert cells(s) == {"nodes": 0, "edges": 0, "faces": 0, "blocks": 0}


def test_a_geometry_can_be_dropped_again(tmp_path):
    s = gecko.Session()
    s.load_geometry(_square(tmp_path))
    s.clear_geometry()
    assert s.stats["geometry"] == {"generation": 2}
    assert len(bytes(s.geometry_buffer)) == 48, "a header and nothing else"


def test_a_file_that_is_not_a_mesh_is_refused_with_its_own_message(tmp_path):
    s = gecko.Session()
    with pytest.raises(ValueError, match=r"\$MeshFormat"):
        s.load_geometry(_square(tmp_path, "hello\n"))
    # A version gmsh writes by default today, and the message has to say
    # what to do about it rather than merely that it did not work.
    with pytest.raises(ValueError, match="-format msh2"):
        s.load_geometry(_square(tmp_path, SQUARE_MSH.replace("2.2 0 8", "4.1 0 8")))
    assert s.stats["geometry"] == {"generation": 0}


def test_a_path_that_is_not_there_names_itself(tmp_path):
    s = gecko.Session()
    with pytest.raises(ValueError, match="nowhere.msh"):
        s.load_geometry(str(tmp_path / "nowhere.msh"))


def test_the_geometry_buffer_is_the_bytes_the_browser_decodes(tmp_path):
    s = gecko.Session()
    s.load_geometry(_square(tmp_path))
    buf = bytes(s.geometry_buffer)
    assert buf[:4] == b"GKG1"
    # 4 nodes, 2 triangles, 4 segments, 4 corners — see GeometryBuffer.
    assert len(buf) == 48 + 4 * 12 + 2 * 16 + 4 * 12 + 4 * 8


def test_a_face_is_grown_into_a_block():
    s = gecko.Session()
    s.create_cube([0.0, 0.0, 0.0], 1.0)
    top = max(s.faces, key=lambda f: f["centre"][2])

    message = s.extrude_face(top["id"], top["block"], 2.0)
    assert "grown from face" in message
    # Welded, not standing on it: eleven faces rather than twelve.
    assert cells(s) == {"nodes": 12, "edges": 20, "faces": 11, "blocks": 2}
    assert max(n["pos"][2] for n in s.nodes) == pytest.approx(3.0)
    # And the face it grew from has not moved.
    assert sorted({round(n["pos"][2], 9) for n in s.nodes}) == [0.0, 1.0, 3.0]


def test_an_extrusion_that_cannot_happen_says_why():
    s = gecko.Session()
    s.create_cube([0.0, 0.0, 0.0], 1.0)
    top = max(s.faces, key=lambda f: f["centre"][2])
    with pytest.raises(ValueError, match="positive distance"):
        s.extrude_face(top["id"], top["block"], 0.0)

    s.extrude_face(top["id"], top["block"], 1.0)
    inside = max(
        (f for f in s.faces if abs(f["centre"][2] - 1.0) < 1e-9),
        key=lambda f: f["block"],
    )
    with pytest.raises(ValueError, match="inside the model"):
        s.extrude_face(inside["id"], inside["block"], 1.0)
    assert cells(s)["blocks"] == 2


CUBE_MSH = """$MeshFormat
2.2 0 8
$EndMeshFormat
$Nodes
8
1 0 0 0
2 2 0 0
3 2 1 0
4 0 1 0
5 0 0 3
6 2 0 3
7 2 1 3
8 0 1 3
$EndNodes
$Elements
12
1 2 2 0 1 1 2 3
2 2 2 0 1 1 3 4
3 2 2 0 2 5 6 7
4 2 2 0 2 5 7 8
5 2 2 0 3 1 2 6
6 2 2 0 3 1 6 5
7 2 2 0 4 4 3 7
8 2 2 0 4 4 7 8
9 2 2 0 5 1 4 8
10 2 2 0 5 1 8 5
11 2 2 0 6 2 3 7
12 2 2 0 6 2 7 6
$EndElements
"""


def _boxed(tmp_path, **kwargs):
    s = gecko.Session()
    path = tmp_path / "cube.msh"
    path.write_text(CUBE_MSH)
    s.load_geometry(str(path))
    s.box_geometry(**kwargs)
    return s


def _span(session, axis):
    all_of = [n["pos"][axis] for n in session.nodes]
    return min(all_of), max(all_of)


def test_a_box_round_the_whole_geometry(tmp_path):
    s = _boxed(tmp_path, margin=0.0)
    assert cells(s) == {"nodes": 8, "edges": 12, "faces": 6, "blocks": 1}
    assert _span(s, 0) == pytest.approx((0.0, 2.0))
    assert _span(s, 1) == pytest.approx((0.0, 1.0))
    assert _span(s, 2) == pytest.approx((0.0, 3.0))


def test_the_margin_defaults_to_the_one_the_cxx_uses(tmp_path):
    s = _boxed(tmp_path)
    # 10% of each half-side, as `BiyApp::create_bounding_box(0.1)`.
    assert _span(s, 0) == pytest.approx((-0.1, 2.1))
    assert _span(s, 2) == pytest.approx((-0.15, 3.15))


def test_a_box_round_a_selection_of_surfaces(tmp_path):
    s = _boxed(tmp_path, surfaces=[1, 2], margin=0.0)
    assert cells(s)["blocks"] == 1
    assert _span(s, 2) == pytest.approx((0.0, 3.0))


def test_a_box_needs_a_geometry_and_some_depth(tmp_path):
    s = gecko.Session()
    with pytest.raises(ValueError, match="no geometry is loaded"):
        s.box_geometry()

    path = tmp_path / "cube.msh"
    path.write_text(CUBE_MSH)
    s.load_geometry(str(path))
    with pytest.raises(ValueError, match="flat"):
        s.box_geometry(surfaces=[1])
    with pytest.raises(ValueError, match="no surface of this geometry is tagged 99"):
        s.box_geometry(surfaces=[99])
    assert cells(s)["blocks"] == 0


BOX_MSH = """$MeshFormat
2.2 0 8
$EndMeshFormat
$Nodes
8
1 0 0 0
2 1 0 0
3 1 1 0
4 0 1 0
5 0 0 1
6 1 0 1
7 1 1 1
8 0 1 1
$EndNodes
$Elements
32
1 15 2 0 101 1
2 15 2 0 102 2
3 15 2 0 103 3
4 15 2 0 104 4
5 15 2 0 105 5
6 15 2 0 106 6
7 15 2 0 107 7
8 15 2 0 108 8
9 1 2 0 201 1 2
10 1 2 0 202 2 3
11 1 2 0 203 3 4
12 1 2 0 204 4 1
13 1 2 0 205 5 6
14 1 2 0 206 6 7
15 1 2 0 207 7 8
16 1 2 0 208 8 5
17 1 2 0 209 1 5
18 1 2 0 210 2 6
19 1 2 0 211 3 7
20 1 2 0 212 4 8
21 2 2 0 1 1 2 3
22 2 2 0 1 1 3 4
23 2 2 0 2 5 6 7
24 2 2 0 2 5 7 8
25 2 2 0 3 1 2 6
26 2 2 0 3 1 6 5
27 2 2 0 4 2 3 7
28 2 2 0 4 2 7 6
29 2 2 0 5 3 4 8
30 2 2 0 5 3 8 7
31 2 2 0 6 4 1 5
32 2 2 0 6 4 5 8
$EndElements
"""


def _on_the_box(tmp_path):
    """A session holding the unit cube's boundary and one block on it."""
    s = gecko.Session()
    path = tmp_path / "box.msh"
    path.write_text(BOX_MSH)
    s.load_geometry(str(path))
    s.box_geometry(margin=0.0)
    return s


def test_classifying_puts_the_blocking_on_the_geometry(tmp_path):
    s = _on_the_box(tmp_path)
    assert s.stats["classified"] == {"nodes": 0, "edges": 0, "faces": 0, "frozen": 0}

    message = s.classify()
    assert "8 nodes" in message and "12 edges" in message and "6 faces" in message
    assert s.stats["classified"] == {"nodes": 8, "edges": 12, "faces": 6, "frozen": 0}

    # Nodes on corners — the lowest dimension with anything in range, not
    # the curves and surfaces that also pass through them.
    assert all(n["on"]["dim"] == 0 for n in s.nodes)
    assert sorted(n["on"]["tag"] for n in s.nodes) == list(range(101, 109))
    # Edges on curves, inferred from their two ends rather than measured.
    assert all(e["on"]["dim"] == 1 for e in s.edges)
    assert sorted(e["on"]["tag"] for e in s.edges) == list(range(201, 213))
    # And faces on surfaces, from their four edges — one dimension up, the
    # same rule and the same reason.
    assert all(f["on"]["dim"] == 2 for f in s.faces)
    assert sorted(f["on"]["tag"] for f in s.faces) == list(range(1, 7))


def test_a_node_slides_on_what_it_is_classified_on(tmp_path):
    s = _on_the_box(tmp_path)
    node = s.nodes[0]["id"]
    s.classify_node(node, 2, 1)          # surface 1 is the face at z = 0

    s.move_node(node, [0.3, 0.7, 9.0])
    at = next(n["pos"] for n in s.nodes if n["id"] == node)
    assert at == pytest.approx([0.3, 0.7, 0.0])

    # Dragged far away it stays on the face: coming off is something you
    # say, not something that happens to you.
    s.move_node(node, [40.0, 40.0, 40.0])
    assert next(n["on"] for n in s.nodes if n["id"] == node) == {"dim": 2, "tag": 1}

    s.unclassify_node(node)
    assert next(n["on"] for n in s.nodes if n["id"] == node) is None
    s.move_node(node, [40.0, 40.0, 40.0])
    assert next(n["pos"] for n in s.nodes if n["id"] == node) == pytest.approx([40.0] * 3)


def test_a_frozen_node_is_left_out_of_everything(tmp_path):
    s = _on_the_box(tmp_path)
    node = s.nodes[0]["id"]
    where = next(n["pos"] for n in s.nodes if n["id"] == node)
    s.freeze_node(node)
    assert s.stats["classified"]["frozen"] == 1

    with pytest.raises(ValueError, match="frozen"):
        s.move_node(node, [5.0, 5.0, 5.0])
    # Nor classified, since classifying is followed by a projection and a
    # projection is a move.
    s.classify()
    assert next(n["on"] for n in s.nodes if n["id"] == node) is None
    assert next(n["pos"] for n in s.nodes if n["id"] == node) == pytest.approx(where)

    s.thaw_node(node)
    s.classify()
    assert s.stats["classified"]["nodes"] == 8


def test_an_edge_goes_where_its_two_ends_agree(tmp_path):
    s = _on_the_box(tmp_path)
    edge = s.edges[0]
    a, b = edge["nodes"]
    # Corners 101 and 103 are the ends of the bottom face's diagonal: no
    # curve holds both, surface 1 holds both.
    s.classify_node(a, 0, 101)
    s.classify_node(b, 0, 103)
    assert next(e["on"] for e in s.edges if e["id"] == edge["id"]) == {"dim": 2, "tag": 1}

    s.unclassify_node(a)
    assert next(e["on"] for e in s.edges if e["id"] == edge["id"]) is None


def test_classifying_needs_a_geometry():
    s = gecko.Session()
    s.create_cube([0.0, 0.0, 0.0], 1.0)
    with pytest.raises(ValueError, match="no geometry"):
        s.classify()


def test_reading_a_session_does_not_get_slower_than_linearly():
    """The guard against a lookup by name inside a loop over cells.

    `node_by_id` and its siblings are scans of the whole map. Putting one
    inside a per-cell loop makes the whole thing quadratic, and it is
    silent: everything still answers correctly, only slowly. It has
    happened twice — once to sheet removal, once to the classification —
    so this is a budget rather than a measurement.

    Sixty-four blocks took nine seconds per refresh when `stats` and
    `nodes` resolved a name per cell, and take about forty milliseconds
    when they do not. Twenty of them is therefore either about a second or
    about three minutes, and the threshold wants to sit between those two
    with room on both sides — a CI runner is three times slower than the
    machine this was written on, which the first attempt at this number
    did not allow for and which cost a red build.
    """
    s = gecko.Session()
    s.create_cube([0.0, 0.0, 0.0], 4.0)
    for axis in range(3):
        for _ in range(3):
            edge = next(
                e["id"]
                for e in s.edges
                if _along(s, e, axis)
            )
            s.split_sheet(edge, 0.5)
    assert s.stats["blocks"] == 64

    started = time.perf_counter()
    for _ in range(20):
        s.stats
        bytes(s.view_buffer)
        s.nodes
    spent = time.perf_counter() - started
    # Seven times the slowest seen on CI, and a ninth of what the version
    # before this would take.
    assert spent < 20.0, f"twenty refreshes of 64 blocks took {spent:.1f} s"


def _along(session, edge, axis):
    """Whether an edge runs along one axis and no other."""
    at = {n["id"]: n["pos"] for n in session.nodes}
    a, b = (at[n] for n in edge["nodes"])
    gap = [abs(a[k] - b[k]) for k in range(3)]
    return gap[axis] > 1e-9 and max(gap[(axis + 1) % 3], gap[(axis + 2) % 3]) < 1e-9


BENT_MSH = """$MeshFormat
2.2 0 8
$EndMeshFormat
$Nodes
5
1 0 0 0
2 1 0 0
3 1 1 0
4 0 1 0
5 0.5 0.25 0
$EndNodes
$Elements
7
1 15 2 0 101 1
2 15 2 0 102 2
3 1 2 0 201 1 5
4 1 2 0 201 5 2
5 2 2 0 1 1 5 4
6 2 2 0 1 5 2 3
7 2 2 0 1 5 3 4
$EndElements
"""


def test_cutting_a_classified_edge_leaves_a_node_on_the_curve(tmp_path):
    # The curve bends up to (0.5, 0.25, 0) between its two ends, so the
    # straight middle of the edge is nowhere near it. Inheriting the
    # classification without being pulled onto it would leave the node at
    # y = 0, which is the thing this is checking.
    s = gecko.Session()
    path = tmp_path / "bent.msh"
    path.write_text(BENT_MSH)
    s.load_geometry(str(path))
    s.create_cube([0.0, 0.0, 0.0], 1.0)

    for want, tag in (([0.0, 0.0, 0.0], 101), ([1.0, 0.0, 0.0], 102)):
        node = next(
            n for n in s.nodes if all(abs(n["pos"][k] - want[k]) < 1e-9 for k in range(3))
        )
        s.classify_node(node["id"], 0, tag)

    edge = next(e for e in s.edges if e["on"] == {"dim": 1, "tag": 201})
    s.split_sheet(edge["id"], 0.5)

    on_curve = [n for n in s.nodes if n["on"] == {"dim": 1, "tag": 201}]
    assert len(on_curve) == 1
    assert on_curve[0]["pos"] == pytest.approx([0.4, 0.2, 0.0])
    # And both halves of the edge are on it too.
    assert len([e for e in s.edges if e["on"] == {"dim": 1, "tag": 201}]) == 2


def test_the_geometrys_entities_can_be_listed(tmp_path):
    # There was no way to find out what a `.msh` called things, which
    # makes `classify_node` unusable from a script.
    s = gecko.Session()
    assert s.entities == []
    path = tmp_path / "box.msh"
    path.write_text(BOX_MSH)
    s.load_geometry(str(path))

    assert [e for e in s.entities if e["dim"] == 0] == [
        {"dim": 0, "tag": t} for t in range(101, 109)
    ]
    assert [e["tag"] for e in s.entities if e["dim"] == 1] == list(range(201, 213))
    assert [e["tag"] for e in s.entities if e["dim"] == 2] == list(range(1, 7))


# --- going back -------------------------------------------------------------


def test_a_change_can_be_taken_back():
    s = gecko.Session()
    s.add_block(CUBE)
    before = bytes(s.view_buffer)
    nodes = s.nodes

    s.add_block(shifted(dx=3))
    assert cells(s)["blocks"] == 2

    assert "undone" in s.undo()
    assert cells(s)["blocks"] == 1
    # The same bytes, which is the whole claim: the names came back with the
    # positions, so a node id held across the undo still means that node.
    assert bytes(s.view_buffer) == before
    assert s.nodes == nodes


def test_the_bottom_of_the_stack_is_an_answer_not_an_error():
    s = gecko.Session()
    assert s.undo() == "nothing to undo"


def test_how_far_back_it_goes_is_a_setting():
    s = gecko.Session()
    assert s.stats["history"] == {"steps": 5, "back": 0}

    assert s.set_history(1) == "one step of undo"
    s.add_block(CUBE)
    s.add_block(shifted(dx=3))
    assert s.stats["history"] == {"steps": 1, "back": 1}

    s.undo()
    assert cells(s)["blocks"] == 1
    assert s.undo() == "nothing to undo"

    # And none at all keeps none at all.
    assert s.set_history(0) == "undo is off"
    s.add_block(shifted(dx=6))
    assert s.undo() == "nothing to undo"
    assert cells(s)["blocks"] == 2


def test_a_cut_is_taken_back_whole():
    s = gecko.Session()
    s.create_cube()
    before = bytes(s.view_buffer)
    edge = s.edges[0]["id"]

    s.split_sheet(edge, 0.5)
    assert cells(s)["blocks"] == 2

    s.undo()
    assert bytes(s.view_buffer) == before
    # The edge that was cut is still that edge, so the same cut can be made
    # again — which is what keeping states rather than replaying commands
    # buys, and what a renumbering rebuild would lose.
    s.split_sheet(edge, 0.5)
    assert cells(s)["blocks"] == 2


def test_how_near_a_dropped_node_has_to_come_is_the_gestures_own(tmp_path):
    path = tmp_path / "box.msh"
    path.write_text(BOX_MSH)
    s = gecko.Session()
    s.load_geometry(str(path))
    s.box_geometry(margin=0.0)
    node = s.nodes[0]["id"]

    # Hard against the corner at the origin, and refused the geometry.
    s.move_node(node, [0.01, 0.01, 0.01], snap=0)
    assert next(n for n in s.nodes if n["id"] == node)["on"] is None

    # The same drop, with the usual reach, is taken by that corner.
    s.move_node(node, [0.01, 0.01, 0.01])
    landed = next(n for n in s.nodes if n["id"] == node)
    assert landed["on"] == {"dim": 0, "tag": 101}
    assert landed["pos"] == [0.0, 0.0, 0.0]


def test_several_nodes_go_onto_one_entity_at_once(tmp_path):
    path = tmp_path / "box.msh"
    path.write_text(BOX_MSH)
    s = gecko.Session()
    s.load_geometry(str(path))
    s.box_geometry(margin=0.0)

    bottom = [n["id"] for n in s.nodes if abs(n["pos"][2]) < 1e-9]
    assert len(bottom) == 4

    # One act: one step of undo, and the edges above them settled once.
    s.classify_node(bottom, 1, 201)
    assert s.stats["classified"]["nodes"] == 4
    assert all(n["on"] == {"dim": 1, "tag": 201} for n in s.nodes if n["id"] in bottom)
    # Curve 201 runs along y = z = 0, and each corner was projected onto it.
    assert all(
        abs(n["pos"][1]) < 1e-9 and abs(n["pos"][2]) < 1e-9
        for n in s.nodes
        if n["id"] in bottom
    )

    s.undo()
    assert s.stats["classified"]["nodes"] == 0

    # One on its own is still one, written the way anyone would write it.
    s.freeze_node(bottom[0])
    s.freeze_node(bottom[1:])
    assert s.stats["classified"]["frozen"] == 4


def test_a_move_can_carry_the_other_nodes_with_it():
    s = gecko.Session()
    s.create_cube()
    before = {n["id"]: n["pos"] for n in s.nodes}
    ids = sorted(before)[:3]

    # The first is put somewhere; the other two come by the same offset.
    target = [before[ids[0]][0] + 1.0, before[ids[0]][1], before[ids[0]][2]]
    assert "3 nodes moved" in s.move_node(ids[0], target, carrying=ids)

    after = {n["id"]: n["pos"] for n in s.nodes}
    for i in ids:
        assert after[i][0] == pytest.approx(before[i][0] + 1.0)
        assert after[i][1:] == pytest.approx(before[i][1:])
    for i in set(before) - set(ids):
        assert after[i] == before[i]

    # One gesture, one command, one step back.
    s.undo()
    assert {n["id"]: n["pos"] for n in s.nodes} == before


# --- meshing ----------------------------------------------------------------


def test_a_blocking_is_meshed_into_hexahedra():
    s = gecko.Session()
    s.create_cube()
    assert s.stats["mesh"] == {"generation": 0}
    assert s.vtk is None

    assert s.mesh(3) == "meshed — 64 nodes, 27 hexahedra"
    assert s.stats["mesh"] == {"generation": 1, "nodes": 64, "hexes": 27}

    # Not a change to the blocking, so not a step of undo either.
    s.undo()
    assert cells(s)["blocks"] == 0


def test_how_many_segments_is_a_property_of_the_sheet():
    s = gecko.Session()
    s.create_cube()
    edge = s.edges[0]["id"]
    assert "2 segments" in s.set_segments(edge, 2)

    s.mesh(4)
    assert s.stats["mesh"]["hexes"] == 2 * 4 * 4


def test_a_mesh_does_not_outlive_the_blocking(tmp_path):
    s = gecko.Session()
    s.create_cube()
    s.mesh(2)
    assert s.vtk is not None

    s.create_cube(origin=[3, 0, 0])
    assert s.vtk is None, "the mesh outlived the blocking it was made from"

    # And exporting meshes again rather than writing nothing.
    path = tmp_path / "mesh.vtk"
    s.export_vtk(str(path), segments=2)
    assert "CELL_TYPES 16" in path.read_text(), "two blocks of eight"


def test_the_file_is_vtk_legacy(tmp_path):
    s = gecko.Session()
    s.create_cube()
    s.set_segments(s.edges[0]["id"], 1)
    path = tmp_path / "mesh.vtk"
    s.export_vtk(str(path), segments=1)

    lines = path.read_text().splitlines()
    assert lines[0] == "# vtk DataFile Version 2.0"
    assert lines[2:5] == ["ASCII", "DATASET UNSTRUCTURED_GRID", "POINTS 8 double"]
    assert "CELLS 1 9" in lines
    assert "SCALARS block int 1" in lines


def test_a_pillowed_node_leaves_its_classification_on_the_surface(tmp_path):
    # The layer leaves a copy of each node where it was; that copy is the
    # one still where the geometry is, so it is the one that keeps what
    # the node was classified on. The node drawn in is somewhere new, and
    # where it is is decided again.
    path = tmp_path / "box.msh"
    path.write_text(BOX_MSH)
    s = gecko.Session()
    s.load_geometry(str(path))
    s.box_geometry(margin=0.0)
    s.classify()
    assert s.stats["classified"]["nodes"] == 8

    before = {tuple(n["pos"]): n["on"] for n in s.nodes}
    top = [f for f in s.faces if abs(f["centre"][2] - 1.0) < 1e-9]
    assert len(top) == 1
    s.pillow([(top[0]["id"], top[0]["block"])])

    for n in s.nodes:
        x, y, z = n["pos"]
        if abs(z - 1.0) < 1e-9:
            # The copy the layer left where the node was: it is the one
            # still on the geometry, so it keeps what the node was on.
            assert n["on"] == before[(x, y, 1.0)], n
        elif z > 0.5:
            # Drawn in, so off the corner it was: it slid down the upright
            # of the cube that corner was on, and is on **that** now.
            was = before[(x, y, 1.0)]
            assert n["on"] != was, "it kept the entity it was pushed off"
            assert n["on"]["dim"] > was["dim"], n
