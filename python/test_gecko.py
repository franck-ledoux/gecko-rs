"""The Python façade, used the way a session would use it.

Nothing here tests geometry: that is the kernel's own suite, in Rust, where it
runs in microseconds. These check that the binding exposes it faithfully —
argument shapes, error mapping, and the station it can start.
"""

import json
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
    assert int.from_bytes(buf[4:8], "little") == 4
    n_nodes = int.from_bytes(buf[32:36], "little")
    n_edges = int.from_bytes(buf[36:40], "little")
    n_quads = int.from_bytes(buf[40:44], "little")
    assert (n_nodes, n_edges, n_quads) == (8, 12, 6)
    # Per node: three f32 of position and one u32 of name. Per edge: two node
    # indices and a name. Per quadrangle: four corner indices and the face
    # and block names it can be coloured by.
    assert len(buf) == 44 + n_nodes * 16 + n_edges * 12 + n_quads * 24


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
