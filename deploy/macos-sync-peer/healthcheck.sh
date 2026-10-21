#!/usr/bin/env bash
set -euo pipefail
readonly API_BASE="${WRFCOIN_SYNC_PEER_API_BASE:-http://127.0.0.1:8556}"
readonly EXPECTED_NETWORK=testnet
readonly EXPECTED_CHAIN=777
readonly EXPECTED_GENESIS=0x805efc973fe50829ae2abf7c73bde0a5ed8511f3dfa97ff50a86615703442625
[[ "$API_BASE" == http://127.0.0.1:8556 ]] || { printf 'FAIL: health endpoint must remain local\n' >&2; exit 1; }

sync="$(curl --fail --silent --show-error "$API_BASE/node/sync/status")"
network="$(curl --fail --silent --show-error "$API_BASE/network/info")"
python3 - "$sync" "$network" "$EXPECTED_NETWORK" "$EXPECTED_CHAIN" "$EXPECTED_GENESIS" <<'PY'
import json, sys
sync, network = map(json.loads, sys.argv[1:3])
expected_network, expected_chain, expected_genesis = sys.argv[3:]
if network.get("network_id") != expected_network or str(network.get("chain_id")) != expected_chain or network.get("genesis_hash") != expected_genesis:
    raise SystemExit("FAIL: network identity mismatch")
if sync.get("sync_status") != "synced" or int(sync.get("peer_count", 0)) < 1 or int(sync.get("chain_height", 0)) < 1:
    raise SystemExit("FAIL: peer is not synced with at least one peer at a positive height")
print("PASS: frozen identity and sync status verified")
PY
