"""The two modes must be one kernel.

`front/tests/unit/kernel.test.ts` drives a sequence through the wasm façade and
records what came out. This replays the very same sequence through PyO3 and
checks the bytes agree.

It can only pass while both façades are pure transports onto
`gecko_cmd::Session`. The moment either grows logic of its own, this is what
notices.
"""

import json

import pytest

import gecko
from conftest import ROOT, fnv1a

SESSION = ROOT / "front" / "artifacts" / "static-session.json"


@pytest.fixture(scope="module")
def static_session():
    if not SESSION.exists():
        pytest.skip(f"run the Vitest suite first to produce {SESSION}")
    return json.loads(SESSION.read_text())


def test_the_same_commands_produce_the_same_bytes(static_session):
    s = gecko.Session()
    for command in static_session["commands"]:
        s.apply(json.dumps(command))

    buf = bytes(s.view_buffer)
    assert len(buf) == static_session["length"]
    assert fnv1a(buf) == static_session["fnv1a"]


def test_the_two_modes_agree_on_what_they_hold(static_session):
    s = gecko.Session()
    for command in static_session["commands"]:
        s.apply(json.dumps(command))
    # The whole of `stats`, geometry included: if the two modes disagreed
    # about which geometry is loaded they would be two different sessions,
    # whatever the cell counts said.
    assert s.stats == static_session["stats"]
