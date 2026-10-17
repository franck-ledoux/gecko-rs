#!/usr/bin/env python3
"""One version's section of the changelog, for a release's notes.

So that the notes and the changelog cannot disagree: there is one
description of what changed, and this reads it rather than restating it.
"""

import pathlib
import re
import sys

TRAILER = """
______________________________________________________________________

**Nothing to install to try it:** the application runs in a browser at
<https://franck-ledoux.github.io/gecko-rs/app/>.

The binaries below are the same application with a filesystem attached: they
serve the page and open it, and a script can then read and write real files.
Pick the one for your machine, make it executable, and run it. macOS keeps a
downloaded binary quarantined until you say otherwise —
`xattr -d com.apple.quarantine gecko-macos-arm64` does that. `gecko-web.zip`
is the static page, for hosting it yourself.
"""


def section(text: str, version: str) -> str | None:
    """The body under `## <version>`, up to the next `## `."""
    lines = text.splitlines()
    start = None
    for number, line in enumerate(lines):
        if re.match(rf"^##\s+\[?{re.escape(version)}\]?\b", line):
            start = number + 1
            break
    if start is None:
        return None
    out = []
    for line in lines[start:]:
        if line.startswith("## "):
            break
        out.append(line)
    return "\n".join(out).strip()


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: release_notes.py <version>", file=sys.stderr)
        return 2
    version = sys.argv[1]
    path = pathlib.Path(__file__).resolve().parent.parent / "CHANGELOG.md"
    body = section(path.read_text(), version) if path.is_file() else None
    if body is None:
        # Not an error: a tag can be cut before its section is written, and a
        # release with thin notes beats a release that failed to publish.
        print(f"No changelog section for {version} — see the commit history.")
    else:
        print(body)
    print(TRAILER)
    return 0


if __name__ == "__main__":
    sys.exit(main())
