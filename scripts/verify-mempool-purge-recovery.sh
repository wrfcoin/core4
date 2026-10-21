#!/bin/bash
set -euo pipefail

# Validation helper for core4 #1199.
# Confirms the mempool purge endpoint responds successfully and optionally
# checks that block production still advances after the purge.

API_BASE_URL="${WRFCOIN_API_BASE_URL:-http://127.0.0.1:8556}"
PURGE_PATH="${WRFCOIN_MEMPOOL_PURGE_PATH:-/mempool/purge}"
OPERATOR_TOKEN="${WRFCOIN_OPERATOR_TOKEN:-}"
MAX_AGE_SECS="${WRFCOIN_PURGE_MAX_AGE_SECS:-8100}"
WATCH_SECS=0

usage() {
    cat <<'EOF'
Usage: scripts/verify-mempool-purge-recovery.sh --operator-token TOKEN [--watch-secs N]

Examples:
  scripts/verify-mempool-purge-recovery.sh --operator-token "$WRFCOIN_OPERATOR_TOKEN"
  scripts/verify-mempool-purge-recovery.sh --operator-token "$WRFCOIN_OPERATOR_TOKEN" --watch-secs 180

Environment overrides:
  WRFCOIN_API_BASE_URL         launch_api base URL (default: http://127.0.0.1:8556)
  WRFCOIN_MEMPOOL_PURGE_PATH   purge route (default: /mempool/purge)
  WRFCOIN_PURGE_MAX_AGE_SECS   max_age_secs query value (default: 8100)
EOF
}

extract_metric() {
    local metric_name="$1"
    curl -fsS "${API_BASE_URL%/}/metrics" \
        | awk -v metric="$metric_name" '$1 == metric { print $2; exit }'
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --operator-token)
            OPERATOR_TOKEN="${2:?missing value for --operator-token}"
            shift 2
            ;;
        --watch-secs)
            WATCH_SECS="${2:?missing value for --watch-secs}"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "❌ Unknown argument: $1" >&2
            usage >&2
            exit 1
            ;;
    esac
done

if [ -z "$OPERATOR_TOKEN" ]; then
    echo "❌ --operator-token or WRFCOIN_OPERATOR_TOKEN is required" >&2
    exit 1
fi

before_height="$(extract_metric "wrfcoin_blockchain_block_height")"
before_mempool="$(extract_metric "wrfcoin_blockchain_mempool_size")"

if [ -z "$before_height" ] || [ -z "$before_mempool" ]; then
    echo "❌ Failed to read block height or mempool size from ${API_BASE_URL%/}/metrics" >&2
    exit 1
fi

echo "Before purge:"
echo "  block_height=${before_height}"
echo "  mempool_size=${before_mempool}"

response="$(
    curl -fsS -X POST \
        -H "Authorization: Bearer ${OPERATOR_TOKEN}" \
        "${API_BASE_URL%/}${PURGE_PATH}?max_age_secs=${MAX_AGE_SECS}"
)"

removed="$(printf '%s' "$response" | jq -r '.removed')"
remaining="$(printf '%s' "$response" | jq -r '.remaining')"

echo "Purge response:"
echo "  removed=${removed}"
echo "  remaining=${remaining}"

if [ -z "$removed" ] || [ -z "$remaining" ] || [ "$removed" = "null" ] || [ "$remaining" = "null" ]; then
    echo "❌ Purge response was not parseable JSON: $response" >&2
    exit 1
fi

after_mempool="$(extract_metric "wrfcoin_blockchain_mempool_size")"
echo "After purge:"
echo "  mempool_size=${after_mempool}"

if [ -z "$after_mempool" ]; then
    echo "❌ Failed to read post-purge mempool size from /metrics" >&2
    exit 1
fi

if [ "$after_mempool" -gt "$before_mempool" ]; then
    echo "❌ Mempool grew after purge (${before_mempool} -> ${after_mempool})" >&2
    exit 1
fi

if [ "$WATCH_SECS" -gt 0 ]; then
    echo "Watching ${WATCH_SECS}s to confirm block production continues..."
    sleep "$WATCH_SECS"
    after_height="$(extract_metric "wrfcoin_blockchain_block_height")"
    echo "Follow-up block height: ${after_height}"
    if [ -z "$after_height" ]; then
        echo "❌ Failed to read follow-up block height from /metrics" >&2
        exit 1
    fi
    if [ "$after_height" -le "$before_height" ]; then
        echo "❌ Block height did not advance after purge" >&2
        exit 1
    fi
fi

echo "✅ Mempool purge validation passed"
