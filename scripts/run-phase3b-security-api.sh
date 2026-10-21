#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CORE4_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
WORKSPACE_DIR="$(cd "$CORE4_DIR/.." && pwd)"

API_HOST="${WRFCOIN_API_HOST:-127.0.0.1}"
API_PORT="${WRFCOIN_API_PORT:-8556}"
API_HEALTH_URL="http://${API_HOST}:${API_PORT}/health"
LOG_DIR="${WRFCOIN_PHASE3B_LOG_DIR:-/tmp/wrfcoin-phase3b-security-api}"
mkdir -p "$LOG_DIR"
RUN_TS="$(date +%Y%m%d-%H%M%S)"
API_LOG_FILE="$LOG_DIR/launch_api-${RUN_TS}.log"
DEFAULT_DATA_DIR="$LOG_DIR/data-${RUN_TS}"

LAUNCH_API_PID=""
STARTED_LAUNCH_API=0

if [ -f "$WORKSPACE_DIR/tests/security/test-quality-slashing.cjs" ] && [ -f "$WORKSPACE_DIR/tests/security/test-reputation-decay.cjs" ]; then
  SECURITY_TEST_DIR="$WORKSPACE_DIR/tests/security"
elif [ -f "$CORE4_DIR/tests/security/test-quality-slashing.cjs" ] && [ -f "$CORE4_DIR/tests/security/test-reputation-decay.cjs" ]; then
  SECURITY_TEST_DIR="$CORE4_DIR/tests/security"
else
  echo "ERROR: Could not locate Phase 3B security tests."
  echo "Checked:"
  echo "  - $WORKSPACE_DIR/tests/security"
  echo "  - $CORE4_DIR/tests/security"
  exit 1
fi

resolve_operator_token() {
  if [ -n "${WRFCOIN_OPERATOR_TOKEN:-}" ]; then
    printf '%s' "$WRFCOIN_OPERATOR_TOKEN"
    return
  fi

  local env_candidates token env_path
  env_candidates=(
    "$CORE4_DIR/.env"
    "$WORKSPACE_DIR/core4/.env"
    "$WORKSPACE_DIR/backend/.env"
    "$CORE4_DIR/../backend/.env"
  )

  for env_path in "${env_candidates[@]}"; do
    if [ -f "$env_path" ]; then
      token=$(awk -F= '/^WRFCOIN_OPERATOR_TOKEN=/{v=$2} END{print v}' "$env_path")
      if [ -n "$token" ]; then
        printf '%s' "$token"
        return
      fi
    fi
  done

  printf ''
}

require_valid_token() {
  local token="$1"
  if ! printf '%s' "$token" | grep -Eq '^[0-9a-f]{64}$'; then
    echo "ERROR: WRFCOIN_OPERATOR_TOKEN is missing/invalid (expected 64 lowercase hex chars)."
    exit 1
  fi
}

api_is_healthy() {
  local code
  code="$(curl -s -o /dev/null -w '%{http_code}' "$API_HEALTH_URL" || true)"
  [[ "$code" =~ ^2[0-9][0-9]$ ]]
}

wait_for_api_health() {
  local timeout_secs="$1"
  local start now
  start="$(date +%s)"

  while true; do
    if api_is_healthy; then
      echo "launch_api health check passed at $API_HEALTH_URL"
      return 0
    fi

    now="$(date +%s)"
    if [ $((now - start)) -ge "$timeout_secs" ]; then
      echo "ERROR: launch_api did not become healthy within ${timeout_secs}s."
      echo "Log file: $API_LOG_FILE"
      return 1
    fi

    sleep 2
  done
}

cleanup() {
  if [ "$STARTED_LAUNCH_API" -eq 1 ] && [ -n "$LAUNCH_API_PID" ]; then
    if kill -0 "$LAUNCH_API_PID" 2>/dev/null; then
      echo "Stopping launch_api PID $LAUNCH_API_PID"
      kill "$LAUNCH_API_PID" 2>/dev/null || true
      wait "$LAUNCH_API_PID" 2>/dev/null || true
    fi
  fi
}
trap cleanup EXIT

ensure_launch_api() {
  if api_is_healthy; then
    echo "Reusing existing launch_api at $API_HEALTH_URL"
    return 0
  fi

  echo "launch_api not healthy; starting local instance from $CORE4_DIR"
  (
    cd "$CORE4_DIR"
    nohup env \
      WRFCOIN_API_HOST="$API_HOST" \
      WRFCOIN_API_PORT="$API_PORT" \
      WRFCOIN_DATA_DIR="${WRFCOIN_DATA_DIR:-$DEFAULT_DATA_DIR}" \
      WRFCOIN_NETWORK="${WRFCOIN_NETWORK:-testnet}" \
      WRFCOIN_OPERATOR_TOKEN="$WRFCOIN_OPERATOR_TOKEN" \
      RUST_LOG="${RUST_LOG:-info}" \
      cargo run -p wrfcoin-native-chain --bin launch_api >"$API_LOG_FILE" 2>&1 < /dev/null &
    echo $! > "$LOG_DIR/launch_api-${RUN_TS}.pid"
  )

  LAUNCH_API_PID="$(cat "$LOG_DIR/launch_api-${RUN_TS}.pid")"
  STARTED_LAUNCH_API=1
  wait_for_api_health 300
}

run_test() {
  local test_file="$1"
  echo "==> Running ${test_file}"
  if node "$test_file"; then
    echo "PASS: ${test_file}"
    return 0
  fi

  echo "FAIL: ${test_file}"
  return 1
}

echo "==> Phase 3B API Security Gate"
echo "Security test directory: $SECURITY_TEST_DIR"

WRFCOIN_OPERATOR_TOKEN="$(resolve_operator_token)"
require_valid_token "$WRFCOIN_OPERATOR_TOKEN"
export WRFCOIN_OPERATOR_TOKEN

ensure_launch_api

overall_status=0

if ! run_test "$SECURITY_TEST_DIR/test-quality-slashing.cjs"; then
  overall_status=1
fi

if ! run_test "$SECURITY_TEST_DIR/test-reputation-decay.cjs"; then
  overall_status=1
fi

if [ "$overall_status" -eq 0 ]; then
  echo "==> Phase 3B API Security Gate PASSED"
else
  echo "==> Phase 3B API Security Gate FAILED"
  echo "launch_api log: $API_LOG_FILE"
fi

exit "$overall_status"
