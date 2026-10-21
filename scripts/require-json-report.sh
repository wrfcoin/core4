#!/usr/bin/env bash
#
# Assert a scanner report exists and is machine-readable (core4#2316).
#
# Every OSV step in `supply-chain-security.yml` ended in `|| true` and then did:
#
#     if [ -f reports/x.json ]; then
#       VULN_COUNT=$(jq '...' reports/x.json 2>/dev/null || echo 0)
#     else
#       echo "vulnerabilities=0"
#     fi
#
# so a scanner that failed to run, or wrote a truncated/HTML/error payload,
# produced `vulnerabilities=0` — indistinguishable from a clean scan. "We could
# not check" must never be reported as "there is nothing to find".
#
# Usage: scripts/require-json-report.sh <report-path> <jq-filter-that-must-exist>
#
# Exit 0 = the file exists, parses as JSON, and the filter yields a non-null
#          value. Exit 1 = anything else, with the reason on stderr.

set -euo pipefail

REPORT="${1:?usage: require-json-report.sh <report-path> <jq-filter>}"
FILTER="${2:?usage: require-json-report.sh <report-path> <jq-filter>}"

if [[ ! -f "${REPORT}" ]]; then
    echo "::error::expected scanner report ${REPORT} is missing — the scan did not run to completion" >&2
    exit 1
fi

if [[ ! -s "${REPORT}" ]]; then
    echo "::error::scanner report ${REPORT} is empty — treating as scan failure, not as zero findings" >&2
    exit 1
fi

if ! jq empty "${REPORT}" >/dev/null 2>&1; then
    echo "::error::scanner report ${REPORT} is not valid JSON — treating as scan failure, not as zero findings" >&2
    head -c 400 "${REPORT}" >&2 || true
    exit 1
fi

VALUE="$(jq -r "${FILTER} // \"null\"" "${REPORT}" 2>/dev/null || echo "null")"
if [[ "${VALUE}" == "null" ]]; then
    echo "::error::scanner report ${REPORT} has no ${FILTER} — malformed, treating as scan failure" >&2
    exit 1
fi

echo "${VALUE}"
exit 0
