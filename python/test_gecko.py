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
    assert int.from_bytes(buf[4:8], "little") == 3
    n_nodes = int.from_bytes(buf[32:36], "little")
    n_edges = int.from_bytes(buf[36:40], "little")
    n_quads = int.from_bytes(buf[40:44], "little")
    assert (n_nodes, n_edges, n_quads) == (8, 12, 6)
    # Per node: three f32 of position and one u32 of name. Per quadrangle:
    # four corner indices and the face and block names it can be coloured by.
    assert len(buf) == 44 + n_nodes * 16 + n_edges * 8 + n_quads * 24


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
