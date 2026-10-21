These synthetic vectors were produced by core4
`65134a64492135b6ac81344dd0d2edd8dfaf1309` with the actual B3 features
`full-mining,p2p,hsm,upgrade-notice-fetch` (no `zk`). The authoritative image
recipe is infra `750298cdf20c:infra/docker/Dockerfile.core`; the paired compose
override sets revision/creation arguments, with no feature override.

The local reproduction used two production `P2pNode` instances on loopback TCP.
A broadcast each serialized transaction; B received `NewTransactions`, decoded
it and called `add_transaction_with_signing_keys`, as launch_api does. An
unmodified `ProofOfPublishing::create_next_block` then included it and advanced
the local producer to height 1. Both producer and peer used a pinned clock
at 1800000060, V1 and an empty activation table. B then gossiped its produced
block back to A, whose verifier checked those exact received bytes. No external
nodes were contacted.

* ConfidentialTransfer: the producer included the Standard transaction and
  applied its commitment, but the same-feature peer rejected its range proof:
  `ZK verification unavailable: node compiled without zk feature`.
* RewardClaim: a valid eight-member MLSAG over eight active synthetic ring-key
  stations was included and accepted by the peer. This example does not
  establish a RewardClaim liveness failure.

Each JSON contains exact received bincode `block_bytes`, canonical
`consensus_bytes`, readable block, seeded
stations, prior peer verdict and post-replay state root. The regression checks
canonical byte identity, the sealed hash, unchanged default-verifier verdict and unchanged
historical replay root. Replay of a rejected confidential vector demonstrates
state semantics only; it does not represent accepting that block at ingress.

The packet proves a producer/peer divergence on a fresh synthetic chain; it
does not claim a finalized live-chain fork or measure a halt duration. Production
producer authority, BFT finality and live chain state were not exercised. Local
admission and relay refusal prevents this transaction from reaching candidate
selection without altering historical validation, state application or schedules.

`b3_probe.rs` is the source of the baseline probe. To reproduce, use a detached
worktree at the B3 SHA above, copy it to `native-chain/src/privacy_b3_probe.rs`,
and add `#[cfg(test)] mod privacy_b3_probe;` to that worktree's native library.
After checking MemAvailable is at least 8 GiB, run exactly one heavy job:

```sh
CARGO_BUILD_JOBS=3 cargo test --offline --locked -p wrfcoin-native-chain \
  --features full-mining,p2p,hsm,upgrade-notice-fetch --lib privacy_b3_probe -- --nocapture
```

The generator writes its synthetic packet into the SOL-G evidence directory
under `<system-temp>/`. Its signing
randomness creates a new valid packet; the checked-in vectors are the immutable
output of the recorded local run. Clean only that worktree's target afterward.
