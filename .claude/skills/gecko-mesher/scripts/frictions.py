#!/usr/bin/env python3
"""Rank the frictions logged while meshing, across parts: the feature list.

    python3 frictions.py run1/ run2/ ... [--md out.md]

Reads every `frictions.jsonl` in the folders given (and their subfolders),
groups entries that propose the same `idea` — compared on their words, so
"gecko.mesh{ size = h }" and "mesh with a target size" stay apart unless the
log says them alike; write ideas as Gecko would name them — and ranks the
groups by parts hit × worst severity, then by how often.

Prints a markdown table; --md also writes it to a file.
"""
import argparse
import json
import os
import re
from collections import defaultdict


def entries(folders):
    for folder in folders:
        for root, _, files in os.walk(folder):
            if "frictions.jsonl" in files:
                with open(os.path.join(root, "frictions.jsonl")) as f:
                    for n, line in enumerate(f, 1):
                        line = line.strip()
                        if not line:
                            continue
                        try:
                            yield json.loads(line)
                        except json.JSONDecodeError:
                            print(f"skipped {root}/frictions.jsonl:{n}: not JSON")


def key(entry):
    text = (entry.get("idea") or entry.get("need") or "").lower()
    words = re.findall(r"[a-z0-9_.]+", text)
    return " ".join(sorted(set(words))[:12])


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("folders", nargs="+")
    ap.add_argument("--md")
    a = ap.parse_args()

    groups = defaultdict(list)
    for e in entries(a.folders):
        groups[key(e)].append(e)
    rows = []
    for items in groups.values():
        parts = sorted({str(e.get("part", "?")) for e in items})
        severity = max(int(e.get("severity", 1)) for e in items)
        rows.append((len(parts) * severity, len(items), severity, parts, items))
    rows.sort(key=lambda r: (-r[0], -r[1]))

    lines = ["| rank | idea | kind | parts | severity | need | cost |", "| --- | --- | --- | --- | --- | --- | --- |"]
    for n, (_, count, severity, parts, items) in enumerate(rows, 1):
        first = items[0]
        cell = lambda s: str(s or "").replace("|", "\\|").replace("\n", " ")
        lines.append(
            f"| {n} | {cell(first.get('idea'))} | {cell(first.get('kind'))} | {', '.join(parts)} ({count}) "
            f"| {severity} | {cell(first.get('need'))} | {cell('; '.join(str(e.get('cost', '')) for e in items[:3]))} |"
        )
    text = "\n".join(lines)
    print(text if rows else "no frictions logged")
    if a.md:
        with open(a.md, "w") as f:
            f.write(text + "\n")


if __name__ == "__main__":
    main()
