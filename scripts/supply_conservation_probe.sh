#!/usr/bin/env bash
# Acceptance probe for INV-SUPPLY-1, the global supply-conservation identity
# (core4#2097, PR #2250).
#
# An identity that cannot fail proves nothing. If both sides were derived from
# the same accumulator, `check_supply` would pass on a chain that had just
# minted a billion coins from nowhere — the #2302 shape, where a gate compared
# re-exports to themselves, in arithmetic form. So before trusting any green run
# of the supply tests, prove the mechanism is capable of failing: plant a
# deliberate supply violation in PRODUCTION code, assert the invariant catches
# it, restore the file.
#
#   scripts/supply_conservation_probe.sh
#
# Exit 0 = both planted defects were caught (the invariant and its enforcement
#          path are load-bearing).
# Exit 1 = at least one planted defect went undetected, which means the green
#          runs above it establish nothing.
# Exit 2 = the probe could not run (missing anchor text), so it proved nothing
#          either way — fix the anchors rather than trusting the silence.
#
# Two stages, because supply conservation has two independently breakable parts:
#
#   A. DETECTION — drop the mint journal entry on the real `ApplyReward` path
#      while leaving its account credit intact. Value then exists on the
#      right-hand side that the left-hand side never authorized, so the
#      committed positive-path test must FAIL with a signed delta. This is the
#      experiment the #2250 reviewer ran by hand; scripting it makes it
#      repeatable by anyone later.
#
#   B. ENFORCEMENT — drop the publish into `SharedSupplyInvariant`. Detection
#      still latches into `State::supply_violation`, but nothing reaches the
#      supervisor that halts the node, so the registry test must FAIL. A latch
#      no supervisor can see is the #2056 failure class: an error that is
#      logged and swallowed.
#
# Cleanup runs on EVERY exit path via trap, including Ctrl-C and a failed cargo
# invocation. The mechanism that proves a gate works must not be able to break
# the gate (the lesson from the #2251 review, where a self-test's cleanup left a
# directory behind and turned the real gate red).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STATE_FILE="${REPO_ROOT}/shared-protocol/src/blockchain/state.rs"
BACKUP="$(mktemp)"

if [[ ! -f "${STATE_FILE}" ]]; then
  echo "ERROR: ${STATE_FILE} not found" >&2
  exit 2
fi

cleanup() {
  if [[ -s "${BACKUP}" ]]; then
    cp "${BACKUP}" "${STATE_FILE}"
    # Reset mtime to now so the restored (content-correct) file does not carry a
    # far-future timestamp into a later real build.
    touch "${STATE_FILE}"
  fi
  rm -f "${BACKUP}"
}
trap cleanup EXIT INT TERM

cp "${STATE_FILE}" "${BACKUP}"

# ---------------------------------------------------------------------------
# force_recompile — defeat cargo's mtime fingerprint (core4#2088 flake class).
#
# cargo decides whether to rebuild a crate by comparing each source file's mtime
# against the mtime it recorded at the last successful build. A rapid
# mutate -> build -> restore -> mutate cycle can produce two writes whose mtimes
# land in the same filesystem tick (1 s granularity on some overlay/WSL2
# filesystems), so the second mutation looks "unchanged" and cargo tests a stale
# binary — the probe then passes or fails for the wrong reason, intermittently.
#
# Bumping the file to a strictly-increasing FAR-FUTURE mtime removes the race in
# both directions: the file is always newer than any real fingerprint cargo
# recorded this run, and each stage is strictly newer than the previous one, so
# cargo is forced to recompile before every `cargo test` below. Content is
# untouched; git tracks content, not mtime. `cleanup` resets the mtime to now.
# ---------------------------------------------------------------------------
MTIME_MARK=0
force_recompile() {
  MTIME_MARK=$((MTIME_MARK + 1))
  local stamp
  stamp="$(printf '2100-01-01 00:00:%02d' "${MTIME_MARK}")"
  touch -d "${stamp}" "${STATE_FILE}" 2>/dev/null || touch "${STATE_FILE}"
}

# ---------------------------------------------------------------------------
# plant <python-expression-file> — replace exact anchor text, or exit 2.
# ---------------------------------------------------------------------------
plant() {
  local anchor="$1" replacement="$2" label="$3"
  cp "${BACKUP}" "${STATE_FILE}"
  ANCHOR="${anchor}" REPLACEMENT="${replacement}" LABEL="${label}" \
    python3 - "${STATE_FILE}" <<'PY'
import os, sys
path = sys.argv[1]
anchor = os.environ["ANCHOR"]
replacement = os.environ["REPLACEMENT"]
label = os.environ["LABEL"]
text = open(path).read()
if text.count(anchor) != 1:
    sys.stderr.write(
        f"\nERROR: probe anchor for '{label}' matched {text.count(anchor)} times, expected 1.\n"
        "The production code moved. This probe proved NOTHING; update the anchor.\n"
    )
    sys.exit(2)
open(path, "w").write(text.replace(anchor, replacement))
PY
  # Force cargo to rebuild against the just-planted source, not a stale artifact.
  force_recompile
}

# ---------------------------------------------------------------------------
# expect_failure <test-filter> <needle> <stage-label> <diagnosis>
# ---------------------------------------------------------------------------
expect_failure() {
  local filter="$1" needle="$2" label="$3" diagnosis="$4"
  local output status
  set +e
  output="$(cd "${REPO_ROOT}" && cargo test --locked -p wrfcoin-shared-protocol \
    --lib "${filter}" 2>&1)"
  status=$?
  set -e

  if [[ ${status} -eq 0 ]]; then
    echo "${output}" | tail -15
    printf '\nFAIL [%s]: the planted defect was NOT caught.\n\n%s\n' "${label}" "${diagnosis}" >&2
    exit 1
  fi

  if ! grep -qF "${needle}" <<<"${output}"; then
    echo "${output}" | tail -25
    printf '\nFAIL [%s]: the test failed, but not on its planted assertion.\n' "${label}" >&2
    printf 'Expected to find %q in the output. The run died for an unrelated\n' "${needle}" >&2
    printf 'reason (compile error, panic elsewhere) and proved nothing.\n' >&2
    exit 1
  fi

  echo "[supply-probe] OK [${label}] — caught:"
  grep -m1 -F "${needle}" <<<"${output}" | sed 's/^/[supply-probe]   /'
}

# ---------------------------------------------------------------------------
# Stage A — DETECTION: an unjournalled mint on the production reward path.
# ---------------------------------------------------------------------------
echo "[supply-probe] stage A: dropping the ApplyReward mint journal entry"
plant \
'                self.supply_authorized_minting = self
                    .supply_authorized_minting
                    .checked_add(amount.into_inner())
                    .ok_or(StateError::ArithmeticOverflow)?;' \
'                // PLANTED BY scripts/supply_conservation_probe.sh — removed
                // automatically. If you are reading this in a committed file,
                // the cleanup trap failed; restore the journal update.
                let _ = amount.into_inner();' \
'stage-A mint journal'

expect_failure \
  'supply_enforcement_tests::authorized_reward_keeps_the_identity_balanced' \
  'INV-SUPPLY-1 violated' \
  'A: detection' \
  'A reward credited 250 units to an account with no matching journal entry and
the identity still balanced. That means the left-hand side is not independently
derived from the issuance journal — it is tracking the account sum, so it can
never disagree with it. Every green supply test below this is vacuous
(core4#2097).'

# ---------------------------------------------------------------------------
# Stage B — ENFORCEMENT: detection that never reaches the supervisor.
# ---------------------------------------------------------------------------
echo "[supply-probe] stage B: dropping the publish into SharedSupplyInvariant"
plant \
'                if let Some(handle) = &self.supply_invariant {
                    handle.observe_violation(detail.clone());
                }' \
'                // PLANTED BY scripts/supply_conservation_probe.sh — removed
                // automatically. If you are reading this in a committed file,
                // the cleanup trap failed; restore the publish.
                let _ = &self.supply_invariant;' \
'stage-B supervisor publish'

expect_failure \
  'supply_enforcement_tests::a_constructed_violation_reaches_the_supervisor_registry' \
  'the supervisor must observe the constructed violation' \
  'B: enforcement' \
  'A detected violation did not reach the stateful-invariant registry, yet the
test passed anyway. That means the registry test is not actually reading the
handle the state publishes to, so nothing proves a supply breach can halt the
node — it would be logged and swallowed (core4#2056 failure class).'

echo "[supply-probe] both planted defects were caught; restoring ${STATE_FILE##*/}"
