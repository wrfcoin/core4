#!/bin/bash
set -euo pipefail

# TDD-6.3: P2P Partition Recovery
# Starts two real launch_api nodes with `--features p2p`, disconnects the
# joining node, mines on the bootstrap side while the peer is isolated, then
# restarts the joining node and verifies it reconnects and syncs back up.

source "$(cd "$(dirname "$0")" && pwd)/phase6-network-common.sh"

phase6_setup_core4_context "$0"
phase6_load_operator_token

TMP_ROOT="$(mktemp -d /tmp/wrfcoin-phase6-partition.XXXXXX)"
BOOTSTRAP_API="http://127.0.0.1:8656"
JOINER_API="http://127.0.0.1:8657"
BOOTSTRAP_P2P_PORT=8755
JOINER_P2P_PORT=8756
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

echo "🚀 Starting TDD-6.3: P2P Partition Recovery"

BOOTSTRAP_PID="$(phase6_start_launch_api_node \
    bootstrap \
    8656 \
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
    8657 \
    "$JOINER_P2P_PORT" \
    "$TMP_ROOT/joiner-data" \
    "$JOINER_LOG" \
    "$BOOTSTRAP_MULTIADDR")"
phase6_wait_for_http "$JOINER_API/health"
phase6_wait_for_log_line "$JOINER_LOG" "P2P peer connected: ${BOOTSTRAP_PEER_ID}" 60 1
echo "Step 2: Joining node discovered the bootstrap peer"

phase6_stop_pid "$JOINER_PID"
JOINER_PID=""
phase6_wait_for_http_down "$JOINER_API/health" 30 1
sleep 2
echo "Step 3: Simulated partition by stopping the joining node"

PARTITION_MINE="$(phase6_mine_block "$BOOTSTRAP_API" "$MINER_ADDRESS")"
PARTITION_HEIGHT="$(printf '%s' "$PARTITION_MINE" | jq -r '.height')"
if [ -z "$PARTITION_HEIGHT" ] || [ "$PARTITION_HEIGHT" = "null" ]; then
    echo "❌ Failed to parse mined block height during partition"
    exit 1
fi
echo "Step 4: Bootstrap node advanced alone to height $PARTITION_HEIGHT"

JOINER_PID="$(phase6_start_launch_api_node \
    joiner \
    8657 \
    "$JOINER_P2P_PORT" \
    "$TMP_ROOT/joiner-data" \
    "$JOINER_LOG" \
    "$BOOTSTRAP_MULTIADDR")"
phase6_wait_for_http "$JOINER_API/health" 90 1
phase6_wait_for_log_line "$JOINER_LOG" "P2P peer connected: ${BOOTSTRAP_PEER_ID}" 90 1
phase6_wait_for_jq_condition "$JOINER_API/metrics" ".data.chain_height >= ${PARTITION_HEIGHT}" 90 1
echo "Step 5: Joining node reconnected and synced to height $(phase6_chain_height "$JOINER_API")"

echo "✅ TDD-6.3 PASSED: partitioned peer rejoined and synced after recovery"
