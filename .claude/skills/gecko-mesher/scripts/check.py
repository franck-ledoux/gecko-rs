#!/usr/bin/env python3
"""Hold a `gecko run --json` report to the quality rules.

    python3 check.py report.json [--known curves=27 corners=19] [--rules rules.json]

Prints each rule with its value and whether it passed, then a verdict, and
exits 1 if a hard rule failed. The rules are the defaults of
references/quality-rules.md unless a rules.json gives others — beside this
skill's references/, or named with --rules:

    {"min_sj": 0.3, "target_min_sj": 0.5, "max_share_below_07": 0.01,
     "target_mean_sj": 0.9, "max_aspect_ratio": 10, "max_skewness": 0.8}

If the script returned what its positions were tuned on —
`return { tuned = L.tuned{ gecko = "0.2.10", min_sj = 0.644, curves = "17/17" } }`
— that is compared with the Gecko that ran it and what it gave: a different
version, or a minimum or curve count that moved, says to retune.

`--known` lists CAD defects (tags) that no mesh can follow: they are reported
but do not fail the check. The fundamental-blocking rule is the one of
scripts/fundamental.py, which gives the details.
"""
import argparse
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fundamental  # noqa: E402  (beside this script)

DEFAULTS = {
    "min_sj": 0.30,
    "target_min_sj": 0.50,
    "max_share_below_07": 0.01,
    "target_mean_sj": 0.90,
    "max_aspect_ratio": 10.0,
    "max_skewness": 0.80,
}


def rules_from(path):
    rules = dict(DEFAULTS)
    if path is None:
        here = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "references", "rules.json")
        path = here if os.path.exists(here) else None
    if path:
        with open(path) as f:
            rules.update(json.load(f))
    return rules


def known_from(items):
    known = {"curves": set(), "corners": set(), "surfaces": set()}
    for item in items or []:
        kind, _, tags = item.partition("=")
        if kind not in known:
            sys.exit(f"--known {item}: say curves=…, corners=… or surfaces=…")
        known[kind].update(int(t) for t in tags.split(",") if t)
    return known


def share_below(q, threshold):
    """Share of cells below `threshold` from the histogram, whose bins start at
    `bins[i]`; a bin straddling the threshold is counted below, which errs on
    the side of failing."""
    total = sum(q["histogram"]) or 1
    below = sum(n for start, n in zip(q["bins"], q["histogram"]) if start < threshold)
    return below / total


def check(report, rules, known):
    rows = []  # (hard?, name, value, passed)

    def rule(hard, name, value, passed):
        rows.append((hard, name, value, passed))

    rule(True, "script ran to the end", report.get("error") or "yes", bool(report.get("ok")))
    q = (report.get("quality") or {}).get("scaled_jacobian")
    if not q:
        rule(True, "a mesh was built", "no mesh", False)
    else:
        rule(True, "no inverted cell", q["inverted"], q["inverted"] == 0)
        rule(True, f"min scaled Jacobian >= {rules['min_sj']}", round(q["min"], 4), q["min"] >= rules["min_sj"])
        rule(False, f"min scaled Jacobian >= {rules['target_min_sj']}", round(q["min"], 4), q["min"] >= rules["target_min_sj"])
        s = share_below(q, 0.7)
        rule(False, f"share below 0.7 <= {rules['max_share_below_07']:.0%}", f"{s:.2%}", s <= rules["max_share_below_07"])
        rule(False, f"mean scaled Jacobian >= {rules['target_mean_sj']}", round(q["mean"], 4), q["mean"] >= rules["target_mean_sj"])
        ar = report["quality"].get("aspect_ratio") or {}
        if "max" in ar:
            rule(False, f"aspect ratio <= {rules['max_aspect_ratio']}", round(ar["max"], 2), ar["max"] <= rules["max_aspect_ratio"])
        sk = report["quality"].get("skewness") or {}
        if "max" in sk:
            rule(False, f"skewness <= {rules['max_skewness']}", round(sk["max"], 3), sk["max"] <= rules["max_skewness"])

    f = report.get("fidelity")
    if f:
        curves = [c["tag"] for c in f["curves"]["missed"]]
        corners = [c["tag"] for c in f["corners"]["missed"]]
        real_curves = [t for t in curves if t not in known["curves"]]
        real_corners = [t for t in corners if t not in known["corners"]]
        rule(True, "every real curve followed", f"{f['curves']['followed']}/{f['curves']['checked']}"
             + (f", missed {real_curves}" if real_curves else "")
             + (f", known {sorted(set(curves) - set(real_curves))}" if len(curves) > len(real_curves) else ""),
             not real_curves)
        rule(True, "every real corner on a node", f"missed {real_corners}" if real_corners else "yes"
             + (f", known {sorted(set(corners) - set(real_corners))}" if len(corners) > len(real_corners) else ""),
             not real_corners)
        # The skin on the part: no skin node past the tolerance, and no
        # surface of the part that no face is on — the fidelity check only
        # samples surfaces something covers, so a surface missed whole would
        # otherwise score as followed. Known defects do not excuse either:
        # they are curves and corners, not where the skin lies.
        beyond = f["skin"].get("beyond", 0)
        uncovered = []
        if report.get("blocking"):
            uncovered = [i["on"]["tag"] for i in report["blocking"]["issues"]
                         if i["kind"] == "surface_not_covered" and i["on"]["tag"] not in known["surfaces"]]
        said = f"skin max {f['skin']['max']:.3f} ({beyond} nodes past the tolerance)"
        if uncovered:
            said += f", surfaces {uncovered} covered by no face and never measured"
        rule(True, "skin on the part", said, beyond == 0 and not uncovered)
    elif q:
        rule(False, "fidelity measured", "no geometry", False)

    # Tuned on this Gecko? Positions tuned against one version's
    # projection are not the best against another's.
    tuned = (report.get("returned") or {}).get("tuned") if isinstance(report.get("returned"), dict) else None
    if tuned is None:
        rule(False, "tuning stamped", "no L.tuned{} returned: say what the positions were tuned on", False)
    else:
        now_version = report.get("gecko")
        drift = []
        if str(tuned.get("gecko")) != str(now_version):
            drift.append(f"tuned with Gecko {tuned.get('gecko')}, run with {now_version}")
        if q and tuned.get("min_sj") is not None and q["min"] < float(tuned["min_sj"]) - 0.01:
            drift.append(f"min SJ {float(tuned['min_sj']):.3f} when tuned, {q['min']:.3f} now")
        fid = report.get("fidelity")
        if fid and tuned.get("curves"):
            now_curves = f"{fid['curves']['followed']}/{fid['curves']['checked']}"
            if now_curves != str(tuned["curves"]):
                drift.append(f"curves {tuned['curves']} when tuned, {now_curves} now")
        rule(False, "tuned on this Gecko", "; ".join(drift) + " — retune (optimize.py from these positions)" if drift
             else f"yes (Gecko {now_version})", not drift)

    b = report.get("blocking")
    if b:
        faces = [i for i in b["issues"] if i["kind"] == "outer_face_on_nothing"]
        rule(True, "every outer face attached", f"{len(faces)} on nothing" if faces else "yes", not faces)
        f = fundamental.analyse(report, {"surfaces": known["surfaces"], "curves": known["curves"]})
        if "surfaces" in f:
            failed = f["failed"]
            said = []
            if failed["surfaces"]:
                said.append(f"surfaces {failed['surfaces']} not captured by one sheet")
            if failed["curves"]:
                said.append(f"curves {failed['curves']} without fundamental chords")
            levels = f"level 1 sheets {f['levels']['1']}, level 2 {f['levels']['2']}"
            rule(True, "the blocking is fundamental", "; ".join(said) if said else f"yes ({levels})", f["fundamental"])
        rule(False, "blocks", b["summary"]["blocks"], True)
    return rows


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("report")
    ap.add_argument("--known", nargs="*", default=[])
    ap.add_argument("--rules")
    a = ap.parse_args()
    with open(a.report) as f:
        report = json.load(f)
    rows = check(report, rules_from(a.rules), known_from(a.known))
    failed_hard = [r for r in rows if r[0] and not r[3]]
    missed_targets = [r for r in rows if not r[0] and not r[3]]
    width = max(len(r[1]) for r in rows)
    for hard, name, value, passed in rows:
        mark = "ok  " if passed else ("FAIL" if hard else "miss")
        print(f"{mark}  {'hard' if hard else 'goal'}  {name.ljust(width)}  {value}")
    if failed_hard:
        print(f"\nNOT DONE: {len(failed_hard)} hard rule(s) failed")
    elif missed_targets:
        print(f"\nACCEPTABLE: hard rules pass, {len(missed_targets)} target(s) missed")
    else:
        print("\nGOOD: every rule and target met")
    sys.exit(1 if failed_hard else 0)


if __name__ == "__main__":
    main()
