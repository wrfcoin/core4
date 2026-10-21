# ValidatorSet.tla — notes

Story #2099, epic #2093. Companion to `formal/README.md`, which covers running
TLC and the `WrfConsensus` spec.

> Kept as a separate file rather than a section of `formal/README.md` so that
> #2098 and #2099 — two branches off `main` — do not collide on one file.
> Whoever merges second may want to fold this in.

Derived from, at commit `b0bd2060c`:

- `consensus-engines/src/selection/validator_set_source.rs` —
  `validators_from_state` (line 41)
- `consensus-engines/src/selection/bft_finality.rs` — `is_supermajority`
  (line 511), `required_weight` (line 516), `FinalityVote` (line 80),
  `content_hash` (line 94)

## Why small validator sets are the whole point

Quorum-rounding mistakes hide at `|V| = 1, 2, 3`, where integer division
behaves least like the "two thirds of the validators" intuition. A 12-node
testnet never exercises them. Each size is run explicitly:

| Config | Universe | Result |
|---|---|---|
| `ValidatorSet_V1.cfg` | 1 validator | pass — 138 distinct states, <1s |
| `ValidatorSet_V2.cfg` | 2 validators | pass — 6,165 distinct states, <1s |
| `ValidatorSet_V3.cfg` | 3 validators | pass — 382,023 distinct states, ~6s |
| `ValidatorSet_V4.cfg` | 4 validators | pass — 23,122,179 distinct states, ~3m |
| `ValidatorSet_StaleGadget.cfg` | 3 validators, `GadgetTracksSet = FALSE` | **expected FAIL** — `FinalizedOnlyByRealSupermajority` violated |

The model reaches every subset of the universe, including the empty set, so
`|V| = 4` also covers all the smaller *reachable* sets; the dedicated configs
exist because the story asks for each size explicitly and because a small
config that fails is far easier to read.

Invariants: `TypeOK`, `QuorumThresholdSound`,
`EmptySetCannotFinalize`, `NoStaleVoteCounted`, `OnlyMembersContributeWeight`.

### The threshold arithmetic checked

`QuorumThresholdSound` asserts, for every reachable set, that
`RequiredWeight(total)` is the *smallest* weight that is strictly more than two
thirds of total power — in both directions:

- the threshold **is** a supermajority (too low lets a minority finalize)
- one less is **not** (too high stalls the chain)

Both use TLA+ `\div` — floor division, matching Rust's `/` on unsigned
integers. The rounding is the thing under test, so it is deliberately not
"cleaned up" into rational arithmetic.

Worked values, confirming the Rust formula behaves at the sizes that matter:

| Total power | `(total*2) \div 3 + 1` | Strictly > 2/3 total? |
|---:|---:|---|
| 0 | 1 | vacuous — no vote can be cast |
| 1 | 1 | 1 > 0.67 ✓ |
| 2 | 2 | 2 > 1.33 ✓ (1 would not be) |
| 3 | 3 | 3 > 2 ✓ — unanimity, f = 0 |
| 4 | 3 | 3 > 2.67 ✓ — f = 1 |

`EmptySetCannotFinalize` pins the `total = 0` case: required weight is 1 and no
vote can exist, so an empty set fails closed. Asserting it means a future
special-case for `total = 0` cannot silently make zero votes a quorum.

### What this models — and what it does not

**This is a model of the designed protocol, not of the running testnet.** Two
facts, both checked in the Rust rather than assumed:

1. **The finality gadget is not attached in production.**
   `native-chain/src/bin/launch_api/main.rs:961` sets
   `bft_finality_for_p2p = None`, and the comment above it says the intended
   `ProofOfStake::attach_bft_finality` wiring is future work. The testnet runs
   PoP. **Nothing in this spec describes a decision the deployed node makes
   today.**
2. **The gadget has no set-rotation API.** `BftFinalityGadget::new`
   (`bft_finality.rs:344-353`) sums `total_weight` once from its constructor
   argument, and nothing rotates it. Epoch-boundary rotation is a property of
   the *design*; the built gadget cannot express it.

Both counterexamples below are therefore **design-level findings**, not
evidence that a running node executes these transitions.

### Stake-weighted, unlike the finality spec in #2255

This model is **stake-weighted**: `power[v] \in 1..MaxPower` mirrors
`FinalityValidator::stake_weight`, and `IsSupermajority` sums weights exactly as
`bft_finality.rs:511` does. The sibling TLA+ finality spec (#2255) is
**count-weighted**, which is a real difference and not a presentational one — a
safety property proved under count-weighting can be *false* under
stake-weighting, because one validator holding a stake majority behaves nothing
like one validator holding one vote. Nothing in this spec should be read as
transferring to that one, or vice versa.

### `PowerConservation` was removed, because it proved a path that does not exist

An earlier revision maintained `trackedTotal` incrementally on add/remove and
asserted it against a recomputed sum, described as mirroring production. It did
not: production sums once in `new()` and never updates. The invariant checked
for drift in a running total that no Rust code maintains. It is gone, and totals
are recomputed from the active set.

What replaced it is the *real* staleness hazard: `gadgetTotal` is frozen at
construction, exactly as `total_weight` is, and
`FinalizedOnlyByRealSupermajority` asks whether a finalisation taken against
that frozen total was sound against the stake that actually existed.


## The finding: votes are not scoped to a committee

`ValidatorSet_UnscopedVotes.cfg` sets `EnforceVoteEpoch = FALSE`, modelling
`FinalityVote` as it exists today. **TLC reports a violation, and that is
correct:**

```
Error: Invariant NoStaleVoteCounted is violated.
```

The counterexample is five states long: `v3` is admitted, casts a vote in
epoch 1, the epoch advances, and the epoch-1 vote is delivered and counted
toward epoch 2's quorum.

`FinalityVote` carries `block_hash`, `block_height`, `validator_id` and a
signature over exactly those three. There is no epoch, no validator-set
identity, and `content_hash` binds neither. A vote cast under one committee is
byte-identical to the same vote replayed under the next.

Filed as **#2256**. The fix is in the Rust — bind an epoch or a hash of the
ordered validator set into the vote and its signed preimage — not in this
model. Note it would be a v2 preimage change (`content_hash` carries the
domain separator `wrfcoin-finality-vote-v1|`) and therefore consensus-visible.

Do not "fix" `ValidatorSet_UnscopedVotes.cfg` by weakening
`NoStaleVoteCounted`. It should keep failing until #2256 is resolved, at which
point the correct change is to delete the config, because
`EnforceVoteEpoch = FALSE` will no longer describe the implementation.

## What the model deliberately abstracts away

| Abstracted | Why |
|---|---|
| Block identity | Votes carry `(voter, epoch)` only. Conflicting finalization is `WrfConsensus.tla`'s job; this spec is about committee membership and threshold arithmetic. |
| Signature verification | Votes are authentic by construction. The finding above is about what the signature *covers*, not whether it verifies. |
| `min_stake` filtering | `validators_from_state` admits accounts with `stake_locked >= min_stake`. Modelled as "power is assigned on admission", since the filter only determines membership, which the model explores exhaustively. |
| Stake saturation to `u64` | `MaxPower` is small. Saturation matters for extreme stakes, not for rounding at small committee sizes. |
| Epoch-length / timing rules | `AdvanceEpoch` is untimed and nondeterministic. Safety must not depend on epoch duration. |
| Byzantine behaviour | `WrfConsensus.tla` models equivocation. Here every actor follows the protocol — the point is that correct actors alone can still break the property when votes are unscoped. |
