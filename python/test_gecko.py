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
    assert int.from_bytes(buf[4:8], "little") == 1
    n_nodes = int.from_bytes(buf[32:36], "little")
    n_edges = int.from_bytes(buf[36:40], "little")
    assert (n_nodes, n_edges) == (8, 12)
    assert len(buf) == 40 + n_nodes * 12 + n_edges * 8


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
