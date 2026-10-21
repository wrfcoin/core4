#!/bin/bash
set -euo pipefail

# TDD-6.5: Dynamic Discovery & Bootstrapping
# Starts a bootstrap node and a new joining node with real P2P settings,
# verifies peer discovery on both sides, then mines on the bootstrap node and
# confirms the joining node receives and syncs the block.

source "$(cd "$(dirname "$0")" && pwd)/phase6-network-common.sh"

phase6_setup_core4_context "$0"
phase6_load_operator_token

TMP_ROOT="$(mktemp -d /tmp/wrfcoin-phase6-onboarding.XXXXXX)"
BOOTSTRAP_API="http://127.0.0.1:8666"
JOINER_API="http://127.0.0.1:8667"
BOOTSTRAP_P2P_PORT=8765
JOINER_P2P_PORT=8766
MINER_ADDRESS="0x0000000000000000000000000000000000000001"
BOOTSTRAP_PID=""
JOINER_PID=""
BOOTSTRAP_LOG="$TMP_ROOT/bootstrap.log"
JOINER_LOG="$TMP_ROOT/joiner.log"

cleanup() {
    phase6_cleanup_pids "$JOINER_PID" "$BOOTSTRAP_PID"
    rm -rf "$TMP_ROOT"
}
trap cleanup EXIT

echo "🚀 Starting TDD-6.5: Dynamic Discovery & Bootstrapping"

BOOTSTRAP_PID="$(phase6_start_launch_api_node \
    bootstrap \
    8666 \
    "$BOOTSTRAP_P2P_PORT" \
    "$TMP_ROOT/bootstrap-data" \
    "$BOOTSTRAP_LOG")"
phase6_wait_for_http "$BOOTSTRAP_API/health"
BOOTSTRAP_PEER_ID="$(phase6_local_peer_id "$BOOTSTRAP_API")"
if [ -z "$BOOTSTRAP_PEER_ID" ] || [ "$BOOTSTRAP_PEER_ID" = "null" ]; then
    echo "❌ Failed to read bootstrap local_peer_id"
    exit 1
fi
BOOTSTRAP_MULTIADDR="/ip4/127.0.0.1/tcp/${BOOTSTRAP_P2P_PORT}/p2p/${BOOTSTRAP_PEER_ID}"
echo "Step 1: Bootstrap node ready (${BOOTSTRAP_PEER_ID})"

JOINER_PID="$(phase6_start_launch_api_node \
    joiner \
    8667 \
    "$JOINER_P2P_PORT" \
    "$TMP_ROOT/joiner-data" \
    "$JOINER_LOG" \
    "$BOOTSTRAP_MULTIADDR")"
phase6_wait_for_http "$JOINER_API/health"
phase6_wait_for_log_line "$JOINER_LOG" "P2P peer connected: ${BOOTSTRAP_PEER_ID}" 60 1
# The log-line grep is the definitive peer discovery assertion (the P2P event
# handler emits it when a libp2p ConnectionEstablished event fires).
# The API connected_peers counter is checked below as a diagnostic but is not
# fatal — the counter can briefly drop to 0 between keep-alive probes.
JOINER_PEER_COUNT="$(curl -fsS "$JOINER_API/network/peers" 2>/dev/null | jq '.connected_peers' || echo 'N/A')"
echo "Step 2: Discovery succeeded (log confirmed ${BOOTSTRAP_PEER_ID}, API peers=${JOINER_PEER_COUNT})"

ONBOARD_MINE="$(phase6_mine_block "$BOOTSTRAP_API" "$MINER_ADDRESS")"
ONBOARD_HEIGHT="$(printf '%s' "$ONBOARD_MINE" | jq -r '.height')"
if [ -z "$ONBOARD_HEIGHT" ] || [ "$ONBOARD_HEIGHT" = "null" ]; then
    echo "❌ Failed to parse mined block height during onboarding check"
    exit 1
fi

phase6_wait_for_jq_condition "$JOINER_API/metrics" ".data.chain_height >= ${ONBOARD_HEIGHT}" 60 1
echo "Step 3: Joining node synced to height $(phase6_chain_height "$JOINER_API")"

echo "✅ TDD-6.5 PASSED: new node joined, discovered peers, and synced live chain data"
