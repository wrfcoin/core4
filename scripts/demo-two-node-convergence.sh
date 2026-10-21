#!/bin/bash
# Two-node hardening of the prove-the-loop demo (R2 Branch A).
#
# The single-node demo proved the earn loop pays. It proved nothing about
# whether a SECOND node agrees. This runs the identical loop on node A, lets
# node B acquire everything purely by syncing, and asserts consensus-authoritative
# convergence.
#
# Usage:  scripts/demo-two-node-convergence.sh [real|control|isolated]
#
#   real       loop runs on A; B syncs.        expect CONVERGED + B sees the paid reward
#   control    synthetic station.               expect CONVERGED + zero paid on BOTH
#   isolated   B never connects to A.           expect DIVERGED  (the RED control)
#
# `isolated` is the RED control: if the convergence assertion cannot fail when
# two nodes genuinely have different chains, it is not an assertion.
#
# GENESIS VERSION IS PINNED to v2 on both nodes. At the V1 default,
# `verify_tip_state_root_commitment_at` returns Ok(()) unconditionally, so the
# #2219 tip state_root commitment is NOT CHECKED AT ALL — a node that lost
# genesis-only state starts happily and serves a state_root that differs from
# every peer's. Omission picks the riskier branch, so a multi-node run must pin.
# Both nodes must agree from block 1 (the #2076 asymmetry).
#
# Convergence criteria follow infra/scripts/verify-two-node-convergence.sh:
# CONSENSUS-AUTHORITATIVE = genesis_hash match AND state_root_hex match at a
# common height AND wrfcoin_state_divergence_total == 0 on BOTH nodes.
# snapshot_root_hex / entry_count are WARN-only: they are deliberately excluded
# from the consensus state_root (#1680/#1681) and can legitimately differ
# between a producer and a syncing follower.
#
# REQUIRES core4 PR #2578. On an uncut chain the startup gate arms the signing
# version but not the activation SCHEDULE, so p2p refuses to start (#1396) and
# both nodes come up with networking silently disabled — node B stays at height
# 0 forever and this script reports DIVERGED. That is a real defect in merged
# code, not a harness problem; #2578 is the one-hunk fix.
#
# Prereq:
#   cargo build --bin launch_api --features full-mining,p2p
#   cargo build -p wrfcoin_cli --bin wrfcoin
set -uo pipefail

REPO="${WRFCOIN_REPO:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
RUN="${WRFCOIN_TWONODE_DEMO_DIR:-${TMPDIR:-/tmp}/wrfcoin-twonode-loop}"
TARGET_DIR="${CARGO_TARGET_DIR:-$REPO/target}"
BIN="$TARGET_DIR/debug/launch_api"
WRF="$TARGET_DIR/debug/wrfcoin"
for r in "$BIN" "$WRF"; do [ -x "$r" ] || { echo "FAIL: $r missing"; exit 2; }; done

ARM="${1:-real}"
case "$ARM" in
  real)     CLASS=real;      CONNECT=1 ;;
  control)  CLASS=synthetic; CONNECT=1 ;;
  isolated) CLASS=real;      CONNECT=0 ;;
  *) echo "usage: $0 [real|control|isolated]"; exit 2 ;;
esac

CHAIN_ID=777
NETWORK_ID=testnet
TOKEN=twonode-loop-operator-token
PROTOCOL_VERSION="${WRFCOIN_SIGNING_PROTOCOL_VERSION:-v2}"
# Default, do not OVERWRITE. These were unconditional assignments, so the
# collision guard below advertised a recovery path — "set A_API/B_API/A_P2P/
# B_P2P to an unused range" — that could never work: the override was
# discarded here before the guard ever read it (Grace Tier-1 on #2612; the
# reviewer retracted an APPROVE that had repeated the false advice).
A_API="${A_API:-18576}"; A_P2P="${A_P2P:-19020}"
B_API="${B_API:-18577}"; B_P2P="${B_P2P:-19021}"
STATION_ID="demo-station-1"
SYNC_ATTEMPTS="${WRFCOIN_TWONODE_SYNC_ATTEMPTS:-60}"
SYNC_SLEEP="${WRFCOIN_TWONODE_SYNC_SLEEP:-2}"

rm -rf "$RUN"; mkdir -p "$RUN/a" "$RUN/b" "$RUN/oracles" "$RUN/gov" "$RUN/prod" "$RUN/owner"
log()  { printf '\n=== %s ===\n' "$*"; }
fail() { printf '\nFAIL: %s\n' "$*"; tail -25 "$RUN/a.log" 2>/dev/null; exit 1; }

log "1. keys and genesis (oracle set N=3, K=2; protocol_version pinned $PROTOCOL_VERSION)"
"$WRF" setup keygen --output "$RUN/oracles" --count 3 >/dev/null || fail "oracle keygen"
"$WRF" setup keygen --output "$RUN/gov"     --count 1 >/dev/null || fail "gov keygen"
"$WRF" setup keygen --output "$RUN/prod"    --count 1 >/dev/null || fail "prod keygen"
"$WRF" setup keygen --output "$RUN/owner"   --count 1 >/dev/null || fail "owner keygen"
"$WRF" setup genesis --validator-keys "$RUN/oracles" --oracle-quorum 2 \
    --network-id "$NETWORK_ID" --output "$RUN/genesis.json" >/dev/null || fail "setup genesis"

GOV_PK=$(cat "$RUN/gov/validator-1.pub")
PROD_PK=$(cat "$RUN/prod/validator-1.pub"); PROD_SK=$(cat "$RUN/prod/validator-1.key")
OWNER_PK=$(cat "$RUN/owner/validator-1.pub")
PAYOUT="0x$(printf '%s' "$PROD_PK" | cut -c1-40)"
CONTRIBUTOR="0x$(printf '%s' "$OWNER_PK" | cut -c1-40)"
PRODUCER_SET=$(printf '{"version":1,"members":[{"producer_id":"demo","payout_address":"%s","public_key":"%s"}]}' "$PAYOUT" "$PROD_PK")
GENESIS_ORACLE_SET=$(jq -c '{oracle_set: .oracle_config.authorized_oracles, oracle_quorum: .oracle_config.quorum}' "$RUN/genesis.json")
export WRFCOIN_CHAIN_ID=$CHAIN_ID WRFCOIN_NETWORK_ID=$NETWORK_ID WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS=$GOV_PK
echo "arm=$ARM class=$CLASS connect=$CONNECT contributor=$CONTRIBUTOR"

start_node () {  # $1=name $2=api $3=p2p $4=bootstrap(optional)
    local name=$1 api=$2 p2p=$3 boot=${4:-}
    local -a envs=(
        WRFCOIN_API_HOST=127.0.0.1 WRFCOIN_API_PORT=$api
        WRFCOIN_P2P_ENABLED=true WRFCOIN_P2P_PORT=$p2p
        WRFCOIN_DATA_DIR=$RUN/$name
        WRFCOIN_CHAIN_ID=$CHAIN_ID WRFCOIN_NETWORK_ID=$NETWORK_ID
        WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS=$GOV_PK
        WRFCOIN_OPERATOR_TOKEN=$TOKEN
        ALLOW_UNENCRYPTED_STORAGE=true
        WRFCOIN_GENESIS_PRODUCER_SET=$PRODUCER_SET
        WRFCOIN_PRODUCER_SIGNER_PRIVATE_KEY=$PROD_SK
        WRFCOIN_GENESIS_ORACLE_SET=$GENESIS_ORACLE_SET
        WRFCOIN_MIN_WORK_FACTOR=${WRFCOIN_MIN_WORK_FACTOR:-0}
        # The pin. Both nodes, from block 1 — a node cannot activate a version
        # by itself, and disagreement here is #2076 (v1 signer, v2 verifier).
        WRFCOIN_SIGNING_PROTOCOL_VERSION=$PROTOCOL_VERSION
        RUST_LOG=${RUST_LOG:-info}
    )
    [ -n "$boot" ] && envs+=( "WRFCOIN_P2P_BOOTSTRAP=$boot" )
    env "${envs[@]}" "$BIN" >"$RUN/$name.log" 2>&1 &
    echo $! > "$RUN/$name.pid"
}
wait_up () { for _ in $(seq 1 90); do curl -fsS "http://127.0.0.1:$1/network/info" >/dev/null 2>&1 && return 0; sleep 1; done; return 1; }

# Refuse to run if our ports are already taken. `wait_up` below only checks that
# SOMETHING answers on the port — if another agent's node is already listening,
# this script would drive THEIR chain and could report CONVERGED about a run it
# never performed. That is not hypothetical: a foreign node from another
# reviewer's worktree was found holding 18576/18577 mid-sprint, and the run it
# produced had to be discarded. A verdict that can be produced by looking at
# someone else's chain is not a verdict.
for port in $A_API $B_API $A_P2P $B_P2P; do
    if ss -ltn 2>/dev/null | grep -qE "[:.]${port}[[:space:]]"; then
        echo "FAIL: port $port is already in use — refusing to run rather than risk"
        echo "      measuring another process's chain. Free it, or set"
        echo "      A_API/B_API/A_P2P/B_P2P to an unused range."
        ss -ltnp 2>/dev/null | grep -E "[:.]${port}[[:space:]]" || true
        exit 2
    fi
done

# Observation seam for the override control (#2612 review): print the exact
# values the launch commands and the bootstrap multiaddr will be built from,
# then stop. Placed AFTER the collision guard so one run proves the override
# reached the guard AND the launch/bootstrap construction — Grace's point was
# that proving it only reaches the preflight message proves nothing.
if [ "${WRFCOIN_TWONODE_PRINT_PLAN:-0}" = "1" ]; then
    echo "PLAN guard_ports=$A_API $B_API $A_P2P $B_P2P"
    echo "PLAN start_node_a=a $A_API $A_P2P"
    echo "PLAN start_node_b=b $B_API $B_P2P"
    echo "PLAN bootstrap_prefix=/ip4/127.0.0.1/tcp/$A_P2P"
    exit 0
fi

log "2. boot node A, then node B"
start_node a $A_API $A_P2P
wait_up $A_API || fail "node A did not come up"
A_PEER=$(curl -fsS http://127.0.0.1:$A_API/network/info | jq -r '.peer_id // empty')
BOOT="/ip4/127.0.0.1/tcp/$A_P2P${A_PEER:+/p2p/$A_PEER}"
if [ "$CONNECT" = "1" ]; then start_node b $B_API $B_P2P "$BOOT"; else start_node b $B_API $B_P2P; fi
wait_up $B_API || fail "node B did not come up"
trap 'kill "$(cat "$RUN/a.pid" 2>/dev/null)" "$(cat "$RUN/b.pid" 2>/dev/null)" 2>/dev/null' EXIT
echo "A=127.0.0.1:$A_API  B=127.0.0.1:$B_API  bootstrap=${CONNECT:+$BOOT}"

api()    { curl -fsS -H "Authorization: Bearer $TOKEN" "$@"; }
post()   { curl -sS -X POST "http://127.0.0.1:$A_API$1" -H "Authorization: Bearer $TOKEN" \
             -H 'Content-Type: application/json' --data-binary "@$2" -o "$3" -w '%{http_code}'; }
height() { curl -fsS "http://127.0.0.1:$1/network/info" | jq -r '.chain_height'; }
mine()   { curl -sS -X POST "http://127.0.0.1:$A_API/blocks/mine" -H "Authorization: Bearer $TOKEN" \
             -H 'Content-Type: application/json' -d "{\"miner_address\":\"$PAYOUT\"}" \
             -o "$RUN/mine-$1.json" -w '%{http_code}'; }
snap()   { api "http://127.0.0.1:$1/state/snapshot"; }

# Bucket /state/snapshot/entries by decoded key prefix, following the infra
# verifier. This is the check its own header credits with LOCALIZING a real
# divergence (pending_commit 61 vs 3) — comparing only the state_root tells you
# THAT two nodes differ, never WHERE. Pages to exhaustion; a run that silently
# stopped at the first page would compare two prefixes of the truth.
entry_prefixes() {  # $1=port -> "prefix count" lines, sorted
    local port=$1 off=0 total=1 body
    : > "$RUN/entries-$port.txt"
    while [ "$off" -lt "$total" ]; do
        body=$(api "http://127.0.0.1:$port/state/snapshot/entries?offset=$off&limit=1000" 2>/dev/null)
        [ -n "$body" ] || { echo "ENTRIES_UNREADABLE"; return 1; }
        total=$(printf '%s' "$body" | jq -er '.total | select(type=="number")' 2>/dev/null) || {
            echo "ENTRIES_UNREADABLE"; return 1; }
        printf '%s' "$body" | jq -r '.entries[]? | .key_hex // .key // empty' >> "$RUN/entries-$port.txt"
        local next
        next=$(printf '%s' "$body" | jq -r '.next_offset // empty')
        [ -n "$next" ] || break
        off=$next
    done
    # decode hex -> text, keep the segment before the first '/'
    while read -r h; do
        [ -n "$h" ] || continue
        printf '%s' "$h" | python3 -c 'import sys;b=bytes.fromhex(sys.stdin.read().strip());sys.stdout.write(b.split(b"/")[0].decode("latin-1","replace")+"\n")' 2>/dev/null
    done < "$RUN/entries-$port.txt" | sort | uniq -c | awk '{print $2" "$1}' | sort
}

# ---- the loop, driven entirely against node A ----------------------------
log "3. block 1 — oracle set"
SA=$(( $(date +%s) - 5 ))
"$WRF" oracle build-registrations --genesis "$RUN/genesis.json" --chain-id "$CHAIN_ID" \
    --epoch 1 --activated-at 1785240000 --submitted-at "$SA" \
    --governance-key "$RUN/gov/validator-1.key" --out "$RUN/oracle-artifact.json" >/dev/null || fail "oracle build"
jq -c '{chain_id, network_id, transactions}' "$RUN/oracle-artifact.json" > "$RUN/o-submit.json"
echo "oracle registrations -> HTTP $(post /transactions/oracle/registrations "$RUN/o-submit.json" "$RUN/r-o.json")"
echo "mine -> HTTP $(mine 1)"

log "4. block 2 — station registration ($CLASS)"
SA=$(( $(date +%s) - 5 ))
"$WRF" station build-registration --station-id "$STATION_ID" --owner-address "$CONTRIBUTOR" \
    --owner-key "$RUN/owner/validator-1.key" --governance-key "$RUN/gov/validator-1.key" \
    --chain-id "$CHAIN_ID" --network-id "$NETWORK_ID" --latitude 46.7867 --longitude -92.1005 \
    --provenance-class "$CLASS" --submitted-at "$SA" --out "$RUN/station-artifact.json" || fail "station build"
jq -c '{chain_id, network_id, transactions}' "$RUN/station-artifact.json" > "$RUN/s-submit.json"
echo "station registration -> HTTP $(post /transactions/station/registrations "$RUN/s-submit.json" "$RUN/r-s.json")"
jq -c '.stations' "$RUN/r-s.json" 2>/dev/null
echo "mine -> HTTP $(mine 2)"

log "5. blocks 3-4 — commit, reveal, two attestations"
TS=$(date +%s)
DATA='{"dewpoint_c":13.5,"humidity_percent":60.0,"precipitation_mm":0.0,"pressure_hpa":1014.0,"quality":1.0,"temperature_c":21.5,"uniqueness":1.0,"volume":1.0,"wind_speed_ms":2.8}'
"$WRF" station build-observation --station-id "$STATION_ID" --contributor "$CONTRIBUTOR" \
    --signer-key "$RUN/owner/validator-1.key" --chain-id "$CHAIN_ID" --network-id "$NETWORK_ID" \
    --nonce demo-nonce-1 --timestamp "$TS" --data "$DATA" --out "$RUN/obs.json" || fail "build-observation"
COMMIT_ID=$(jq -r '.commit_id' "$RUN/obs.json")
jq -c '.commit_body' "$RUN/obs.json" > "$RUN/c.json"; jq -c '.reveal_body' "$RUN/obs.json" > "$RUN/r.json"
echo "commit -> HTTP $(post /transactions/contribution/commit "$RUN/c.json" "$RUN/r-c.json")"
echo "mine -> HTTP $(mine 3)"
echo "reveal -> HTTP $(post /transactions/contribution/reveal "$RUN/r.json" "$RUN/r-r.json")"
for i in 1 2; do
    OID=$(cat "$RUN/oracles/validator-$i.pub")
    "$WRF" station build-attestation --commit-id "$COMMIT_ID" --oracle-id "$OID" \
        --oracle-key "$RUN/oracles/validator-$i.key" --timestamp $(( TS + i )) \
        --nonce "att-$i" --out "$RUN/att-$i.json" >/dev/null || fail "attestation $i"
    echo "attestation $i -> HTTP $(post /transactions/oracle/attestation "$RUN/att-$i.json" "$RUN/r-a$i.json")"
done
echo "mine -> HTTP $(mine 4)"

log "6. wait for B to sync"
for _ in $(seq 1 "$SYNC_ATTEMPTS"); do
    HA=$(height $A_API); HB=$(height $B_API)
    [ "$HA" = "$HB" ] && [ "$HA" != "0" ] && break
    sleep "$SYNC_SLEEP"
done
HA=$(height $A_API); HB=$(height $B_API)
echo "A height=$HA   B height=$HB"

# ---- convergence verdict --------------------------------------------------
log "7. CONVERGENCE VERDICT (consensus-authoritative)"
SNAP_A=$(snap $A_API); SNAP_B=$(snap $B_API)
ROOT_A=$(printf '%s' "$SNAP_A" | jq -er '.state_root_hex | select(type=="string" and length>0)' 2>/dev/null)
ROOT_B=$(printf '%s' "$SNAP_B" | jq -er '.state_root_hex | select(type=="string" and length>0)' 2>/dev/null)
# genesis_hash lives on /network/info, not /chain/block/0 (which 404s at height 0).
GEN_A=$(curl -fsS "http://127.0.0.1:$A_API/network/info" | jq -er '.genesis_hash | select(type=="string" and length>0)' 2>/dev/null)
GEN_B=$(curl -fsS "http://127.0.0.1:$B_API/network/info" | jq -er '.genesis_hash | select(type=="string" and length>0)' 2>/dev/null)
DIV_A=$(api "http://127.0.0.1:$A_API/metrics" | awk '$1 ~ /^wrfcoin_state_divergence_total(\{|$)/ { sum += $2; found = 1 } END { if (found) print sum }')
DIV_B=$(api "http://127.0.0.1:$B_API/metrics" | awk '$1 ~ /^wrfcoin_state_divergence_total(\{|$)/ { sum += $2; found = 1 } END { if (found) print sum }')
PVER_A=$(api "http://127.0.0.1:$A_API/chain/block/1" | jq -r '..|objects|.protocol_version? // empty' 2>/dev/null | head -1)
PVER_B=$(api "http://127.0.0.1:$B_API/chain/block/1" | jq -r '..|objects|.protocol_version? // empty' 2>/dev/null | head -1)

echo "genesis_hash  A=$GEN_A"
echo "genesis_hash  B=$GEN_B"
echo "state_root    A=${ROOT_A:-<none>}"
echo "state_root    B=${ROOT_B:-<none>}"
echo "divergence    A=${DIV_A:-<absent>}  B=${DIV_B:-<absent>}"
echo "block-1 protocol_version  A=${PVER_A:-<unset>}  B=${PVER_B:-<unset>}"

DIVERGED=()
# Fail closed on an absent root: two missing values must never compare equal.
[ -n "$ROOT_A" ] && [ -n "$ROOT_B" ] || DIVERGED+=("state_root unreadable on at least one node")
[ -n "$ROOT_A" ] && [ "$ROOT_A" = "$ROOT_B" ] || DIVERGED+=("state_root mismatch")
[ -n "$GEN_A" ] && [ "$GEN_A" = "$GEN_B" ] || DIVERGED+=("genesis_hash mismatch")
[ "$HA" = "$HB" ] || DIVERGED+=("height mismatch A=$HA B=$HB")
[ "${DIV_A:-0}" = "0" ] || DIVERGED+=("A reports state divergence ${DIV_A}")
[ "${DIV_B:-0}" = "0" ] || DIVERGED+=("B reports state divergence ${DIV_B}")

# Entry-prefix parity. Telemetry prefixes are EXCLUDED from the consensus
# state_root (#1680/#1681) and may legitimately differ between a producer and a
# syncing follower, so they are reported and never failed on.
PFX_A=$(entry_prefixes $A_API); PFX_B=$(entry_prefixes $B_API)
TELEMETRY='^(quality_event|quality_event_archive|audit)'
CONS_A=$(printf '%s\n' "$PFX_A" | grep -Ev "$TELEMETRY" | sort)
CONS_B=$(printf '%s\n' "$PFX_B" | grep -Ev "$TELEMETRY" | sort)
echo "consensus entry prefixes A: $(printf '%s' "$CONS_A" | tr '\n' ' ')"
echo "consensus entry prefixes B: $(printf '%s' "$CONS_B" | tr '\n' ' ')"
if [ "$PFX_A" = "ENTRIES_UNREADABLE" ] || [ "$PFX_B" = "ENTRIES_UNREADABLE" ]; then
    DIVERGED+=("snapshot entries unreadable on at least one node")
elif [ "$CONS_A" != "$CONS_B" ]; then
    DIVERGED+=("consensus entry-prefix counts differ: $(diff <(printf '%s\n' "$CONS_A") <(printf '%s\n' "$CONS_B") | tr '\n' ' ')")
fi
# The pin is only meaningful if block 1 actually recorded it, and both nodes
# must read the same value back out of chain content.
[ -n "$PVER_A" ] || DIVERGED+=("block 1 recorded no protocol_version — the pin did not take")
[ "$PVER_A" = "$PVER_B" ] || DIVERGED+=("protocol_version mismatch A=$PVER_A B=$PVER_B")
[ "$PVER_A" = "$PROTOCOL_VERSION" ] || DIVERGED+=("block 1 recorded protocol_version '$PVER_A', pinned '$PROTOCOL_VERSION'")

log "8. the reward, as seen by EACH node independently"
for P in $A_API $B_API; do
    N=$([ "$P" = "$A_API" ] && echo A || echo B)
    S=$(api "http://127.0.0.1:$P/transactions/commit/$COMMIT_ID/status" 2>/dev/null)
    B_=$(api "http://127.0.0.1:$P/addresses/$CONTRIBUTOR" 2>/dev/null)
    echo "$N status : $(printf '%s' "$S" | jq -c '{status,provenance_class,approvals,has_quorum,reward_status,reward_amount}' 2>/dev/null)"
    echo "$N balance: $(printf '%s' "$B_" | jq -c '.data | {earned_balance_total, balance}' 2>/dev/null)"
done
EARN_A=$(api "http://127.0.0.1:$A_API/addresses/$CONTRIBUTOR" 2>/dev/null | jq -r '.data.earned_balance_total // 0')
EARN_B=$(api "http://127.0.0.1:$B_API/addresses/$CONTRIBUTOR" 2>/dev/null | jq -r '.data.earned_balance_total // 0')
EARN_A=${EARN_A:-0}; EARN_B=${EARN_B:-0}

log "RESULT (arm=$ARM)"
echo "earned_balance_total  A=$EARN_A  B=$EARN_B"
if [ "$ARM" = "isolated" ]; then
    # The control claims two GENUINELY DIFFERENT chains diverge. Two BROKEN
    # chains also diverge, and prove nothing — so require that node A actually
    # completed the loop first. Without this, any run where A falls over (a
    # keygen error, a failed mine, a CLI regression) reports PASS. The port
    # guard removed the likeliest cause of that; this removes the class.
    # Raised by Mia in review of #2581.
    [ "$EARN_A" -gt 0 ] 2>/dev/null || {
        echo "RED CONTROL VACUOUS: A never paid (earned=$EARN_A), so divergence proves nothing"
        exit 1
    }
    if [ ${#DIVERGED[@]} -gt 0 ]; then
        printf 'PASS (RED control): two unconnected chains DIVERGED as they must:\n'
        printf '  - %s\n' "${DIVERGED[@]}"; exit 0
    fi
    echo "RED CONTROL FAILED: unconnected nodes reported convergence — the assertion cannot fail"; exit 1
fi
if [ ${#DIVERGED[@]} -gt 0 ]; then
    printf 'DIVERGED:\n'; printf '  - %s\n' "${DIVERGED[@]}"; exit 1
fi
if [ "$ARM" = "real" ]; then
    [ "$EARN_A" -gt 0 ] 2>/dev/null || { echo "DIVERGED: A never paid"; exit 1; }
    [ "$EARN_A" = "$EARN_B" ] || { echo "DIVERGED: B disagrees on the paid balance"; exit 1; }
    echo "CONVERGED: state_root identical, and BOTH nodes independently report earned=$EARN_A"; exit 0
else
    [ "$EARN_A" = "0" ] && [ "$EARN_B" = "0" ] || { echo "CONTROL FAILED: synthetic paid A=$EARN_A B=$EARN_B"; exit 1; }
    echo "CONVERGED (control): state_root identical and BOTH nodes paid zero"; exit 0
fi
