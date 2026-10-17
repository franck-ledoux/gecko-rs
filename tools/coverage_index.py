#!/usr/bin/env python3
"""One page in front of the two coverage reports.

A reader who follows a badge lands here, and the first thing they need is
not a percentage: it is what the number covers and what it leaves out. The
site is built with mkdocs, so this is deliberately plain HTML — it sits
beside the generated reports rather than inside the documentation.
"""

import json
import pathlib
import sys

PAGE = """<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>gecko-rs coverage</title>
<style>
  :root {{ color-scheme: light dark; --ink: #1a1a1a; --paper: #fdfdfd; --dim: #666; }}
  @media (prefers-color-scheme: dark) {{
    :root {{ --ink: #e8e8e8; --paper: #1b1b1d; --dim: #a0a0a0; }}
  }}
  body {{ margin: 0 auto; padding: 2.5rem 1rem; max-width: 42rem; background: var(--paper);
         color: var(--ink); font: 16px/1.6 -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; }}
  h1 {{ font-size: 1.6rem; margin: 0 0 .25rem; }}
  p.sub {{ color: var(--dim); margin: 0 0 2rem; }}
  ul {{ list-style: none; padding: 0; }}
  li {{ margin: 0 0 1.25rem; }}
  a.card {{ display: block; padding: 1rem 1.25rem; border: 1px solid color-mix(in srgb, var(--ink) 20%, transparent);
            border-radius: .5rem; text-decoration: none; color: inherit; }}
  a.card:hover {{ border-color: color-mix(in srgb, var(--ink) 45%, transparent); }}
  .pct {{ float: right; font-variant-numeric: tabular-nums; font-weight: 600; }}
  .what {{ color: var(--dim); font-size: .9rem; }}
  footer {{ margin-top: 2.5rem; color: var(--dim); font-size: .9rem; }}
</style>
</head>
<body>
<h1>Coverage</h1>
<p class="sub">Two numbers, because a kernel and an interface are not one thing.</p>
<ul>
{cards}
</ul>
<footer>
<p><strong>What these do not cover.</strong> The kernel as the browser runs it:
neither wasm target instruments, so its figure comes from the native tests —
the same code, built another way. And the glue between the page and wasm,
which only the browser suite exercises, and only as “it passed”.</p>
<p><a href="../">Back to the documentation</a></p>
</footer>
</body>
</html>
"""

CARD = """<li><a class="card" href="{where}/index.html">
<span class="pct">{pct}</span><strong>{title}</strong><br>
<span class="what">{what}</span></a></li>"""

HALVES = [
    (
        "kernel",
        "kernel-badge.json",
        "The kernel",
        "The Rust crates: the block structure, the session, the commands, "
        "and the Lua API — measured by the tests that drive them natively.",
    ),
    (
        "page",
        "page-badge.json",
        "The page",
        "The TypeScript: decoding, the scene, and every gesture — measured by "
        "the headless suite and by a real browser, merged.",
    ),
]


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: coverage_index.py <directory>", file=sys.stderr)
        return 2
    root = pathlib.Path(sys.argv[1])
    cards = []
    for where, badge, title, what in HALVES:
        if not (root / where).is_dir():
            continue
        pct = "—"
        try:
            pct = json.loads((root / badge).read_text())["message"]
        except (OSError, ValueError, KeyError):
            pass
        cards.append(CARD.format(where=where, pct=pct, title=title, what=what))
    if not cards:
        print("no reports to index", file=sys.stderr)
        return 1
    root.mkdir(parents=True, exist_ok=True)
    (root / "index.html").write_text(PAGE.format(cards="\n".join(cards)))
    print(f"wrote {root / 'index.html'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
