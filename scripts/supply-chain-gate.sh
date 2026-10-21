#!/usr/bin/env bash
#
# Supply-chain security gate — the decision, as an executable function
# (core4#2316).
#
# The gate used to live as inline bash in `supply-chain-security.yml`. It read
# `OSV_STATUS`, `AUDIT_STATUS` and `VET_STATUS`, printed them into the job
# summary, and then **never used them** in its failure decision. A
# workflow-shaped probe with all three set to `failure` returned exit 0. Three
# advertised scanners could therefore fail — and `cargo audit` was failing, with
# seven advisories — while the gate reported success.
#
# Inline YAML bash cannot be tested. This can:
#
#   OSV_STATUS=failure ... bash scripts/supply-chain-gate.sh; echo $?
#
# and `scripts/supply_chain_gate_probe.sh` runs the whole truth table.
#
# ---------------------------------------------------------------------------
# Inputs (environment)
# ---------------------------------------------------------------------------
#   OSV_STATUS         needs.osv-scanner.result
#   AUDIT_STATUS       needs.cargo-audit.result
#   VET_STATUS         needs.cargo-vet.result          (ADVISORY — see below)
#   DEP_BUDGET_STATUS  needs.dependency-budget.result
#   TYPO_STATUS        needs.typosquatting-check.result
#   HAS_CRITICAL       osv-scanner output, "true"/"false"
#   HAS_HIGH           osv-scanner output, "true"/"false"
#   SEVERITY_THRESHOLD "HIGH" (default) or "CRITICAL"
#   GITHUB_STEP_SUMMARY  optional; markdown summary is appended when set
#
# Exit 0 = gate passes. Exit 1 = gate fails, with every cause named.
#
# ---------------------------------------------------------------------------
# Fail-closed on unknown status
# ---------------------------------------------------------------------------
# An empty or unrecognised status is treated as FAILURE, not as pass. A job
# that was cancelled, skipped, or never ran produced no scan, and a gate that
# reports green having checked nothing is the defect this file exists to
# remove. `success` is the only value that clears a required scanner.
#
# ---------------------------------------------------------------------------
# What is deliberately NOT blocking, and why
# ---------------------------------------------------------------------------
# `cargo vet` is ADVISORY. This repository has no `supply-chain/` directory —
# cargo-vet has never been initialised here, and the workflow's own step calls
# `cargo vet init` when it finds none. Making it blocking today would fail on
# every one of several hundred unaudited transitive crates on the first run:
# that is a wall, not a signal, and a gate nobody can pass gets `|| true`'d
# again within a month. It stays advisory, it is LABELLED advisory in the
# summary so no reader mistakes it for enforcement, and making it blocking is
# tracked separately. Naming this is the point: an unstated exemption is how
# the swallowing came back.

set -euo pipefail

OSV_STATUS="${OSV_STATUS:-}"
AUDIT_STATUS="${AUDIT_STATUS:-}"
VET_STATUS="${VET_STATUS:-}"
DEP_BUDGET_STATUS="${DEP_BUDGET_STATUS:-}"
TYPO_STATUS="${TYPO_STATUS:-}"
HAS_CRITICAL="${HAS_CRITICAL:-false}"
HAS_HIGH="${HAS_HIGH:-false}"
SEVERITY_THRESHOLD="${SEVERITY_THRESHOLD:-HIGH}"

FAILURES=()

summary() {
    if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
        echo "$1" >> "$GITHUB_STEP_SUMMARY"
    fi
}

# A required scanner clears the gate only on an explicit `success`.
require_success() {
    local label="$1" status="$2"
    if [[ "$status" != "success" ]]; then
        local shown="${status:-<unset>}"
        echo "::error::${label} did not succeed (result: ${shown}) — supply chain gate FAILED"
        FAILURES+=("${label}: ${shown}")
    fi
}

summary "## Supply Chain Security Gate Results"
summary ""
summary "| Check | Status | Blocking |"
summary "|-------|--------|----------|"
summary "| OSV Scanner | ${OSV_STATUS:-<unset>} | yes |"
summary "| Cargo Audit | ${AUDIT_STATUS:-<unset>} | yes |"
summary "| Cargo Vet | ${VET_STATUS:-<unset>} | **no — advisory** |"
summary "| Dependency Budget | ${DEP_BUDGET_STATUS:-<unset>} | yes |"
summary "| Typosquatting | ${TYPO_STATUS:-<unset>} | yes |"
summary ""

require_success "OSV Scanner" "$OSV_STATUS"
require_success "Cargo Audit" "$AUDIT_STATUS"
require_success "Dependency Budget" "$DEP_BUDGET_STATUS"
require_success "Typosquatting Detection" "$TYPO_STATUS"

# Severity flags from the OSV analysis step. These are a second, independent
# reason to fail: OSV can exit 0 on a scan that found HIGH-severity issues
# depending on invocation, so the flag is checked as well as the job result.
if [[ "$HAS_CRITICAL" == "true" ]]; then
    echo "::error::Critical vulnerabilities detected - security gate FAILED"
    FAILURES+=("OSV: critical vulnerabilities present")
fi

if [[ "$SEVERITY_THRESHOLD" != "CRITICAL" && "$HAS_HIGH" == "true" ]]; then
    echo "::error::High vulnerabilities detected - security gate FAILED"
    FAILURES+=("OSV: high vulnerabilities present (threshold ${SEVERITY_THRESHOLD})")
fi

# Advisory only — reported, never fatal. Stated out loud in the log so the
# difference between "passed" and "not enforced" is visible without reading
# this file.
if [[ "$VET_STATUS" != "success" ]]; then
    echo "::warning::cargo-vet did not succeed (result: ${VET_STATUS:-<unset>}) — ADVISORY, not blocking this gate"
fi

if [[ ${#FAILURES[@]} -gt 0 ]]; then
    summary "**FAILED** — the gate blocks on:"
    summary ""
    for failure in "${FAILURES[@]}"; do
        summary "- ${failure}"
        echo "gate failure: ${failure}" >&2
    done
    summary ""
    summary "## Required Actions"
    summary "1. Review the scanner reports in this run's artifacts"
    summary "2. Update the affected dependencies, or record an explicit, dated"
    summary "   exemption (see \`.cargo/audit.toml\`) in a reviewed diff"
    summary "3. Re-run the scan"
    exit 1
fi

summary "**PASSED**: every blocking supply-chain check succeeded (cargo-vet advisory)"
echo "supply chain gate: PASS"
exit 0
