#!/usr/bin/env bash
# Wait for `main`'s own run over this commit, and pass only if it passed.
#
# A tag points at a commit `main` has already pushed, and `main`'s run has
# already put that commit through `./check.sh` — formatting, lint, every
# Rust test, both suites of the page. Running all of it a second time on the
# tag was eight minutes spent learning nothing new. So the tag's run trusts
# that result instead of repeating it, and this is what makes the trust
# checked rather than assumed: nothing is published from a tag until the
# check job of `main`'s run over the **same commit** has come back green.
#
# Only that one job is waited on. The macOS and Windows jobs are worth
# reading, but a macOS runner GitHub never hands out is not a failure of the
# code, and it must not hold back a release.
#
#   tools/wait_for_main.sh [sha]      GH_TOKEN and GITHUB_REPOSITORY set
#
# Read-only: it lists runs and jobs, nothing more.
set -euo pipefail

sha=${1:-${GITHUB_SHA:?no commit given}}
repo=${GITHUB_REPOSITORY:?no repository given}
job="The whole page, in a browser"
# How long `main`'s run may take to appear, and then to finish.
appear=${APPEAR_SECONDS:-180}
finish=${FINISH_SECONDS:-2700}
pause=${PAUSE_SECONDS:-20}

run=""
waited=0
while :; do
  run=$(gh api "repos/$repo/actions/workflows/ci.yml/runs?head_sha=$sha&branch=main&event=push" \
    --jq '.workflow_runs[0].id // empty')
  [ -n "$run" ] && break
  if [ "$waited" -ge "$appear" ]; then
    echo "::error::no run of main over $sha. Push main before the tag, so the commit is tested there." >&2
    exit 1
  fi
  sleep "$pause"
  waited=$((waited + pause))
done
echo "main's run over $sha: https://github.com/$repo/actions/runs/$run"

waited=0
while :; do
  state=$(gh api "repos/$repo/actions/runs/$run/jobs?per_page=100" \
    --jq ".jobs[] | select(.name == \"$job\") | \"\(.status) \(.conclusion)\"")
  case "$state" in
    "completed success")
      echo "\"$job\" passed on main"
      exit 0
      ;;
    completed*)
      echo "::error::\"$job\" did not pass on main (${state#completed }): nothing is published." >&2
      exit 1
      ;;
  esac
  if [ "$waited" -ge "$finish" ]; then
    echo "::error::\"$job\" on main has not finished after ${finish}s (${state:-not started})." >&2
    exit 1
  fi
  sleep "$pause"
  waited=$((waited + pause))
done
