#!/usr/bin/env bash
# Real CLI <-> launch_api process-boundary contract smoke (#2328).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CORE4_DIR="$(cd "${SCRIPT_DIR}/../../../.." && pwd)"
# Honor CARGO_TARGET_DIR: the self-hosted n5pro runners build into a shared
# /build/target (#2921), so `${CORE4_DIR}/target` never exists there and this
# smoke exited 2 ("required binary is not executable") on every run since the
# move — on main and on every PR. Explicit *_BIN overrides still win.
TARGET_DIR="${CARGO_TARGET_DIR:-${CORE4_DIR}/target}"
CLI_BIN="${WRFCOIN_CLI_BIN:-${TARGET_DIR}/debug/wrfcoin}"
API_BIN="${WRFCOIN_LAUNCH_API_BIN:-${TARGET_DIR}/debug/launch_api}"
API_PORT="${WRFCOIN_CLI_CONTRACT_PORT:-8556}"
API_URL="http://127.0.0.1:${API_PORT}"
DATA_DIR="$(mktemp -d "${TMPDIR:-/tmp}/wrfcoin-cli-contract.XXXXXX")"
API_LOG="${DATA_DIR}/launch_api.log"
API_PID=""

cleanup() {
  if [ -n "${API_PID}" ]; then
    kill "${API_PID}" 2>/dev/null || true
    wait "${API_PID}" 2>/dev/null || true
  fi
  rm -rf "${DATA_DIR}"
}
trap cleanup EXIT

for required in "${CLI_BIN}" "${API_BIN}"; do
  if [ ! -x "${required}" ]; then
    printf 'ERROR: required binary is not executable: %s\n' "${required}" >&2
    exit 2
  fi
done
command -v jq >/dev/null 2>&1 || {
  printf 'ERROR: jq is required\n' >&2
  exit 2
}
command -v openssl >/dev/null 2>&1 || {
  printf 'ERROR: openssl is required (ephemeral producer key generation)\n' >&2
  exit 2
}

# --- #2290 producer authentication -------------------------------------------
#
# Genesis now seeds an authorized producer set (#2406), and a node on such a
# chain refuses to start without a producer signer (#2290 P1-4). That is correct
# production behaviour and is NOT weakened here: instead this harness provisions
# a throwaway producer identity for the duration of the smoke run.
#
# The key is generated fresh on every invocation and lives only in this process's
# environment and the temp data dir, both discarded by the EXIT trap. Nothing is
# written to the repo and no private key material is committed — the value below
# is produced at runtime, never stored.
PRODUCER_SK="$(openssl rand -hex 32)"
# Derive the matching ed25519 public key with openssl alone (already required
# above): wrap the 32-byte seed in the fixed PKCS#8 Ed25519 header, ask openssl
# for the SubjectPublicKeyInfo, and take its last 32 bytes. No python module is
# needed — the n5pro runners (#2921) have no `cryptography`, which made this
# smoke exit 1 with ModuleNotFoundError right after the binaries resolved.
# python3 + `cryptography` remains as a fallback where openssl lacks ed25519.
derive_ed25519_pub() {
  local seed_hex="$1" der_hex pub
  der_hex="302e020100300506032b657004220420${seed_hex}"
  pub="$(printf "$(printf '%s' "${der_hex}" | sed 's/../\\x&/g')" \
    | openssl pkey -inform DER -pubout -outform DER 2>/dev/null \
    | tail -c 32 | od -An -v -tx1 | tr -d ' \n')"
  if [ "${#pub}" -ne 64 ]; then
    pub="$(WRFCOIN_EPHEMERAL_SK="${seed_hex}" python3 - <<'PY' 2>/dev/null
import os
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

sk = Ed25519PrivateKey.from_private_bytes(bytes.fromhex(os.environ["WRFCOIN_EPHEMERAL_SK"]))
print(
    sk.public_key()
    .public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
    .hex()
)
PY
)"
  fi
  printf '%s' "${pub}"
}
PRODUCER_PUB="$(derive_ed25519_pub "${PRODUCER_SK}")"
if [ -z "${PRODUCER_PUB}" ]; then
  printf 'ERROR: could not derive the ephemeral producer public key (openssl with ed25519, or python3 + cryptography, required)\n' >&2
  exit 2
fi
# The genesis member payout is the canonical Address form of the key prefix,
# matching `genesis_producers::parse_and_validate`.
PRODUCER_PAYOUT="0x${PRODUCER_PUB:0:40}"
GENESIS_PRODUCER_SET="$(printf '{"version":1,"members":[{"producer_id":"cli-contract-smoke","payout_address":"%s","public_key":"%s"}]}' \
  "${PRODUCER_PAYOUT}" "${PRODUCER_PUB}")"

# Ephemeral per-run credentials. The API only needs SOME token configured to
# start with auth enabled; nothing in this script ever reads them back, so there
# is no reason for a fixed value to live in the repo. Generating them per run
# also removes the committed `TOKEN=<64 hex>` shape, which is indistinguishable
# from a real leak to any scanner — and to any human skimming the diff.
SMOKE_OPERATOR_TOKEN="$(openssl rand -hex 32)"
SMOKE_READER_TOKEN="$(openssl rand -hex 32)"

env \
  WRFCOIN_API_HOST=127.0.0.1 \
  WRFCOIN_API_PORT="${API_PORT}" \
  WRFCOIN_DATA_DIR="${DATA_DIR}" \
  WRFCOIN_NETWORK_ID=testnet \
  WRFCOIN_CHAIN_ID=777 \
  WRFCOIN_OPERATOR_TOKEN="${SMOKE_OPERATOR_TOKEN}" \
  WRFCOIN_READER_TOKEN="${SMOKE_READER_TOKEN}" \
  ALLOW_UNENCRYPTED_STORAGE=true \
  WRFCOIN_GENESIS_PRODUCER_SET="${GENESIS_PRODUCER_SET}" \
  WRFCOIN_PRODUCER_SIGNER_PRIVATE_KEY="${PRODUCER_SK}" \
  WRFCOIN_PRODUCER_PAYOUT_ADDRESS="${PRODUCER_PAYOUT}" \
  RUST_LOG=warn \
  "${API_BIN}" >"${API_LOG}" 2>&1 &
API_PID=$!

for attempt in $(seq 1 120); do
  if curl -fsS "${API_URL}/health" >/dev/null 2>&1; then
    break
  fi
  if ! kill -0 "${API_PID}" 2>/dev/null; then
    printf 'ERROR: launch_api exited before becoming healthy\n' >&2
    sed -n '1,240p' "${API_LOG}" >&2
    exit 1
  fi
  if [ "${attempt}" -eq 120 ]; then
    printf 'ERROR: launch_api did not become healthy at %s\n' "${API_URL}" >&2
    sed -n '1,240p' "${API_LOG}" >&2
    exit 1
  fi
  sleep 1
done

if [ "${API_PORT}" = "8556" ]; then
  OUTPUT="$(env -u WRFCOIN_API_URL "${CLI_BIN}" --json chain info)"
else
  OUTPUT="$(WRFCOIN_API_URL="${API_URL}" "${CLI_BIN}" --json chain info)"
fi

printf '%s\n' "${OUTPUT}" | jq -e '
  (.data.network // .network) as $network
  | $network.network_id == "testnet"
  and $network.chain_id == "777"
  and ($network.node_version | type == "string" and length > 0)
  and ($network.chain_height | type == "number")
  and ($network.peer_count | type == "number")
' >/dev/null

printf 'PASS: CLI read live launch_api contract at %s\n' "${API_URL}"
