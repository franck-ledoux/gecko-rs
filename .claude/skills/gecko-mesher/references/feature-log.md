# The friction log

The user builds Gecko. Every workaround you needed is a candidate feature,
and the log across many parts is how they decide what to build next. So it
is a deliverable, not a diary: one JSON object per line in
`frictions.jsonl`, in the part's folder, written **when it happens**.

## Fields

| field | what to write |
| ----- | ------------- |
| `part` | the model, e.g. `"B1"` |
| `step` | `understand`, `plan`, `build`, `check`, `size`, `mesh`, `diagnose`, `optimise`, `deliver` |
| `kind` | `feature-request` (a command or read Gecko should have — every script written to make up for Gecko, and every stopgap of the skill used), `missing-read` (Gecko could not tell me X), `missing-command` (could not do X), `wrong-result` (did X badly), `bug`, `cad-defect` (the file, not Gecko), `manual-work` (I did by hand what a tool could do), `docs` |
| `need` | what you were trying to get or do, in one sentence |
| `workaround` | what you did instead |
| `cost` | what it cost: time, tool calls, quality, blocks — numbers where you have them |
| `idea` | the feature that would remove it, as Gecko would name it |
| `severity` | `1` cosmetic · `2` slowed me down · `3` cost quality or blocks · `4` could not do it |

## Examples (from meshing B1)

```json
{"part":"B1","step":"optimise","kind":"manual-work","need":"place 30 vertex parameters to maximise min SJ","workaround":"wrote a Python random search calling gecko run 2500 times","cost":"0.41 -> 0.64 min SJ; 2 h of iteration","idea":"gecko.optimize{ params, objective = 'min_sj' } in the kernel, or block-vertex optimisation against mesh quality","severity":3}
{"part":"B1","step":"optimise","kind":"wrong-result","need":"tidy the blocking","workaround":"did not use gecko.smooth","cost":"smooth inverted 5 cells and pulled nodes off the hole rim","idea":"smoothing that keeps classified nodes on curves and rejects steps that lower mesh min SJ","severity":3}
{"part":"B1","step":"understand","kind":"cad-defect","need":"follow the outer trim curve to the step corner","workaround":"merged curves 20+23+26","cost":"5 outer faces left on nothing","idea":"detect duplicated curves (same ends, same points) and coincident corners on load, and merge them","severity":3}
{"part":"B1","step":"check","kind":"missing-read","need":"know whether the blocking is fundamental: which sheet captures each surface, which columns run along each curve","workaround":"scripts/fundamental.py rebuilds sheets and chords from the --json report","cost":"a separate script; no picture of the sheets","idea":"gecko.fundamental() in the kernel, and gecko.render{ show = {\"sheets\"} } colouring the capturing sheets","severity":2}
{"part":"B1","step":"size","kind":"missing-command","need":"cells of about 0.4 everywhere","workaround":"looped over edges, set_segments per sheet from the longest edge","cost":"30 lines of Lua","idea":"gecko.mesh{ size = 0.4 }","severity":2}
```

## What makes an entry useful

- **Specific**: "no way to get a cylinder's axis" beats "geometry info lacking".
- **Measured**: what it cost in quality, blocks or time.
- **Actionable**: the `idea` names a command or read, with its arguments.
- **Not duplicated within a part**: log a friction once per part, with the
  count if it recurred.

`scripts/frictions.py dir1 dir2 …` merges the logs, groups entries with the
same `idea` (or close `need`), and ranks them by how many parts hit them ×
severity — the summary to give the user.
