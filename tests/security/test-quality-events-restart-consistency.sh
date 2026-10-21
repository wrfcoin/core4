#!/usr/bin/env bash
# TDD-3C.5 – TDD-3C.6: Quality-events restart/observability consistency
#
# Verifies that the quality-events endpoint returns identical results:
#   TDD-3C.5  Idempotency – same query twice without restart (no state drift)
#   TDD-3C.6  Restart durability – same results after API process restart
#             (only when this script owns the API process; skipped otherwise)
#
# Exit 0 = all exercised checks PASSED
# Exit 1 = one or more checks FAILED
#
# Required env:
#   WRFCOIN_OPERATOR_TOKEN  – 64-char lowercase hex operator token
#
# Optional env:
#   WRFCOIN_API_HOST        – default: 127.0.0.1
#   WRFCOIN_API_PORT        – default: 8556
#   WRFCOIN_DATA_DIR        – persistent data dir for the launch_api process
#   WRFCOIN_NETWORK         – default: testnet
#   RUST_LOG                – default: info
#   WRFCOIN_PHASE3C_LOG_DIR – log directory; default: /tmp/wrfcoin-phase3c-restart

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CORE4_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"

API_HOST="${WRFCOIN_API_HOST:-127.0.0.1}"
API_PORT="${WRFCOIN_API_PORT:-8556}"
API_BASE="http://${API_HOST}:${API_PORT}"
API_HEALTH_URL="${API_BASE}/health"

LOG_DIR="${WRFCOIN_PHASE3C_LOG_DIR:-/tmp/wrfcoin-phase3c-restart}"
mkdir -p "$LOG_DIR"
RUN_TS="$(date +%Y%m%d-%H%M%S)"

LAUNCH_API_PID=""
STARTED_LAUNCH_API=0
overall_status=0

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

log()  { echo "[$(date +%H:%M:%S)] $*"; }
pass() { echo "PASS: $*"; }
fail() { echo "FAIL: $*" >&2; overall_status=1; }

require_commands() {
    for cmd in curl node; do
        if ! command -v "$cmd" &>/dev/null; then
            echo "ERROR: required command '$cmd' not found in PATH." >&2
            exit 1
        fi
    done
}

# Resolve token from env or .env files
resolve_operator_token() {
    if [ -n "${WRFCOIN_OPERATOR_TOKEN:-}" ]; then
        printf '%s' "$WRFCOIN_OPERATOR_TOKEN"
        return
    fi
    local env_candidates=(
        "$CORE4_DIR/.env"
        "$CORE4_DIR/../backend/.env"
    )
    for env_path in "${env_candidates[@]}"; do
        if [ -f "$env_path" ]; then
            local token
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
        echo "ERROR: WRFCOIN_OPERATOR_TOKEN is missing or invalid (need 64 lowercase hex chars)." >&2
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
    local label="${2:-launch_api}"
    local start now
    start="$(date +%s)"
    log "Waiting up to ${timeout_secs}s for ${label} to become healthy at ${API_HEALTH_URL}…"
    while true; do
        if api_is_healthy; then
            log "${label} is healthy."
            return 0
        fi
        now="$(date +%s)"
        if [ $((now - start)) -ge "$timeout_secs" ]; then
            echo "ERROR: ${label} did not become healthy within ${timeout_secs}s." >&2
            return 1
        fi
        sleep 2
    done
}

start_launch_api() {
    local log_file="$LOG_DIR/launch_api-${RUN_TS}.log"
    local pid_file="$LOG_DIR/launch_api-${RUN_TS}.pid"
    local data_dir="${WRFCOIN_DATA_DIR:-$LOG_DIR/data-${RUN_TS}}"

    log "Starting launch_api from $CORE4_DIR …"
    (
        cd "$CORE4_DIR"
        cargo build -p wrfcoin-native-chain --bin launch_api
    )
    (
        cd "$CORE4_DIR"
        nohup env \
            WRFCOIN_API_HOST="$API_HOST" \
            WRFCOIN_API_PORT="$API_PORT" \
            WRFCOIN_DATA_DIR="$data_dir" \
            WRFCOIN_NETWORK="${WRFCOIN_NETWORK:-testnet}" \
            WRFCOIN_OPERATOR_TOKEN="$WRFCOIN_OPERATOR_TOKEN" \
            RUST_LOG="${RUST_LOG:-info}" \
            "$CORE4_DIR/target/debug/launch_api" \
            >"$log_file" 2>&1 </dev/null &
        echo $! >"$pid_file"
    )
    LAUNCH_API_PID="$(cat "$LOG_DIR/launch_api-${RUN_TS}.pid")"
    STARTED_LAUNCH_API=1
    wait_for_api_health 300 "launch_api (initial)"
}

stop_launch_api() {
    if [ -n "$LAUNCH_API_PID" ] && kill -0 "$LAUNCH_API_PID" 2>/dev/null; then
        log "Stopping launch_api PID $LAUNCH_API_PID …"
        kill "$LAUNCH_API_PID" 2>/dev/null || true
        for _ in $(seq 1 100); do
            if ! kill -0 "$LAUNCH_API_PID" 2>/dev/null; then
                break
            fi
            sleep 0.1
        done
        if kill -0 "$LAUNCH_API_PID" 2>/dev/null; then
            echo "ERROR: launch_api PID $LAUNCH_API_PID did not stop within 10s." >&2
            return 1
        fi
        LAUNCH_API_PID=""
    fi
}

cleanup() {
    if [ "$STARTED_LAUNCH_API" -eq 1 ]; then
        stop_launch_api
    fi
}
trap cleanup EXIT

ensure_launch_api() {
    if api_is_healthy; then
        log "Reusing existing launch_api at ${API_HEALTH_URL}"
        return 0
    fi
    start_launch_api
}

# ---------------------------------------------------------------------------
# Snapshot helpers (parse JSON with inline Node.js to avoid extra deps)
# ---------------------------------------------------------------------------

# Fetch quality events (all, unfiltered) and write a snapshot JSON to a file.
# Usage: snapshot_events <output_file>
snapshot_events() {
    local out_file="$1"
    curl -sf \
        -H "Authorization: Bearer ${WRFCOIN_OPERATOR_TOKEN}" \
        "${API_BASE}/transactions/quality/events?offset=0&limit=500" \
        -o "$out_file"
}

# Extract scalar field from JSON file using Node.js
# Usage: json_field <file> <field_path_in_js>
json_field() {
    local file="$1"
    local expr="$2"
    node -e "
const d = JSON.parse(require('fs').readFileSync('${file}', 'utf8'));
console.log(${expr});
"
}

# Produce a sorted comma-separated list of event_ids from a snapshot file.
event_id_list() {
    local file="$1"
    node -e "
const d = JSON.parse(require('fs').readFileSync('${file}', 'utf8'));
const ids = (d.items || []).map(e => e.event_id).sort((a,b)=>a-b);
console.log(ids.join(','));
"
}

# Compare two snapshot files; set overall_status on mismatch.
# Usage: compare_snapshots <label> <file1> <file2>
compare_snapshots() {
    local label="$1"
    local file1="$2"
    local file2="$3"
    local passed=1

    local total1 total2
    total1="$(json_field "$file1" 'd.total')"
    total2="$(json_field "$file2" 'd.total')"

    if [ "$total1" != "$total2" ]; then
        fail "${label}: total changed (${total1} → ${total2})"
        passed=0
    else
        log "${label}: total stable (${total1})"
    fi

    local ids1 ids2
    ids1="$(event_id_list "$file1")"
    ids2="$(event_id_list "$file2")"

    if [ "$ids1" != "$ids2" ]; then
        fail "${label}: event_id lists differ"
        # Show first 10 diverging IDs for diagnostics
        echo "  Before: ${ids1:0:200}" >&2
        echo "  After:  ${ids2:0:200}" >&2
        passed=0
    else
        log "${label}: event_id lists identical"
    fi

    if [ "$passed" -eq 1 ]; then
        pass "${label}"
    fi
}

# ---------------------------------------------------------------------------
# Token setup
# ---------------------------------------------------------------------------

require_commands
WRFCOIN_OPERATOR_TOKEN="$(resolve_operator_token)"
require_valid_token "$WRFCOIN_OPERATOR_TOKEN"
export WRFCOIN_OPERATOR_TOKEN

log "==> Phase 3C Restart-Consistency Gate"
log "API target: ${API_BASE}"

ensure_launch_api

# ---------------------------------------------------------------------------
# TDD-3C.5 – Idempotency: two sequential queries must return identical results
# ---------------------------------------------------------------------------

log ""
log "--- TDD-3C.5: Idempotency (no restart) ---"

SNAP1="${LOG_DIR}/snap1-${RUN_TS}.json"
SNAP2="${LOG_DIR}/snap2-${RUN_TS}.json"

snapshot_events "$SNAP1"
log "Baseline snapshot written ($(json_field "$SNAP1" 'd.total') events)"

# Brief pause to ensure no in-flight block commits change the window
sleep 1

snapshot_events "$SNAP2"
log "Re-query snapshot written ($(json_field "$SNAP2" 'd.total') events)"

compare_snapshots "TDD-3C.5" "$SNAP1" "$SNAP2"

# ---------------------------------------------------------------------------
# TDD-3C.6 – Restart durability (only when we own the API process)
# ---------------------------------------------------------------------------

log ""
log "--- TDD-3C.6: Restart durability ---"

if [ "$STARTED_LAUNCH_API" -eq 0 ]; then
    log "SKIP TDD-3C.6: using external API (PID not managed by this script)"
    log "  To exercise TDD-3C.6, ensure no launch_api is running before invoking this script."
else
    SNAP_PRE="${LOG_DIR}/snap-pre-restart-${RUN_TS}.json"
    SNAP_POST="${LOG_DIR}/snap-post-restart-${RUN_TS}.json"

    snapshot_events "$SNAP_PRE"
    log "Pre-restart snapshot ($(json_field "$SNAP_PRE" 'd.total') events)"

    # Restart the API
    stop_launch_api
    log "launch_api stopped; restarting…"
    start_launch_api

    snapshot_events "$SNAP_POST"
    log "Post-restart snapshot ($(json_field "$SNAP_POST" 'd.total') events)"

    compare_snapshots "TDD-3C.6" "$SNAP_PRE" "$SNAP_POST"
fi

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------

log ""
log "========================================"
if [ "$overall_status" -eq 0 ]; then
    log "Phase 3C Restart-Consistency Gate: PASSED"
else
    log "Phase 3C Restart-Consistency Gate: FAILED" >&2
fi

exit "$overall_status"
