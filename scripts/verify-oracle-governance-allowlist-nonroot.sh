#!/bin/sh
# Read-only O2b check for a workstation user that cannot read the host's
# /proc/<container-pid>/environ. Run inside the node container as its own uid.
set -eu

ENV_KEY=WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS

usage() {
    printf '%s\n' \
        "usage: $0 --container NAME --expected-pubkey 64-lowercase-hex [--container-uid UID]" \
        "       $0 --self-test" >&2
    exit 2
}

fail() {
    printf 'ERROR: %s\n' "$*" >&2
    exit 1
}

check_records() {
    expected=$1
    awk -v key="$ENV_KEY" -v expected="$expected" '
        BEGIN { RS = "\0"; count = 0; exact = 0 }
        index($0, key "=") == 1 {
            count++
            if ($0 == key "=" expected) exact++
        }
        END {
            if (count != 1) {
                printf "ERROR: expected exactly one %s entry; found %d\n", key, count > "/dev/stderr"
                exit 1
            }
            if (exact != 1) {
                printf "ERROR: %s does not exactly equal the expected public key\n", key > "/dev/stderr"
                exit 1
            }
        }
    '
}

self_test() {
    key=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
    other_key=fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210
    printf 'PATH=/bin\0%s=%s\0' "$ENV_KEY" "$key" | check_records "$key"
    if printf '%s=%s\0%s=%s\0' "$ENV_KEY" "$key" "$ENV_KEY" "$other_key" | check_records "$key" 2>/dev/null; then
        fail "self-test duplicate control unexpectedly passed"
    fi
    if printf '%s=%s0\0' "$ENV_KEY" "$key" | check_records "$key" 2>/dev/null; then
        fail "self-test mismatch control unexpectedly passed"
    fi
    printf 'SELF-TEST PASS: exact match passes; duplicate and mismatch controls fail\n'
}

container=
expected=
container_uid=1001

while [ "$#" -gt 0 ]; do
    case "$1" in
        --container) [ "$#" -ge 2 ] || usage; container=$2; shift 2 ;;
        --expected-pubkey) [ "$#" -ge 2 ] || usage; expected=$2; shift 2 ;;
        --container-uid) [ "$#" -ge 2 ] || usage; container_uid=$2; shift 2 ;;
        --self-test) [ "$#" -eq 1 ] || usage; self_test; exit 0 ;;
        *) usage ;;
    esac
done

[ -n "$container" ] || usage
[ -n "$expected" ] || usage
case "$expected" in
    *[!0-9a-f]*|'') fail "--expected-pubkey must be 64 lowercase hex characters" ;;
esac
[ "${#expected}" -eq 64 ] || fail "--expected-pubkey must be 64 lowercase hex characters"
case "$container_uid" in
    *[!0-9]*|'') fail "--container-uid must be numeric" ;;
esac

command -v docker >/dev/null 2>&1 || fail "docker is required"
docker exec -u "$container_uid" "$container" sh -c 'cat /proc/1/environ' |
    check_records "$expected"
printf 'PASS: container %s PID 1 has exactly %s=%s (read as uid %s)\n' \
    "$container" "$ENV_KEY" "$expected" "$container_uid"
