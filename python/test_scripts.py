"""The port guard in `./dev` and `./view`.

Both scripts need fixed ports, so a process left over from an earlier run is
in the way — and it used to be in the way *quietly*. Vite exits on "port
already in use", but it is started in the background, so the script carried on
and its readiness check was answered by the leftover server. The console then
came up in front of a page that forwarded nowhere and ran a kernel of its own.

This is the check that stops that, so it is worth a test of its own.
"""

import os
import socket
import subprocess

from conftest import ROOT

GUARD = "source scripts/ports.sh; require_free {port} 'the thing'"


def run_guard(port):
    return subprocess.run(
        ["bash", "-c", GUARD.format(port=port)],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )


def test_a_free_port_passes_without_a_word():
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        free = probe.getsockname()[1]
    # Closed again, so nothing is listening on it.
    done = run_guard(free)
    assert done.returncode == 0, done.stderr
    assert done.stdout == ""
    assert done.stderr == ""


def test_an_occupied_port_stops_the_script_and_names_the_culprit():
    with socket.socket() as held:
        held.bind(("127.0.0.1", 0))
        held.listen(1)
        port = held.getsockname()[1]

        done = run_guard(port)

    # Stopping is the point: a warning would be read by nobody, because the
    # next thing on screen is a minute of wasm build output.
    assert done.returncode == 1
    assert str(port) in done.stderr
    assert "the thing" in done.stderr
    # And it says which process, and how to be rid of it. Anything less sends
    # the reader hunting for `lsof` incantations.
    assert f"kill {os.getpid()}" in done.stderr
