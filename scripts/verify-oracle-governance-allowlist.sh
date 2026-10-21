#!/usr/bin/env bash
# Verify the actual node process has the exact oracle-governance allowlist used
# at genesis. This is deliberately a process-environment check, not a rendered
# config check: Rust resolves this variable from the running process.
set -euo pipefail

ENV_KEY="WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS"

usage() {
  cat <<'EOF'
Usage:
  scripts/verify-oracle-governance-allowlist.sh \
    --expected-pubkey <64-lowercase-hex> \
    (--container <node-container> | --pid <node-pid>) [--proc-root <path>]

Run this on each node's CT. With --container, Docker is used to resolve the
running container PID in that CT's PID namespace. --pid is for a non-container
deployment. --proc-root defaults to /proc and exists only to make the checker
testable on a disposable filesystem fixture.

The command exits nonzero unless the target process has exactly one
WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS entry and its value is exactly the expected
lowercase governance public key.
EOF
}

fail() {
  echo "ERROR: $*" >&2
  exit 1
}

expected=""
pid=""
container=""
proc_root="/proc"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --expected-pubkey)
      expected="${2:-}"
      shift 2
      ;;
    --pid)
      pid="${2:-}"
      shift 2
      ;;
    --container)
      container="${2:-}"
      shift 2
      ;;
    --proc-root)
      proc_root="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage >&2
      fail "unknown argument: $1"
      ;;
  esac
done

[[ "$expected" =~ ^[0-9a-f]{64}$ ]] || fail "--expected-pubkey must be 64 lowercase hex characters"
[[ -n "$pid" || -n "$container" ]] || fail "supply exactly one of --container or --pid"
[[ -z "$pid" || -z "$container" ]] || fail "--container and --pid are mutually exclusive"

if [[ -n "$container" ]]; then
  command -v docker >/dev/null 2>&1 || fail "docker is required for --container"
  pid="$(docker inspect --format '{{if .State.Running}}{{.State.Pid}}{{end}}' "$container" 2>/dev/null)" \
    || fail "cannot inspect container $container"
  [[ "$pid" =~ ^[1-9][0-9]*$ ]] || fail "container $container is not running"
fi

[[ "$pid" =~ ^[1-9][0-9]*$ ]] || fail "--pid must be a positive integer"
environ_path="$proc_root/$pid/environ"
[[ -r "$environ_path" ]] || fail "cannot read running-process environment: $environ_path"

key_count=0
exact_count=0

# /proc/<pid>/environ is NUL-delimited. Do not translate records to lines:
# a newline inside a value would otherwise look like a second record and let
# the prefix before it falsely satisfy the exact-equality check.
while IFS= read -r -d '' entry; do
  if [[ "$entry" == "$ENV_KEY="* ]]; then
    ((key_count += 1))
    if [[ "$entry" == "$ENV_KEY=$expected" ]]; then
      ((exact_count += 1))
    fi
  fi
done < "$environ_path"

[[ "$key_count" == "1" ]] || fail "expected exactly one $ENV_KEY entry in process $pid; found $key_count"
[[ "$exact_count" == "1" ]] || fail "$ENV_KEY in process $pid does not exactly equal the O1 governance public key"

echo "PASS: process $pid has exactly $ENV_KEY=$expected"
