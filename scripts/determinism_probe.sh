#!/usr/bin/env bash
# Acceptance probe for the determinism matrix's anchor check (#2104).
#
# A gate that only compares the five matrix legs against EACH OTHER cannot fail
# when a deterministic semantic change shifts all five identically. This probe
# proves the anchor check added in #2243 closes that hole, by planting exactly
# such a change and asserting BOTH halves of the claim:
#
#   1. cross-leg-only comparison still reports PASS  (the hole is real)
#   2. anchored comparison reports FAIL              (the anchor closes it)
#
# Asserting only (2) would be weaker: a mutation that also broke cross-leg
# agreement would satisfy it without demonstrating anything about common mode.
# Both assertions together are what make this evidence about the anchor
# specifically.
#
#   scripts/determinism_probe.sh
#
# Exit 0 = the anchor caught a common-mode shift that cross-leg comparison missed.
# Exit 1 = it did not, which means the determinism gate is not load-bearing and
#          every green run below it is compatible with silently changed roots.
#
# The planted change is in PRODUCTION state-transition code, not in the fixture
# or the harness: `StateTransition::ApplyReward` credits the stake-locked and
# spendable portions of a reward to each other's field. That is a plausible real
# regression (a swapped pair of assignments in a reward split), it is fully
# deterministic so every platform computes the same wrong root, and it is
# type-identical so it always compiles.
#
# Cleanup runs on EVERY exit path via trap, and the script verifies the restored
# tree reproduces the anchor before exiting. Per the #2251 review: the mechanism
# that proves a gate works must not be able to break it.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${REPO_ROOT}/shared-protocol/src/blockchain/state.rs"
ANCHOR="${REPO_ROOT}/tests/integration/fixtures/determinism/expected_roots.json"
COMPARE="${REPO_ROOT}/scripts/compare_state_roots.py"
BACKUP="$(mktemp)"
WORK="$(mktemp -d)"

for f in "${TARGET}" "${ANCHOR}" "${COMPARE}"; do
  [[ -f "${f}" ]] || { echo "ERROR: ${f} not found" >&2; exit 2; }
done

cleanup() {
  if [[ -s "${BACKUP}" ]]; then
    cp "${BACKUP}" "${TARGET}"
  fi
  rm -f "${BACKUP}"
  rm -rf "${WORK}"
}
trap cleanup EXIT INT TERM

cp "${TARGET}" "${BACKUP}"

emit() {
  # $1 = destination file. Build quietly; surface real build errors.
  if ! cargo run --locked -q -p test_server --example emit_roots >"$1" 2>"${WORK}/build.err"; then
    echo "ERROR: emit_roots failed to build/run:" >&2
    cat "${WORK}/build.err" >&2
    exit 2
  fi
}

echo "=== 0. Baseline: unmutated tree must reproduce the anchor ==="
emit "${WORK}/base.json"
if ! python3 - "${WORK}/base.json" "${ANCHOR}" <<'PY'
import json, sys
a = json.load(open(sys.argv[1])); b = json.load(open(sys.argv[2]))
sys.exit(0 if a == b else 1)
PY
then
  echo "ERROR: the tree does not reproduce expected_roots.json before any mutation." >&2
  echo "  The probe cannot attribute anything to its own change. Aborting." >&2
  exit 2
fi
echo "OK: baseline roots match the committed anchor."

echo
echo "=== 1. Planting a deterministic semantic change in ApplyReward ==="
python3 - "${TARGET}" <<'PY'
import sys

path = sys.argv[1]
src = open(path, encoding="utf-8").read()

# Swap the credit targets of the reward split: stake_locked receives the
# spendable portion and vice versa. Type-identical, so it always compiles.
old = """                account.stake_locked = account
                    .stake_locked
                    .checked_add(to_lock)
                    .ok_or(StateError::ArithmeticOverflow)?;
                account.spendable_balance = account
                    .spendable_balance
                    .checked_add(to_spendable)
                    .ok_or(StateError::ArithmeticOverflow)?;"""

new = """                // PLANTED BY scripts/determinism_probe.sh — restored automatically.
                // If you are reading this in a committed file, the cleanup trap
                // failed; restore the two arguments to to_lock / to_spendable.
                account.stake_locked = account
                    .stake_locked
                    .checked_add(to_spendable)
                    .ok_or(StateError::ArithmeticOverflow)?;
                account.spendable_balance = account
                    .spendable_balance
                    .checked_add(to_lock)
                    .ok_or(StateError::ArithmeticOverflow)?;"""

if old not in src:
    sys.exit("ERROR: ApplyReward split not found in state.rs — the probe's anchor "
             "text has drifted and must be re-pointed before this proves anything.")

open(path, "w", encoding="utf-8").write(src.replace(old, new, 1))
print("planted: ApplyReward credits to_lock/to_spendable to swapped fields")
PY

echo
echo "=== 2. Emitting roots from the mutated tree, as five identical legs ==="
# Every leg on every platform computes the SAME wrong roots — that is what makes
# this common-mode. Copying one emission to five leg files models exactly that;
# the legs are identical by construction, as they would be in CI.
emit "${WORK}/mutated.json"
for leg in a b c d e; do
  cp "${WORK}/mutated.json" "${WORK}/roots-leg-${leg}.json"
done

if python3 - "${WORK}/mutated.json" "${WORK}/base.json" <<'PY'
import json, sys
sys.exit(0 if json.load(open(sys.argv[1])) == json.load(open(sys.argv[2])) else 1)
PY
then
  echo "ERROR: the planted change did not alter any state root." >&2
  echo "  Nothing is being tested. Re-point the mutation at load-bearing code." >&2
  exit 2
fi
echo "OK: the planted change moved the roots."

echo
echo "=== 3. Cross-leg-only comparison (the OLD behaviour) — expect PASS ==="
if python3 "${COMPARE}" --no-expected "${WORK}"/roots-leg-*.json; then
  echo "CONFIRMED: five legs agree with each other. The old gate was GREEN here."
else
  echo "ERROR: cross-leg comparison failed on identical legs." >&2
  echo "  The probe cannot demonstrate a COMMON-MODE miss if the legs disagree." >&2
  exit 1
fi

echo
echo "=== 4. Anchored comparison (the NEW behaviour) — expect FAIL ==="
set +e
OUT="$(python3 "${COMPARE}" --expected "${ANCHOR}" "${WORK}"/roots-leg-*.json 2>&1)"
RC=$?
set -e
echo "${OUT}"

if [[ ${RC} -eq 0 ]]; then
  echo
  echo "PROBE FAILED: the anchored gate stayed GREEN on a common-mode shift." >&2
  echo "  The anchor is not load-bearing." >&2
  exit 1
fi

if ! grep -q "COMMON-MODE SHIFT" <<<"${OUT}"; then
  echo
  echo "PROBE FAILED: gate went red, but did not diagnose it as a COMMON-MODE SHIFT." >&2
  echo "  It must name this failure mode; 'red for some reason' is not the claim." >&2
  exit 1
fi

echo
echo "=== 5. Restoring and re-verifying ==="
cp "${BACKUP}" "${TARGET}"
emit "${WORK}/restored.json"
if ! python3 "${COMPARE}" --expected "${ANCHOR}" "${WORK}/restored.json" "${WORK}/base.json"; then
  echo "ERROR: the restored tree does not reproduce the anchor." >&2
  echo "  Cleanup left the worktree in a bad state — fix before trusting any run." >&2
  exit 2
fi

echo
echo "PROBE PASSED."
echo "  A deterministic semantic change shifted every leg identically."
echo "  Cross-leg comparison alone:  PASS (would have shipped the regression)"
echo "  Anchored comparison:         FAIL (COMMON-MODE SHIFT, height reported)"
echo "  The anchor is load-bearing."
