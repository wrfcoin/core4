#!/bin/bash
# Prove the loop — registration -> commit -> reveal -> oracle quorum -> PAID.
#
# Single-node local cut. Drives the node ONLY over HTTP, with no test harness:
# every step is a real route the collector or an operator would call.
#
# Usage:  scripts/demo-prove-the-loop.sh [real|control]
#
#   real     station registered with provenance_class=real   -> expects PAID
#   control  station registered with provenance_class=synthetic -> expects ZERO
#
# The control arm is the anti-vacuity half. Steps 1-4 are byte-identical
# between the two arms; only the governance-authorized provenance class
# differs. If `control` also pays, this demo proves nothing about eligibility.
#
# Prereq:
#   cargo build --bin launch_api --features full-mining,p2p
#   cargo build -p wrfcoin_cli --bin wrfcoin
set -uo pipefail

REPO="${WRFCOIN_REPO:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
RUN="${WRFCOIN_DEMO_RUN_DIR:-${TMPDIR:-/tmp}/wrfcoin-prove-the-loop}"
TARGET_DIR="${CARGO_TARGET_DIR:-$REPO/target}"
BIN="$TARGET_DIR/debug/launch_api"
WRF="$TARGET_DIR/debug/wrfcoin"

for required in "$BIN" "$WRF"; do
    [ -x "$required" ] || { echo "FAIL: $required missing. Build both binaries first."; exit 2; }
done

ARM="${1:-real}"
case "$ARM" in
  real)    CLASS=real ;;
  control) CLASS=synthetic ;;
  restart) CLASS=real ;;   # real loop, then kill -9 the node and re-read from WAL
  # Mainnet compute-fee posture (min_work_factor=1000, the real default) WITH a
  # signed ComputeReceipt attached to the reveal. Proves the gate is the ONLY
  # thing blocking the loop at mainnet defaults — supply the receipt the gate
  # asks for and it closes.
  mainnet) CLASS=real; WRFCOIN_MIN_WORK_FACTOR=1000; RECEIPT=1 ;;
  *) echo "usage: $0 [real|control|restart|mainnet]"; exit 2 ;;
esac
RECEIPT="${RECEIPT:-0}"

CHAIN_ID=777
NETWORK_ID=testnet
TOKEN=prove-the-loop-operator-token
API=18566
P2P=19010
STATION_ID="demo-station-1"
OBS_NONCE="demo-nonce-1"

rm -rf "$RUN"; mkdir -p "$RUN/node" "$RUN/oracles" "$RUN/gov" "$RUN/prod" "$RUN/owner"
log() { printf '\n=== %s ===\n' "$*"; }
fail() { printf '\nFAIL: %s\n' "$*"; [ -f "$RUN/node.log" ] && tail -30 "$RUN/node.log"; exit 1; }

# --------------------------------------------------------------------------
log "1. keys and genesis  (oracle set N=3, quorum K=2)"
# N=3/K=2, not N=1: quorum needs TWO DISTINCT oracle identities. A single-oracle
# genesis set can never reach a quorum of 2, and a quorum of 1 would not
# exercise multi-attester counting at all.
"$WRF" setup keygen --output "$RUN/oracles" --count 3 >/dev/null || fail "oracle keygen"
"$WRF" setup keygen --output "$RUN/gov"     --count 1 >/dev/null || fail "gov keygen"
"$WRF" setup keygen --output "$RUN/prod"    --count 1 >/dev/null || fail "producer keygen"
"$WRF" setup keygen --output "$RUN/owner"   --count 1 >/dev/null || fail "owner keygen"
"$WRF" setup genesis --validator-keys "$RUN/oracles" --oracle-quorum 2 \
    --network-id "$NETWORK_ID" --output "$RUN/genesis.json" >/dev/null || fail "setup genesis"

GOV_PK=$(cat "$RUN/gov/validator-1.pub")
PROD_PK=$(cat "$RUN/prod/validator-1.pub")
PROD_SK=$(cat "$RUN/prod/validator-1.key")
OWNER_PK=$(cat "$RUN/owner/validator-1.pub")
PAYOUT="0x$(printf '%s' "$PROD_PK" | cut -c1-40)"
CONTRIBUTOR="0x$(printf '%s' "$OWNER_PK" | cut -c1-40)"
PRODUCER_SET=$(printf '{"version":1,"members":[{"producer_id":"demo","payout_address":"%s","public_key":"%s"}]}' "$PAYOUT" "$PROD_PK")

echo "arm=$ARM  provenance_class=$CLASS"
echo "governance pubkey : $GOV_PK"
echo "contributor       : $CONTRIBUTOR"
echo "producer payout   : $PAYOUT"
# The cut-time declaration (#2523): block 1 metadata is written from this, and
# from that instant the set is chain content. It MUST match what the block-1
# registrations declare, so both are derived from the same genesis.json.
GENESIS_ORACLE_SET=$(jq -c '{oracle_set: .oracle_config.authorized_oracles, oracle_quorum: .oracle_config.quorum}' "$RUN/genesis.json")
echo "oracle set        : $(jq -r '.oracle_config.authorized_oracles | join(", ")' "$RUN/genesis.json")"
echo "oracle quorum     : $(jq -r '.oracle_config.quorum' "$RUN/genesis.json")"

# The offline builders recompute the authorization message to self-verify, and
# that recomputation resolves chain identity and the governance allowlist from
# THIS process's environment. Without these the CLI cannot check its own work.
export WRFCOIN_CHAIN_ID=$CHAIN_ID
export WRFCOIN_NETWORK_ID=$NETWORK_ID
export WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS=$GOV_PK

# --------------------------------------------------------------------------
log "2. boot node"
envs=(
    WRFCOIN_API_HOST=127.0.0.1
    WRFCOIN_API_PORT=$API
    WRFCOIN_P2P_ENABLED=true          # 'true', not '1' — '1' is silently ignored
    WRFCOIN_P2P_PORT=$P2P
    WRFCOIN_DATA_DIR=$RUN/node
    WRFCOIN_CHAIN_ID=$CHAIN_ID
    WRFCOIN_NETWORK_ID=$NETWORK_ID
    WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS=$GOV_PK
    WRFCOIN_OPERATOR_TOKEN=$TOKEN
    ALLOW_UNENCRYPTED_STORAGE=true
    WRFCOIN_GENESIS_PRODUCER_SET=$PRODUCER_SET
    WRFCOIN_PRODUCER_SIGNER_PRIVATE_KEY=$PROD_SK
    WRFCOIN_GENESIS_ORACLE_SET=$GENESIS_ORACLE_SET
    RUST_LOG=${RUST_LOG:-info}
    # #821/#1487 compute-fee gate. At the mainnet default (1000) the producer
    # self-screen drops a zero-fee ContributionReveal that carries no valid
    # ComputeReceipt, so the reveal never reaches state and no reward can be
    # evaluated. `0` is the documented testnet-2 posture ("until a real
    # compute-agent is deployed") and is what the live testnet runs. Recorded
    # here as an explicit deviation from mainnet defaults, not a workaround:
    # with it unset this demo reproduces the drop, which is arm `feegate`.
    WRFCOIN_MIN_WORK_FACTOR=${WRFCOIN_MIN_WORK_FACTOR:-0}
)
env "${envs[@]}" "$BIN" >"$RUN/node.log" 2>&1 &
echo $! > "$RUN/node.pid"
trap 'kill "$(cat "$RUN/node.pid" 2>/dev/null)" 2>/dev/null' EXIT

for _ in $(seq 1 90); do
    curl -fsS "http://127.0.0.1:$API/network/info" >/dev/null 2>&1 && break
    sleep 1
done
curl -fsS "http://127.0.0.1:$API/network/info" >/dev/null 2>&1 || fail "node did not come up"
echo "node up on 127.0.0.1:$API"

api()    { curl -fsS -H "Authorization: Bearer $TOKEN" "$@"; }
post()   { curl -sS -X POST "http://127.0.0.1:$API$1" -H "Authorization: Bearer $TOKEN" \
             -H 'Content-Type: application/json' --data-binary "@$2" -o "$3" -w '%{http_code}'; }
height() { curl -fsS "http://127.0.0.1:$API/network/info" | jq -r '.chain_height'; }
mine()   { curl -sS -X POST "http://127.0.0.1:$API/blocks/mine" -H "Authorization: Bearer $TOKEN" \
             -H 'Content-Type: application/json' -d "{\"miner_address\":\"$PAYOUT\"}" \
             -o "$RUN/mine-$1.json" -w '%{http_code}'; }
balance(){ api "http://127.0.0.1:$API/addresses/$CONTRIBUTOR" 2>/dev/null; }

# --------------------------------------------------------------------------
log "3. block 1 — seat the oracle set (genesis content)"
SUBMITTED_AT=$(( $(date +%s) - 5 ))
"$WRF" oracle build-registrations --genesis "$RUN/genesis.json" --chain-id "$CHAIN_ID" \
    --epoch 1 --activated-at 1785240000 --submitted-at "$SUBMITTED_AT" \
    --governance-key "$RUN/gov/validator-1.key" --out "$RUN/oracle-artifact.json" >/dev/null \
    || fail "oracle build-registrations"
jq -c '{chain_id, network_id, transactions}' "$RUN/oracle-artifact.json" > "$RUN/oracle-submit.json"
echo "submit oracle registrations -> HTTP $(post /transactions/oracle/registrations "$RUN/oracle-submit.json" "$RUN/r-oracle.json")"
jq -c '{accepted, oracle_quorum}' "$RUN/r-oracle.json" 2>/dev/null || cat "$RUN/r-oracle.json"
echo "mine -> HTTP $(mine 1)"
echo "height=$(height)"
api "http://127.0.0.1:$API/transactions/oracle/config" | jq -c '{oracle_set, oracle_quorum}' \
    > "$RUN/oracle-config.json" 2>/dev/null
cat "$RUN/oracle-config.json"

# --------------------------------------------------------------------------
log "4. block 2 — register the station through the production path"
SUBMITTED_AT=$(( $(date +%s) - 5 ))
"$WRF" station build-registration \
    --station-id "$STATION_ID" --owner-address "$CONTRIBUTOR" \
    --owner-key "$RUN/owner/validator-1.key" --governance-key "$RUN/gov/validator-1.key" \
    --chain-id "$CHAIN_ID" --network-id "$NETWORK_ID" \
    --latitude 46.7867 --longitude -92.1005 \
    --provenance-class "$CLASS" --data-quality-tier gold \
    --submitted-at "$SUBMITTED_AT" --out "$RUN/station-artifact.json" \
    || fail "station build-registration"
jq -c '{chain_id, network_id, transactions}' "$RUN/station-artifact.json" > "$RUN/station-submit.json"
echo "submit station registration -> HTTP $(post /transactions/station/registrations "$RUN/station-submit.json" "$RUN/r-station.json")"
cat "$RUN/r-station.json"; echo
echo "mine -> HTTP $(mine 2)"
echo "height=$(height)"

# --------------------------------------------------------------------------
log "5. BEFORE balance"
BEFORE_JSON=$(balance)
echo "${BEFORE_JSON:-<no account yet>}"
BEFORE=$(printf '%s' "$BEFORE_JSON" | jq -r '.data.earned_balance_total // 0' 2>/dev/null)
BEFORE=${BEFORE:-0}
echo "earned_balance_total BEFORE = $BEFORE"

# --------------------------------------------------------------------------
log "6. block 3 — commit (the route the collector uses)"
OBS_TS=$(date +%s)
OBS_DATA='{"dewpoint_c":13.5,"humidity_percent":60.0,"precipitation_mm":0.0,"pressure_hpa":1014.0,"quality":1.0,"temperature_c":21.5,"uniqueness":1.0,"volume":1.0,"wind_speed_ms":2.8}'
"$WRF" station build-observation --station-id "$STATION_ID" --contributor "$CONTRIBUTOR" \
    --signer-key "$RUN/owner/validator-1.key" --chain-id "$CHAIN_ID" --network-id "$NETWORK_ID" \
    --nonce "$OBS_NONCE" --timestamp "$OBS_TS" --data "$OBS_DATA" \
    ${RECEIPT:+$([ "$RECEIPT" = "1" ] && echo "--compute-receipt-key ${DEMO_RECEIPT_KEY:-$RUN/owner/validator-1.key} --work-factor ${DEMO_WORK_FACTOR:-1000}")} \
    --out "$RUN/observation.json" || fail "build-observation"
[ "$RECEIPT" = "1" ] && echo "compute receipt attached: $(jq -c '.reveal_body.compute_receipt | {worker_id: (.worker_id[0:16]+\"...\"), work_factor, sig: (.signature[0:16]+\"...\")}' "$RUN/observation.json")"
COMMIT_ID=$(jq -r '.commit_id' "$RUN/observation.json")
echo "commit_id = $COMMIT_ID"
jq -c '.commit_body' "$RUN/observation.json" > "$RUN/commit-body.json"
echo "submit commit -> HTTP $(post /transactions/contribution/commit "$RUN/commit-body.json" "$RUN/r-commit.json")"
head -c 300 "$RUN/r-commit.json"; echo
echo "mine -> HTTP $(mine 3)"; echo "height=$(height)"

# --------------------------------------------------------------------------
log "7. block 4 — reveal + TWO independent oracle attestations, then mine"
jq -c '.reveal_body' "$RUN/observation.json" > "$RUN/reveal-body.json"
echo "submit reveal -> HTTP $(post /transactions/contribution/reveal "$RUN/reveal-body.json" "$RUN/r-reveal.json")"
head -c 300 "$RUN/r-reveal.json"; echo

for i in 1 2; do
    OID=$(cat "$RUN/oracles/validator-$i.pub")
    "$WRF" station build-attestation --commit-id "$COMMIT_ID" --oracle-id "$OID" \
        --oracle-key "$RUN/oracles/validator-$i.key" \
        --timestamp $(( OBS_TS + i )) --nonce "att-$i" --out "$RUN/att-$i.json" \
        || fail "build-attestation $i"
    echo "submit attestation $i (oracle ${OID:0:16}...) -> HTTP $(post /transactions/oracle/attestation "$RUN/att-$i.json" "$RUN/r-att-$i.json")"
    head -c 200 "$RUN/r-att-$i.json"; echo
done

echo "mine -> HTTP $(mine 4)"; echo "height=$(height)"

# --------------------------------------------------------------------------
log "8. AFTER balance + commit status"
api "http://127.0.0.1:$API/transactions/commit/$COMMIT_ID/status" > "$RUN/status.json" 2>/dev/null
jq -c '{status, provenance_class, approvals, has_quorum, oracle_quorum, reward_status, reward_amount, reward_finalized}' \
    "$RUN/status.json" 2>/dev/null || cat "$RUN/status.json"

AFTER_JSON=$(balance)
echo "${AFTER_JSON:-<no account>}"
AFTER=$(printf '%s' "$AFTER_JSON" | jq -r '.data.earned_balance_total // 0' 2>/dev/null)
AFTER=${AFTER:-0}

log "RESULT (arm=$ARM, class=$CLASS)"
echo "earned_balance_total BEFORE = $BEFORE"
echo "earned_balance_total AFTER  = $AFTER"
echo "settlement block: $(jq -c '{height, transaction_count}' "$RUN/mine-4.json" 2>/dev/null)"
echo "per-class totals: $(printf '%s' "$AFTER_JSON" | jq -c '.data.contribution_rewards' 2>/dev/null)"
# The producer coinbase accumulates adjusted_block_reward, and the #2509 per-user
# cap is adjusted_block_reward/100. Printed so the contributor reward can be read
# against the cap that now bounds it (#2577).
MINER_BAL=$(api "http://127.0.0.1:$API/addresses/$PAYOUT" 2>/dev/null | jq -r '.data.balance // 0')
echo "producer coinbase balance = ${MINER_BAL:-?}  (per-user emission cap = adjusted_block_reward/100)"

if [ "$ARM" = "mainnet" ]; then
    if [ "$AFTER" -gt "$BEFORE" ] 2>/dev/null; then
        echo "PASS (mainnet posture): min_work_factor=1000 AND a signed ComputeReceipt -> loop CLOSED, $BEFORE -> $AFTER"
        echo "  => the compute-fee gate is the ONLY blocker at mainnet defaults; nothing else in the path fails."
        exit 0
    fi
    # Below-floor control: a receipt that does not clear the floor must NOT pay.
    if [ "${DEMO_WORK_FACTOR:-1000}" -lt 1000 ] 2>/dev/null; then
        echo "PASS (below-floor control): work_factor=${DEMO_WORK_FACTOR} < 1000 -> still dropped, balance stayed $AFTER"
        echo "  => the floor is enforced; it is not 'any receipt will do'."
        exit 0
    fi
    echo "LOOP DID NOT CLOSE even WITH a valid receipt — something beyond the compute-fee gate blocks mainnet posture"
    exit 1
fi
if [ "$ARM" = "restart" ]; then
    [ "$AFTER" -gt "$BEFORE" ] 2>/dev/null || { echo "LOOP DID NOT CLOSE before restart: $AFTER"; exit 1; }
    log "9. RESTART — kill -9 the node, restart on the SAME data dir, re-read"
    # Captured while the node is still ALIVE. Reading it after the kill returns
    # empty, which silently falls back to the pinned TESTNET genesis and the
    # node then refuses to restart against our local cut.
    CUT_GENESIS=$(curl -fsS "http://127.0.0.1:$API/network/info" 2>/dev/null | jq -r '.genesis_hash // empty')
    [ -n "$CUT_GENESIS" ] || { echo "RESTART SETUP FAILED: could not read genesis_hash before kill"; exit 1; }
    # kill -9, not a graceful stop: a clean shutdown can flush state that a
    # crash would not, which would prove the wrong thing (#2464 criterion 6).
    NODE_PID=$(cat "$RUN/node.pid")
    kill -9 "$NODE_PID" 2>/dev/null
    for _ in $(seq 1 30); do kill -0 "$NODE_PID" 2>/dev/null || break; sleep 1; done
    echo "killed pid $NODE_PID (kill -9)"
    # THE RE-PIN. Without this the node refuses to restart: with
    # WRFCOIN_GENESIS_HASH unset it falls back to the canonical testnet
    # expectation and rejects our freshly cut genesis —
    #   "genesis hash mismatch: local 0x66cc… != expected a3301def… (#2035)"
    # That is genesis-cut runbook step 6, and it is invisible until the FIRST
    # RESTART after a cut, because an uncut chain has nothing to compare.
    echo "re-pinning WRFCOIN_GENESIS_HASH=$CUT_GENESIS (runbook step 6)"
    envs+=( "WRFCOIN_GENESIS_HASH=$CUT_GENESIS" )
    env "${envs[@]}" "$BIN" >"$RUN/node-restart.log" 2>&1 &
    echo $! > "$RUN/node.pid"
    for _ in $(seq 1 90); do curl -fsS "http://127.0.0.1:$API/network/info" >/dev/null 2>&1 && break; sleep 1; done
    curl -fsS "http://127.0.0.1:$API/network/info" >/dev/null 2>&1 || {
        echo "RESTART FAILED: node did not come back up"; tail -25 "$RUN/node-restart.log"; exit 1; }
    RH=$(curl -fsS "http://127.0.0.1:$API/network/info" | jq -r '.chain_height')
    RJSON=$(balance)
    RAFTER=$(printf '%s' "$RJSON" | jq -r '.data.earned_balance_total // 0' 2>/dev/null); RAFTER=${RAFTER:-0}
    RSTATUS=$(api "http://127.0.0.1:$API/transactions/commit/$COMMIT_ID/status" 2>/dev/null)
    echo "post-restart height        = $RH"
    echo "post-restart status        = $(printf '%s' "$RSTATUS" | jq -c '{status,reward_status,reward_amount,reward_finalized}' 2>/dev/null)"
    echo "post-restart earned_balance = $RAFTER"
    [ "$RH" = "4" ] || { echo "RESTART FAILED: height $RH != 4 — the chain did not come back"; exit 1; }
    [ "$RAFTER" = "$AFTER" ] || { echo "RESTART FAILED: credit changed across restart, $AFTER -> $RAFTER"; exit 1; }
    echo "PASS (restart): credit survived kill -9 + WAL recovery, earned=$RAFTER at height $RH"; exit 0
fi
if [ "$ARM" = "real" ]; then
    if [ "$AFTER" -gt "$BEFORE" ] 2>/dev/null; then
        echo "PASS: contributor balance increased $BEFORE -> $AFTER"; exit 0
    fi
    echo "LOOP DID NOT CLOSE: balance unchanged at $AFTER"; exit 1
else
    if [ "$AFTER" = "$BEFORE" ]; then
        echo "PASS (control): synthetic station paid nothing, balance stayed $AFTER"; exit 0
    fi
    echo "CONTROL FAILED: a synthetic station was PAID ($BEFORE -> $AFTER)"; exit 1
fi
