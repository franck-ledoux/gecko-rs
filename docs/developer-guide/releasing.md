# Releasing

A release is what somebody can download. It is cut by hand, on purpose: it
costs minutes and publishes something permanent, so it is never a side effect
of a commit.

## Cutting one

1. **Decide the version.** `[workspace.package] version` in `Cargo.toml` is
   the one the binaries report, so it moves first.
1. **Write the changelog section.** `CHANGELOG.md`, under `## <version>`, in
   terms of what a reader can now do — not of commits. The release notes are
   that section, read straight out of the file by `tools/release_notes.py`, so
   there is one description of what changed and not two that can disagree.
1. **Check it.** `./check.sh` and `./coverage`: the floors are a condition of
   delivery, and this is the delivery.
1. **Tag and push the tag.**

```bash
git tag v0.1.0
git push origin v0.1.0
```

Pushing to `main` runs the tests and builds the site without publishing it:
`docs.sh` passes `--strict`, so a dead link fails at the commit that wrote
it rather than greeting a reader.

Pushing a `v*` tag does everything else at once. It builds the binaries, it
measures the page's coverage — a release-time measurement, for the seven
minutes it costs — and **it is the only thing that publishes the site**.

That last point is not a preference. `actions/deploy-pages` names a
deployment by the commit, so two runs over one commit are one version to
Pages and the first to claim it is the one served. A release tags a commit
`main` has already pushed, so a deploy from `main` would always beat the
deploy from the tag — and the tag's run is the one carrying the page's
coverage report. The site therefore shows the last release, not the tip of
`main`.

## What the workflow does

```
page        the Emscripten SDK, both wasm modules, the bundle   ->  artefact
  ↓
binaries    linux x86-64 · macOS arm64 · windows x86-64         ->  three artefacts
  ↓             each downloads the page and bakes it in with include_dir!
publish     gh release create, with the changelog's own section
```

The page is built **once**. It is the expensive half — the SDK alone is about
a gigabyte — and it has nothing to do with which machine a binary is for, so
the three binary jobs download it instead of each building it again.

Every binary is the whole application: the page is inside it, and there is
nothing else to install. `gecko-web.zip` is the same page as static files, for
anyone who would rather host it.

## What a reader sees

GitHub puts **Releases** in the right-hand column of the repository as soon as
one exists, and `releases/latest/download/<asset>` always resolves to the
newest one — which is why the assets are named without a version in them, and
why the links in the README do not go stale.

!!! note "The macOS binary is not signed"
    A downloaded binary is quarantined until the person says otherwise, and
    signing it needs a paid Apple developer account. The README says so, with
    the `xattr` line that lifts it. Honest, and one line for the reader.
