#!/usr/bin/env python3
"""Tune a blocking's vertex positions for the best minimum scaled Jacobian.

    python3 optimize.py part.lua params.json [--iters 800] [--h 0.8] [--seed 1]
                        [--gecko PATH] [--out best.json]

`part.lua` is your blocking script. It keeps its positions in a table of
defaults that a global `PARAMS` overrides key by key, reads its cell size
from a global `H`, and meshes itself — the B1 example does:

    local P = { sT = -6.6, zR = 1.58, ... }
    for k, v in pairs(PARAMS or {}) do P[k] = v end
    local H = H or 0.4

so `params.json` need only name what is to move.

`params.json` says what may move:

    {"sT": [-6.6, 0.5, -11.5, -5.5], ...}     name: [start, step, low, high]

A parameter set the script refuses — by raising an error, e.g. when two
vertices would cross — scores as failed and is skipped, so put the
constraints in the script. `--h` is the cell size to optimise at: coarse is
fast, and the result has to be checked again at the final size, since the
minimum moves with the cell aspect.

The search is a random coordinate search: one to three parameters moved by a
normal step at a time, kept when the score improves, steps shrunk by 0.7
every 150 tries. Score = min SJ + 0.2 × mean SJ; a failed run, or a run with
inverted cells, scores below any valid one.

Runs from the script's folder. Writes the best parameters to --out
(default best-params.json) and prints each improvement.
"""
import argparse
import json
import os
import random
import subprocess
import sys


def find_gecko(given):
    if given:
        return given
    here = os.path.dirname(os.path.abspath(__file__))
    for p in (os.path.join(here, "..", "..", "..", "..", "target", "release", "gecko"),):
        if os.path.exists(p):
            return os.path.abspath(p)
    return "gecko"


def evaluate(gecko, folder, script, params, h):
    lines = ["PARAMS = {" + ", ".join(f"{k} = {v!r}" for k, v in params.items()) + "}", f"H = {h!r}", "NO_EXPORT = true",
             f'dofile("{script}")']
    runner = os.path.join(folder, "_optimize_run.lua")
    with open(runner, "w") as f:
        f.write("\n".join(lines) + "\n")
    out = subprocess.run([gecko, "run", "_optimize_run.lua", "--json"], cwd=folder, capture_output=True, text=True)
    try:
        report = json.loads(out.stdout)
    except json.JSONDecodeError:
        return None, out.stderr.strip()[:200]
    q = ((report.get("quality") or {}).get("scaled_jacobian")) if report.get("ok") else None
    if not q:
        return None, (report.get("error") or "no mesh")[:200]
    return q, None


def score(q):
    if q is None:
        return float("-inf")
    s = q["min"] + 0.2 * q["mean"]
    return s - 10 if q["inverted"] else s


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("script")
    ap.add_argument("params")
    ap.add_argument("--iters", type=int, default=800)
    ap.add_argument("--h", type=float, default=None, help="cell size to optimise at")
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--gecko")
    ap.add_argument("--out", default="best-params.json")
    a = ap.parse_args()

    folder = os.path.dirname(os.path.abspath(a.script))
    script = os.path.basename(a.script)
    gecko = find_gecko(a.gecko)
    with open(a.params) as f:
        spec = json.load(f)
    try:
        search(a, gecko, folder, script, spec)
    finally:
        runner = os.path.join(folder, "_optimize_run.lua")
        if os.path.exists(runner):
            os.remove(runner)


def search(a, gecko, folder, script, spec):
    best = {k: float(v[0]) for k, v in spec.items()}
    step = {k: float(v[1]) for k, v in spec.items()}
    lo = {k: float(v[2]) for k, v in spec.items()}
    hi = {k: float(v[3]) for k, v in spec.items()}
    q, why = evaluate(gecko, folder, script, best, a.h)
    if q is None:
        sys.exit(f"the starting parameters do not mesh: {why}")
    best_score = score(q)
    print(f"start: min {q['min']:.4f} mean {q['mean']:.4f}", flush=True)
    rng = random.Random(a.seed)
    keys = list(spec)
    for i in range(a.iters):
        cand = dict(best)
        for k in rng.sample(keys, min(len(keys), rng.choice([1, 1, 2, 3]))):
            cand[k] = min(hi[k], max(lo[k], cand[k] + rng.gauss(0, step[k])))
        q, _ = evaluate(gecko, folder, script, cand, a.h)
        s = score(q)
        if s > best_score:
            best, best_score = cand, s
            print(f"{i}: min {q['min']:.4f} mean {q['mean']:.4f}", flush=True)
        if i % 150 == 149:
            for k in step:
                step[k] *= 0.7
    with open(a.out, "w") as f:
        json.dump(best, f, indent=1)
    print(f"best: score {best_score:.4f}, written to {a.out}")


if __name__ == "__main__":
    main()
