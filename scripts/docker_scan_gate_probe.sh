#!/usr/bin/env bash
#
# Acceptance probe for the Docker security scan gate (core4#2312).
#
# The property under test: **a finding turns the gate red, and the gate says
# which kind of finding.** Before this change none of that held —
#
#   * HIGH/CRITICAL enforcement lived behind `if: github.event_name ==
#     'pull_request'`, an event the workflow never receives, so it never ran;
#   * `docker build … || echo "Build failed"` exited 0, so a build failure was a
#     masked success;
#   * compose validation was permanently red on six unrelated files, so a real
#     new failure was invisible in the noise.
#
# This runs the parts that CAN be checked here, and is explicit about the one
# that cannot:
#
#   PART 1 — per-image enforcement decision (scripts/docker-scan-enforce.sh).
#            Truth table over build/scan outcomes: a build failure is red and
#            labelled "build failed"; a non-success scan is red and labelled as
#            a scan failure (HIGH/CRITICAL finding or scanner error); a
#            built-but-not-scanned anomaly fails closed; only built+clean is
#            green. RUNNABLE — pure bash.
#
#   PART 2 — build masking is gone. The old `docker build … || echo` shape
#            exited 0 on a failed build; the new shape exits nonzero. Shown by
#            running a build that cannot succeed both ways. RUNNABLE — the build
#            command fails fast without needing a working image.
#
#   PART 3 — compose validation is falsifiable. Plant a malformed compose file,
#            assert scripts/validate-compose.sh goes red; remove it, assert
#            green. RUNNABLE ONLY IF `docker compose` is present — SKIPPED with a
#            loud note otherwise.
#
#   NOT COVERED — Trivy's own exit-code behaviour on a real vulnerable image.
#            Trivy is not installed in this environment and pulling scanner
#            images is out of scope for a probe. The workflow relies on
#            `exit-code: '1'` from the pinned trivy-action; that this maps a
#            HIGH/CRITICAL finding to a nonzero exit is asserted by Trivy's own
#            test suite, not re-verified here. Stated, not softened.
#
# Usage: scripts/docker_scan_gate_probe.sh
# Exit 0 = every runnable property holds. Exit 1 = one failed. Exit 2 = a
#          prerequisite script is missing (proved nothing).
#
# Self-restoring: PART 3 writes one untracked file (docker-compose.PROBE-*.yml)
# that the validator's glob discovers, then removes it via a trap on every exit
# path — including Ctrl-C and a failed command. No tracked file is modified.

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

ENFORCE="${REPO_ROOT}/scripts/docker-scan-enforce.sh"
VALIDATE="${REPO_ROOT}/scripts/validate-compose.sh"
PLANTED="${REPO_ROOT}/docker-compose.PROBE-planted.yml"
PLANTED_LINK="${REPO_ROOT}/docker-compose.PROBE-dangling.yml"
FAILURES=0

cleanup() { rm -f "${PLANTED}" "${PLANTED_LINK}"; }
trap cleanup EXIT INT TERM

for required in "${ENFORCE}" "${VALIDATE}"; do
    if [[ ! -f "${required}" ]]; then
        echo "ERROR: ${required} not found — this probe proved nothing" >&2
        exit 2
    fi
done

# run_enforce <label> <expected-exit> BUILD SCAN
run_enforce() {
    local label="$1" expected="$2" build="$3" scan="$4"
    local status=0
    BUILD_OUTCOME="${build}" SCAN_OUTCOME="${scan}" DOCKERFILE="probe/${label}" \
        bash "${ENFORCE}" >/dev/null 2>&1 || status=$?
    if [[ "${status}" -eq "${expected}" ]]; then
        printf '[docker-probe]   OK   %-42s exit %d\n' "${label}" "${status}"
    else
        printf '[docker-probe]   FAIL %-42s exit %d, expected %d\n' "${label}" "${status}" "${expected}"
        FAILURES=$((FAILURES + 1))
    fi
}

echo "[docker-probe] PART 1 — per-image enforcement truth table"
run_enforce "built + scan clean"            0 success success
run_enforce "build FAILED"                  1 failure ""
run_enforce "build failed (skipped scan)"   1 failure skipped
run_enforce "scan found HIGH/CRITICAL"      1 success failure
run_enforce "built but scan skipped"        1 success skipped
run_enforce "built but scan never ran"      1 success ""

# Confirm the labels distinguish the two failure kinds, not just the exit codes.
BUILD_LABEL="$(BUILD_OUTCOME=failure SCAN_OUTCOME="" DOCKERFILE="probe/x" bash "${ENFORCE}" 2>&1 || true)"
SCAN_LABEL="$(BUILD_OUTCOME=success SCAN_OUTCOME=failure DOCKERFILE="probe/x" bash "${ENFORCE}" 2>&1 || true)"
# The build failure must say "BUILD FAILED"; the scan failure must NOT (it says
# "scan ... HIGH/CRITICAL finding or tool/DB error" — #2338 review clarified the
# label so a scanner error is not asserted as a vulnerability). Distinct = the
# scan label is about the scan and does not claim a build failure.
if grep -qi "BUILD FAILED" <<<"${BUILD_LABEL}" \
   && grep -qi "scan" <<<"${SCAN_LABEL}" \
   && ! grep -qi "BUILD FAILED" <<<"${SCAN_LABEL}"; then
    echo "[docker-probe]   OK   build failure and scan failure are labelled distinctly"
else
    echo "[docker-probe]   FAIL the two failure kinds are not distinguished in output"
    FAILURES=$((FAILURES + 1))
fi

echo "[docker-probe] PART 2 — build masking is removed"
# Old shape: `docker build <bad> || echo "Build failed"` — exits 0.
old_status=0
{ docker build -f /nonexistent/Dockerfile.PROBE /nonexistent 2>/dev/null || echo "Build failed"; } >/dev/null 2>&1 || old_status=$?
# New shape: the bare build command — exits nonzero.
new_status=0
docker build -f /nonexistent/Dockerfile.PROBE /nonexistent >/dev/null 2>&1 || new_status=$?
if [[ "${old_status}" -eq 0 && "${new_status}" -ne 0 ]]; then
    echo "[docker-probe]   OK   old \`|| echo\` shape exited ${old_status}; bare build exits ${new_status}"
else
    echo "[docker-probe]   FAIL masking check inconclusive (old=${old_status}, new=${new_status})"
    FAILURES=$((FAILURES + 1))
fi

echo "[docker-probe] PART 3 — compose validation is falsifiable"
if ! docker compose version >/dev/null 2>&1; then
    echo "[docker-probe]   SKIP docker compose unavailable — PART 3 not run (say so, don't soften)"
else
    # Baseline: the real tree must validate green.
    base_status=0
    bash "${VALIDATE}" >/dev/null 2>&1 || base_status=$?
    if [[ "${base_status}" -eq 0 ]]; then
        echo "[docker-probe]   OK   clean tree validates (exit 0)"
    else
        echo "[docker-probe]   FAIL clean tree did not validate (exit ${base_status})"
        FAILURES=$((FAILURES + 1))
    fi

    # Plant a structurally malformed compose file the glob will discover.
    printf 'services:\n  broken:\n    image: [this, is, not, a, string]\n  : missing-key\n' > "${PLANTED}"
    planted_status=0
    bash "${VALIDATE}" >/dev/null 2>&1 || planted_status=$?
    if [[ "${planted_status}" -ne 0 ]]; then
        echo "[docker-probe]   OK   a malformed compose file turns validation red (exit ${planted_status})"
    else
        echo "[docker-probe]   FAIL a malformed compose file did NOT fail validation"
        FAILURES=$((FAILURES + 1))
    fi

    rm -f "${PLANTED}"
    restored_status=0
    bash "${VALIDATE}" >/dev/null 2>&1 || restored_status=$?
    if [[ "${restored_status}" -eq 0 ]]; then
        echo "[docker-probe]   OK   removing it restores green (exit 0)"
    else
        echo "[docker-probe]   FAIL tree did not return to green after cleanup (exit ${restored_status})"
        FAILURES=$((FAILURES + 1))
    fi

    # A DANGLING compose symlink must fail, not be silently skipped (Codex
    # review of #2338, P2). This is the recurring extraction-orphan scenario:
    # the exact shape that produced the original six failures. Plant one
    # pointing at a nonexistent target and assert the gate goes red.
    ln -s ./this-target-does-not-exist.yml "${PLANTED_LINK}"
    dangling_status=0
    bash "${VALIDATE}" >/dev/null 2>&1 || dangling_status=$?
    if [[ "${dangling_status}" -ne 0 ]]; then
        echo "[docker-probe]   OK   a dangling compose symlink turns validation red (exit ${dangling_status})"
    else
        echo "[docker-probe]   FAIL a dangling compose symlink did NOT fail validation"
        FAILURES=$((FAILURES + 1))
    fi

    rm -f "${PLANTED_LINK}"
    final_status=0
    bash "${VALIDATE}" >/dev/null 2>&1 || final_status=$?
    if [[ "${final_status}" -eq 0 ]]; then
        echo "[docker-probe]   OK   removing the dangling symlink restores green (exit 0)"
    else
        echo "[docker-probe]   FAIL tree did not return to green after symlink cleanup (exit ${final_status})"
        FAILURES=$((FAILURES + 1))
    fi
fi

echo ""
if [[ "${FAILURES}" -gt 0 ]]; then
    cat >&2 <<'MSG'
FAIL: the Docker scan gate does not discriminate.

A build failure, a scan finding, or a malformed compose file failed to turn the
gate red — or a clean tree failed to pass. Until this probe is green, a green
Docker Security Scan is not evidence that images and compose files are sound
(core4#2312).
MSG
    exit 1
fi

echo "[docker-probe] PASS — findings turn the gate red and are labelled; clean passes."
echo "[docker-probe] NOTE: Trivy's finding->exit-code mapping is NOT verified here (Trivy absent)."
exit 0
