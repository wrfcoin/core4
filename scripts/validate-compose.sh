#!/usr/bin/env bash
#
# Hermetic docker-compose structural validation (core4#2312).
#
# The docker-security-scan workflow validated every `docker-compose*.yml` with a
# bare `docker compose config`, and was permanently red on six files for reasons
# that have nothing to do with a compose file being malformed:
#
#   * TWO were broken symlinks into the extracted `infra/` tree (PR #374), so
#     `config` failed with "no such file or directory". Those are removed from
#     the repo in this change. A dangling symlink is now a FAILURE, not a skip
#     (Codex review of PR #2338, P2): the exact scenario that produced these two
#     — a target renamed/extracted while a `docker-compose*.yml` symlink is left
#     behind — is recurring in this repo, and silently skipping it would let a
#     future orphan pass the gate with a discovered compose file never
#     validated. With the two known orphans removed, failing on dangling
#     symlinks costs nothing now and catches the next one instead of hiding it.
#   * FOUR are well-formed files that use `${SECRET:?message}` interpolation and
#     therefore require runtime secrets (POSTGRES_PASSWORD, ORACLE_REDIS_PASSWORD,
#     PACT_BROKER_PASSWORD). "Needs a secret to run" is not "is structurally
#     invalid", and a validation gate must not conflate them.
#
# This script makes validation HERMETIC: it injects clearly-fake placeholder
# values (PLACEHOLDER_SECRETS below) purely so interpolation resolves, then runs
# the structural check. The placeholders are never used to run anything —
# `docker compose config` only parses and interpolates. They live in this script
# rather than a committed `.env` on purpose: the repo `.gitignore` blocks
# `*.env`, and a committed env file — even one full of obvious fakes — is exactly
# the shape a secret scanner is built to flag.
#
# It stays FALSIFIABLE: a genuinely malformed compose file (bad YAML, unknown
# top-level key, a NEW required secret not in PLACEHOLDER_SECRETS) still fails.
# `scripts/docker_scan_gate_probe.sh` proves this by planting a malformed file
# and asserting the loop goes red.
#
# Usage: scripts/validate-compose.sh
#
# Exit 0 = every discovered compose file validated (none dangling, none malformed).
# Exit 1 = at least one failure — a dangling symlink OR a structural error.
# Exit 2 = docker compose is unavailable, so nothing could be checked
#          (fail-closed: "could not validate" must not read as "valid").

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

# Placeholder secrets for interpolation ONLY. Every value is the literal
# "ci-validation-placeholder-not-a-secret" so a scanner sees an obvious
# non-secret and a reader of a validated config sees a stand-in. This is the set
# the CURRENT tree needs; a NEW `${SECRET:?}` not listed here still fails
# validation, which is correct — adding it is then a conscious line here.
# DO NOT put a real secret in this list.
PLACEHOLDER_PROBE="ci-validation-placeholder-not-a-secret"
PLACEHOLDER_SECRETS=(
    POSTGRES_PASSWORD
    REDIS_PASSWORD
    ORACLE_REDIS_PASSWORD
    PACT_BROKER_PASSWORD
    WRFCOIN_OPERATOR_TOKEN
    JWT_SECRET
)

if ! docker compose version >/dev/null 2>&1; then
    echo "::error::docker compose is not available — cannot validate compose files" >&2
    exit 2
fi

for var in "${PLACEHOLDER_SECRETS[@]}"; do
    export "${var}=${PLACEHOLDER_PROBE}"
done
echo "Injected ${#PLACEHOLDER_SECRETS[@]} placeholder secret(s) for interpolation."

mapfile -t COMPOSE_FILES < <(find . -name 'docker-compose*.yml' \
    -not -path '*/node_modules/*' \
    -not -path '*/archive/*' | sort)

echo "Validating ${#COMPOSE_FILES[@]} docker-compose file(s)..."
ERRORS=0

for file in "${COMPOSE_FILES[@]}"; do
    # A dangling symlink is a FAILURE (Codex review of #2338, P2). A discovered
    # compose file that cannot be resolved was never validated; passing the gate
    # anyway is the "green while a file went unchecked" hole this workflow exists
    # to close. Failing here catches the next extraction orphan instead of
    # hiding it. If a dangling compose symlink ever becomes legitimate, add an
    # explicit allowlist entry with a reason — do not re-introduce a blanket skip.
    if [[ -L "${file}" && ! -e "${file}" ]]; then
        echo "  FAILED  ${file} (dangling symlink -> $(readlink "${file}"))"
        echo "            target does not exist — remove the orphaned symlink, or"
        echo "            restore its target. A discovered compose file must resolve."
        ERRORS=$((ERRORS + 1))
        continue
    fi

    if docker compose -f "${file}" config >/dev/null 2>&1; then
        echo "  OK      ${file}"
    else
        echo "  FAILED  ${file}"
        # Surface the reason (drop the noisy unset-with-default warnings).
        docker compose -f "${file}" config 2>&1 | grep -iv 'level=warning' | head -5 | sed 's/^/            /'
        ERRORS=$((ERRORS + 1))
    fi
done

echo ""
if [[ "${ERRORS}" -gt 0 ]]; then
    echo "::error::${ERRORS} docker-compose file(s) failed validation (dangling symlink or structural error)"
    exit 1
fi

echo "All ${#COMPOSE_FILES[@]} discovered compose file(s) are structurally valid."
exit 0
