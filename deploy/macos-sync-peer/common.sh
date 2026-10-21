#!/usr/bin/env bash
# Shared data-only configuration and release validation. Do not source config.
set -euo pipefail

die() { printf '%s: %s\n' "${0##*/}" "$*" >&2; exit 1; }
is_set() { case "|${seen_keys:-}|" in *"|$1|"*) return 0;; *) return 1;; esac; }
set_value() {
  local key="$1" value="$2"
  is_set "$key" && die "duplicate configuration key: $key"
  seen_keys="${seen_keys:-}|$key"
  case "$key" in
    WRFCOIN_NETWORK_ID|WRFCOIN_CHAIN_ID|WRFCOIN_GENESIS_HASH|WRFCOIN_P2P_BOOTSTRAP|WRFCOIN_API_ADDR|WRFCOIN_ARTIFACT_PATH|WRFCOIN_ARTIFACT_SHA256|WRFCOIN_ARTIFACT_SIGNATURE_PATH|WRFCOIN_BUILD_MANIFEST_PATH|WRFCOIN_BUILD_MANIFEST_SIGNATURE_PATH|WRFCOIN_RELEASE_KEY_FINGERPRINT|WRFCOIN_SOURCE_COMMIT|WRFCOIN_SYNC_ONLY|WRFCOIN_PRODUCER_MODE|WRFCOIN_ORACLE_MODE) printf -v "$key" '%s' "$value" ;;
    *) die "unsupported configuration key: $key" ;;
  esac
}
load_config() {
  local path="$1" line key value
  [[ -f "$path" && ! -L "$path" ]] || die "configuration must be a regular file"
  seen_keys=''
  while IFS= read -r line || [[ -n "$line" ]]; do
    [[ -z "$line" || "$line" == \#* ]] && continue
    [[ "$line" =~ ^([A-Z0-9_]+)=(.*)$ ]] || die "invalid configuration line"
    key="${BASH_REMATCH[1]}"; value="${BASH_REMATCH[2]}"
    [[ "$value" != *'$('* && "$value" != *'`'* && "$value" != *';'* && "$value" != *$'\n'* ]] || die "configuration value contains code syntax"
    set_value "$key" "$value"
  done < "$path"
}
require_value() { local name="$1" value; value="${!name:-}"; [[ -n "$value" && "$value" != REPLACE_WITH_* ]] || die "$name is required"; }
validate_config() {
  local config="$1"
  load_config "$config"
  for key in WRFCOIN_NETWORK_ID WRFCOIN_CHAIN_ID WRFCOIN_GENESIS_HASH WRFCOIN_P2P_BOOTSTRAP WRFCOIN_SYNC_ONLY WRFCOIN_PRODUCER_MODE WRFCOIN_ORACLE_MODE; do require_value "$key"; done
  [[ "$WRFCOIN_NETWORK_ID" == testnet && "$WRFCOIN_CHAIN_ID" == 777 && "$WRFCOIN_GENESIS_HASH" == 805efc973fe50829ae2abf7c73bde0a5ed8511f3dfa97ff50a86615703442625 ]] || die "frozen network identity mismatch"
  local host port peer
  [[ "$WRFCOIN_P2P_BOOTSTRAP" =~ ^/(ip4|dns4|dns6)/([^/[:space:]]+)/tcp/([0-9]+)/p2p/([^/[:space:]]+)$ ]] || die "bootstrap input must be a complete TCP multiaddr"
  host="${BASH_REMATCH[2]}"; port="${BASH_REMATCH[3]}"; peer="${BASH_REMATCH[4]}"
  [[ -n "$host" && -n "$peer" ]] || die "bootstrap input must include host and peer"
  (( 10#$port >= 1 && 10#$port <= 65535 )) || die "bootstrap TCP port must be between 1 and 65535"
  [[ "${WRFCOIN_API_ADDR:-127.0.0.1:8556}" == 127.0.0.1:* ]] || die "API must remain loopback-only"
  [[ "$WRFCOIN_SYNC_ONLY" == true && "$WRFCOIN_PRODUCER_MODE" == false && "$WRFCOIN_ORACLE_MODE" == false ]] || die "sync-only role flags are required"
}
validate_archive() {
  local archive="$1" listing details name
  listing="$(tar -tzf "$archive")" || die "invalid gzip archive"
  while IFS= read -r name; do
    [[ -n "$name" && "$name" != /* && "$name" != *'../'* && "$name" != '..' ]] || die "unsafe archive member"
  done <<< "$listing"
  details="$(tar -tvzf "$archive")" || die "cannot inspect archive members"
  while IFS= read -r name; do
    case "$name" in -[!-]*|d*) ;; *) die "archive contains non-regular member";; esac
  done <<< "$details"
}
require_arm64_binary() {
  local binary="$1" architectures
  [[ "$(uname -s)" == Darwin ]] || die "Apple M4 requires macOS"
  [[ "$(uname -m)" == arm64 ]] || die "Apple M4 requires an arm64 host"
  architectures="$(lipo -archs "$binary" 2>/dev/null)" || die "cannot inspect binary architecture"
  [[ " $architectures " == *' arm64 '* ]] || die "artifact lacks arm64 support"
}
