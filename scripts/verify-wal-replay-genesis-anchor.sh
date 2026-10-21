#!/bin/bash
# Regression test for core4#2501 / PR #2510 — WAL replay must anchor on the
# persisted genesis state, not on State::new().
#
# Usage:  scripts/verify-wal-replay-genesis-anchor.sh
# Prereq: BOTH binaries -- the second was missing from this list until #2510
#         review, so a fresh checkout following it died at "cannot obtain a
#         producer keypair" (found by Iris):
#           cargo build -p wrfcoin-native-chain --bin launch_api --features full-mining,p2p
#           cargo build -p wrfcoin_cli --bin wrfcoin
#
# THE DEFECT. The genesis producer set is written into genesis `State` by
# `State::seed_genesis_producer_set` and is described by no `StateTransition`,
# so the WAL cannot carry it. `Storage::load_state` anchors replay on
# `State::new()` when no checkpoint is recorded, replaying a faithful log onto
# the wrong base.
#
# THE CONDITION. Only an ABRUPT stop below the periodic checkpoint interval
# exposes it: on clean exit the node calls `force_checkpoint` at any height > 0,
# which writes a correct anchor. So this test uses `kill -9`, not SIGTERM. A
# graceful shutdown here would make the test vacuous — it would pass with or
# without the fix.
#
# WHAT IS ASSERTED. state_root before the kill == state_root after restart, at
# the same height. Not block hash, and not the presence of the producer set:
# the producer set is re-seeded from the environment on every boot, so it looks
# present either way. The state root is what actually diverges, and it is what
# peers compare.
#
# EXPECTED. PASS on this branch. On a base without the fix it FAILS, reporting
# two different roots at the same height while the node starts happily and logs
# nothing wrong -- that silent success is the reason this needs a test rather
# than an operator runbook note.
#
# THIS SCRIPT DOES NOT RUN IN CI. Nothing under .github/ references it; it is a
# MANUAL tool and must be invoked by hand. Do not read its presence in scripts/
# as coverage -- nine neighbours here ARE CI-wired, which makes that the easy
# and wrong assumption. A regression that re-introduces the #2501 anchor loss
# will produce a GREEN pull request and this file, named after the very
# invariant it broke, will be the reason nobody looks further.
#
# Verified both directions on 2026-07-28 (#2510), so the assertion is known to
# discriminate rather than to pass unconditionally:
#     with the fix     51657f0e... -> 51657f0e...   PASS, exit 0
#     fix reverted     c7656de1... -> 818fdf82...   FAIL, exit 1
# Wiring it into a gate is tracked in #2552. Delete these lines when it lands.
set -uo pipefail

REPO="${WRFCOIN_REPO:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
RUN="${WRFCOIN_WAL_REGRESS_RUN_DIR:-${TMPDIR:-/tmp}/wrfcoin-wal-replay-2501}"
BIN="$REPO/target/debug/launch_api"
API_PORT="${WRFCOIN_WAL_REGRESS_API_PORT:-18570}"
P2P_PORT="${WRFCOIN_WAL_REGRESS_P2P_PORT:-19020}"
TOKEN=wal-regress-operator-token
CHAIN_ID=777
NETWORK_ID=wal-regress-local   # NOT "testnet": #2035 pins testnet to the
                               # canonical genesis and would refuse the restart.

[ -x "$BIN" ] || { echo "missing $BIN — build with --features full-mining,p2p"; exit 2; }
# Checked separately and BEFORE use: the keypair step below shells out to this
# binary, and without this guard its absence surfaced as the misleading
# "cannot obtain a producer keypair" rather than "you did not build it".
[ -x "$REPO/target/debug/wrfcoin" ] || {
    echo "missing $REPO/target/debug/wrfcoin — build with: cargo build -p wrfcoin_cli --bin wrfcoin"
    exit 2
}

rm -rf "$RUN"; mkdir -p "$RUN/data"

# Deterministic throwaway producer key. Not secret: this chain exists for the
# duration of one test run. secret-leak-ok: fixed test vector, never a real key.
PROD_SK=1111111111111111111111111111111111111111111111111111111111111111
PROD_PK=$("$REPO/target/debug/wrfcoin" wallet pubkey --private-key "$PROD_SK" 2>/dev/null \
          | jq -r '.data.publicKey // empty')
if [ -z "$PROD_PK" ]; then
    # Fall back to generating one; the value does not matter, only that the
    # producer set and the signer agree.
    "$REPO/target/debug/wrfcoin" setup keygen --output "$RUN/prod" --count 1 >/dev/null 2>&1 \
        || { echo "cannot obtain a producer keypair"; exit 2; }
    PROD_PK=$(cat "$RUN/prod/validator-1.pub")
    PROD_SK=$(cat "$RUN/prod/validator-1.key")
fi
PAYOUT="0x$(printf '%s' "$PROD_PK" | cut -c1-40)"
PRODUCER_SET=$(printf \
  '{"version":1,"members":[{"producer_id":"wal-regress","payout_address":"%s","public_key":"%s"}]}' \
  "$PAYOUT" "$PROD_PK")

GENESIS_HASH=""
start () {
    local -a extra=()
    [ -n "$GENESIS_HASH" ] && extra+=( "WRFCOIN_GENESIS_HASH=$GENESIS_HASH" )
    env "${extra[@]}" \
        WRFCOIN_API_HOST=127.0.0.1 WRFCOIN_API_PORT="$API_PORT" \
        WRFCOIN_P2P_ENABLED=true WRFCOIN_P2P_PORT="$P2P_PORT" \
        WRFCOIN_DATA_DIR="$RUN/data" \
        WRFCOIN_CHAIN_ID="$CHAIN_ID" WRFCOIN_NETWORK_ID="$NETWORK_ID" \
        WRFCOIN_OPERATOR_TOKEN="$TOKEN" \
        ALLOW_UNENCRYPTED_STORAGE=true \
        WRFCOIN_GENESIS_PRODUCER_SET="$PRODUCER_SET" \
        WRFCOIN_PRODUCER_SIGNER_PRIVATE_KEY="$PROD_SK" \
        "$BIN" >>"$RUN/node.log" 2>&1 &
    echo $! > "$RUN/node.pid"
    for _ in $(seq 1 60); do
        curl -fsS "http://127.0.0.1:$API_PORT/network/info" >/dev/null 2>&1 && return 0
        sleep 1
    done
    echo "node failed to start:"; tail -20 "$RUN/node.log"; return 1
}
api ()   { curl -fsS -H "Authorization: Bearer $TOKEN" "$@"; }
sr ()    { api "http://127.0.0.1:$API_PORT/state/snapshot" | jq -r '.state_root_hex'; }
height() { curl -fsS "http://127.0.0.1:$API_PORT/network/info" | jq -r '.chain_height'; }

echo "=== boot and mine block 1 ==="
start || exit 1
curl -sS -o /dev/null -w 'mine HTTP %{http_code}\n' \
    -X POST "http://127.0.0.1:$API_PORT/blocks/mine" \
    -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
    -d "{\"miner_address\":\"$PAYOUT\"}"

BEFORE_H=$(height); BEFORE_SR=$(sr)
GENESIS_HASH=$(curl -fsS "http://127.0.0.1:$API_PORT/network/info" | jq -r '.genesis_hash // empty')
echo "before: height=$BEFORE_H state_root=$BEFORE_SR"

echo
echo "=== kill -9 at height $BEFORE_H (no clean shutdown => no force_checkpoint) ==="
kill -9 "$(cat "$RUN/node.pid")" 2>/dev/null
sleep 3

echo "=== restart: recovery is WAL replay only ==="
start || exit 1
AFTER_H=$(height); AFTER_SR=$(sr)
echo "after:  height=$AFTER_H state_root=$AFTER_SR"
grep -E "Replayed .* WAL entries|Seeded genesis producer set" "$RUN/node.log" | tail -3

kill "$(cat "$RUN/node.pid")" 2>/dev/null; wait 2>/dev/null

echo
if [ "$BEFORE_H" != "$AFTER_H" ]; then
    echo "FAIL: height changed across replay ($BEFORE_H -> $AFTER_H)"; exit 1
fi
if [ "$BEFORE_SR" = "$AFTER_SR" ] && [ -n "$BEFORE_SR" ]; then
    echo "PASS: state_root survived WAL replay after an abrupt stop"
    exit 0
fi
echo "FAIL: state_root changed across WAL replay at height $AFTER_H"
echo "  before $BEFORE_SR"
echo "  after  $AFTER_SR"
echo "  replay anchored on the wrong base — genesis-only state was lost (#2501)"
exit 1
