#!/bin/bash

# ── Resource limits ──────────────────────────────────────────────────────────
# Cap per-node virtual memory to 4 GB (enough for launch_api + ZK proving,
# prevents a runaway node from OOM-killing the system).
# Override with PHASE6_MEM_LIMIT_KB (in KB).
PHASE6_MEM_LIMIT_KB="${PHASE6_MEM_LIMIT_KB:-4194304}"   # 4 GB

# ── Context helpers ──────────────────────────────────────────────────────────

phase6_setup_core4_context() {
    local script_path="$1"
    PHASE6_SCRIPT_DIR="$(cd "$(dirname "$script_path")" && pwd)"
    PHASE6_CORE4_DIR="$(cd "$PHASE6_SCRIPT_DIR/.." && pwd)"
    PHASE6_GIT_COMMON_DIR="$(git -C "$PHASE6_CORE4_DIR" rev-parse --git-common-dir 2>/dev/null || true)"
    if [ -n "$PHASE6_GIT_COMMON_DIR" ]; then
        PHASE6_COMMON_CORE4_DIR="$(cd "$(dirname "$PHASE6_GIT_COMMON_DIR")" && pwd)"
    else
        PHASE6_COMMON_CORE4_DIR="$PHASE6_CORE4_DIR"
    fi
    PHASE6_REPO_ROOT="$(cd "$PHASE6_COMMON_CORE4_DIR/.." && pwd)"
}

phase6_load_operator_token() {
    if [ -n "${WRFCOIN_OPERATOR_TOKEN:-}" ]; then
        export WRFCOIN_OPERATOR_TOKEN
        return 0
    fi

    local env_file="$PHASE6_CORE4_DIR/.env"
    if [ ! -f "$env_file" ] && [ -f "$PHASE6_COMMON_CORE4_DIR/.env" ]; then
        env_file="$PHASE6_COMMON_CORE4_DIR/.env"
    fi

    if [ ! -f "$env_file" ]; then
        echo "❌ No .env found for WRFCOIN_OPERATOR_TOKEN lookup" >&2
        return 1
    fi

    export WRFCOIN_OPERATOR_TOKEN
    WRFCOIN_OPERATOR_TOKEN="$(awk -F= '/^WRFCOIN_OPERATOR_TOKEN=/{print $2; exit}' "$env_file" | tr -d '\r\n')"
    if [ -z "$WRFCOIN_OPERATOR_TOKEN" ]; then
        echo "❌ WRFCOIN_OPERATOR_TOKEN missing from $env_file" >&2
        return 1
    fi
}

phase6_require_node_path() {
    if [ -n "${NODE_PATH:-}" ]; then
        export NODE_PATH
        return 0
    fi

    local backend_node_modules="$PHASE6_REPO_ROOT/backend/node_modules"
    if [ -d "$backend_node_modules" ]; then
        export NODE_PATH="$backend_node_modules"
        return 0
    fi

    echo "❌ NODE_PATH is not set and backend/node_modules was not found at $backend_node_modules" >&2
    return 1
}

# ── Pre-flight safety ────────────────────────────────────────────────────────

# Kill any stale launch_api processes listening on well-known TDD ports and
# remove orphaned temp directories from earlier crashed runs.
phase6_preflight_cleanup() {
    local port
    for port in 8556 8656 8657 8666 8667; do
        local pid
        pid="$(lsof -ti "tcp:$port" 2>/dev/null || true)"
        if [ -n "$pid" ]; then
            echo "   Killing stale process on port $port (PID $pid)" >&2
            kill "$pid" 2>/dev/null || true
        fi
    done
    # Prune temp dirs older than 1 hour
    find /tmp -maxdepth 1 -name 'wrfcoin-phase6-*' -type d -mmin +60 -exec rm -rf {} + 2>/dev/null || true
}

# ── Wait helpers ─────────────────────────────────────────────────────────────

phase6_wait_for_http() {
    local url="$1"
    local attempts="${2:-60}"
    local delay="${3:-1}"
    local i

    for ((i = 0; i < attempts; i++)); do
        if curl -fsS "$url" >/dev/null 2>&1; then
            return 0
        fi
        sleep "$delay"
    done

    echo "❌ Timed out waiting for $url" >&2
    return 1
}

phase6_wait_for_http_down() {
    local url="$1"
    local attempts="${2:-30}"
    local delay="${3:-1}"
    local i

    for ((i = 0; i < attempts; i++)); do
        if ! curl -fsS "$url" >/dev/null 2>&1; then
            return 0
        fi
        sleep "$delay"
    done

    echo "❌ Timed out waiting for $url to stop responding" >&2
    return 1
}

phase6_wait_for_jq_condition() {
    local url="$1"
    local filter="$2"
    local attempts="${3:-60}"
    local delay="${4:-1}"
    local i
    local payload

    for ((i = 0; i < attempts; i++)); do
        payload="$(curl -fsS "$url" 2>/dev/null || true)"
        if [ -n "$payload" ] && printf '%s' "$payload" | jq -e "$filter" >/dev/null 2>&1; then
            return 0
        fi
        sleep "$delay"
    done

    echo "❌ Timed out waiting for condition '$filter' at $url" >&2
    return 1
}

phase6_wait_for_log_line() {
    local log_file="$1"
    local pattern="$2"
    local attempts="${3:-60}"
    local delay="${4:-1}"
    local i

    for ((i = 0; i < attempts; i++)); do
        if [ -f "$log_file" ] && grep -q "$pattern" "$log_file"; then
            return 0
        fi
        sleep "$delay"
    done

    echo "❌ Timed out waiting for pattern '$pattern' in $log_file" >&2
    return 1
}

# ── Node lifecycle ───────────────────────────────────────────────────────────

phase6_start_launch_api_node() {
    local name="$1"
    local api_port="$2"
    local p2p_port="$3"
    local data_dir="$4"
    local log_file="$5"
    local bootstrap="${6:-}"
    local features="${7:-p2p}"

    mkdir -p "$data_dir"
    : >"$log_file"

    (
        cd "$PHASE6_CORE4_DIR" || exit 1
        # Cap virtual memory so a single node can't OOM the host.
        ulimit -v "$PHASE6_MEM_LIMIT_KB" 2>/dev/null || true
        export WRFCOIN_OPERATOR_TOKEN
        export WRFCOIN_API_HOST="127.0.0.1"
        export WRFCOIN_API_PORT="$api_port"
        export WRFCOIN_P2P_ENABLED="true"
        export WRFCOIN_P2P_PORT="$p2p_port"
        export WRFCOIN_DATA_DIR="$data_dir"
        if [ -n "$bootstrap" ]; then
            export WRFCOIN_P2P_BOOTSTRAP="$bootstrap"
        else
            unset WRFCOIN_P2P_BOOTSTRAP
        fi
        cargo run -p wrfcoin-native-chain --features "$features" --bin launch_api
    ) >"$log_file" 2>&1 &

    local pid=$!
    echo "$pid"
}

phase6_stop_pid() {
    local pid="${1:-}"
    if [ -n "$pid" ] && kill -0 "$pid" >/dev/null 2>&1; then
        kill "$pid" >/dev/null 2>&1 || true
        wait "$pid" >/dev/null 2>&1 || true
    fi
}

phase6_cleanup_pids() {
    local pid
    for pid in "$@"; do
        phase6_stop_pid "$pid"
    done
}

# ── Query helpers ────────────────────────────────────────────────────────────

phase6_local_peer_id() {
    local api_base="$1"
    curl -fsS "$api_base/network/peers" | jq -r '.local_peer_id'
}

phase6_connected_peers() {
    local api_base="$1"
    curl -fsS "$api_base/network/peers" | jq -r '.connected_peers'
}

phase6_chain_height() {
    local api_base="$1"
    curl -fsS "$api_base/metrics" | jq -r '.data.chain_height'
}

phase6_blocks_received() {
    local api_base="$1"
    curl -fsS "$api_base/network/stats" | jq -r '.blocks_received'
}

phase6_mine_block() {
    local api_base="$1"
    local miner_address="$2"
    curl -fsS \
        -X POST \
        -H "Authorization: Bearer $WRFCOIN_OPERATOR_TOKEN" \
        -H "Content-Type: application/json" \
        -d "{\"miner_address\":\"$miner_address\"}" \
        "$api_base/blocks/mine"
}
