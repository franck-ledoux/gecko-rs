#!/usr/bin/env bash
# How far the cellular engine has got, as a number that may not go down.
#
# The suite is the oracle: `docs/developer-guide/cellular.md` replaces the
# combinatorial map behind the same public surface, so the tests that speak
# only of named cells describe the engine being written as much as the one
# being replaced. Progress is therefore how many of them pass under the
# `cellular` feature, and this ratchets that number.
#
# A fall is a failure. A rise is not: it prints what to write into
# `cellular.ratchet`, and the number is bumped in the commit that earned it,
# which is what keeps it a record rather than a reading.
set -uo pipefail
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"

log=$(mktemp)
trap 'rm -f "$log"' EXIT
# `--no-fail-fast`, or cargo stops at the first test binary that fails and
# the count is of whatever happened to run first.
cargo test --no-fail-fast -p gecko-core -p gecko-cmd --features gecko-cmd/cellular >"$log" 2>&1

# Summed over every test binary, whether its own run came out ok or not: a
# binary with one failure still tells us about the rest. A compile error
# prints no result line at all, so the count is zero and the ratchet catches
# it.
count() { grep -oE "test result: [a-zA-Z]+\. [0-9]+ passed" "$log" | grep -oE "[0-9]+ passed" | grep -oE "[0-9]+" | awk '{s+=$1} END {print s+0}'; }
passed=$(count)
failed=$(grep -oE "test result: [a-zA-Z]+\. [0-9]+ passed; [0-9]+ failed" "$log" | grep -oE "[0-9]+ failed" | grep -oE "[0-9]+" | awk '{s+=$1} END {print s+0}')
want=$(tr -dc '0-9' <cellular.ratchet)

# A build error, as against a test failure: cargo says "error: test failed"
# for the latter, which is the thing being counted and not a reason to stop.
if grep -qE "^(error\[|error: could not compile)" "$log"; then
  echo "── the cellular engine does not build ─────────────────────────"
  grep -E "^(error\[|error: could not compile)" -A 4 "$log" | head -40
  exit 1
fi

echo "cellular: $passed passing, $failed failing (the ratchet says $want)"
if [ "$passed" -lt "$want" ]; then
  echo "!! that is $((want - passed)) fewer than it was. The ratchet does not go down." >&2
  grep -E "^    [a-z_]+::" "$log" | head -40 >&2
  exit 1
fi
if [ "$passed" -gt "$want" ]; then
  echo "** $((passed - want)) more than the ratchet. Write $passed into cellular.ratchet."
fi
exit 0
