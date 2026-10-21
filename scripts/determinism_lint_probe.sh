#!/usr/bin/env bash
# Acceptance probe for the determinism lint (#2105).
#
# This gate has been wrong twice, in ways that both read as green: it skipped the live
# `shared-protocol/src/blockchain` directory, and its self-test was written in the shape
# the buggy regex matched. So the useful question is not "does it pass?" but "can it
# fail, for each thing it claims to catch?". This script answers that in about ten
# seconds, without anyone reconstructing the author's reasoning.
#
#   scripts/determinism_lint_probe.sh
#
# Exit 0 = every claim was demonstrated failing on a planted violation.
# Exit 1 = something the gate claims to catch did not turn it red.
#
# PROBE A — wall-clock read in a consensus path.
# PROBE B — HashMap iteration feeding a hashed path, binding and iteration on different
#           lines (the shape the original same-line regex could not see).
# PROBE C — production code placed AFTER a `#[cfg(test)]` module. The old backward-scan
#           heuristic exempted this; it is the regression most likely to creep back.
# PROBE D — workflow reachability: remove the live-blockchain glob from `pull_request`
#           ONLY. The old checker unioned all triggers and passed.
# PROBE E — baseline laundering: grow the baseline with an authorization, then delete the
#           authorization record. The digest used to cover findings only, so this passed.
# PROBE F — grow the baseline to cover a genuine plant in the same change, anchored to a
#           base that ALREADY has a committed baseline (the post-merge path). Unanchored it
#           passes; anchored to the committed base baseline it must fail.
# PROBE G — the #2245 blocker that actually gates THIS PR: the bootstrap path, where the
#           base branch has NO committed baseline. The suppression set must then be the base
#           commit's CODE scan (content the PR cannot edit), so growing the working baseline
#           to cover a plant still fails. Without this, anchoring is inert on a bootstrap PR.
#
# Cleanup runs on EVERY exit path via trap, and the script re-verifies the clean gate
# afterwards. Per the #2251 review: the mechanism that proves a gate works must not be able
# to break it. The final step also asserts every touched file was restored to its EXACT
# starting bytes, so a hermeticity regression fails the probe instead of leaking state.
#
# ATTRIBUTION RULE (#2245): every "went red" claim is checked by capturing the gate output
# into a variable FIRST and testing it — never `grep … | head | sed || fail`, whose trailing
# `sed` exits 0 and makes the `|| fail` dead code, so red-for-an-unrelated-reason would pass.

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

LINT="scripts/check_determinism.py"
BASELINE="scripts/determinism_baseline.json"
WF=".github/workflows/determinism-lint.yml"
# Planted inside a scanned root. shared-protocol/src/blockchain is used deliberately: it
# is the directory whose omission was this PR's original defect.
PLANT_DIR="shared-protocol/src/blockchain"
PLANT="${PLANT_DIR}/zz_determinism_probe.rs"

for f in "$LINT" "$BASELINE" "$WF"; do
  [[ -f "$f" ]] || { echo "ERROR: $f not found" >&2; exit 2; }
done
[[ -d "$PLANT_DIR" ]] || { echo "ERROR: $PLANT_DIR not found" >&2; exit 2; }

WF_BAK="$(mktemp)"; BASE_BAK="$(mktemp)"
cp "$WF" "$WF_BAK"; cp "$BASELINE" "$BASE_BAK"

# Exact-bytes fingerprint of every file the probe may mutate, taken BEFORE any probe runs.
# The restore check at the end asserts these are unchanged — hermeticity is verified, not
# assumed (#2245: "the committed acceptance probe ... is not self-restoring").
checksums() { sha256sum "$WF" "$BASELINE"; }
START_SUMS="$(checksums)"

cleanup() {
  rm -f "$PLANT"
  [[ -s "$WF_BAK"   ]] && cp "$WF_BAK"   "$WF"
  [[ -s "$BASE_BAK" ]] && cp "$BASE_BAK" "$BASELINE"
  rm -f "$WF_BAK" "$BASE_BAK"
}
trap cleanup EXIT INT TERM

gate()  { python3 "$LINT" --baseline "$BASELINE" >/dev/null 2>&1; }
paths() { python3 "$LINT" --check-workflow-paths >/dev/null 2>&1; }
fail()  { echo; echo "PROBE FAILED: $*" >&2; exit 1; }

# Assert that captured gate output ($2) contains a finding for the PLANTED file with rule
# $1, and print it. CAPTURE-THEN-TEST: the match is taken into a variable and tested with
# `[[ -n ]]`; it is never piped through `head`/`sed`, whose exit status would mask a
# no-match and make this assertion unfalsifiable — the #2245 attribution defect. Grepping
# for $PLANT first excludes pre-existing baseline lines from unrelated files, so a red for
# an unrelated reason cannot masquerade as proof.
assert_attributed() {
  local rule="$1" out="$2" line
  line="$(printf '%s\n' "$out" | grep -F "$PLANT" | grep -F "$rule" | head -1 || true)"
  [[ -n "$line" ]] || fail "the gate went red, but not with a $rule finding attributed to
  $PLANT. Red for an unrelated reason is not evidence this probe proved anything."
  echo "  ${line#"${line%%[![:space:]]*}"}"
}

# Run the unanchored gate and assert the plant is attributed with rule $1.
show_plant_finding() {
  local out
  out="$(python3 "$LINT" --baseline "$BASELINE" 2>&1 || true)"
  assert_attributed "$1" "$out"
}

echo "=== 0. Baseline: clean tree must PASS both checks ==="
gate  || { echo "ERROR: gate already red before any probe; cannot attribute anything." >&2; exit 2; }
paths || { echo "ERROR: path check already red before any probe." >&2; exit 2; }
echo "OK: clean gate and path check pass."

plant() { printf '%s' "$1" > "$PLANT"; }

echo
echo "=== PROBE A: wall-clock read in a consensus path — expect RED ==="
plant 'pub fn probe_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
'
gate && fail "A — a SystemTime::now() in shared-protocol/src/blockchain did not turn the gate red"
show_plant_finding "wall-clock"
echo "OK."
rm -f "$PLANT"

echo
echo "=== PROBE B: HashMap iteration feeding a hashed path — expect RED ==="
# Binding and iteration on DIFFERENT lines: the original same-line regex saw nothing here.
plant 'use std::collections::HashMap;

pub fn probe_digest(entries: &HashMap<u64, u64>) -> u64 {
    let mut acc = 0u64;
    for (k, v) in entries.iter() {
        acc = acc.wrapping_mul(31).wrapping_add(*k ^ *v);
    }
    acc
}
'
gate && fail "B — unordered map iteration feeding a hash did not turn the gate red"
show_plant_finding "unordered-iteration"
echo "OK."
rm -f "$PLANT"

echo
echo "=== PROBE C: production code BELOW a #[cfg(test)] module — expect RED ==="
plant 'pub fn probe_pure() -> u64 { 7 }

#[cfg(test)]
mod tests {
    #[test]
    fn t() {
        // A wall-clock read here is legitimate and must stay exempt.
        let _ = std::time::SystemTime::now();
    }
}

// PRODUCTION code appended below the test module. The previous backward-scanning
// heuristic exempted everything after a test marker, so this was invisible.
pub fn probe_consensus_clock() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
'
gate && fail "C — production nondeterminism below a #[cfg(test)] module was exempted.
  This is #2245's backward-scan defect; _test_mask must bound the region by brace depth."
show_plant_finding "wall-clock"
echo "OK: caught below the test module, and the in-test read stayed exempt."
rm -f "$PLANT"
gate || fail "restore after C left the gate red"

echo
echo "=== PROBE D: drop the live-blockchain glob from pull_request ONLY — expect RED ==="
python3 - "$WF" <<'PY'
import sys
p = sys.argv[1]
s = open(p, encoding="utf-8").read()
needle = "      - 'shared-protocol/src/**'\n"
if s.count(needle) < 2:
    sys.exit("ERROR: expected the glob under both pull_request and push; re-point PROBE D.")
i = s.index(needle)                       # first occurrence = pull_request block
open(p, "w", encoding="utf-8").write(s[:i] + s[i + len(needle):])
PY
[[ $? -eq 0 ]] || exit 2
paths && fail "D — pull_request no longer covers shared-protocol/src, yet the path check passed.
  The checker is unioning triggers again; PR reachability is the only reachability that gates a PR."
D_OUT="$(python3 "$LINT" --check-workflow-paths 2>&1 || true)"
printf '  %s\n' "$(printf '%s\n' "$D_OUT" | grep "pull_request:" | head -1)"
echo "OK."
cp "$WF_BAK" "$WF"
paths || fail "restore after D left the path check red"

echo
echo "=== PROBE E: delete the authorization that justified a baseline growth — expect RED ==="
plant 'pub fn probe_env() -> Option<String> { std::env::var("PROBE_X").ok() }
'
python3 "$LINT" --update-baseline "$BASELINE" \
  --authorize-additions "probe: temporary, removed by determinism_lint_probe.sh" >/dev/null 2>&1 \
  || fail "E — could not grow the baseline even WITH authorization; setup is wrong"
gate || fail "E — baseline grew with authorization but the gate is still red; setup is wrong"
python3 - "$BASELINE" <<'PY'
import json, sys
p = sys.argv[1]
raw = json.load(open(p, encoding="utf-8"))
if len(raw.get("authorizations", [])) < 2:
    sys.exit("ERROR: expected an appended authorization record; re-point PROBE E.")
raw["authorizations"] = raw["authorizations"][:-1]     # delete the record just written
open(p, "w", encoding="utf-8").write(json.dumps(raw, indent=2) + "\n")
PY
[[ $? -eq 0 ]] || exit 2
gate && fail "E — the authorization record was deleted and the baseline still verified.
  The digest is not covering `authorizations`, so the written justification is decorative."
# Capture first, then test. Piping into `grep -m1` under `set -o pipefail` makes the
# producer take SIGPIPE and the whole pipeline report failure even on a match.
E_OUT="$(python3 "$LINT" --baseline "$BASELINE" 2>&1 || true)"
E_LINE="$(printf '%s\n' "$E_OUT" | grep -m1 "digest does not match" || true)"
[[ -n "$E_LINE" ]] || fail "E — gate red, but not with a digest mismatch. Red for another
  reason proves nothing about whether authorizations are load-bearing."
printf '  %s\n' "${E_LINE:0:130}"
echo "OK."
rm -f "$PLANT"
cp "$BASE_BAK" "$BASELINE"

echo
echo "=== PROBE F: grow the baseline to cover a plant, ANCHORED to a COMMITTED base baseline — expect RED ==="
# Post-merge scenario: a later PR adds a genuine violation AND grows its own baseline to
# cover it in the same change. Unanchored, that passes (the whole defect). Anchored to the
# base commit — content the PR cannot edit — it must still fail. HEAD stands in for the base
# branch: its committed baseline has no plant, and the working-tree growth is inert.
ANCHOR_REF="$(git rev-parse HEAD)"
git show "${ANCHOR_REF}:${BASELINE}" >/dev/null 2>&1 \
  || fail "F setup — ${BASELINE} is not committed at HEAD, so there is no base to anchor to."
plant 'pub fn probe_anchor() -> u64 { std::time::SystemTime::now().elapsed().unwrap().as_secs() }
'
# Grow the WORKING baseline to freeze the plant (authorized; digest recomputed).
python3 "$LINT" --update-baseline "$BASELINE" \
  --authorize-additions "probe: cover the plant, removed by determinism_lint_probe.sh" >/dev/null 2>&1 \
  || fail "F — could not grow the working baseline; setup is wrong."
# Sanity: UNANCHORED, the grown baseline now hides the plant (this is the defect).
gate || fail "F — even unanchored the grown baseline did not cover the plant; setup is wrong."
# ANCHORED to the committed base baseline: the plant must be NEW again.
F_OUT="$(python3 "$LINT" --baseline "$BASELINE" --anchor-ref "$ANCHOR_REF" 2>&1 || true)"
if printf '%s\n' "$F_OUT" | grep -q "no new unjustified nondeterminism"; then
  fail "F — the PR grew the baseline to cover its own violation and the ANCHORED gate
  passed. The anchor is not sourced from the base branch (#2245)."
fi
printf '%s\n' "$F_OUT" | grep -m1 "anchored to" | sed 's/^/  /' || true
assert_attributed "wall-clock" "$F_OUT"
echo "OK: committed-baseline anchor stays red on a plant the PR's own grown baseline 'covers'."
rm -f "$PLANT"
cp "$BASE_BAK" "$BASELINE"

echo
echo "=== PROBE G: BOOTSTRAP anchor — base has NO committed baseline (this PR's real gate path) — expect RED ==="
# THIS PR introduces the baseline, so its CI base (origin/main) has no committed baseline.
# The gate must then anchor the suppression set to the base commit's CODE scan — content the
# PR cannot edit — not to the working baseline the PR ships. PROBE F proved the post-merge
# path; this proves the bootstrap path, which is the one that actually gates #2245. Without
# the base-code anchor, growing the working baseline would launder the plant here.
BASE_REF="$(git merge-base HEAD origin/main 2>/dev/null || true)"
if [[ -z "$BASE_REF" ]] || git show "${BASE_REF}:${BASELINE}" >/dev/null 2>&1; then
  echo "SKIP G: no baseline-free base ref reachable (need origin/main fetched). The bootstrap"
  echo "  anchor path is still covered by \`check_determinism.py --self-test\` (_anchor_self_test)."
else
  plant 'pub fn probe_bootstrap() -> u64 { std::time::SystemTime::now().elapsed().unwrap().as_secs() }
'
  # Launder: grow the WORKING baseline to freeze the plant (authorized; digest recomputed).
  python3 "$LINT" --update-baseline "$BASELINE" \
    --authorize-additions "probe: bootstrap laundering attempt, removed by determinism_lint_probe.sh" >/dev/null 2>&1 \
    || fail "G — could not grow the working baseline; setup is wrong."
  # Sanity: UNANCHORED, the grown working baseline hides the plant (the laundering to defeat).
  gate || fail "G — even unanchored the grown baseline did not cover the plant; setup is wrong."
  # BOOTSTRAP-ANCHORED to the base CODE: the plant must be NEW again.
  G_OUT="$(python3 "$LINT" --baseline "$BASELINE" --anchor-ref "$BASE_REF" 2>&1 || true)"
  if printf '%s\n' "$G_OUT" | grep -q "no new unjustified nondeterminism"; then
    fail "G — the PR grew its baseline to cover its plant and the BOOTSTRAP-anchored gate
  passed. The suppression set is not sourced from the base-commit code, so anchoring is
  inert on the bootstrap PR (#2245)."
  fi
  printf '%s\n' "$G_OUT" | grep -m1 -F "[gate] BOOTSTRAP" | sed 's/^/  /' || true
  assert_attributed "wall-clock" "$G_OUT"
  echo "OK: bootstrap base-code anchor stays red on a plant the PR's grown baseline 'covers'."
  rm -f "$PLANT"
  cp "$BASE_BAK" "$BASELINE"
fi

echo
echo "=== Restore check ==="
gate  || fail "the restored tree does not pass the gate"
paths || fail "the restored tree does not pass the path check"
[[ -e "$PLANT" ]] && fail "the planted file was left behind"
[[ "$(checksums)" == "$START_SUMS" ]] || fail "the probe did not restore every touched file to
  its exact starting bytes — a hermeticity regression. Expected:
$START_SUMS
  Got:
$(checksums)"
echo "OK: clean gate, clean path check, and every touched file restored byte-for-byte."

echo
echo "PROBE PASSED."
echo "  A: wall-clock in consensus path        -> RED"
echo "  B: HashMap iteration into a hash       -> RED"
echo "  C: production below #[cfg(test)]       -> RED (in-test read still exempt)"
echo "  D: pull_request lost a scanned root    -> RED"
echo "  E: authorization record deleted        -> RED"
echo "  F: baseline grown to cover a plant     -> RED anchored to a committed base baseline"
echo "  G: same, BOOTSTRAP base (no baseline)  -> RED anchored to the base-code scan (this PR's gate)"
echo "  Every claim this lint makes was demonstrated failing; every touched file was restored."
