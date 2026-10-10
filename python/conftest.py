import pathlib

ROOT = pathlib.Path(__file__).resolve().parent.parent

CUBE = [
    [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0],
]


def fnv1a(data: bytes) -> int:
    """The digest the browser computes over the same bytes (see kernel.test.ts)."""
    h = 0x811C9DC5
    for b in data:
        h = ((h ^ b) * 0x01000193) & 0xFFFFFFFF
    return h


def cells(session):
    """The four cell counts, without the geometry's stats beside them.

    `stats` also reports which geometry is loaded, which almost no test
    about blocks has an opinion about — and spelling it out in every one of
    them would bury the counts that are the point.
    """
    return {k: v for k, v in session.stats.items() if k != "geometry"}


def shifted(dx=0.0, dy=0.0, dz=0.0):
    return [[p[0] + dx, p[1] + dy, p[2] + dz] for p in CUBE]
