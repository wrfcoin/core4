#!/usr/bin/env bash
# Exercise the built release binary itself: two copies must start libp2p
# transport and form a real localhost peer connection.
set -euo pipefail

die() { printf 'release P2P artifact check: %s\n' "$*" >&2; exit 1; }
[[ $# == 1 ]] || die "usage: $0 PATH_TO_WRFCOIN_NODE"
artifact=$1
[[ -x "$artifact" && -f "$artifact" ]] || die "artifact is not an executable file: $artifact"
command -v curl >/dev/null 2>&1 || die "curl is required"
command -v python3 >/dev/null 2>&1 || die "python3 is required"

tmp_root=$(mktemp -d "${TMPDIR:-/tmp}/wrfcoin-release-p2p.XXXXXX")
bootstrap_pid=''
joiner_pid=''
cleanup() {
  [[ -z "$joiner_pid" ]] || kill "$joiner_pid" >/dev/null 2>&1 || true
  [[ -z "$bootstrap_pid" ]] || kill "$bootstrap_pid" >/dev/null 2>&1 || true
  [[ -z "$joiner_pid" ]] || wait "$joiner_pid" >/dev/null 2>&1 || true
  [[ -z "$bootstrap_pid" ]] || wait "$bootstrap_pid" >/dev/null 2>&1 || true
  rm -rf "$tmp_root"
}
trap cleanup EXIT

mapfile -t ports < <(python3 -c 'import socket
sockets=[socket.socket() for _ in range(4)]
for sock in sockets: sock.bind(("127.0.0.1", 0))
for sock in sockets: print(sock.getsockname()[1])
for sock in sockets: sock.close()')
[[ ${#ports[@]} == 4 ]] || die "could not allocate fixture ports"
bootstrap_api=${ports[0]}
bootstrap_p2p=${ports[1]}
joiner_api=${ports[2]}
joiner_p2p=${ports[3]}
token=release-p2p-artifact-smoke-token-3222

start_node() {
  local api_port=$1 p2p_port=$2 data_dir=$3 log_file=$4 bootstrap=${5:-}
  env \
    WRFCOIN_OPERATOR_TOKEN="$token" \
    WRFCOIN_CHAIN_ID=3222 \
    WRFCOIN_NETWORK_ID=release-p2p-smoke \
    WRFCOIN_GENESIS_HASH=1111111111111111111111111111111111111111111111111111111111111111 \
    WRFCOIN_API_HOST=127.0.0.1 \
    WRFCOIN_API_PORT="$api_port" \
    WRFCOIN_P2P_ENABLED=true \
    WRFCOIN_P2P_PORT="$p2p_port" \
    WRFCOIN_P2P_BOOTSTRAP="$bootstrap" \
    WRFCOIN_SYNC_ONLY=true \
    WRFCOIN_DATA_DIR="$data_dir" \
    ALLOW_UNENCRYPTED_STORAGE=true \
    "$artifact" >"$log_file" 2>&1 &
  printf '%s\n' "$!"
}

wait_http() {
  local url=$1
  for _ in $(seq 1 90); do
    curl -fsS "$url" >/dev/null 2>&1 && return 0
    sleep 1
  done
  return 1
}

json_field() {
  local url=$1 field=$2
  curl -fsS "$url" | python3 -c "import json,sys; print(json.load(sys.stdin).get('$field'))"
}

bootstrap_pid=$(start_node "$bootstrap_api" "$bootstrap_p2p" "$tmp_root/bootstrap" "$tmp_root/bootstrap.log")
wait_http "http://127.0.0.1:${bootstrap_api}/health" || {
  cat "$tmp_root/bootstrap.log" >&2
  die "bootstrap artifact did not become healthy"
}
bootstrap_peer=$(json_field "http://127.0.0.1:${bootstrap_api}/network/peers" local_peer_id)
[[ -n "$bootstrap_peer" && "$bootstrap_peer" != None && "$bootstrap_peer" != null ]] || {
  cat "$tmp_root/bootstrap.log" >&2
  die "artifact has no P2P peer identity; was it built without p2p?"
}

bootstrap_addr="/ip4/127.0.0.1/tcp/${bootstrap_p2p}/p2p/${bootstrap_peer}"
joiner_pid=$(start_node "$joiner_api" "$joiner_p2p" "$tmp_root/joiner" "$tmp_root/joiner.log" "$bootstrap_addr")
wait_http "http://127.0.0.1:${joiner_api}/health" || {
  cat "$tmp_root/joiner.log" >&2
  die "joining artifact did not become healthy"
}

for _ in $(seq 1 90); do
  joiner_count=$(json_field "http://127.0.0.1:${joiner_api}/network/peers" connected_peers || true)
  bootstrap_count=$(json_field "http://127.0.0.1:${bootstrap_api}/network/peers" connected_peers || true)
  if [[ "$joiner_count" =~ ^[1-9][0-9]*$ && "$bootstrap_count" =~ ^[1-9][0-9]*$ ]]; then
    printf 'release P2P artifact PASS: bootstrap=%s joiner_peers=%s bootstrap_peers=%s\n' \
      "$bootstrap_peer" "$joiner_count" "$bootstrap_count"
    exit 0
  fi
  sleep 1
done

cat "$tmp_root/bootstrap.log" >&2
cat "$tmp_root/joiner.log" >&2
die "release artifacts started but did not form a peer connection"
