#!/usr/bin/env python3
"""Every action the workflows name, checked against the action itself.

A workflow is the one part of the project that no test touches: it is read
by GitHub and by nothing else, so a reference that has rotted — a version
that no longer exists, an input an action has dropped, a runtime that has
been deprecated — is found by burning a run. This reads each `uses:` at its
exact ref and answers three questions without burning anything:

  * does the ref resolve, and does it carry an action at all?
  * is every key passed under `with:` an input that action declares *there*?
  * what runtime does it ask the runner for? Node 16 and Node 20 are
    deprecated, and that is the whole reason this script exists.

What it deliberately does not check: whether a newer version exists. A
version that is behind is not a fault, and a tool that nags about it would
be ignored. Run it when you touch a workflow, and when a run warns about a
runtime. `gh api repos/<owner>/<repo>/releases/latest` answers the other
question in one line.
"""

import argparse
import pathlib
import sys
import urllib.error
import urllib.request

try:
    import yaml
except ModuleNotFoundError:  # pragma: no cover - a missing environment
    sys.exit(
        "PyYAML is needed. Run it from the documentation's environment —\n"
        "  ./.venv/bin/python tools/check_actions.py\n"
        "which ./docs.sh builds, or install PyYAML where python3 can see it."
    )

# What the runner still accepts without complaining. A warning in a log is
# a deadline: `node16` was removed, and `node20` is on the same path.
DEPRECATED = {"node12", "node16", "node20"}

RAW = "https://raw.githubusercontent.com/{repo}/{ref}/{name}"


def action_at(repo: str, ref: str) -> dict | None:
    """The action's own metadata at that ref, or None when there is none.

    Both spellings, because both are in the wild: GitHub's own actions use
    `action.yml` and plenty of others use `action.yaml`.
    """
    for name in ("action.yml", "action.yaml"):
        try:
            with urllib.request.urlopen(RAW.format(repo=repo, ref=ref, name=name)) as f:
                return yaml.safe_load(f.read())
        except urllib.error.HTTPError as problem:
            if problem.code != 404:
                raise
        except urllib.error.URLError as problem:
            raise SystemExit(f"cannot reach raw.githubusercontent.com: {problem}")
    return None


def steps(workflow: dict):
    """Every step of every job, and the jobs that are themselves a `uses:`."""
    for name, job in (workflow.get("jobs") or {}).items():
        if not isinstance(job, dict):
            continue
        if "uses" in job:  # a reusable workflow, called as a whole job
            yield name, job
        for step in job.get("steps") or []:
            if isinstance(step, dict) and "uses" in step:
                yield name, step


def check(paths: list[pathlib.Path]) -> int:
    """One line per reference. Returns the number of faults."""
    faults = 0
    seen: dict[str, str] = {}
    for path in paths:
        workflow = yaml.safe_load(path.read_text())
        for job, step in steps(workflow):
            ref = step["uses"]
            passed = set(step.get("with") or {})
            where = f"{path.name} › {job}"

            # Nothing to fetch for these, and nothing to get wrong either.
            if ref.startswith(("./", "docker://")):
                print(f"skipped  {ref}  ({where})")
                continue

            owner, _, at = ref.partition("@")
            if not at:
                print(f"UNPINNED {ref}  ({where}) — no ref to check")
                faults += 1
                continue
            # `owner/repo/path/to/action@ref` is an action in a subdirectory;
            # its metadata is not at the root, so say so rather than guess.
            if owner.count("/") != 1:
                print(f"skipped  {ref}  ({where}) — action in a subdirectory")
                continue

            meta = action_at(owner, at)
            if meta is None:
                print(f"MISSING  {ref}  ({where}) — no action.yml at that ref")
                faults += 1
                continue

            inputs = meta.get("inputs") or {}
            unknown = sorted(passed - set(inputs))
            needed = sorted(
                key
                for key, spec in inputs.items()
                if isinstance(spec, dict)
                and spec.get("required")
                and "default" not in spec
                and key not in passed
            )
            using = (meta.get("runs") or {}).get("using", "?")

            trouble = []
            if unknown:
                trouble.append(f"inputs it does not declare: {', '.join(unknown)}")
            if needed:
                trouble.append(f"required inputs not passed: {', '.join(needed)}")
            if using in DEPRECATED:
                trouble.append(f"deprecated runtime: {using}")

            if trouble:
                faults += 1
                print(f"FAULT    {ref}  ({where})")
                for line in trouble:
                    print(f"         {line}")
            elif ref not in seen:
                print(f"ok       {ref:44s} {using}")
            seen[ref] = using
    return faults


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "workflows",
        nargs="*",
        type=pathlib.Path,
        help="defaults to every workflow in .github/workflows",
    )
    here = pathlib.Path(__file__).resolve().parent.parent
    chosen = parser.parse_args().workflows or sorted(
        (here / ".github/workflows").glob("*.yml")
    )
    if not chosen:
        sys.exit("no workflows to check")

    faults = check(chosen)
    print()
    if faults:
        print(f"{faults} to fix")
        return 1
    print(f"{len(chosen)} workflow(s), nothing to fix")
    return 0


if __name__ == "__main__":
    sys.exit(main())
