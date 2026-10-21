#!/usr/bin/env bash
#
# `cargo audit` as an enforcing step (core4#2316).
#
# The workflow used to run:
#
#     cargo audit --json > reports/cargo-audit.json 2>&1 || true
#     cargo audit 2>&1 | tee reports/cargo-audit.txt || true
#     VULN_COUNT=$(jq '.vulnerabilities.count' ... || echo 0)
#     if [ "$VULN_COUNT" -gt 0 ]; then echo "::warning::..." ; fi
#
# Three ways to report clean while being dirty, all of them live on this repo:
#
#  1. `|| true` discards cargo-audit's exit code. On the audited head it was
#     exiting 1 with SEVEN advisories.
#  2. `2>&1` into the JSON file means a tool error is written *into* the report,
#     so the report is not valid JSON and jq's `|| echo 0` turns that into "no
#     vulnerabilities".
#  3. A warning annotation is not a failure. The job stayed green, and the
#     aggregator ignored the job result anyway.
#
# This script keeps the reports and the counting, and makes every one of those
# paths fatal. It is what CI runs, and what `supply_chain_gate_probe.sh` runs,
# so the demonstration and the gate cannot drift apart.
#
# Usage: scripts/cargo-audit-gate.sh [report-dir]
#
# Exit 0 = no vulnerabilities. Exit 1 = vulnerabilities found. Exit 2 = the scan
# could not be performed or its report is unusable (fail-closed: "we could not
# check" must never read as "there is nothing to find").

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPORT_DIR="${1:-${REPO_ROOT}/reports}"
JSON_REPORT="${REPORT_DIR}/cargo-audit.json"
TEXT_REPORT="${REPORT_DIR}/cargo-audit.txt"

mkdir -p "${REPORT_DIR}"

if ! command -v cargo-audit >/dev/null 2>&1 && ! cargo audit --version >/dev/null 2>&1; then
    echo "::error::cargo-audit is not installed — cannot run the Rust advisory scan" >&2
    exit 2
fi

# An ignored advisory is acceptable only while its recorded reachability
# assumption remains executable fact. Fail before scanning if feature drift has
# made any #2336 exemption reachable.
bash "${REPO_ROOT}/scripts/rustsec-exemption-gate.sh"

# stderr is kept OUT of the JSON file. Mixing it in is what made the report
# unparseable and the count silently zero.
set +e
cargo audit --json > "${JSON_REPORT}" 2> "${TEXT_REPORT}.stderr"
AUDIT_EXIT=$?
set -e

# Human-readable companion report; advisory output only, never the decision.
cargo audit > "${TEXT_REPORT}" 2>&1 || true

if [[ ! -s "${JSON_REPORT}" ]]; then
    echo "::error::cargo audit produced no JSON report (exit ${AUDIT_EXIT}) — treating as FAILURE" >&2
    sed 's/^/  cargo-audit stderr: /' "${TEXT_REPORT}.stderr" >&2 || true
    exit 2
fi

if ! jq empty "${JSON_REPORT}" >/dev/null 2>&1; then
    echo "::error::cargo audit report is not valid JSON — treating as FAILURE, not as zero findings" >&2
    head -c 400 "${JSON_REPORT}" >&2 || true
    exit 2
fi

VULN_COUNT="$(jq -r '.vulnerabilities.count // "null"' "${JSON_REPORT}")"
if [[ "${VULN_COUNT}" == "null" ]]; then
    echo "::error::cargo audit report has no .vulnerabilities.count field — malformed, treating as FAILURE" >&2
    exit 2
fi

echo "cargo-audit: ${VULN_COUNT} vulnerabilities (exit ${AUDIT_EXIT})"

if [[ "${VULN_COUNT}" -gt 0 ]]; then
    echo "::error::cargo-audit found ${VULN_COUNT} vulnerabilities — supply chain gate FAILED"
    jq -r '.vulnerabilities.list[]
             | "  \(.advisory.id)  \(.package.name) \(.package.version)  \(.advisory.title)"' \
        "${JSON_REPORT}" >&2
    echo "" >&2
    echo "To resolve: upgrade the affected crate, or record a dated, justified" >&2
    echo "exemption in .cargo/audit.toml — in a reviewed diff, never with '|| true'." >&2
    exit 1
fi

# A non-zero exit with zero counted vulnerabilities means cargo-audit failed for
# some other reason (network, corrupt advisory DB). Fail closed.
if [[ ${AUDIT_EXIT} -ne 0 ]]; then
    echo "::error::cargo audit exited ${AUDIT_EXIT} with no vulnerabilities counted — the scan itself failed" >&2
    sed 's/^/  cargo-audit stderr: /' "${TEXT_REPORT}.stderr" >&2 || true
    exit 2
fi

echo "cargo-audit: clean"
exit 0
