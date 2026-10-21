#!/usr/bin/env bash
# Acceptance probe for the Phase 3C consensus gate (#2083).
#
# The gate (tdd-phase3c.yml, TDD-12.2/12.3) is supposed to catch regressions in the
# slashing and reward consensus paths a public earn-loop depends on. It was permanent-red
# for 2+ months on ROTTED tests (commits unsigned against the #1848 fail-closed auth), and
# it ran on no PR at all. Those are fixed. This proves the fixed gate is load-bearing:
# plant a real slashing/reward violation in PRODUCTION consensus code and require the gate
# to go red, so nobody has to trust that a green run means anything.
#
#   scripts/phase3c_probe.sh
#
# Exit 0 = both planted violations turned their gate test red.
# Exit 1 = a violation slipped through green — the gate is not load-bearing.
# Exit 2 = could not attribute (gate red before a probe, anchor moved, missing file).
#
# PROBE A — REWARD. Force the per-oracle reputation factor to 1.0 in the reward path of
#           `proof_of_publishing`, so reputation no longer scales the reward. TDD-12.3
#           (`tdd_6_2_reward_integration_high_reputation_out_earns_low_reputation`) asserts
#           high-reputation out-earns low; with reputation flattened, high == low, so it
#           must fail.
# PROBE B — SLASHING. Disable the low-quality slash trigger. TDD-12.2
#           (`tdd_6_1_slashing_integration_low_quality_streak`) asserts a slashing event is
#           recorded after the streak threshold; with the slash never applied, it must fail.
#
# Both mutations are in the consensus block path the gate tests actually exercise. (An
# earlier draft mutated `RewardConfig::apply_adjustments`; the probe caught that the reward
# test does NOT reach that helper — its differential comes from `rep_factor` here. That
# miss is exactly what this probe exists to prevent: a mutation the gate ignores proves
# nothing.) Planted temporarily and restored on EVERY exit via trap; the script re-verifies
# the clean gate afterwards. Per the #2251 review: the mechanism that proves a gate works
# must not be able to break it.
#
# DETERMINISM (#2088 review). The probe rewrites a production source file, builds, restores
# it, and builds again in rapid succession. Cargo decides whether to recompile from the
# source file's mtime, and that comparison is only as fine-grained as the filesystem's mtime
# resolution. On a rapid mutate -> build -> restore cycle the mutated and restored writes
# could land in the same mtime tick, so cargo reused a STALE artifact and the test result no
# longer matched the source on disk. That produced exactly the reported flakiness — attempt
# 1 exited with `restore after A left the reward gate red` (restored source, stale MUTATED
# binary), and a fresh attempt 2 exited with `slashing gate already red before any probe` (a
# mutated artifact from a prior interrupted run surviving into the next run's baseline).
#
# The fix is `force_rebuild` — `cargo clean -p wrfcoin-shared-protocol` after every mutation
# AND every restore, before the build that must observe it. It is filesystem-independent (no
# reliance on mtime granularity at all) rather than a `touch` trick that only holds where the
# clock is fine-grained; a probe runs rarely, so the extra compile time buys determinism the
# `touch` approach cannot guarantee across environments. Cargo output is retained in a log
# and printed on any UNEXPECTED result, so a future failure is diagnosable without rerunning.

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

SRC="shared-protocol/src/consensus/proof_of_publishing.rs"
[[ -f "$SRC" ]] || { echo "ERROR: $SRC not found" >&2; exit 2; }

BAK="$(mktemp)"
GATE_LOG="$(mktemp)"
cp "$SRC" "$BAK"
cleanup() {
  [[ -s "$BAK" ]] && cp "$BAK" "$SRC"
  rm -f "$BAK" "$GATE_LOG"
}
trap cleanup EXIT INT TERM

show_gate_log() {
  echo "----- cargo output from the last gate run (tail) -----" >&2
  tail -40 "$GATE_LOG" >&2
  echo "------------------------------------------------------" >&2
}
fail() { echo; echo "PROBE FAILED: $*" >&2; show_gate_log; exit 1; }

# `cargo clean -p` forces a from-clean recompile of the shared-protocol lib and its test
# targets, so the next `gate` reflects the source exactly as it is on disk — see the
# DETERMINISM note above.
force_rebuild() { cargo clean -p wrfcoin-shared-protocol >/dev/null 2>&1; }

# Run one Phase 3C gate test; keep its combined output in $GATE_LOG for diagnosis. $@ is
# passed straight to cargo test (e.g. `--test reward_integration tdd_6_2_...`).
gate() { cargo test --locked -p wrfcoin-shared-protocol "$@" >"$GATE_LOG" 2>&1; }

echo "=== 0. Baseline: the Phase 3C Rust gate must PASS on the clean tree ==="
# Clean first, so an artifact left mutated by a prior interrupted run cannot make the clean
# tree look red (attempt-2 failure mode in the #2088 review).
force_rebuild
gate --test slashing_integration tdd_6_1_slashing_integration_low_quality_streak \
  || { echo "ERROR: slashing gate already red before any probe; cannot attribute." >&2; show_gate_log; exit 2; }
gate --test reward_integration tdd_6_2_reward_integration_high_reputation_out_earns_low_reputation \
  || { echo "ERROR: reward gate already red before any probe." >&2; show_gate_log; exit 2; }
echo "OK: TDD-12.2 (slashing) and TDD-12.3 (reward) both pass clean."

echo
echo "=== PROBE A: flatten the per-oracle reputation factor — expect TDD-12.3 RED ==="
python3 - "$SRC" <<'PY'
import sys
p = sys.argv[1]
s = open(p, encoding="utf-8").read()
old = "            let rep_factor = (raw_rep as f64 / SCORE_SCALE as f64).clamp(0.1, 2.0);"
new = "            let rep_factor = { let _ = raw_rep; 1.0 }; // PLANTED by phase3c_probe.sh — reputation flattened"
if old not in s:
    sys.exit("ERROR: reward anchor moved; re-point PROBE A before trusting this script.")
open(p, "w", encoding="utf-8").write(s.replace(old, new, 1))
PY
[[ $? -eq 0 ]] || exit 2
force_rebuild
if gate --test reward_integration tdd_6_2_reward_integration_high_reputation_out_earns_low_reputation; then
  fail "A — reputation no longer scales the reward, yet TDD-12.3 stayed green. The reward
  gate does not actually verify reputation-weighted earning."
fi
echo "OK: TDD-12.3 red when reputation stops affecting the reward."
cp "$BAK" "$SRC"
force_rebuild
gate --test reward_integration tdd_6_2_reward_integration_high_reputation_out_earns_low_reputation \
  || fail "restore after A left the reward gate red"

echo
echo "=== PROBE B: disable the low-quality slash trigger — expect TDD-12.2 RED ==="
python3 - "$SRC" <<'PY'
import sys
p = sys.argv[1]
s = open(p, encoding="utf-8").read()
old = "                if streak >= quality_policy.low_quality_slash_streak {"
new = "                if false && streak >= quality_policy.low_quality_slash_streak { // PLANTED by phase3c_probe.sh — slash disabled"
if old not in s:
    sys.exit("ERROR: slash anchor moved; re-point PROBE B before trusting this script.")
open(p, "w", encoding="utf-8").write(s.replace(old, new, 1))
PY
[[ $? -eq 0 ]] || exit 2
force_rebuild
if gate --test slashing_integration tdd_6_1_slashing_integration_low_quality_streak; then
  fail "B — the low-quality slash never fires, yet TDD-12.2 stayed green. The slashing gate
  does not actually verify that a low-quality streak is slashed."
fi
echo "OK: TDD-12.2 red when the low-quality slash is disabled."
cp "$BAK" "$SRC"
force_rebuild
gate --test slashing_integration tdd_6_1_slashing_integration_low_quality_streak \
  || fail "restore after B left the slashing gate red"

echo
echo "=== Restore check ==="
gate --test slashing_integration tdd_6_1_slashing_integration_low_quality_streak \
  || fail "restored tree does not pass the slashing gate"
gate --test reward_integration tdd_6_2_reward_integration_high_reputation_out_earns_low_reputation \
  || fail "restored tree does not pass the reward gate"
git diff --quiet -- "$SRC" || fail "planted mutations were not fully restored"
echo "OK: both gate tests pass again; production source restored."

echo
echo "PROBE PASSED."
echo "  A: reputation stops scaling reward -> TDD-12.3 (reward) RED"
echo "  B: low-quality slash disabled      -> TDD-12.2 (slashing) RED"
echo "  The Phase 3C reward/slashing gate is load-bearing."
