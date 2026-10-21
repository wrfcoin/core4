# Fresh testnet genesis policy

The new testnet reads emission, governance authority, and node timing from block 1.
Environment inputs are declarations for the cutting node; they cannot retune an
existing chain. Chain 777 retains its historical reward curve, governance pins,
checkpoint encoding, and state-root material.

Testnet tokens have no monetary value. This document does not authorize a cut,
restart, transaction, or publication.

## Owner-selected 888 inputs

| Cut input | Recorded block-1 key | Value |
| --- | --- | --- |
| `WRFCOIN_CHAIN_ID` | `chain_id` | `888` |
| `WRFCOIN_NETWORK_ID` | `network_id` | `testnet` |
| `WRFCOIN_SIGNING_PROTOCOL_VERSION` | `protocol_version` | `v1` |
| `WRFCOIN_GENESIS_FLAT_REWARD_BASE_UNITS` | `testnet_flat_reward_base_units` | `2222222222` (22.22222222 WRF), REQUIRED |
| `WRFCOIN_GENESIS_FLAT_REWARD_SECONDS` | `testnet_flat_reward_seconds` | `600` |
| `WRFCOIN_GENESIS_FAUCET_ADDRESS` | `faucet_address` | Owner's faucet address |
| `WRFCOIN_GENESIS_FAUCET_ALLOCATION_BASE_UNITS` | `testnet_faucet_allocation_base_units` | `10000000000000` (100,000 WRF) |
| `WRFCOIN_GENESIS_SCHEDULE_GOVERNANCE_KEYS` | `schedule_governance_keys` | Existing public governance key below |
| `WRFCOIN_GENESIS_NODE_SHARE_EPOCH_BLOCKS` | `node_share_epoch_blocks` | `512` |
| `WRFCOIN_GENESIS_NODE_SHARE_CHALLENGE_BLOCKS` | `node_share_challenge_blocks` | `512` |

The governance key is
`e7bec166ad446314d26fb7526bfa8cec1b363b438440262fb001f7b04b20bf00`
(the existing public Ed25519 key; no private signing material belongs here).
Additional genesis keys must be canonical, sorted, unique public keys. Changing
this input after block 1 does not rotate authority.

Set `WRFCOIN_GENESIS_ACTIVATION_SCHEDULE` to:

```text
0:v1,2:schedule_extension,3:reward_gross_outlier_guard,4:weather_skew_block_time_allowance,5:station_active_reward_gate,6:station_owner_actions,7:station_owner_consent,8:station_registration_nonce,9:node_share_settlement
```

This enables the eight named features and the ninth function through the recorded
faucet address. The legacy `faucet_sink_binding` row belongs to a particular 777
genesis; 888 does not import that row. Signing remains v1. No other feature is
scheduled. In particular, paymaster, social recovery, session keys, reference
sea-level pressure, and stake submission nonce remain off.

Node pay must start after block 1 because its registry binds the final genesis
hash. Missing schedules refuse a fresh testnet boot. Both block producers and
verifiers reject new-testnet block 1 with absent schedule/faucet, malformed
policy, invalid governance keys when needed, or node pay at height 0/1.

The 600-second tier declaration requires an explicit
`testnet_flat_reward_base_units` amount; there is no tier-derived default.
Producer/verifier block gates, State admission, and checkpoint-envelope decoding
reject a seconds declaration without the amount. The owner selected `2222222222` base units/block for 888 on 2026-10-19,
matching current 777 arithmetic after height 43000 (22.22222222 WRF).
The initial 600-second arithmetic (33.33333333 WRF) is not the selected amount;
tests also cover it to verify that consensus uses the recorded input. Role shares and supply caps still apply.
This is a budget, not a promise that every slice is minted. The faucet allocation
is a separate, one-time, supply-accounted credit to the recorded faucet only.
Replaying the same genesis declaration cannot credit it twice. There is no list
of arbitrary premine recipients.

## Compatibility and validation

The optional new policy uses a versioned storage envelope inside the existing
nested two-field checkpoint frame. An absent policy emits the old bytes. This
avoids inserting a positional field before the next State checkpoint field.
The envelope is decoded before any chain identity or signing operation. New
policy contents also contribute explicit state-root material; legacy 777 emits
none of that extra material.

Focused controls live in `shared-protocol/tests/genesis_testnet_888.rs` and
`shared-protocol/tests/genesis_777_replay_differential.rs`. The latter can emit
301 synthetic WAL-replay roots using `WRFCOIN_TEST_REPLAY_OUTPUT`; run the same
fixture against independent baseline and candidate build directories and compare
bytes. It is synthetic replay evidence, not a capture of the running chain.

The local three-node rehearsal uses only generated disposable keys, loopback
addresses, and owned scratch databases. `WRFCOIN_P2P_HOST=127.0.0.1` restricts the
transport listener; its default remains `0.0.0.0`. UPnP gateway discovery is
suppressed when all configured TCP/QUIC listeners are loopback. Do not use live node ports or
volumes. `scripts/genesis_888_scratch.py` builds no binaries; it expects debug
`launch_api` (with `p2p`) and the `genesis_888_scratch_registration` example under
`target/debug`. It stops all children in a `finally` block and records public
evidence below `round-r-evidence/`. This directory contains disposable test key
material and must never be committed or published.

For a disposable rehearsal using the owner-selected budget (not a live cut):

```sh
python3 scripts/genesis_888_scratch.py --flat-reward-base-units 2222222222
```

## Real-chain replay input

The API snapshot export is a current-tip State plus block 1 and a bounded recent
block window. Those recent blocks are context, already included in the exported
State. It is not a historical State anchor. `replay_verify` currently reads a
persisted genesis State and contiguous block archive, not this API response.
Therefore the API export alone cannot reproduce a past 200-block window.

The real 777 differential remains unverified pending a suitable offline artifact:
a State checkpoint before the window, block 1, and at least 200 subsequent
contiguous blocks. A future-window alternative would export once, then collect
200 new blocks. Either requires a bounded offline-anchor adapter and per-height
comparison between main and the candidate; never replay old blocks on tip State.
