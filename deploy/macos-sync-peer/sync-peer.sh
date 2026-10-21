#!/usr/bin/env bash
# Launch wrapper for the sync-only peer. It deliberately constructs a minimal
# environment, so ambient role-bearing variables cannot reach the node.
set -euo pipefail

readonly PEER_HOME="${WRFCOIN_SYNC_PEER_HOME:-$HOME/Library/Application Support/WRFCoinSyncPeer}"
readonly CONFIG_PATH="${WRFCOIN_SYNC_PEER_CONFIG:-$PEER_HOME/sync-peer.conf}"
readonly NODE_PATH="$PEER_HOME/bin/wrfcoin-node"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/common.sh"

validate_config "$CONFIG_PATH"
if [[ "${1:-}" == --validate-config ]]; then
  printf 'sync-peer configuration is valid\n'
  exit 0
fi
[[ -x "$NODE_PATH" ]] || die "installed node executable is missing: $NODE_PATH"

exec /usr/bin/env -i \
  HOME="$HOME" \
  PATH='/usr/local/bin:/usr/bin:/bin' \
  WRFCOIN_NETWORK_ID="$WRFCOIN_NETWORK_ID" \
  WRFCOIN_CHAIN_ID="$WRFCOIN_CHAIN_ID" \
  WRFCOIN_GENESIS_HASH="$WRFCOIN_GENESIS_HASH" \
  WRFCOIN_P2P_BOOTSTRAP="$WRFCOIN_P2P_BOOTSTRAP" \
  WRFCOIN_API_ADDR="${WRFCOIN_API_ADDR:-127.0.0.1:8556}" \
  WRFCOIN_SYNC_ONLY=true WRFCOIN_PRODUCER_MODE=false WRFCOIN_ORACLE_MODE=false \
  "$NODE_PATH"
