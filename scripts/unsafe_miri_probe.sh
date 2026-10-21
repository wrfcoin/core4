#!/usr/bin/env bash
# Acceptance probe for the #2106 unsafe/Miri gates.
#
# Both halves of this story are the kind of gate that passes green forever without
# ever having been shown capable of failing. This script proves each half can fail,
# in about a minute, without anyone reconstructing the author's reasoning.
#
#   scripts/unsafe_miri_probe.sh            # all probes
#   scripts/unsafe_miri_probe.sh --no-miri  # skip C (no nightly/miri available)
#
# Exit 0 = every gate caught its planted violation.
# Exit 1 = a gate stayed green on a real violation, and is not load-bearing.
#
# PROBE A — a bare `unsafe {}` with no `// SAFETY:`, in production code, in a file that
#           already has baselined sites. The basic claim the gate makes.
# PROBE B — RETIRED by #2454, and deliberately not replaced. It swapped the body under a
#           grandfathered opener whose justification was written for reclaiming an FFI
#           CString pointer. That block no longer exists: this PR removes the FFI bridge,
#           so there is no such site to swap and the probe had nothing left to plant.
#           Reported by review of this PR: the script still ANNOUNCED B as passing after
#           its execution section was removed, which is a harness asserting coverage it
#           does not run -- exactly the vacuity this gate exists to catch. The announcement
#           is now A and D only. If a comparable grandfathered site ever reappears, restore
#           a body-swap probe against THAT site rather than resurrecting this text.
# PROBE D — same-change baseline laundering: add a new unjustified unsafe AND a baseline entry
#           that cites a real issue, in one change. Every current-tree check passes; only the
#           ratchet (comparison against the base ref, here HEAD) catches it. This is the
#           round-92 bypass of #2247.
# PROBE C — genuine undefined behaviour (a Stacked Borrows violation) in wrf-primitives,
#           to prove `cargo miri test` actually detects UB rather than merely running.
#           wrf-primitives is used because it is the fast leg: ~0.7s under Miri locally.
#
# Cleanup runs on EVERY exit path via trap, and the script re-verifies the clean gate
# afterwards. Per the #2251 review: the mechanism that proves a gate works must not be
# able to break it.

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

GATE="scripts/check_unsafe_justified.py"
BASELINE="scripts/unsafe_baseline.json"
PRIM="primitives/src/lib.rs"

RUN_MIRI=1
[[ "${1:-}" == "--no-miri" ]] && RUN_MIRI=0

for f in "$GATE" "$BASELINE" "$PRIM"; do
  [[ -f "$f" ]] || { echo "ERROR: $f not found" >&2; exit 2; }
done

PRIM_BAK="$(mktemp)"
cp "$PRIM" "$PRIM_BAK"

cleanup() {
  [[ -s "$PRIM_BAK" ]] && cp "$PRIM_BAK" "$PRIM"
  rm -f "$PRIM_BAK"
}
trap cleanup EXIT INT TERM

gate() { python3 "$GATE" --baseline "$BASELINE" >/dev/null 2>&1; }

fail() { echo; echo "PROBE FAILED: $*" >&2; exit 1; }

echo "=== 0. Baseline: the clean tree must PASS the gate ==="
if ! gate; then
  echo "ERROR: the gate is already red before any probe. Nothing here can be attributed" >&2
  echo "  to the planted violations. Aborting." >&2
  exit 2
fi
echo "OK: clean gate passes."

echo
echo "=== PROBE A: bare unsafe with no // SAFETY: in production code — expect RED ==="
cat >>"$PRIM" <<'PLANT'

// PLANTED BY scripts/unsafe_miri_probe.sh — removed automatically. If you are reading
// this in a committed file, the cleanup trap failed; delete this function.
pub fn probe_planted_bare_unsafe() -> u8 {
    unsafe { core::ptr::read_volatile(&7u8) }
}
PLANT
if gate; then fail "A — a bare unjustified unsafe block did not turn the gate red"; fi
python3 "$GATE" --baseline "$BASELINE" 2>&1 | grep -E "NEW|probe_planted" | head -3
echo "OK: gate red on a new unjustified unsafe."
cp "$PRIM_BAK" "$PRIM"

echo
# PROBE B (body swap inside the grandfathered FFI block) was REMOVED with the FFI
# bridge (#2454). Its subject was a specific `unsafe { CString::from_raw(s) }` in
# consensus-engines/src/ffi.rs; that file is gone and the unsafe baseline is empty,
# so the probe could only ever abort with "the grandfathered FFI block has moved".

echo "=== PROBE D: same-change baseline laundering — expect the RATCHET to go RED ==="
# The round-92 bypass: add a new unjustified unsafe AND a baseline entry citing a real issue,
# in one change. Every current-tree check passes; only comparison with the base ref catches it.
# HEAD (the committed tree, without the plant) stands in for the base ref.
BASE_TREE="$(mktemp -d)"
LAUNDERED_BASELINE="$(mktemp)"
ratchet_cleanup() { cp "$PRIM_BAK" "$PRIM"; rm -rf "$BASE_TREE" "$LAUNDERED_BASELINE"; }
git archive HEAD | tar -x -C "$BASE_TREE" || { echo "ERROR: git archive HEAD failed" >&2; ratchet_cleanup; exit 2; }
cat >>"$PRIM" <<'PLANT'

// PLANTED BY scripts/unsafe_miri_probe.sh — removed automatically.
pub fn probe_launder_via_baseline() -> u8 {
    unsafe { core::ptr::read_volatile(&5u8) }
}
PLANT
# Freeze the new site in a temp baseline with a genuine citation (the laundering).
cp "$BASELINE" "$LAUNDERED_BASELINE"
python3 "$GATE" --update-baseline "$LAUNDERED_BASELINE" >/dev/null 2>&1
python3 - "$LAUNDERED_BASELINE" <<'PY'
import json, sys
p = sys.argv[1]; d = json.load(open(p))
for k, v in d.items():
    if "TODO" in v or "#NNNN" in v:
        d[k] = "grandfathered pending FFI audit (#2106)"   # real citation, still a lie
json.dump(d, open(p, "w"), indent=2)
PY
# Without the ratchet, this laundering passes — that is the whole reason the ratchet exists.
if ! python3 "$GATE" --baseline "$LAUNDERED_BASELINE" >/dev/null 2>&1; then
  ratchet_cleanup
  fail "D setup — the laundered baseline was expected to pass the current-tree-only check"
fi
# With the ratchet (--base-root HEAD), the new grandfathered site must be caught as GROWN.
if python3 "$GATE" --baseline "$LAUNDERED_BASELINE" --base-root "$BASE_TREE" >/dev/null 2>&1; then
  ratchet_cleanup
  fail "D — a baseline entry freezing a NEW unsafe (absent on the base ref) did not turn the
  gate red. Same-change baseline laundering is still open (#2247 review, round 92)."
fi
python3 "$GATE" --baseline "$LAUNDERED_BASELINE" --base-root "$BASE_TREE" 2>&1 | grep -E "GROWN|probe_launder" | head -3
echo "OK: ratchet red on a new site laundered through the baseline."
ratchet_cleanup
if ! gate; then fail "restore after D left the gate red"; fi

if [[ "$RUN_MIRI" -eq 0 ]]; then
  echo
  echo "SKIPPED PROBE C (--no-miri). Gate probes A and D passed."
  exit 0
fi

echo
echo "=== PROBE C: genuine UB in wrf-primitives — expect Miri to REPORT IT ==="
if ! cargo +nightly miri --version >/dev/null 2>&1; then
  echo "ERROR: cargo +nightly miri unavailable. Re-run with --no-miri, or install:" >&2
  echo "  rustup toolchain install nightly --component miri" >&2
  exit 2
fi
cat >>"$PRIM" <<'PLANT'

// PLANTED BY scripts/unsafe_miri_probe.sh — removed automatically. If you are reading
// this in a committed file, the cleanup trap failed; delete this module.
#[cfg(test)]
mod probe_planted_ub {
    #[test]
    fn probe_stacked_borrows_violation() {
        let mut x = 42u8;
        let p = &mut x as *mut u8;
        unsafe {
            let a = &mut *p;
            // Creating `b` invalidates `a` under Stacked Borrows; using `a` afterwards is
            // undefined behaviour that compiles, passes ordinary `cargo test`, and is
            // exactly what Miri exists to catch.
            let b = &mut *p;
            *a += 1;
            *b += 1;
        }
    }
}
PLANT
echo "  running cargo miri test -p wrf-primitives (planted UB) ..."
MIRI_OUT="$(MIRIFLAGS="-Zmiri-disable-isolation -Zmiri-ignore-leaks" \
  cargo +nightly miri test -p wrf-primitives --lib probe_planted_ub 2>&1)"
MIRI_RC=$?
if [[ $MIRI_RC -eq 0 ]]; then
  echo "$MIRI_OUT" | tail -5
  fail "C — Miri exited 0 on deliberate undefined behaviour. It is running but not detecting."
fi
if ! grep -q "Undefined Behavior" <<<"$MIRI_OUT"; then
  echo "$MIRI_OUT" | tail -15
  fail "C — Miri failed, but not with an Undefined Behavior diagnosis. A failure for some
  other reason is not evidence that UB detection works."
fi
grep -m1 "Undefined Behavior" <<<"$MIRI_OUT" | sed 's/^/  /'
echo "OK: Miri detected the planted UB."
cp "$PRIM_BAK" "$PRIM"

echo
echo "=== Restore check ==="
if ! gate; then fail "the restored tree does not pass the clean gate"; fi
echo "OK: clean gate passes again."

echo
echo "PROBE PASSED."
echo "  A: bare unjustified unsafe        -> gate RED"
echo "  D: baseline laundering vs base    -> ratchet RED"
echo "  C: deliberate UB in primitives    -> Miri reported Undefined Behavior"
echo "  All three executed gates are load-bearing (B retired with the FFI, #2454)."
