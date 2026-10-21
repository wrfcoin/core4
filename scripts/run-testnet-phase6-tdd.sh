#!/bin/bash

# WRFCoin Testnet Phase 6 Full TDD Suite
# Decentralized Governance & Ecosystem Resilience
#
# Starts a shared launch_api node (port 8556, features: p2p,zk) for TDD-6.1,
# 6.2, and 6.4 (Node.js tests), then runs the self-contained bash scripts for
# TDD-6.3 and 6.5 which manage their own P2P node pairs.
#
# Safety:
#   - Preflight cleanup kills stale nodes on well-known ports
#   - Each node is memory-capped via ulimit (see PHASE6_MEM_LIMIT_KB)
#   - Global 10-minute watchdog kills the entire suite if it hangs
#   - EXIT trap guarantees all child nodes are stopped and tmpdirs removed

set -uo pipefail

# ── Global watchdog ──────────────────────────────────────────────────────────
# Kill the entire process group after 10 minutes to prevent runaway hangs.
PHASE6_TIMEOUT_SECS="${PHASE6_TIMEOUT_SECS:-600}"
( sleep "$PHASE6_TIMEOUT_SECS" && echo "❌ Global timeout (${PHASE6_TIMEOUT_SECS}s) reached — killing suite" >&2 && kill -9 -$$ 2>/dev/null ) &
WATCHDOG_PID=$!

echo "🏁 Starting Testnet Phase 6 TDD Suite..."

# ── Resolve paths (worktree-safe) ────────────────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CORE4_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$CORE4_DIR"

source "$SCRIPT_DIR/phase6-network-common.sh"
phase6_setup_core4_context "$0"
phase6_load_operator_token
phase6_require_node_path

# ── Preflight safety ────────────────────────────────────────────────────────
phase6_preflight_cleanup

# ── Start shared node for Node.js tests ─────────────────────────────────────
SHARED_TMP="$(mktemp -d /tmp/wrfcoin-phase6-shared.XXXXXX)"
SHARED_LOG="$SHARED_TMP/shared-node.log"
SHARED_API="http://127.0.0.1:8556"
SHARED_PID=""

cleanup_shared() {
    kill "$WATCHDOG_PID" 2>/dev/null || true
    phase6_stop_pid "$SHARED_PID"
    rm -rf "$SHARED_TMP"
}
trap cleanup_shared EXIT

echo "   Starting shared launch_api node on :8556 (features: p2p,zk)..."
SHARED_PID="$(phase6_start_launch_api_node \
    shared \
    8556 \
    9876 \
    "$SHARED_TMP/data" \
    "$SHARED_LOG" \
    "" \
    "p2p,zk")"
phase6_wait_for_http "$SHARED_API/health" 120 1
echo "   Shared node ready (PID $SHARED_PID)"

# ── Run tests ────────────────────────────────────────────────────────────────

# TDD-6.1: Governance Lifecycle (uses shared node on 8556)
node tests/security/test-governance-lifecycle.cjs
G_RESULT=$?

# TDD-6.2: Validator Slashing (uses shared node on 8556)
node tests/security/test-validator-slashing.cjs
S_RESULT=$?

# TDD-6.4: ZK Performance (uses shared node on 8556)
node tests/stress/test-zk-batch.cjs --count 10
Z_RESULT=$?

# Stop shared node before P2P tests (free resources)
phase6_stop_pid "$SHARED_PID"
SHARED_PID=""
echo "   Shared node stopped"

# TDD-6.3: P2P Partition (manages its own nodes on 8656/8657)
bash scripts/test-p2p-partition.sh
P_RESULT=$?

# TDD-6.5: Node Onboarding (manages its own nodes on 8666/8667)
bash scripts/verify-node-onboarding.sh
O_RESULT=$?

# ── Summary ──────────────────────────────────────────────────────────────────
echo -e "\n=== Phase 6 TDD Summary ==="
[ $G_RESULT -eq 0 ] && echo "✅ TDD-6.1 Governance: PASS" || echo "❌ TDD-6.1 Governance: FAIL"
[ $S_RESULT -eq 0 ] && echo "✅ TDD-6.2 Slashing  : PASS" || echo "❌ TDD-6.2 Slashing  : FAIL"
[ $P_RESULT -eq 0 ] && echo "✅ TDD-6.3 P2P Part. : PASS" || echo "❌ TDD-6.3 P2P Part. : FAIL"
[ $Z_RESULT -eq 0 ] && echo "✅ TDD-6.4 ZK Scaling: PASS" || echo "❌ TDD-6.4 ZK Scaling: FAIL"
[ $O_RESULT -eq 0 ] && echo "✅ TDD-6.5 Onboarding: PASS" || echo "❌ TDD-6.5 Onboarding: FAIL"

if [ $((G_RESULT + S_RESULT + P_RESULT + Z_RESULT + O_RESULT)) -eq 0 ]; then
    echo -e "\n🎉 PHASE 6 TDD SUCCESSFUL"
    exit 0
else
    echo -e "\n🚨 PHASE 6 TDD FAILED"
    exit 1
fi
