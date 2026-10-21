#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

HOST="${WRFCOIN_API_HOST:-127.0.0.1}"
PORT="${WRFCOIN_API_PORT:-8556}"
TOKEN="${WRFCOIN_OPERATOR_TOKEN:-}"
BURST_COUNT=1000
SDK_COUNT=100
STATIONS=2
READINGS=5

usage() {
  cat <<'EOF'
Usage: bash scripts/run-testnet-phase5-tdd.sh [options]

Options:
  --host <value>         WRFCoin API host (default: 127.0.0.1)
  --port <value>         WRFCoin API port (default: 8556)
  --token <value>        Operator token (env: WRFCOIN_OPERATOR_TOKEN)
  --burst-count <int>    TDD-5.1 transfer burst count (default: 1000)
  --sdk-count <int>      TDD-5.5 transfer burst count (default: 100)
  --stations <int>       TDD-5.2 station count (default: 2)
  --readings <int>       TDD-5.2 readings per station (default: 5)
  --help                 Show this help message
EOF
}

parse_value() {
  local current="$1"
  local next="${2:-}"
  if [[ "$current" == *=* ]]; then
    printf '%s' "${current#*=}"
    return 0
  fi
  if [[ -z "$next" || "$next" == --* ]]; then
    echo "Missing value for $current" >&2
    exit 1
  fi
  printf '%s' "$next"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --host|--host=*)
      value="$(parse_value "$1" "${2:-}")"
      HOST="$value"
      [[ "$1" == *=* ]] || shift
      ;;
    --port|--port=*)
      value="$(parse_value "$1" "${2:-}")"
      PORT="$value"
      [[ "$1" == *=* ]] || shift
      ;;
    --token|--token=*)
      value="$(parse_value "$1" "${2:-}")"
      TOKEN="$value"
      [[ "$1" == *=* ]] || shift
      ;;
    --burst-count|--burst-count=*)
      value="$(parse_value "$1" "${2:-}")"
      BURST_COUNT="$value"
      [[ "$1" == *=* ]] || shift
      ;;
    --sdk-count|--sdk-count=*)
      value="$(parse_value "$1" "${2:-}")"
      SDK_COUNT="$value"
      [[ "$1" == *=* ]] || shift
      ;;
    --stations|--stations=*)
      value="$(parse_value "$1" "${2:-}")"
      STATIONS="$value"
      [[ "$1" == *=* ]] || shift
      ;;
    --readings|--readings=*)
      value="$(parse_value "$1" "${2:-}")"
      READINGS="$value"
      [[ "$1" == *=* ]] || shift
      ;;
    --help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
  shift
done

if [[ -z "$TOKEN" && -f ".env" ]]; then
  TOKEN="$(awk -F= '/^WRFCOIN_OPERATOR_TOKEN=/{v=$2} END{print v}' .env)"
fi

if [[ -z "$TOKEN" ]]; then
  echo "Missing operator token. Set WRFCOIN_OPERATOR_TOKEN or pass --token." >&2
  exit 1
fi

export WRFCOIN_API_HOST="$HOST"
export WRFCOIN_API_PORT="$PORT"
export WRFCOIN_OPERATOR_TOKEN="$TOKEN"

echo "🏁 Starting Testnet Phase 5 TDD Suite..."
echo "ℹ️  Runtime: api=http://${HOST}:${PORT}"
echo "ℹ️  Operator token: ${TOKEN:0:4}…${TOKEN: -4}"
echo "ℹ️  burst-count=${BURST_COUNT} sdk-count=${SDK_COUNT} stations=${STATIONS} readings-per-station=${READINGS}"

set +e
node tests/stress/test-burst-transfers.cjs --host "$HOST" --port "$PORT" --count "$BURST_COUNT"
BURST_STATUS=$?

node tests/security/test-batch-rewards.cjs --host "$HOST" --port "$PORT" --readings "$READINGS" --stations "$STATIONS"
REWARD_STATUS=$?

node tests/security/test-emergency-protocol.cjs --host "$HOST" --port "$PORT"
EMERGENCY_STATUS=$?

node tests/security/test-snapshot-sync.cjs --host "$HOST" --port "$PORT"
SNAPSHOT_STATUS=$?

node tests/stress/test-burst-transfers.cjs --host "$HOST" --port "$PORT" --count "$SDK_COUNT"
SDK_STATUS=$?
set -e

echo
echo "=== Phase 5 TDD Summary ==="
[[ $BURST_STATUS -eq 0 ]] && echo "✅ TDD-5.1 Burst: PASS" || echo "❌ TDD-5.1 Burst: FAIL"
[[ $REWARD_STATUS -eq 0 ]] && echo "✅ TDD-5.2 Rewards: PASS" || echo "❌ TDD-5.2 Rewards: FAIL"
[[ $EMERGENCY_STATUS -eq 0 ]] && echo "✅ TDD-5.3 Emergency: PASS" || echo "❌ TDD-5.3 Emergency: FAIL"
[[ $SNAPSHOT_STATUS -eq 0 ]] && echo "✅ TDD-5.4 Snapshot: PASS" || echo "❌ TDD-5.4 Snapshot: FAIL"
[[ $SDK_STATUS -eq 0 ]] && echo "✅ TDD-5.5 SDK: PASS" || echo "❌ TDD-5.5 SDK: FAIL"

TOTAL_STATUS=$((BURST_STATUS + REWARD_STATUS + EMERGENCY_STATUS + SNAPSHOT_STATUS + SDK_STATUS))
if [[ $TOTAL_STATUS -eq 0 ]]; then
  echo
  echo "🎉 PHASE 5 TDD SUCCESSFUL"
  exit 0
fi

echo
echo "🚨 PHASE 5 TDD FAILED"
exit 1
