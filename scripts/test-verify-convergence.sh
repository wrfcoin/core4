#!/bin/bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

mkdir -p "$tmp/bin"
cat > "$tmp/bin/curl" <<'EOF'
#!/bin/bash
set -euo pipefail
url="${!#}"
case "$url" in
    http://desktop/network/info|http://n5/network/info)
        printf '%s\n' '{"chain_height":100,"peer_count":1,"sync_status":"synced"}'
        ;;
    http://desktop/state/snapshot)
        printf '{"state_root_hex":"%s"}\n' "$DESKTOP_ROOT"
        ;;
    http://n5/state/snapshot)
        printf '{"state_root_hex":"%s"}\n' "$N5_ROOT"
        ;;
    *)
        echo "unexpected curl URL: $url" >&2
        exit 1
        ;;
esac
EOF
chmod +x "$tmp/bin/curl"

root_a="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
root_b="bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
run_gate() {
    PATH="$tmp/bin:$PATH" \
    DESKTOP_ROOT="$1" N5_ROOT="$2" \
    "$repo_root/scripts/verify-convergence.sh" --desktop http://desktop --n5 http://n5
}

equal_output="$(run_gate "$root_a" "$root_a")"
[[ "$equal_output" == *"OK"* ]] || {
    echo "equal-root control did not pass: $equal_output" >&2
    exit 1
}

mixed_case_output="$(run_gate "${root_a^^}" "$root_a")"
[[ "$mixed_case_output" == *"OK"* ]] || {
    echo "mixed-case equivalent roots did not pass: $mixed_case_output" >&2
    exit 1
}

if mismatch_output="$(run_gate "$root_a" "$root_b" 2>&1)"; then
    echo "divergent equal-height roots incorrectly passed: $mismatch_output" >&2
    exit 1
fi
[[ "$mismatch_output" == *"state_root_mismatch="* ]] || {
    echo "mismatch failure lacked state-root evidence: $mismatch_output" >&2
    exit 1
}

if unavailable_output="$(run_gate "" "$root_a" 2>&1)"; then
    echo "missing state root incorrectly passed: $unavailable_output" >&2
    exit 1
fi
[[ "$unavailable_output" == *"state_root_unavailable=http://desktop/state/snapshot"* ]] || {
    echo "missing state-root failure lacked endpoint diagnostic: $unavailable_output" >&2
    exit 1
}

echo "verify-convergence root equality controls passed"
