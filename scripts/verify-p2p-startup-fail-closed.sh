#!/usr/bin/env bash
# Exercise P2P identity/config/transport failures through the release binary's
# real startup path. This script is hermetic: all sockets are loopback/local.
set -euo pipefail

die() { printf 'P2P startup refusal check: %s\n' "$*" >&2; exit 1; }
[[ $# == 1 ]] || die "usage: $0 PATH_TO_LAUNCH_API"
artifact=$1
[[ -x "$artifact" && -f "$artifact" ]] || die "artifact is not executable: $artifact"
command -v curl >/dev/null 2>&1 || die "curl is required"
command -v python3 >/dev/null 2>&1 || die "python3 is required"
command -v timeout >/dev/null 2>&1 || die "timeout is required"

tmp_root=$(mktemp -d "${TMPDIR:-/tmp}/wrfcoin-p2p-startup.XXXXXX")
holder_pid=''
developer_pid=''
cleanup() {
  [[ -z "$developer_pid" ]] || kill "$developer_pid" >/dev/null 2>&1 || true
  [[ -z "$holder_pid" ]] || kill "$holder_pid" >/dev/null 2>&1 || true
  [[ -z "$developer_pid" ]] || wait "$developer_pid" >/dev/null 2>&1 || true
  [[ -z "$holder_pid" ]] || wait "$holder_pid" >/dev/null 2>&1 || true
  rm -rf "$tmp_root"
}
trap cleanup EXIT

next_port() {
  python3 -c 'import socket
s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()'
}

base_env() {
  local api_port=$1 p2p_port=$2 data_dir=$3
  local operator_token=p2p-startup-refusal-token-3223 # pragma: allowlist secret
  printf '%s\n' \
    "WRFCOIN_OPERATOR_TOKEN=$operator_token" \
    "WRFCOIN_CHAIN_ID=3223" \
    "WRFCOIN_NETWORK_ID=p2p-startup-refusal" \
    "WRFCOIN_GENESIS_HASH=1111111111111111111111111111111111111111111111111111111111111111" \
    "WRFCOIN_API_HOST=127.0.0.1" \
    "WRFCOIN_API_PORT=$api_port" \
    "WRFCOIN_P2P_PORT=$p2p_port" \
    "WRFCOIN_SYNC_ONLY=true" \
    "WRFCOIN_DATA_DIR=$data_dir" \
    "ALLOW_UNENCRYPTED_STORAGE=true"
}

expect_refusal() {
  local name=$1 expected=$2 data_dir=$3
  shift 3
  local api_port p2p_port status log_file
  api_port=$(next_port)
  p2p_port=$(next_port)
  log_file="$tmp_root/$name.log"
  mkdir -p "$data_dir"
  mapfile -t environment < <(base_env "$api_port" "$p2p_port" "$data_dir")
  set +e
  timeout 20 env "${environment[@]}" WRFCOIN_P2P_PROFILE=required "$@" \
    "$artifact" >"$log_file" 2>&1
  status=$?
  set -e
  [[ $status -ne 0 && $status -ne 124 ]] || {
    cat "$log_file" >&2
    die "$name did not fail closed (exit=$status)"
  }
  grep -Fq "$expected" "$log_file" || {
    cat "$log_file" >&2
    die "$name refusal did not report expected evidence: $expected"
  }
  printf 'PASS refusal: %s\n' "$name"
}

invalid_key_dir="$tmp_root/invalid-key"
mkdir -p "$invalid_key_dir"
printf '%s' 'not a protobuf keypair' >"$invalid_key_dir/peer_identity.key"
chmod 600 "$invalid_key_dir/peer_identity.key"
expect_refusal invalid-identity 'required P2P identity could not be loaded' "$invalid_key_dir"

expect_refusal malformed-port 'NC_NODE_CFG_INVALID_PORT' "$tmp_root/malformed-port" \
  WRFCOIN_P2P_PORT=not-a-port
expect_refusal malformed-bootstrap 'NC_NODE_CFG_INVALID_PEER_ADDR' "$tmp_root/malformed-bootstrap" \
  WRFCOIN_P2P_BOOTSTRAP=not-a-multiaddr

bind_port=$(next_port)
python3 -c 'import socket,sys,time
s=socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("0.0.0.0", int(sys.argv[1]))); s.listen(); time.sleep(60)' "$bind_port" &
holder_pid=$!
sleep 1
api_port=$(next_port)
mapfile -t bind_env < <(base_env "$api_port" "$bind_port" "$tmp_root/bind-conflict")
set +e
timeout 20 env "${bind_env[@]}" WRFCOIN_P2P_PROFILE=required \
  "$artifact" >"$tmp_root/bind-conflict.log" 2>&1
bind_status=$?
set -e
[[ $bind_status -ne 0 && $bind_status -ne 124 ]] || {
  cat "$tmp_root/bind-conflict.log" >&2
  die "bind conflict did not fail closed (exit=$bind_status)"
}
grep -Fq 'P2P transport failed to start' "$tmp_root/bind-conflict.log" || {
  cat "$tmp_root/bind-conflict.log" >&2
  die "bind conflict did not reach the transport-start refusal"
}
kill "$holder_pid" >/dev/null 2>&1 || true
wait "$holder_pid" >/dev/null 2>&1 || true
holder_pid=''
printf 'PASS refusal: bind-conflict/transport-start\n'

# The only permitted fallback is explicit developer mode, and readiness must
# make both the ephemeral identity and SIMULATED proof class visible.
developer_dir="$tmp_root/developer"
mkdir -p "$developer_dir"
printf '%s' 'not a protobuf keypair' >"$developer_dir/peer_identity.key"
chmod 600 "$developer_dir/peer_identity.key"
developer_api=$(next_port)
developer_p2p=$(next_port)
mapfile -t developer_env < <(base_env "$developer_api" "$developer_p2p" "$developer_dir")
env "${developer_env[@]}" WRFCOIN_P2P_PROFILE=developer \
  "$artifact" >"$tmp_root/developer.log" 2>&1 &
developer_pid=$!
for _ in $(seq 1 30); do
  if curl -fsS -H 'Authorization: Bearer p2p-startup-refusal-token-3223' \
      "http://127.0.0.1:${developer_api}/readyz" >"$tmp_root/developer-ready.json"; then
    break
  fi
  sleep 1
done
python3 - "$tmp_root/developer-ready.json" <<'PY'
import json
import sys
with open(sys.argv[1], encoding="utf-8") as handle:
    check = json.load(handle)["checks"]["p2p_startup"]
assert check["status"] == "running", check
assert check["profile"] == "developer", check
assert check["identity"] == "ephemeral", check
assert check["proof_class"] == "SIMULATED", check
PY
grep -Fq 'SIMULATED developer P2P profile' "$tmp_root/developer.log"
printf 'PASS developer profile: ephemeral identity visibly SIMULATED\n'
