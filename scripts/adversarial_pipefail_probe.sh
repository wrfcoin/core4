#!/usr/bin/env bash
# Committed control for the adversarial gate's pipe-to-tee exit handling
# (#2311, review of #2352).
#
# GitHub's default shell is `bash -e {0}` — NO `pipefail`. So
# `python3 scripts/adversarial_suite.py --run | tee adversarial-output.txt` would
# exit with tee's 0 and MASK a failing adversarial target: the scheduled suite
# could go green on a real failure, the exact false-green this gate exists to
# prevent. The workflow now sets `set -o pipefail` on every piped step; this probe
# proves that is load-bearing. Removing pipefail from the workflow turns step 2
# below into the masked-green case, so this control reverts RED.
#
#   scripts/adversarial_pipefail_probe.sh
#
# Exit 0 = pipefail correctly propagates a failing producer through `tee`.
# Exit 1 = the pipeline is not fail-closed, or the control's setup is wrong.

set -uo pipefail

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fail() { echo "PROBE FAILED: $*" >&2; exit 1; }

# A producer that emits output and then FAILS (exit 1) — exactly like
# `adversarial_suite.py --run` on a failing target, or `cargo test | tee` on a
# failing determinism run.
producer() { echo "adversarial output line"; return 1; }

# 1. WITHOUT pipefail (the default GitHub shell, the bug): the pipeline exits with
#    tee's 0, so the producer's failure is masked.
set +o pipefail
producer | tee "$TMP/out.txt" >/dev/null
rc_masked=$?
set -o pipefail
[[ "$rc_masked" -eq 0 ]] || fail "control setup wrong: expected a masked 0 without pipefail, got $rc_masked"
echo "OK: without pipefail, a failing producer is masked to exit 0 (the #2352 bug)."

# 2. WITH pipefail (the fix, as every piped workflow step now sets): the producer's
#    exit 1 propagates and the pipeline fails.
if producer | tee "$TMP/out.txt" >/dev/null; then
  fail "with pipefail, a failing producer did NOT fail the pipeline — the fix is not load-bearing"
fi
echo "OK: with pipefail, a failing producer fails the pipeline (the fix)."

# 3. And a succeeding producer must still pass under pipefail (no false red).
if ! ( echo ok | tee "$TMP/out.txt" >/dev/null ); then
  fail "with pipefail, a succeeding producer failed — that would be a false red"
fi
echo "OK: with pipefail, a succeeding producer passes."

# 4. Guard the WORKFLOW itself, so this stays a real regression alarm rather than a
#    demonstration: every piped `| tee` step in adversarial-tests.yml must set
#    `set -o pipefail`. Removing pipefail from a piped step (while keeping the tee)
#    makes the count fall short and turns this probe RED.
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WF="$REPO_ROOT/.github/workflows/adversarial-tests.yml"
if [[ -f "$WF" ]]; then
  # Count COMMAND lines only (leading whitespace, not `#` comments), so a mention
  # of `set -o pipefail` in a comment cannot inflate the count and mask a real
  # removal.
  tee_steps=$(grep -cE '^[[:space:]]+[^#].*\| tee ' "$WF" || true)
  pipefail_steps=$(grep -cE '^[[:space:]]+set -o pipefail([[:space:]]|$)' "$WF" || true)
  if (( tee_steps > pipefail_steps )); then
    fail "adversarial-tests.yml has $tee_steps piped '| tee' step(s) but only $pipefail_steps 'set -o pipefail' — a piped step can mask a failure (#2352)"
  fi
  echo "OK: workflow sets pipefail for every piped step ($pipefail_steps pipefail >= $tee_steps tee)."
else
  fail "adversarial-tests.yml not found at $WF — cannot verify the piped steps"
fi

echo "PROBE PASSED: the adversarial suite's piped steps fail closed only with pipefail, and the workflow sets it on every piped step."
