#!/usr/bin/env bash
# #3224 panic drill: prove the inbound-P2P-consumer panic policy on a RELEASE
# artifact (owner decision 2026-09-18, Option 3).
#
#   A consumer panic is a whole-process fail-stop; recovery is external.
#
# The artifact must be built with the test-only `panic-drill` feature, e.g.
#
#   cargo build --release --locked -p wrfcoin-native-chain --bin launch_api \
#     --features "full-mining,p2p,panic-drill"
#
# and must NEVER be shipped (scripts/check_release_endpoints.py fails any build
# profile or release binary carrying the drill). Setting
# WRFCOIN_PANIC_DRILL_P2P_CONSUMER_TRIGGER=<file> makes the node's supervised
# consumer task panic once <file> appears; the file is removed first, so a
# restarted node runs normally.
#
# Two scenarios, both on loopback with a private data dir:
#   restart-policy  a supervisor loop that behaves like compose
#                   `restart: unless-stopped` (re-run on any exit until told to
#                   stop). Expect: ready -> trigger -> exit 134 (SIGABRT) ->
#                   the SAME data dir boots again -> /readyz ready.
#   no-restart      RED control: the same node without the loop. Expect: exit
#                   134 and the API stays unreachable for the whole window.
#
# Exit codes are taken from `wait`, never through a pipe.
set -euo pipefail

die() { printf 'P2P consumer panic drill: FAIL: %s\n' "$*" >&2; exit 1; }
[[ $# == 1 ]] || die "usage: $0 PATH_TO_PANIC_DRILL_LAUNCH_API"
artifact=$1
[[ -x "$artifact" && -f "$artifact" ]] || die "artifact is not executable: $artifact"
for tool in curl python3 grep; do
  command -v "$tool" >/dev/null 2>&1 || die "$tool is required"
done
grep -aqF WRFCOIN_PANIC_DRILL_P2P_CONSUMER_TRIGGER "$artifact" \
  || die "$artifact was not built with --features panic-drill (trigger literal absent)"

READY_TIMEOUT_SECS=${READY_TIMEOUT_SECS:-60}
EXIT_TIMEOUT_SECS=${EXIT_TIMEOUT_SECS:-30}
DOWN_WINDOW_SECS=${DOWN_WINDOW_SECS:-20}
OPERATOR_TOKEN=p2p-consumer-panic-drill-3224 # pragma: allowlist secret
NETWORK_ID=p2p-consumer-panic-drill # pragma: allowlist secret
ulimit -c 0 # an aborting release binary must not leave core files behind

tmp_root=$(mktemp -d "${TMPDIR:-/tmp}/wrfcoin-p2p-panic-drill.XXXXXX")
loop_pid=''
node_pid=''
cleanup() {
  [[ -z "$loop_pid" ]] || { touch "$tmp_root/stop-loop"; kill "$loop_pid" >/dev/null 2>&1 || true; }
  [[ -z "$node_pid" ]] || kill "$node_pid" >/dev/null 2>&1 || true
  if [[ -f "$tmp_root/restart-policy/current.pid" ]]; then
    kill "$(cat "$tmp_root/restart-policy/current.pid")" >/dev/null 2>&1 || true
  fi
  [[ -z "$loop_pid" ]] || wait "$loop_pid" >/dev/null 2>&1 || true
  [[ -z "$node_pid" ]] || wait "$node_pid" >/dev/null 2>&1 || true
  if [[ -n "${KEEP_DRILL_LOGS:-}" ]]; then
    printf 'drill logs kept in %s\n' "$tmp_root"
  else
    rm -rf "$tmp_root"
  fi
}
trap cleanup EXIT

next_port() {
  python3 -c 'import socket
s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()'
}

node_env() {
  local api_port=$1 p2p_port=$2 data_dir=$3 trigger=$4
  printf '%s\n' \
    "WRFCOIN_OPERATOR_TOKEN=$OPERATOR_TOKEN" \
    "WRFCOIN_CHAIN_ID=3224" \
    "WRFCOIN_NETWORK_ID=$NETWORK_ID" \
    "WRFCOIN_GENESIS_HASH=2222222222222222222222222222222222222222222222222222222222222222" \
    "WRFCOIN_API_HOST=127.0.0.1" \
    "WRFCOIN_API_PORT=$api_port" \
    "WRFCOIN_P2P_PORT=$p2p_port" \
    "WRFCOIN_P2P_PROFILE=required" \
    "WRFCOIN_SYNC_ONLY=true" \
    "WRFCOIN_DATA_DIR=$data_dir" \
    "ALLOW_UNENCRYPTED_STORAGE=true" \
    "WRFCOIN_PANIC_DRILL_P2P_CONSUMER_TRIGGER=$trigger"
}

# Prints the /readyz body into $2 and succeeds only for HTTP 200 with the
# consumer check reporting `running`.
readyz_ready() {
  local api_port=$1 out=$2
  curl -fsS -m 3 -H "Authorization: Bearer $OPERATOR_TOKEN" \
    "http://127.0.0.1:${api_port}/readyz" >"$out" 2>/dev/null || return 1
  python3 - "$out" <<'PY'
import json, sys
body = json.load(open(sys.argv[1], encoding="utf-8"))
consumer = body["checks"]["p2p_event_consumer"]
sys.exit(0 if consumer["status"] == "running" and not consumer["readiness_gate"] else 1)
PY
}

wait_ready() {
  local api_port=$1 out=$2 label=$3
  for _ in $(seq 1 "$READY_TIMEOUT_SECS"); do
    if readyz_ready "$api_port" "$out"; then
      printf 'PASS %s: /readyz 200, p2p_event_consumer=running\n' "$label"
      return 0
    fi
    sleep 1
  done
  [[ -f "$out" ]] && cat "$out" >&2
  return 1
}

api_reachable() {
  curl -sS -m 2 -o /dev/null "http://127.0.0.1:$1/health" >/dev/null 2>&1
}

# ---------------------------------------------------------------------------
# Scenario 1: restart policy present (compose `restart: unless-stopped`).
# ---------------------------------------------------------------------------
data_dir="$tmp_root/restart-policy/data"
trigger="$tmp_root/restart-policy/trigger"
exits="$tmp_root/restart-policy/exits"
mkdir -p "$data_dir"
: >"$exits"
api_port=$(next_port)
p2p_port=$(next_port)
mapfile -t environment < <(node_env "$api_port" "$p2p_port" "$data_dir" "$trigger")

restart_loop() {
  local run=0 pid status
  while [[ ! -e "$tmp_root/stop-loop" ]]; do
    run=$((run + 1))
    env "${environment[@]}" "$artifact" \
      >>"$tmp_root/restart-policy/node.$run.log" 2>&1 &
    pid=$!
    printf '%s\n' "$pid" >"$tmp_root/restart-policy/current.pid"
    set +e
    wait "$pid"
    status=$?
    set -e
    printf 'run=%s pid=%s exit=%s\n' "$run" "$pid" "$status" >>"$exits"
    [[ -e "$tmp_root/stop-loop" ]] && break
    sleep 1 # compose restarts after a short backoff
  done
}
restart_loop &
loop_pid=$!

wait_ready "$api_port" "$tmp_root/restart-policy/ready-1.json" "restart-policy boot #1" \
  || die "restart-policy node never became ready (see $tmp_root/restart-policy/node.1.log)"
first_pid=$(cat "$tmp_root/restart-policy/current.pid")
touch "$trigger"
for _ in $(seq 1 "$EXIT_TIMEOUT_SECS"); do
  [[ -s "$exits" ]] && break
  sleep 1
done
[[ -s "$exits" ]] || die "node did not exit within ${EXIT_TIMEOUT_SECS}s of the drill trigger"
first_exit=$(sed -n '1s/.* exit=//p' "$exits")
[[ "$first_exit" == 134 ]] || die "expected exit 134 (SIGABRT) on consumer panic, got $first_exit"
grep -aqF 'PANIC_DRILL: injected inbound P2P consumer panic' "$tmp_root/restart-policy/node.1.log" \
  || die "node.1.log has no drill panic message; the exit was not the injected panic"
printf 'PASS restart-policy: consumer panic -> process exit %s (SIGABRT), pid %s gone\n' \
  "$first_exit" "$first_pid"
[[ ! -e "$trigger" ]] || die "trigger file survived the panic; a restart would panic again"

wait_ready "$api_port" "$tmp_root/restart-policy/ready-2.json" "restart-policy boot #2 (after restart)" \
  || die "restarted node never became ready (see $tmp_root/restart-policy/node.2.log)"
second_pid=$(cat "$tmp_root/restart-policy/current.pid")
[[ "$second_pid" != "$first_pid" ]] || die "readiness came from the old process"
printf 'PASS restart-policy: the same data dir came back ready in a new process (pid %s)\n' "$second_pid"

touch "$tmp_root/stop-loop"
kill -TERM "$second_pid" >/dev/null 2>&1 || true
wait "$loop_pid" || true
loop_pid=''
runs=$(wc -l <"$exits")
printf 'restart-policy exits: %s\n' "$(tr '\n' ';' <"$exits")"
[[ "$runs" == 2 ]] || die "expected exactly 2 runs (panic + restart), saw $runs"

# ---------------------------------------------------------------------------
# Scenario 2 (RED control): no restart policy. The node must stay down.
# ---------------------------------------------------------------------------
data_dir="$tmp_root/no-restart/data"
trigger="$tmp_root/no-restart/trigger"
mkdir -p "$data_dir"
api_port=$(next_port)
p2p_port=$(next_port)
mapfile -t environment < <(node_env "$api_port" "$p2p_port" "$data_dir" "$trigger")
env "${environment[@]}" "$artifact" \
  >"$tmp_root/no-restart/node.log" 2>&1 &
node_pid=$!
wait_ready "$api_port" "$tmp_root/no-restart/ready.json" "no-restart boot" \
  || die "no-restart node never became ready (see $tmp_root/no-restart/node.log)"
touch "$trigger"
set +e
wait "$node_pid"
red_exit=$?
set -e
node_pid=''
[[ "$red_exit" == 134 ]] || die "RED control: expected exit 134 (SIGABRT), got $red_exit"
grep -aqF 'PANIC_DRILL: injected inbound P2P consumer panic' "$tmp_root/no-restart/node.log" \
  || die "RED control: no drill panic message in the node log"
for second in $(seq 1 "$DOWN_WINDOW_SECS"); do
  if api_reachable "$api_port"; then
    die "RED control: API answered ${second}s after the abort with no restart policy"
  fi
  sleep 1
done
printf 'PASS no-restart (RED control): exit %s, API unreachable for %ss — the node stays down\n' \
  "$red_exit" "$DOWN_WINDOW_SECS"
printf 'P2P consumer panic drill: ALL PASS\n'
