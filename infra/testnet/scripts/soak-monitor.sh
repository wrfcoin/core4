#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  soak-monitor.sh --n5 URL --node2 URL [options]

Required:
  --n5 URL                 N5 Pro launch API base URL
  --node2 URL              second-validator launch API base URL

Options:
  --duration DURATION      total runtime, e.g. 48h, 120m, 3600s (default: 48h)
  --interval SECONDS       poll interval (default: 60)
  --log PATH               JSONL output path (default: stdout)
  --max-height-delta N     allowed node height delta (default: 2)
  --max-block-age SECONDS  allowed latest-block age (default: 600)
  --once                   run one poll and exit
  --help                   show this help

The monitor is read-only. It polls current launch API endpoints and exits
non-zero when a threshold breach is observed.
USAGE
}

N5_URL=""
NODE2_URL=""
DURATION="48h"
INTERVAL=60
LOG_PATH=""
MAX_HEIGHT_DELTA=2
MAX_BLOCK_AGE=600
ONCE=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --n5)
      N5_URL="${2:-}"
      shift 2
      ;;
    --node2)
      NODE2_URL="${2:-}"
      shift 2
      ;;
    --duration)
      DURATION="${2:-}"
      shift 2
      ;;
    --interval)
      INTERVAL="${2:-}"
      shift 2
      ;;
    --log)
      LOG_PATH="${2:-}"
      shift 2
      ;;
    --max-height-delta)
      MAX_HEIGHT_DELTA="${2:-}"
      shift 2
      ;;
    --max-block-age)
      MAX_BLOCK_AGE="${2:-}"
      shift 2
      ;;
    --once)
      ONCE=1
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "required command not found: $1" >&2
    exit 2
  fi
}

duration_to_seconds() {
  local raw="$1"
  if [[ "$raw" =~ ^([0-9]+)([smhd]?)$ ]]; then
    local value="${BASH_REMATCH[1]}"
    local unit="${BASH_REMATCH[2]:-s}"
    case "$unit" in
      s) echo "$value" ;;
      m) echo $((value * 60)) ;;
      h) echo $((value * 60 * 60)) ;;
      d) echo $((value * 24 * 60 * 60)) ;;
      *) return 1 ;;
    esac
  else
    return 1
  fi
}

trim_url() {
  local url="$1"
  printf '%s' "${url%/}"
}

fetch_json() {
  local base="$1"
  local path="$2"
  local tmp_body tmp_meta status
  tmp_body="$(mktemp)"
  tmp_meta="$(mktemp)"

  status="$(
    curl -fsS \
      --connect-timeout 5 \
      --max-time 15 \
      -w '%{http_code}' \
      -o "$tmp_body" \
      "$(trim_url "$base")$path" 2>"$tmp_meta" || true
  )"

  if [[ "$status" == "200" ]] && jq -e . "$tmp_body" >/dev/null 2>&1; then
    jq -n \
      --arg path "$path" \
      --argjson status "$status" \
      --slurpfile body "$tmp_body" \
      '{path: $path, ok: true, status: $status, body: $body[0], error: null}'
  else
    local err body_excerpt
    err="$(tr '\n' ' ' <"$tmp_meta" | sed 's/[[:space:]]\+/ /g' | cut -c1-300)"
    body_excerpt="$(tr '\n' ' ' <"$tmp_body" | sed 's/[[:space:]]\+/ /g' | cut -c1-300)"
    jq -n \
      --arg path "$path" \
      --arg status "${status:-000}" \
      --arg error "$err" \
      --arg body_excerpt "$body_excerpt" \
      '{path: $path, ok: false, status: ($status | tonumber? // 0), body: null, error: {curl: $error, body_excerpt: $body_excerpt}}'
  fi

  rm -f "$tmp_body" "$tmp_meta"
}

poll_node() {
  local name="$1"
  local base="$2"
  local network sync snapshot contributions

  network="$(fetch_json "$base" "/network/info")"
  sync="$(fetch_json "$base" "/node/sync/status")"
  snapshot="$(fetch_json "$base" "/state/snapshot")"
  contributions="$(fetch_json "$base" "/transactions/contributions/status")"

  jq -n \
    --arg name "$name" \
    --arg base_url "$base" \
    --argjson network "$network" \
    --argjson sync "$sync" \
    --argjson snapshot "$snapshot" \
    --argjson contributions "$contributions" '
    def num_or_null(v): if v == null then null else (v | tonumber? // null) end;
    {
      name: $name,
      base_url: $base_url,
      endpoints: {
        network_info: $network,
        node_sync_status: $sync,
        state_snapshot: $snapshot,
        contributions_status: $contributions
      },
      ok: ($network.ok and $sync.ok and $snapshot.ok),
      network_id: $network.body.network_id,
      chain_id: $network.body.chain_id,
      genesis_hash: $network.body.genesis_hash,
      node_version: $network.body.node_version,
      sync_status: ($sync.body.sync_status // $network.body.sync_status),
      chain_height: num_or_null($snapshot.body.height // $sync.body.chain_height // $network.body.chain_height),
      state_root_hex: $snapshot.body.state_root_hex,
      snapshot_root_hex: $snapshot.body.snapshot_root_hex,
      snapshot_entry_count: num_or_null($snapshot.body.entry_count),
      peer_count: num_or_null($network.body.peer_count // $sync.body.peer_count),
      block_age_secs: num_or_null($sync.body.block_age_secs),
      contribution_status_summary: (
        if $contributions.ok then
          {
            type: ($contributions.body | type),
            top_level_keys: (if ($contributions.body | type) == "object" then ($contributions.body | keys) else [] end),
            array_length: (if ($contributions.body | type) == "array" then ($contributions.body | length) else null end)
          }
        else null end
      ),
      errors: [
        $network, $sync, $snapshot, $contributions
        | select(.ok == false)
        | {path, status, error}
      ]
    }'
}

write_line() {
  local line="$1"
  if [[ -n "$LOG_PATH" ]]; then
    mkdir -p "$(dirname "$LOG_PATH")"
    printf '%s\n' "$line" >>"$LOG_PATH"
  else
    printf '%s\n' "$line"
  fi
}

if [[ -z "$N5_URL" || -z "$NODE2_URL" ]]; then
  echo "--n5 and --node2 are required" >&2
  usage >&2
  exit 2
fi

require_command curl
require_command jq
require_command mktemp

if ! [[ "$INTERVAL" =~ ^[0-9]+$ ]] || [[ "$INTERVAL" -lt 1 ]]; then
  echo "--interval must be a positive integer" >&2
  exit 2
fi

if ! [[ "$MAX_HEIGHT_DELTA" =~ ^[0-9]+$ ]]; then
  echo "--max-height-delta must be a non-negative integer" >&2
  exit 2
fi

if ! [[ "$MAX_BLOCK_AGE" =~ ^[0-9]+$ ]]; then
  echo "--max-block-age must be a non-negative integer" >&2
  exit 2
fi

DURATION_SECS="$(duration_to_seconds "$DURATION")" || {
  echo "--duration must look like 48h, 120m, or 3600s" >&2
  exit 2
}

START_EPOCH="$(date -u +%s)"
END_EPOCH=$((START_EPOCH + DURATION_SECS))
BREACH_COUNT=0
POLL_COUNT=0

while true; do
  POLL_COUNT=$((POLL_COUNT + 1))
  NOW_EPOCH="$(date -u +%s)"
  TIMESTAMP="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  N5="$(poll_node n5 "$N5_URL")"
  NODE2="$(poll_node node2 "$NODE2_URL")"

  RECORD="$(
    jq -n \
      --arg timestamp "$TIMESTAMP" \
      --argjson poll "$POLL_COUNT" \
      --argjson started_at "$START_EPOCH" \
      --argjson now_epoch "$NOW_EPOCH" \
      --argjson duration_secs "$DURATION_SECS" \
      --argjson max_height_delta "$MAX_HEIGHT_DELTA" \
      --argjson max_block_age "$MAX_BLOCK_AGE" \
      --argjson n5 "$N5" \
      --argjson node2 "$NODE2" '
      def abs: if . < 0 then -. else . end;
      def nums: map(select(. != null));
      def max_or_null: if length == 0 then null else max end;
      def required_errlist($node):
        ([($node.ok | not) as $bad | select($bad) | {node: $node.name, reason: "required_endpoint_failed"}]
        + ($node.errors | map(select(.path != "/transactions/contributions/status") | . + {node: $node.name})));
      ($n5.chain_height // null) as $h1
      | ($node2.chain_height // null) as $h2
      | (if $h1 != null and $h2 != null then (($h1 - $h2) | abs) else null end) as $height_delta
      | (if $h1 != null and $h2 != null then $h1 == $h2 else false end) as $same_height
      | (if $same_height and $n5.state_root_hex != null and $node2.state_root_hex != null then $n5.state_root_hex == $node2.state_root_hex else null end) as $state_root_match
      | ([$n5.block_age_secs, $node2.block_age_secs] | nums | max_or_null) as $max_block_age_secs
      | (
          (required_errlist($n5) + required_errlist($node2))
          + (if $n5.ok and $node2.ok and (
              $n5.network_id == null or $n5.network_id != $node2.network_id
              or $n5.chain_id == null or $n5.chain_id != $node2.chain_id
              or $n5.genesis_hash == null or $n5.genesis_hash != $node2.genesis_hash
            ) then [{reason: "chain_identity_mismatch"}] else [] end)
          + (if $height_delta != null and $height_delta > $max_height_delta then [{reason: "height_delta_exceeded", height_delta: $height_delta, max_height_delta: $max_height_delta}] else [] end)
          + (if $same_height and $state_root_match == false then [{reason: "state_root_mismatch_same_height"}] else [] end)
          + (if $max_block_age_secs != null and $max_block_age_secs > $max_block_age then [{reason: "block_age_exceeded", max_block_age_secs: $max_block_age_secs, max_block_age: $max_block_age}] else [] end)
        ) as $errors
      | {
          timestamp: $timestamp,
          poll: $poll,
          elapsed_secs: ($now_epoch - $started_at),
          target_duration_secs: $duration_secs,
          n5: $n5,
          node2: $node2,
          same_chain_identity: (
            $n5.network_id != null and $n5.network_id == $node2.network_id
            and $n5.chain_id != null and $n5.chain_id == $node2.chain_id
            and $n5.genesis_hash != null and $n5.genesis_hash == $node2.genesis_hash
          ),
          same_height: $same_height,
          height_delta: $height_delta,
          state_root_match: $state_root_match,
          max_block_age_secs: $max_block_age_secs,
          thresholds: {
            max_height_delta: $max_height_delta,
            max_block_age_secs: $max_block_age
          },
          threshold_breached: (($errors | length) > 0),
          errors: $errors
        }'
  )"

  write_line "$RECORD"

  if jq -e '.threshold_breached == true' <<<"$RECORD" >/dev/null; then
    BREACH_COUNT=$((BREACH_COUNT + 1))
    echo "soak threshold breached at $TIMESTAMP; see JSONL record" >&2
    exit 1
  fi

  if [[ "$ONCE" -eq 1 ]]; then
    exit 0
  fi

  if [[ "$NOW_EPOCH" -ge "$END_EPOCH" ]]; then
    break
  fi

  sleep "$INTERVAL"
done

echo "soak monitor completed: polls=$POLL_COUNT breaches=$BREACH_COUNT duration_secs=$DURATION_SECS" >&2
