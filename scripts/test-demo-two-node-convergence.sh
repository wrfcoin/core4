#!/bin/bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
demo="$repo_root/scripts/demo-two-node-convergence.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

run_demo() {
    local arm="$1"
    WRFCOIN_TWONODE_DEMO_DIR="$tmp/$arm" \
    WRFCOIN_TWONODE_SYNC_ATTEMPTS=3 \
    WRFCOIN_TWONODE_SYNC_SLEEP=1 \
        "$demo" "$arm"
}

# --- port-override control (#2612 review: Grace Tier-1 + the retracted APPROVE) ---
# The four port bindings were assigned unconditionally, so the collision guard's
# advertised recovery ("set A_API/B_API/A_P2P/B_P2P to an unused range") was
# impossible: the override was discarded before the guard read it. Asserting the
# preflight MESSAGE alone would not have caught it either — the override must
# reach the launch commands and the bootstrap multiaddr too.
# The demo refuses to run without built binaries (it checks them before the port
# guard), so an unbuilt tree would make this control fail for a reason that has
# nothing to do with port handling. Skip loudly instead of reporting a false
# failure — a control that fails for the wrong reason is worse than no control.
_tgt="${CARGO_TARGET_DIR:-$repo_root/target}/debug"
if [ ! -x "$_tgt/launch_api" ] || [ ! -x "$_tgt/wrfcoin" ]; then
    echo "SKIP port-override control: launch_api/wrfcoin not built (cargo build --bin launch_api --bin wrfcoin)" >&2
else
port_free() { ! ss -ltn 2>/dev/null | grep -qE "[:.]$1[[:space:]]"; }
for p in 18590 18591 19030 19031; do
    port_free "$p" || { echo "control needs $p free; skipping override control" >&2; exit 0; }
done

# Hold the DEFAULT A_API so a discarded override fails loudly on it.
python3 -c "
import socket,time,sys
s=socket.socket(); s.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1)
s.bind(('127.0.0.1',18576)); s.listen(1); sys.stderr.write('held\n'); sys.stderr.flush(); time.sleep(25)
" 2>/dev/null &
holder=$!
trap 'kill "$holder" 2>/dev/null || true; rm -rf "$tmp"' EXIT
sleep 1

plan="$(A_API=18590 B_API=18591 A_P2P=19030 B_P2P=19031 \
        WRFCOIN_TWONODE_PRINT_PLAN=1 WRFCOIN_TWONODE_DEMO_DIR="$tmp/plan" \
        "$demo" real 2>&1)" || {
    echo "override control FAILED: the documented recovery path does not work." >&2
    printf '%s\n' "$plan" >&2
    exit 1
}
# The override must reach ALL FOUR consumers, not just the preflight.
[[ "$plan" == *"guard_ports=18590 18591 19030 19031"* ]] || { echo "override did not reach the collision guard:"; printf '%s\n' "$plan"; exit 1; }
[[ "$plan" == *"start_node_a=a 18590 19030"* ]] || { echo "override did not reach node A's launch command:"; printf '%s\n' "$plan"; exit 1; }
[[ "$plan" == *"start_node_b=b 18591 19031"* ]] || { echo "override did not reach node B's launch command:"; printf '%s\n' "$plan"; exit 1; }
[[ "$plan" == *"bootstrap_prefix=/ip4/127.0.0.1/tcp/19030"* ]] || { echo "override did not reach the bootstrap multiaddr:"; printf '%s\n' "$plan"; exit 1; }
# And the defaults must still be defaults when nothing is exported.
kill "$holder" 2>/dev/null || true; sleep 1
default_plan="$(WRFCOIN_TWONODE_PRINT_PLAN=1 WRFCOIN_TWONODE_DEMO_DIR="$tmp/plan2" "$demo" real 2>&1)" || true
[[ "$default_plan" == *"guard_ports=18576 18577 19020 19021"* ]] || { echo "defaults regressed when no override is set:"; printf '%s\n' "$default_plan"; exit 1; }
echo "port-override control passed: override reaches guard, both launch commands, and the bootstrap multiaddr; defaults intact"
fi


real_output="$(run_demo real 2>&1)" || {
    echo "real two-node convergence arm failed:" >&2
    printf '%s\n' "$real_output" >&2
    exit 1
}
[[ "$real_output" == *"CONVERGED: state_root identical"* ]] || {
    echo "real arm did not prove state-root equality:" >&2
    printf '%s\n' "$real_output" >&2
    exit 1
}

isolated_output="$(run_demo isolated 2>&1)" || {
    echo "isolated red control did not complete:" >&2
    printf '%s\n' "$isolated_output" >&2
    exit 1
}
[[ "$isolated_output" == *"PASS (RED control):"* && "$isolated_output" == *"state_root mismatch"* ]] || {
    echo "isolated pair did not produce a state-root divergence verdict:" >&2
    printf '%s\n' "$isolated_output" >&2
    exit 1
}

echo "two-node state-root convergence real and isolated controls passed"
