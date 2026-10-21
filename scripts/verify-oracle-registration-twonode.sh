#!/bin/bash
# Two-node acceptance for core4#2530 — genesis oracle registration.
#
# Usage:  scripts/verify-oracle-registration-twonode.sh [green|red]
#
# Prereq: cargo build -p wrfcoin-native-chain --bin launch_api --features full-mining,p2p
#         cargo build -p wrfcoin_cli --bin wrfcoin
#
# GREEN: one ceremony, submitted to node A only. A cuts block 1; B acquires the
#        oracle set purely by syncing it. Asserts equal height, equal
#        state_root, and the set present on B.
#
# RED:   the two nodes run ceremonies differing ONLY in --activated-at, and are
#        left unconnected so neither reorgs onto the other. Their state_roots
#        must diverge. This is what proves the determinism requirement is
#        load-bearing rather than decorative: both nodes report the same height
#        and the same {oracle_set, quorum}, so every surface-level check reads
#        healthy while the chains are already incompatible.
#
# Convergence is asserted on state_root (via /state/snapshot), never on block
# hash.
set -uo pipefail

REPO="${WRFCOIN_REPO:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
RUN="${WRFCOIN_TWONODE_RUN_DIR:-${TMPDIR:-/tmp}/wrfcoin-twonode-2530}"
# Honour CARGO_TARGET_DIR. Hard-coding $REPO/target meant a verifier who built
# into a scratch target dir — the documented way to do this when disk is tight —
# either got "binary not found" after a SUCCESSFUL build, or silently ran
# whatever stale binaries happened to sit in $REPO/target from an older commit.
# The second failure is the dangerous one: a green acceptance run proving
# something about code that is not the code under test. Found by an independent
# verifier who had to override the bin root to test the binaries it had just
# built.
TARGET_DIR="${CARGO_TARGET_DIR:-$REPO/target}"
BIN="$TARGET_DIR/debug/launch_api"
WRF="$TARGET_DIR/debug/wrfcoin"

for required in "$BIN" "$WRF"; do
    [ -x "$required" ] || {
        echo "FAIL: $required is missing or not executable."
        echo "  Build with:  cargo build -p wrfcoin-native-chain --bin launch_api --features full-mining,p2p"
        echo "               cargo build -p wrfcoin_cli --bin wrfcoin"
        echo "  Searched under TARGET_DIR=$TARGET_DIR (set CARGO_TARGET_DIR or WRFCOIN_REPO to change)."
        exit 2
    }
done

MODE="${1:-green}"          # green | red
ACT_A=1785240000
ACT_B=1785240000
[ "$MODE" = "red" ] && ACT_B=1785240777   # the only difference in the RED arm

CHAIN_ID=777
NETWORK_ID=testnet
TOKEN=twonode-operator-token
SUBMITTED_AT=$(( $(date +%s) - 30 ))

# Ports are overridable and CHECKED. They were hard-coded with no guard, so a
# stale listener from any other run silently redirected every request to a
# different node -- one whose operator token differs, which surfaces as
# `submit -> HTTP 403 Invalid API token` and looks like an auth bug in this
# harness. That cost a full diagnostic cycle (#2698).
#
# `demo-two-node-convergence.sh` already had this guard; the lesson was applied
# to one of the two scripts and not to the only two-node acceptance in the repo.
# Its comment states the principle and it applies verbatim here:
#     "A verdict that can be produced by looking at someone else's chain is not
#      a verdict."
A_API="${A_API:-18556}"; A_P2P="${A_P2P:-19000}"
B_API="${B_API:-18557}"; B_P2P="${B_P2P:-19001}"
for port in $A_API $B_API $A_P2P $B_P2P; do
    if ss -ltn 2>/dev/null | grep -qE "[:.]${port}[[:space:]]"; then
        echo "FAIL: port $port is already in use - refusing to run rather than"
        echo "      risk measuring another process's chain. Free it, or set"
        echo "      A_API/B_API/A_P2P/B_P2P to an unused range."
        ss -ltnp 2>/dev/null | grep -E "[:.]${port}[[:space:]]" || true
        exit 2
    fi
done

rm -rf "$RUN"; mkdir -p "$RUN/a" "$RUN/b" "$RUN/keys" "$RUN/gov" "$RUN/prod"

log() { printf '\n=== %s ===\n' "$*"; }

# --- ceremony inputs -------------------------------------------------------
log "ceremony inputs"
"$WRF" setup keygen --output "$RUN/keys" --count 3 >/dev/null
"$WRF" setup keygen --output "$RUN/gov"  --count 1 >/dev/null
"$WRF" setup keygen --output "$RUN/prod" --count 1 >/dev/null
"$WRF" setup genesis --validator-keys "$RUN/keys" --oracle-quorum 2 \
    --network-id "$NETWORK_ID" --output "$RUN/genesis.json" >/dev/null
GOV_PK=$(cat "$RUN/gov/validator-1.pub")
# Cut-time oracle declaration, read ONCE by the node that produces block 1.
# The harness predates #2523, which made this declaration mandatory at cut time,
# so `stamp_genesis_oracle_declaration` refused every cut and the acceptance
# could never reach the assertion it exists to make (#2698).
#
# Node A ONLY, deliberately. It is a cut-time input written straight into block
# 1's metadata, never a validation reference -- that was the #2496 defect this
# variable's role was changed to remove. B must derive the set from the block it
# syncs, which is the whole property under test; exporting it to B would make
# GREEN pass even if syncing were broken.
ORACLE_PKS=$(cat "$RUN"/keys/validator-*.pub | jq -R . | jq -sc .)
CUT_ORACLE_SET=$(printf '{"oracle_set":%s,"oracle_quorum":2}' "$ORACLE_PKS")
PROD_PK=$(cat "$RUN/prod/validator-1.pub")
PROD_SK=$(cat "$RUN/prod/validator-1.key")
PAYOUT="0x$(printf '%s' "$PROD_PK" | cut -c1-40)"
PRODUCER_SET=$(printf '{"version":1,"members":[{"producer_id":"twonode-a","payout_address":"%s","public_key":"%s"}]}' "$PAYOUT" "$PROD_PK")
echo "governance pubkey: $GOV_PK"
echo "producer pubkey:   $PROD_PK"
echo "N=3 K=2  submitted_at=$SUBMITTED_AT  activated_at A=$ACT_A B=$ACT_B"

build_artifact () {  # $1=out  $2=activated_at
    "$WRF" oracle build-registrations \
        --genesis "$RUN/genesis.json" --chain-id "$CHAIN_ID" \
        --epoch 1 --activated-at "$2" --submitted-at "$SUBMITTED_AT" \
        --governance-key "$RUN/gov/validator-1.key" --out "$1" >/dev/null
}
build_artifact "$RUN/artifact-a.json" "$ACT_A"
build_artifact "$RUN/artifact-b.json" "$ACT_B"

# --- nodes -----------------------------------------------------------------
start_node () {  # $1=name  $2=api_port  $3=p2p_port  $4=bootstrap(optional)
    local name=$1 api=$2 p2p=$3 boot=${4:-}
    local -a envs=(
        WRFCOIN_API_HOST=127.0.0.1
        WRFCOIN_API_PORT=$api
        WRFCOIN_P2P_ENABLED=true
        WRFCOIN_P2P_PORT=$p2p
        WRFCOIN_DATA_DIR=$RUN/$name
        WRFCOIN_CHAIN_ID=$CHAIN_ID
        WRFCOIN_NETWORK_ID=$NETWORK_ID
        WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS=$GOV_PK
        WRFCOIN_OPERATOR_TOKEN=$TOKEN
        ALLOW_UNENCRYPTED_STORAGE=true
        WRFCOIN_GENESIS_PRODUCER_SET=$PRODUCER_SET
        WRFCOIN_PRODUCER_SIGNER_PRIVATE_KEY=$PROD_SK
    )
    # Cutting node only -- see CUT_ORACLE_SET above. In RED both nodes cut their
    # own block 1, so both need it; in GREEN only A cuts.
    if [ "$name" = "a" ] || [ "$MODE" = "red" ]; then
        envs+=( "WRFCOIN_GENESIS_ORACLE_SET=$CUT_ORACLE_SET" )
    fi
    [ -n "$boot" ] && envs+=( "WRFCOIN_P2P_BOOTSTRAP=$boot" )
    env "${envs[@]}" "$BIN" >"$RUN/$name.log" 2>&1 &
    echo $! > "$RUN/$name.pid"
}

wait_http () {  # $1=port  $2=label
    for _ in $(seq 1 60); do
        curl -fsS "http://127.0.0.1:$1/network/info" >/dev/null 2>&1 && { echo "$2 up"; return 0; }
        sleep 1
    done
    echo "$2 FAILED to come up"; tail -25 "$RUN/${2}.log"; return 1
}

log "starting nodes"
start_node a $A_API $A_P2P
wait_http $A_API a || exit 1
A_PEER=$(curl -fsS http://127.0.0.1:$A_API/network/info | jq -r '.peer_id // empty')
BOOT="/ip4/127.0.0.1/tcp/$A_P2P${A_PEER:+/p2p/$A_PEER}"
echo "bootstrap: $BOOT"
if [ "$MODE" = "green" ]; then
    start_node b $B_API $B_P2P "$BOOT"
else
    # isolated: each node cuts its own block 1 from its own ceremony
    start_node b $B_API $B_P2P
fi
wait_http $B_API b || exit 1

api () { curl -fsS -H "Authorization: Bearer $TOKEN" "$@"; }
state_root () { api "http://127.0.0.1:$1/state/snapshot" | jq -r '.state_root_hex'; }
height () { curl -fsS "http://127.0.0.1:$1/network/info" | jq -r '.chain_height'; }

log "pre-submit"
echo "A height=$(height $A_API) state_root=$(state_root $A_API)"
echo "B height=$(height $B_API) state_root=$(state_root $B_API)"

# --- submit ----------------------------------------------------------------
submit () {  # $1=port  $2=artifact
    jq -c '{chain_id, network_id, transactions}' "$2" \
    | curl -sS -o "$RUN/submit-$1.json" -w '%{http_code}' \
        -X POST "http://127.0.0.1:$1/transactions/oracle/registrations" \
        -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
        --data-binary @-
}

log "submitting registrations"
if [ "$MODE" = "green" ]; then
    # One ceremony, submitted to A only. B must acquire the oracle set purely by
    # syncing block 1 -- that is the assertion.
    echo "A submit -> HTTP $(submit $A_API "$RUN/artifact-a.json")"
    jq -c '{accepted, oracle_quorum, transaction_ids}' "$RUN/submit-$A_API.json"
else
    # Each node builds its own block 1 from a ceremony differing only in
    # activated_at.
    echo "A submit -> HTTP $(submit $A_API "$RUN/artifact-a.json")"
    echo "B submit -> HTTP $(submit $B_API "$RUN/artifact-b.json")"
fi

mine () { curl -sS -X POST "http://127.0.0.1:$1/blocks/mine" \
          -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
          -d "{\"miner_address\":\"$PAYOUT\"}" \
          -o "$RUN/mine-$1.json" -w '%{http_code}'; echo; }

log "mining block 1"
echo "A mine -> HTTP $(mine $A_API)"; head -c 200 "$RUN/mine-$A_API.json"; echo
if [ "$MODE" = "red" ]; then echo "B mine -> HTTP $(mine $B_API)"; head -c 200 "$RUN/mine-$B_API.json"; echo; fi

log "waiting for convergence"
for _ in $(seq 1 45); do
    HA=$(height $A_API); HB=$(height $B_API)
    [ "$HA" = "$HB" ] && [ "$HA" != "0" ] && break
    sleep 2
done

# --- assertions ------------------------------------------------------------
log "RESULT ($MODE)"
HA=$(height $A_API); HB=$(height $B_API)
SA=$(state_root $A_API); SB=$(state_root $B_API)
printf 'A: height=%s state_root=%s\n' "$HA" "$SA"
printf 'B: height=%s state_root=%s\n' "$HB" "$SB"
echo "--- oracle config as seen by each node ---"
CFG_A=$(api http://127.0.0.1:$A_API/transactions/oracle/config | jq -c '{oracle_quorum, n: (.oracle_set|length)}')
CFG_B=$(api http://127.0.0.1:$B_API/transactions/oracle/config | jq -c '{oracle_quorum, n: (.oracle_set|length)}')
echo "A: $CFG_A"
echo "B: $CFG_B"

# A missing state_root must be a hard FAIL, never a verdict.
#
# The previous version printed EQUAL when the roots matched and DIVERGED
# otherwise, and always exited 0. Two ways that lied. The exit code carried no
# information at all, so a caller checking `$?` learned nothing. And an empty
# `$SA` — dead node, unreachable /state/snapshot, jq miss — fell into the else
# branch and printed DIVERGED, which is precisely the result the RED arm is
# looking for. RED could pass because nothing was running.
FAILED=0
fail() { echo "FAIL: $*"; FAILED=1; }
is_root() { [[ "$1" =~ ^[0-9a-f]{64}$ ]]; }

is_root "$SA" || fail "node A returned no usable state_root (got '$SA') — cannot judge this run"
is_root "$SB" || fail "node B returned no usable state_root (got '$SB') — cannot judge this run"
[ "$HA" = "1" ] || fail "node A height is '$HA', expected 1"
[ "$HB" = "1" ] || fail "node B height is '$HB', expected 1"

# Assert the oracle config, do not merely print it. It was printed and never
# checked, which left the acceptance criterion of #2530 — the set is actually
# seated, N=3 K=2, on both nodes — resting on the operator noticing a wrong
# number in the output. In GREEN the whole claim is that B obtained the set by
# syncing; `{"oracle_quorum":0,"n":0}` on B would still have passed on
# state_root equality alone, because two nodes that both failed to seat the set
# agree perfectly.
EXPECT_CFG='{"oracle_quorum":2,"n":3}'
[ "$CFG_A" = "$EXPECT_CFG" ] || fail "node A oracle config is $CFG_A, expected $EXPECT_CFG"
[ "$CFG_B" = "$EXPECT_CFG" ] || fail "node B oracle config is $CFG_B, expected $EXPECT_CFG"

# RED only: prove the nodes were actually unconnected rather than assuming it.
# The verifier confirmed it by reading the node logs, which is evidence the run
# does not produce on its own. Without this, a RED pass is consistent with two
# PEERED nodes that simply had not synced yet when the roots were sampled —
# divergence that would heal moments later and mean nothing.
if [ "$MODE" = "red" ]; then
    PEERS_A=$(curl -fsS http://127.0.0.1:$A_API/network/info | jq -r '.peer_count // "?"')
    PEERS_B=$(curl -fsS http://127.0.0.1:$B_API/network/info | jq -r '.peer_count // "?"')
    echo "peer_count A=$PEERS_A B=$PEERS_B"
    [ "$PEERS_A" = "0" ] || fail "RED requires unconnected nodes; node A has $PEERS_A peer(s)"
    [ "$PEERS_B" = "0" ] || fail "RED requires unconnected nodes; node B has $PEERS_B peer(s)"
fi

# #2706: "not tested" and "equal" must be distinguishable IN THE OUTPUT.
#
# The comparison below is already gated on $FAILED, and the height assertions
# above set it, so EQUAL cannot print at height 0 at this commit. But when the
# gate fires the run says NOTHING about state_root at all, and absence of a line
# is not a statement -- a reader looking for the convergence verdict finds no
# verdict and must infer why. Two empty chains agree trivially; that agreement
# must be named as untested rather than left silent.
#
# This block also re-derives the precondition explicitly instead of trusting
# $FAILED to have been set for the right reason: a future edit that reorders or
# weakens the height checks would otherwise silently re-open the exact path
# #2706 is about.
if [ "$HA" != "1" ] || [ "$HB" != "1" ]; then
    echo "STATE_ROOT: NOT TESTED - A height=$HA B height=$HB; convergence is"
    echo "            undefined unless BOTH nodes hold block 1. Two chains with"
    echo "            no block agree trivially and prove nothing (#2706)."
    FAILED=1
fi

if [ "$FAILED" = 0 ]; then
    if [ "$SA" = "$SB" ]; then VERDICT=EQUAL; else VERDICT=DIVERGED; fi
    echo "STATE_ROOT: $VERDICT"
    case "$MODE" in
        green)
            [ "$VERDICT" = EQUAL ] ||
                fail "GREEN requires EQUAL state_root — the set did not converge by sync"
            ;;
        red)
            [ "$VERDICT" = DIVERGED ] ||
                fail "RED requires DIVERGED state_root — a nondeterministic input stopped mattering, \
so the GREEN arm proves nothing"
            ;;
    esac
fi

kill "$(cat "$RUN/a.pid")" "$(cat "$RUN/b.pid")" 2>/dev/null
wait 2>/dev/null

if [ "$FAILED" = 0 ]; then echo "ACCEPTANCE ($MODE): PASS"; else echo "ACCEPTANCE ($MODE): FAIL"; fi
exit "$FAILED"
