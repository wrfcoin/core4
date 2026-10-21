#!/usr/bin/env bash
#
# Run the deployment gate's smoke suite and let the EXIT STATUS carry the result
# (#2301).
#
# WHAT WAS WRONG
#
# The previous "Run smoke tests" step in .github/workflows/deploy-gate.yml was:
#
#     if npm run test:smoke 2>/dev/null; then
#       echo "result=pass" >> "$GITHUB_OUTPUT"
#     else
#       echo "Smoke test script not found, running fallback checks..."
#       npm run build --if-present 2>/dev/null || true
#       npm test -- --bail 2>/dev/null || npm run test:unit -- --bail 2>/dev/null || true
#       echo "result=pass" >> "$GITHUB_OUTPUT"
#     fi
#
# Three separate defects, all pointing the same way:
#
#   1. Both branches emit `result=pass`. The else-branch is reached precisely
#      when the smoke suite FAILED, and it reports success anyway.
#   2. Every fallback command ends in `|| true`, so a failing build and failing
#      unit tests are both discarded.
#   3. The branch is labelled "Smoke test script not found", but a smoke suite
#      that ran and failed lands in the same branch. The gate cannot tell "no
#      smoke tests exist" from "every smoke test failed" — and it calls both
#      pass.
#
# A fallback that cannot distinguish those two cases must not emit `pass`.
#
# WHAT THIS DOES INSTEAD
#
# * If a `test:smoke` script exists, run it and propagate its exit status. No
#   suppression, no `|| true`.
# * If it does NOT exist, FAIL CLOSED. A deployment gate with no smoke tests is
#   not a gate. Deliberately shipping without one requires setting
#   DEPLOY_GATE_ALLOW_MISSING_SMOKE to a non-empty justification, which is
#   echoed into the log so the decision is visible to whoever reads the run.
# * The result is the exit status. `result=` is still written to GITHUB_OUTPUT
#   for the workflow, but it is DERIVED from what happened rather than asserted
#   ahead of it.
#
# Usage:  deploy_gate_smoke.sh
# Env:    DEPLOY_GATE_ALLOW_MISSING_SMOKE  non-empty justification to permit a
#                                          missing smoke script (audited escape)
#         GITHUB_OUTPUT                    optional; receives result=pass|fail
# Exit:   0 smoke suite ran and passed (or an audited exemption applies)
#         1 smoke suite failed, or no smoke suite exists and none was authorised

set -uo pipefail

emit() {
    # Derived from the outcome, never asserted before it.
    if [ -n "${GITHUB_OUTPUT:-}" ]; then
        echo "result=$1" >> "$GITHUB_OUTPUT"
    fi
    echo "smoke result: $1"
}

has_smoke_script() {
    [ -f package.json ] || return 1
    if command -v jq >/dev/null 2>&1; then
        [ "$(jq -r '.scripts["test:smoke"] // empty' package.json)" != "" ]
    else
        grep -q '"test:smoke"' package.json
    fi
}

if has_smoke_script; then
    echo "Running smoke suite: npm run test:smoke"
    if npm run test:smoke; then
        emit pass
        exit 0
    fi
    status=$?
    # This is the case the old code mislabelled "script not found" and passed.
    echo "::error::smoke suite FAILED (npm run test:smoke exited ${status})" >&2
    emit fail
    exit 1
fi

# No smoke script. Fail closed unless explicitly and visibly authorised.
if [ -n "${DEPLOY_GATE_ALLOW_MISSING_SMOKE:-}" ]; then
    echo "::warning::no test:smoke script; proceeding under an explicit exemption:" >&2
    echo "  ${DEPLOY_GATE_ALLOW_MISSING_SMOKE}" >&2
    emit pass
    exit 0
fi

echo "::error::no 'test:smoke' script found in package.json." >&2
echo "A deployment gate with no smoke tests is not a gate, so this fails closed." >&2
echo "Add a test:smoke script, or set DEPLOY_GATE_ALLOW_MISSING_SMOKE to a" >&2
echo "justification if shipping without one is a deliberate, reviewed decision." >&2
emit fail
exit 1
