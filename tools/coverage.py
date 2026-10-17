#!/usr/bin/env python3
"""lcov files in, one number out — and the badge that prints it.

Two languages cannot share a percentage: a kernel at ninety and an interface
at fifty average to a figure that describes neither. So this merges the lcov
files that belong to **one** of them and reports that one, and the caller
runs it once per number.

The merge is a union over lines: a line is covered if any run covered it.
That is what two test suites over one body of code mean — the unit tests and
the browser tests exercise different parts of the same page, and neither
alone is the answer.
"""

import argparse
import json
import pathlib
import sys


def read(path: pathlib.Path) -> dict[str, dict[int, int]]:
    """`{source file: {line: hits}}` from one lcov file."""
    files: dict[str, dict[int, int]] = {}
    current: dict[int, int] | None = None
    for line in path.read_text(errors="replace").splitlines():
        if line.startswith("SF:"):
            # Normalised, because one tool writes absolute paths and another
            # writes paths relative to its own working directory.
            name = line[3:].strip().replace("\\", "/")
            for cut in ("/front/", "/crates/"):
                if cut in name:
                    name = name[name.index(cut) + 1 :]
                    break
            current = files.setdefault(name, {})
        elif line.startswith("DA:") and current is not None:
            number, _, hits = line[3:].partition(",")
            try:
                current[int(number)] = max(
                    current.get(int(number), 0), int(float(hits.split(",")[0]))
                )
            except ValueError:
                continue
        elif line.startswith("end_of_record"):
            current = None
    return files


def merge(paths: list[pathlib.Path]) -> dict[str, dict[int, int]]:
    out: dict[str, dict[int, int]] = {}
    for path in paths:
        for name, lines in read(path).items():
            into = out.setdefault(name, {})
            for number, hits in lines.items():
                into[number] = max(into.get(number, 0), hits)
    return out


def colour(share: float) -> str:
    for floor, name in (
        (90, "brightgreen"),
        (80, "green"),
        (70, "yellowgreen"),
        (60, "yellow"),
        (50, "orange"),
    ):
        if share >= floor:
            return name
    return "red"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("lcov", nargs="+", type=pathlib.Path)
    ap.add_argument("--label", required=True, help="what the badge calls it")
    ap.add_argument("--badge", type=pathlib.Path, help="where to write badge.json")
    ap.add_argument(
        "--floor",
        type=float,
        default=None,
        help="fail if the share of covered lines is under this",
    )
    ap.add_argument(
        "--worst",
        type=int,
        default=0,
        help="also print this many of the least covered files",
    )
    args = ap.parse_args()

    missing = [p for p in args.lcov if not p.is_file()]
    if missing:
        print(f"no coverage to read: {', '.join(str(p) for p in missing)}", file=sys.stderr)
        return 2

    files = merge(args.lcov)
    found = sum(len(lines) for lines in files.values())
    hit = sum(1 for lines in files.values() for hits in lines.values() if hits)
    share = 100.0 * hit / found if found else 0.0
    print(f"{args.label}: {share:.1f}% of {found} lines, in {len(files)} files")

    if args.worst:
        ranked = sorted(
            (
                (100.0 * sum(1 for h in lines.values() if h) / len(lines), name, len(lines))
                for name, lines in files.items()
                if lines
            ),
        )
        for each, name, lines in ranked[: args.worst]:
            print(f"  {each:5.1f}%  {name} ({lines} lines)")

    if args.badge:
        args.badge.parent.mkdir(parents=True, exist_ok=True)
        args.badge.write_text(
            json.dumps(
                {
                    "schemaVersion": 1,
                    "label": args.label,
                    "message": f"{share:.0f}%",
                    "color": colour(share),
                }
            )
            + "\n"
        )

    if args.floor is not None and share + 1e-9 < args.floor:
        print(
            f"under the floor: {share:.1f}% < {args.floor:.1f}%."
            " Write a test, or lower the floor in ./coverage and say why.",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
