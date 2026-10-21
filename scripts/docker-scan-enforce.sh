#!/usr/bin/env bash
#
# Per-image build+scan enforcement decision for docker-security-scan (core4#2312).
#
# The workflow used to:
#   * build with `continue-on-error: true` AND `docker build … || echo "Build
#     failed"`, so a build failure exited 0 and `steps.build.outcome` was always
#     `success` — a masked build was indistinguishable from a real one;
#   * run the ONLY blocking Trivy invocation (`exit-code: '1'`) behind
#     `if: github.event_name == 'pull_request'`, which this workflow never
#     receives, so HIGH/CRITICAL findings blocked on no event it can get;
#   * summarise from the enclosing job's post-`continue-on-error` result, so the
#     summary said PASSED when a step had failed.
#
# This script is the decision, extracted so it can be run and truth-tabled
# outside Actions (the #2316 pattern). Given the two step outcomes it:
#   * classifies the result as BUILD FAILURE, SCAN FINDING, ANOMALY, or CLEAN,
#   * writes ONE labelled line to the step summary distinguishing which, and
#   * exits nonzero on anything but CLEAN.
#
# Usage:
#   BUILD_OUTCOME=<success|failure|skipped> \
#   SCAN_OUTCOME=<success|failure|skipped|""> \
#   DOCKERFILE=<path> \
#   scripts/docker-scan-enforce.sh
#
# Exit 0 = built and scan clean. Exit 1 = build failed OR scan found
# HIGH/CRITICAL OR an anomalous state (fail-closed).

set -uo pipefail

BUILD_OUTCOME="${BUILD_OUTCOME:-}"
SCAN_OUTCOME="${SCAN_OUTCOME:-}"
DOCKERFILE="${DOCKERFILE:-<unknown Dockerfile>}"

summary() {
    if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
        echo "$1" >> "${GITHUB_STEP_SUMMARY}"
    fi
}

# 1. Build failure — reported as a BUILD failure, never as a scan finding. The
#    distinction the issue asks for: a red here means "the image did not build",
#    not "the image has vulnerabilities".
if [[ "${BUILD_OUTCOME}" != "success" ]]; then
    echo "::error::BUILD FAILED for ${DOCKERFILE} (build outcome: ${BUILD_OUTCOME:-<unset>})"
    summary "- ❌ \`${DOCKERFILE}\` — **build failed** (not a vulnerability finding)"
    exit 1
fi

# 2. Build succeeded but the scan did not run. The scan is gated on a successful
#    build, so with a successful build a missing scan is an anomaly — fail
#    closed rather than report a clean image nobody scanned.
if [[ -z "${SCAN_OUTCOME}" || "${SCAN_OUTCOME}" == "skipped" ]]; then
    echo "::error::${DOCKERFILE} built but the vulnerability scan did not run (scan outcome: ${SCAN_OUTCOME:-<unset>}) — failing closed"
    summary "- ❌ \`${DOCKERFILE}\` — **built but not scanned** (failing closed)"
    exit 1
fi

# 3. The scan step did not succeed. Trivy's `exit-code: '1'` makes a
#    HIGH/CRITICAL finding the expected cause, but a tool or advisory-database
#    error also surfaces here as the same `failure` outcome — GitHub gives one
#    step outcome for both (#2338 review). The gate fails closed either way; the
#    label says "scan failed" and points at the Trivy step log rather than
#    asserting a vulnerability that might actually be a scanner error.
if [[ "${SCAN_OUTCOME}" != "success" ]]; then
    echo "::error::${DOCKERFILE} scan did not pass (outcome: ${SCAN_OUTCOME}) — HIGH/CRITICAL finding or Trivy tool/DB error; see the Trivy step log"
    summary "- ❌ \`${DOCKERFILE}\` — **scan failed** (HIGH/CRITICAL finding or scanner error — see Trivy step log)"
    exit 1
fi

# 4. Built and clean.
summary "- ✅ \`${DOCKERFILE}\` — built and scan clean"
echo "OK: ${DOCKERFILE} built and scan clean"
exit 0
