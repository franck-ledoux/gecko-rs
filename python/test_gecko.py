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
from conftest import CUBE, shifted


def test_a_session_starts_empty():
    s = gecko.Session()
    assert s.stats == {"nodes": 0, "edges": 0, "faces": 0, "blocks": 0}
    assert s.url is None
    assert "gecko.Session" in repr(s)


def test_adding_a_block_gives_cells_at_every_dimension():
    s = gecko.Session()
    message = s.add_block(CUBE)
    assert "12 edges" in message
    assert s.stats == {"nodes": 8, "edges": 12, "faces": 6, "blocks": 1}

    # A second block, standing apart, is a second component.
    s.add_block(shifted(dx=3))
    assert s.stats == {"nodes": 16, "edges": 24, "faces": 12, "blocks": 2}


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
    assert s.stats == {"nodes": 8, "edges": 12, "faces": 6, "blocks": 1}


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
    assert s.stats == {"nodes": 12, "edges": 20, "faces": 11, "blocks": 2}

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
    assert s.stats == {"nodes": 16, "edges": 24, "faces": 12, "blocks": 2}
    assert [b["centre"][0] for b in s.blocks] == [0.5, 2.5]


def test_removing_a_glued_block_leaves_the_neighbour_whole():
    s = gecko.Session()
    s.create_cube([0.0, 0.0, 0.0], 1.0)
    s.create_cube([1.0, 0.0, 0.0], 1.0)
    blocks = [b["id"] for b in s.blocks]
    seam = [f for f in s.faces if abs(f["centre"][0] - 1.0) < 1e-9]
    s.glue_faces(*[f["id"] for f in seam])
    assert s.stats == {"nodes": 12, "edges": 20, "faces": 11, "blocks": 2}

    s.remove_block(blocks[1])
    # A closed hexahedron again: the shared cells were split off on the way
    # out rather than going with it.
    assert s.stats == {"nodes": 8, "edges": 12, "faces": 6, "blocks": 1}
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
    assert s.stats == {"nodes": 12, "edges": 20, "faces": 11, "blocks": 2}

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
    assert s.stats == {"nodes": 27, "edges": 54, "faces": 36, "blocks": 8}


def test_a_parameter_outside_the_edge_is_refused():
    s = gecko.Session()
    s.create_cube([0.0, 0.0, 0.0], 1.0)
    before = bytes(s.view_buffer)
    edge = _edge_along(s, 2)
    for at in (0.0, 1.0, -0.5, 2.0):
        with pytest.raises(ValueError, match="strictly inside"):
            s.split_sheet(edge, at)
    assert bytes(s.view_buffer) == before
