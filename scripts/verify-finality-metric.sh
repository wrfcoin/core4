#!/bin/bash
set -euo pipefail

# Validation helper for core4 #1198.
# Confirms the launch_api node exposes a non-zero finalized height, reports the
# current tip/finality gap baseline, and optionally verifies that Prometheus is
# scraping the metric and that finality advances over a watch interval.

NODE_METRICS_URL="${WRFCOIN_NODE_METRICS_URL:-http://127.0.0.1:8556/metrics}"
PROMETHEUS_URL="${WRFCOIN_PROMETHEUS_URL:-}"
WATCH_SECS=0

usage() {
    cat <<'EOF'
Usage: scripts/verify-finality-metric.sh [--watch-secs N] [--prometheus-url URL] [--node-metrics-url URL]

Examples:
  scripts/verify-finality-metric.sh
  scripts/verify-finality-metric.sh --watch-secs 180
  scripts/verify-finality-metric.sh --prometheus-url http://127.0.0.1:9090

Environment overrides:
  WRFCOIN_NODE_METRICS_URL   launch_api /metrics endpoint
  WRFCOIN_PROMETHEUS_URL     Prometheus base URL for scrape verification
EOF
}

extract_metric_from_body() {
    local body="$1"
    local metric_name="$2"
    printf '%s\n' "$body" | awk -v metric="$metric_name" '$1 == metric { print $2; exit }'
}

sample_node_metrics() {
    local body
    body="$(curl -fsS "$NODE_METRICS_URL")"

    local tip_height
    local finalized_height
    tip_height="$(extract_metric_from_body "$body" "wrfcoin_blockchain_block_height")"
    finalized_height="$(extract_metric_from_body "$body" "wrfcoin_chain_finalized_height")"

    if [ -z "$tip_height" ] || [ -z "$finalized_height" ]; then
        echo "❌ Failed to parse block/finality metrics from $NODE_METRICS_URL" >&2
        exit 1
    fi

    printf '%s %s\n' "$tip_height" "$finalized_height"
}

query_prometheus_scalar() {
    local query="$1"
    curl -fsS --get \
        --data-urlencode "query=${query}" \
        "${PROMETHEUS_URL%/}/api/v1/query" \
        | jq -r '.data.result[0].value[1] // empty'
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --watch-secs)
            WATCH_SECS="${2:?missing value for --watch-secs}"
            shift 2
            ;;
        --prometheus-url)
            PROMETHEUS_URL="${2:?missing value for --prometheus-url}"
            shift 2
            ;;
        --node-metrics-url)
            NODE_METRICS_URL="${2:?missing value for --node-metrics-url}"
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

read -r initial_tip initial_finalized <<<"$(sample_node_metrics)"
initial_gap=$((initial_tip - initial_finalized))

echo "Node metrics:"
echo "  tip_height=${initial_tip}"
echo "  finalized_height=${initial_finalized}"
echo "  finality_gap=${initial_gap}"

if [ "$initial_finalized" -le 0 ]; then
    echo "❌ wrfcoin_chain_finalized_height is still 0; live finality verification has not passed" >&2
    exit 1
fi

if [ "$initial_tip" -gt 100 ] && [ "$initial_gap" -lt 100 ]; then
    echo "❌ Finality gap ${initial_gap} is below MAX_REORG_DEPTH=100; metric wiring looks inconsistent" >&2
    exit 1
fi

if [ "$initial_gap" -gt 150 ]; then
    echo "❌ Finality gap ${initial_gap} exceeds the FinalityLag alert threshold (>150)" >&2
    exit 1
fi

echo "Baseline finality lag is ${initial_gap} block(s)."

if [ -n "$PROMETHEUS_URL" ]; then
    prom_finalized="$(query_prometheus_scalar "wrfcoin_chain_finalized_height")"
    prom_gap="$(query_prometheus_scalar "wrfcoin_blockchain_block_height - wrfcoin_chain_finalized_height")"

    if [ -z "$prom_finalized" ] || [ -z "$prom_gap" ]; then
        echo "❌ Prometheus did not return finalized-height data from ${PROMETHEUS_URL}" >&2
        exit 1
    fi

    echo "Prometheus scrape:"
    echo "  finalized_height=${prom_finalized}"
    echo "  finality_gap=${prom_gap}"
fi

if [ "$WATCH_SECS" -gt 0 ]; then
    echo "Watching for ${WATCH_SECS}s to confirm finalized height advances..."
    sleep "$WATCH_SECS"

    read -r later_tip later_finalized <<<"$(sample_node_metrics)"
    later_gap=$((later_tip - later_finalized))

    echo "Follow-up sample:"
    echo "  tip_height=${later_tip}"
    echo "  finalized_height=${later_finalized}"
    echo "  finality_gap=${later_gap}"

    if [ "$later_tip" -le "$initial_tip" ]; then
        echo "❌ Block height did not advance during the watch window" >&2
        exit 1
    fi

    if [ "$later_finalized" -le "$initial_finalized" ]; then
        echo "❌ Finalized height did not advance during the watch window" >&2
        exit 1
    fi
fi

echo "✅ Finality metric validation passed"
