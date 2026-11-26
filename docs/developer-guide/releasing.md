# Releasing

A release is the source code at a version, with the changelog's section for
it, and the site published from it. It is cut by hand, on purpose: it
publishes something permanent, so it is never a side effect of a commit.

## Cutting one

1. **Decide the version.** `[workspace.package] version` in `Cargo.toml` is
   the one the binary reports, so it moves first.
1. **Write the changelog section.** `CHANGELOG.md`, under `## <version>`, in
   terms of what a reader can now do — not of commits. The release notes are
   that section, read straight out of the file by `tools/release_notes.py`, so
   there is one description of what changed and not two that can disagree.
1. **Check it.** `./check.sh` and `./coverage`: the floors are a condition of
   delivery, and this is the delivery.
1. **Push `main`, then the tag**, in that order: the tag's run reads
   `main`'s result for the same commit instead of running it again.

```bash
git push origin main
git tag -a v0.1.0 -m "gecko-rs 0.1.0"
git push origin v0.1.0
```

Pushing to `main` runs the tests and builds the site without publishing it:
`docs.sh` passes `--strict`, so a dead link fails at the commit that wrote
it rather than greeting a reader.

Pushing a `v*` tag does everything else, and **it is the only thing that
publishes the site**.

That last point is not a preference. `actions/deploy-pages` names a
deployment by the commit, so two runs over one commit are one version to
Pages and the first to claim it is the one served. A release tags a commit
`main` has already pushed, so a deploy from `main` would always beat the
deploy from the tag — and the tag's run is the one carrying the page's
coverage report. The site therefore shows the last release, not the tip of
`main`.

## What the tag's run does

```
main-passed   waits for main's check job over the same commit    ->  pass / fail
full          the page, built; its coverage, measured             ->  page, report
coverage      the kernel's coverage, measured                     ->  report
  ↓
docs          the site, with the page under /app and both reports
  ↓
deploy-docs   GitHub Pages                    (needs main-passed)
release       gh release create, the changelog's own section     (needs main-passed)
```

**`./check.sh` does not run on the tag.** `main`'s run has just put the same
commit through it — formatting, lint, every Rust test, both suites of the
page — and repeating it was eight minutes that learned nothing new.
`tools/wait_for_main.sh` is what makes that trust checked: it finds `main`'s
run over the tagged commit, waits for its check job, and fails if that job
did not pass, and neither the site nor the release goes out without it. A tag
pushed before `main` has no such run, and fails there with a line saying so.

Only that job is waited on, not the macOS and Windows ones, and those two do
not run on a tag at all: a macOS runner that GitHub never handed out held
0.2.8's release back without a line of the code being run.

The page's coverage still runs both of the page's suites, with the collector
on, so a test that fails on the tag still fails the run.

## What a reader sees

GitHub puts **Releases** in the right-hand column of the repository as soon as
one exists. Each carries the changelog's section as its notes, and the source
code as a zip and a tarball, which GitHub attaches to every release on its
own. There are no binaries: the page is the application, and it is on the
site.
