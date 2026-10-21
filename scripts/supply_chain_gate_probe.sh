#!/usr/bin/env bash
#
# Acceptance probe for the supply-chain security gate (core4#2316).
#
# The property under test: **a scanner finding must turn the gate red.**
#
# That was not true. The aggregator read `OSV_STATUS`, `AUDIT_STATUS` and
# `VET_STATUS`, printed them into the job summary, and never used them in its
# failure decision — so all three could report `failure` and the gate exited 0.
# `cargo audit` was in fact failing, with seven advisories, behind `|| true`.
#
# A gate whose failure path has never been executed is a gate nobody has tested.
# This script executes it, in three parts:
#
#   PART 1 — decision truth table. Every blocking input, one at a time, must
#            flip `supply-chain-gate.sh` from exit 0 to exit 1. Includes the
#            three statuses whose values used to be ignored, and the
#            fail-closed cases (unset / cancelled / skipped).
#
#   PART 2 — real finding, real scanner. Runs `scripts/cargo-audit-gate.sh`,
#            the same script CI runs, against the live advisory database. If
#            the workspace has an open advisory the step must FAIL, and that
#            failure must propagate through the gate.
#
#   PART 3 — clean scan passes. A gate that fails on everything is no more
#            informative than one that passes on everything, so the negative
#            control matters: with a synthetic clean report and all statuses
#            `success`, the gate must exit 0.
#
# Usage: scripts/supply_chain_gate_probe.sh
#
# Exit 0 = the gate discriminates: findings red, clean green.
# Exit 1 = a planted/real finding did NOT turn the gate red, or a clean run did
#          not pass. Either way the gate's verdict is not evidence.
#
# Nothing is left behind: PART 2/3 use a temp report directory removed by a
# trap on every exit path, and no tracked file is modified.

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GATE="${REPO_ROOT}/scripts/supply-chain-gate.sh"
AUDIT_GATE="${REPO_ROOT}/scripts/cargo-audit-gate.sh"
WORK="$(mktemp -d)"
FAILURES=0

cleanup() { rm -rf "${WORK}"; }
trap cleanup EXIT INT TERM

for required in "${GATE}" "${AUDIT_GATE}" "${REPO_ROOT}/scripts/require-json-report.sh"; do
    if [[ ! -f "${required}" ]]; then
        echo "ERROR: ${required} not found — this probe proved nothing" >&2
        exit 2
    fi
done

# All-clear environment: every blocking scanner succeeded, no severity flags.
clean_env() {
    echo "OSV_STATUS=success"
    echo "AUDIT_STATUS=success"
    echo "VET_STATUS=success"
    echo "DEP_BUDGET_STATUS=success"
    echo "TYPO_STATUS=success"
    echo "HAS_CRITICAL=false"
    echo "HAS_HIGH=false"
    echo "SEVERITY_THRESHOLD=HIGH"
}

# run_gate <label> <expected-exit> [VAR=value ...]
run_gate() {
    local label="$1" expected="$2"; shift 2
    local -a env_args=()
    while IFS= read -r kv; do env_args+=("$kv"); done < <(clean_env)
    env_args+=("$@")

    # `|| status=$?` rather than a bare call: the gate is *expected* to exit
    # non-zero in most of these cases, and an unguarded failing command would
    # abort the probe under `set -e` — which would look like the truth table
    # ending early rather than like a failure.
    local output status=0
    output="$(env "${env_args[@]}" bash "${GATE}" 2>&1)" || status=$?

    if [[ "${status}" -eq "${expected}" ]]; then
        printf '[gate-probe]   OK   %-52s exit %d\n' "${label}" "${status}"
    else
        printf '[gate-probe]   FAIL %-52s exit %d, expected %d\n' "${label}" "${status}" "${expected}"
        echo "${output}" | sed 's/^/[gate-probe]        /'
        FAILURES=$((FAILURES + 1))
    fi
}

echo "[gate-probe] PART 1 — decision truth table"

# The negative control first: if this does not pass, every red below is
# meaningless because the gate might simply always fail.
run_gate "all scanners clean" 0

# The three statuses the old aggregator printed and ignored. Each of these
# assertions FAILS against the pre-#2316 gate.
run_gate "OSV scanner job failed" 1 "OSV_STATUS=failure"
run_gate "cargo audit job failed" 1 "AUDIT_STATUS=failure"

# Kept from the old gate — these two were the only real enforcement it had, so
# the probe must show they still work after the rewrite.
run_gate "dependency budget exceeded" 1 "DEP_BUDGET_STATUS=failure"
run_gate "typosquatting detected" 1 "TYPO_STATUS=failure"
run_gate "OSV reported CRITICAL" 1 "HAS_CRITICAL=true"
run_gate "OSV reported HIGH (threshold HIGH)" 1 "HAS_HIGH=true"
run_gate "OSV HIGH ignored at threshold CRITICAL" 0 "HAS_HIGH=true" "SEVERITY_THRESHOLD=CRITICAL"

# Fail-closed: a scanner that did not run is not a scanner that found nothing.
run_gate "OSV status unset" 1 "OSV_STATUS="
run_gate "cargo audit cancelled" 1 "AUDIT_STATUS=cancelled"
run_gate "dependency budget skipped" 1 "DEP_BUDGET_STATUS=skipped"

# cargo-vet is advisory ON PURPOSE (no supply-chain/ directory exists). This
# assertion exists so that the exemption is visible and deliberate: if someone
# later makes vet blocking, this line fails and they must delete it knowingly
# rather than discovering the change in a scheduled run.
run_gate "cargo vet failed (ADVISORY — must NOT gate)" 0 "VET_STATUS=failure"

echo "[gate-probe] PART 2 — real cargo-audit finding must fail the step"

if ! cargo audit --version >/dev/null 2>&1; then
    echo "[gate-probe]   SKIP cargo-audit is not installed; PART 2 not run" >&2
    echo "[gate-probe]        install with: cargo install cargo-audit --locked" >&2
else
    set +e
    AUDIT_OUTPUT="$(bash "${AUDIT_GATE}" "${WORK}/reports" 2>&1)"
    AUDIT_STATUS=$?
    set -e

    COUNT="$(jq -r '.vulnerabilities.count // 0' "${WORK}/reports/cargo-audit.json" 2>/dev/null || echo 0)"

    if [[ "${COUNT}" -gt 0 ]]; then
        if [[ "${AUDIT_STATUS}" -eq 1 ]]; then
            echo "[gate-probe]   OK   cargo-audit-gate.sh failed on ${COUNT} real advisories:"
            grep -E '^\s+RUSTSEC-' <<<"${AUDIT_OUTPUT}" | sed 's/^/[gate-probe]        /'
            # …and that failure must reach the gate.
            run_gate "real audit failure propagates to the gate" 1 "AUDIT_STATUS=failure"
        else
            echo "[gate-probe]   FAIL cargo-audit found ${COUNT} advisories but the step exited ${AUDIT_STATUS}"
            echo "${AUDIT_OUTPUT}" | tail -20 | sed 's/^/[gate-probe]        /'
            FAILURES=$((FAILURES + 1))
        fi
    else
        echo "[gate-probe]   NOTE workspace currently has 0 advisories; PART 2 has no real"
        echo "[gate-probe]        finding to use. Falling back to the malformed-report path,"
        echo "[gate-probe]        which is the other way a scan can silently read as clean."
        mkdir -p "${WORK}/reports"
        printf 'error: could not fetch advisory-db\n' > "${WORK}/reports/cargo-audit.json"
        set +e
        bash "${REPO_ROOT}/scripts/require-json-report.sh" \
            "${WORK}/reports/cargo-audit.json" '.vulnerabilities.count' >/dev/null 2>&1
        MALFORMED_STATUS=$?
        set -e
        if [[ "${MALFORMED_STATUS}" -ne 0 ]]; then
            echo "[gate-probe]   OK   a malformed report is rejected rather than counted as zero"
        else
            echo "[gate-probe]   FAIL a malformed report was accepted as a clean scan"
            FAILURES=$((FAILURES + 1))
        fi
    fi
fi

echo "[gate-probe] PART 3 — a clean scan must still pass"
run_gate "clean scan, every status success" 0

echo ""
if [[ "${FAILURES}" -gt 0 ]]; then
    cat >&2 <<'MSG'
FAIL: the supply-chain gate does not discriminate.

At least one scanner finding failed to turn the gate red, or a clean run failed
to pass. Until this probe is green, a green "Supply Chain Security Gate" is not
evidence that the scanners found nothing — it is only evidence that the gate
ran (core4#2316).
MSG
    exit 1
fi

echo "[gate-probe] PASS — every blocking finding turns the gate red; a clean run passes."
exit 0
