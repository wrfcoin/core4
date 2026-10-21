# Formal models

TLA+ specifications of WRFCoin protocol properties, with TLC model-check
configurations. Epic #2093.

These are **design-level** checks. They find failures in the protocol as
specified, before those failures appear in Rust and long before they appear on
a public testnet. They do not verify the Rust implementation — for that, see
the runtime invariants in `shared-protocol/src/invariants/`.

## What is proved, about which system, under what assumptions

Read this before citing any result here. Stated explicitly because an earlier
revision of this file did not, and the omission would have let a reader take
these runs as verification of the finality protocol the testnet runs (#2255).

**What is proved.** That the *designed* BFT finality protocol — proposal,
voting, per-node vote accumulation, and a `> 2/3` stake threshold — admits no
run in which two nodes finalize conflicting blocks at one height, at the
bounded configurations listed under Results. Plus the reason: any two quorums
meeting that threshold intersect in an honest validator.

**About which system.** The *design*, not the deployed one. Two gaps, both
deliberate and both load-bearing:

1. **The gadget is not attached on the production launch path.**
   `native-chain/src/bin/launch_api/main.rs:1029` sets
   `bft_finality_for_p2p = None`, with the in-code comment that the local
   gadget is *"intentionally unattached here — the active consensus engine on
   this node is PoP, not PoS, so there is no per-block self-vote source yet."*
   Inbound peer votes are dropped; only inbound *certificates* advance local
   finality. So this models a protocol the testnet does not currently run.
   Related: #2256 records `FinalityVote` as FENCED for the same reason.
2. **The model is bounded.** 4 validators, ≤ 2 observing nodes, 2 candidate
   blocks, `MaxHeight ≤ 3`, and a two-level stake distribution. TLC proves the
   property over exactly those states, not for all validator-set sizes or
   arbitrary stake distributions.

**Under what assumptions.** Byzantine validators may equivocate on proposals
and double-vote; honest validators vote once per height; the network may
reorder and drop; and Byzantine *stake* is bounded below 1/3 of the total
(`ByzantineWeightWithinBound`) — not Byzantine *count*, which is the
distinction the pre-#2255 uniform model could not make.

**Therefore:** these runs are evidence about the finality design. They are not
evidence that the built system is correct, and must not be cited as such while
`bft_finality_for_p2p` remains `None`.

| Spec | Story | What it checks |
|---|---|---|
| `tla/WrfConsensus.tla` | #2098 | Block proposal, voting, finalization. Safety under Byzantine equivocation, at uniform **and** non-uniform stake. |
| `tla/ValidatorSet.tla` | #2099 | Validator-set changes across epoch boundaries, quorum arithmetic at small set sizes. |

Each spec carries its own notes; `WrfConsensus` is documented below.

## Running TLC

TLC is not vendored. Fetch the tools jar once:

```bash
curl -sSL -o /tmp/tla2tools.jar \
  https://github.com/tlaplus/tlaplus/releases/latest/download/tla2tools.jar
```

Then, from `formal/tla/`:

```bash
# Safety — the canonical configuration. ~50s.
java -XX:+UseParallelGC -cp /tmp/tla2tools.jar tlc2.TLC \
  -workers 4 -config WrfConsensus.cfg WrfConsensus.tla

# Negative control — MUST FAIL. ~8s.
java -XX:+UseParallelGC -cp /tmp/tla2tools.jar tlc2.TLC \
  -workers 4 -config WrfConsensus_NegativeControl.cfg WrfConsensus.tla

# Liveness — currently reports a counterexample, by design. ~6s.
java -XX:+UseParallelGC -cp /tmp/tla2tools.jar tlc2.TLC \
  -workers 4 -config WrfConsensus_Liveness.cfg WrfConsensus.tla
```

Requires Java 11+. Verified with TLC 2.19 on Java 21.

---

## WrfConsensus

Derived from, at commit `b0bd2060c`:

- `consensus-engines/src/selection/bft_finality.rs` — `is_supermajority`
  (line 511), `required_weight` (line 516), `submit_vote` (line 537)
- `consensus-engines/src/engine.rs`, `validator.rs`, `algorithms/`
- `shared-protocol/src/consensus/finality.rs` — `FinalitySignal` delivery

### What it is really testing

In the design, each node runs its **own** `BftFinalityGadget` and accumulates
the votes that reach it over p2p, so different nodes see different vote
subsets. (In the *deployed* launch path no gadget is attached at all — see
"What is proved" above. The paragraph below describes the protocol being
modelled, not the running testnet.) A Byzantine
validator can exploit that: send `vote(A)` to one node and `vote(B)` to
another. Each node's local equivocation check — the `validator_height_votes`
map, step 3 of `submit_vote` — sees only one of the pair and is satisfied.

So safety cannot come from the local duplicate check. It has to come from
quorum intersection. `NoConflictingFinalization` is the assertion that the
attack fails at `n = 4, f = 1`, and `QuorumIntersection` is the assertion of
*why*.

### Results

Configuration: 4 validators, 1 Byzantine, 2 observing nodes, 2 candidate
blocks per height, `MaxHeight = 3`.

```
Model checking completed. No error has been found.
44,358,940 states generated, 4,260,803 distinct states, depth 41
Finished in 50s
```

Invariants checked: `TypeOK`, `NoConflictingFinalization`, `QuorumIntersection`,
`RequiredWeightMatchesSupermajority`, `ByzantineWeightWithinBound`,
`FinalizedBlocksAreKnownBlocks`.

### Stake weight, not validator count

The pre-#2255 spec fixed `Weight(v) == 1`, so every result above was validator
**count** arithmetic. The Rust is stake-weighted: `bft_finality` sums
`FinalityValidator.stake_weight` into `total_weight` and adds each voter's
stake to `accumulated_weight` before applying `accumulated * 3 > total * 2`. A
safety result under uniform weights does not transfer by itself — one validator
holding half the stake behaves nothing like one validator holding one vote.

Stake is now a configuration constant, supplied as a two-level distribution (a
set of heavy validators, and one stake value per level). That is not arbitrary
stake — TLC's `.cfg` parser cannot express a function literal, and the
validator model values are declared in the configuration rather than the spec,
so a per-validator function cannot be written in either file. Two levels is
enough to separate count from stake, which is what the uniform model could not
do at all.

`WrfConsensus_Weighted.cfg` — `v1 = 5` (honest), others `1`, total 8, Byzantine
weight 1:

```
Model checking completed. No error has been found.
56,570,100 states generated, 5,224,259 distinct states, depth 36
Finished in 37s
```

`WrfConsensus_WeightedNegativeControl.cfg` is the one that matters. Same four
validators, same *single* Byzantine validator — `n = 4, f = 1`, the canonical
safe shape by count — but the stake is skewed onto the Byzantine validator:
`v4 = 5`, others `1`, total 8, so Byzantine weight is 5 and `5 * 3 = 15` is not
`< 8`. TLC reports:

```
Error: The invariant of QuorumIntersection is equal to FALSE
```

**A configuration that is safe by count is unsafe by stake, demonstrated rather
than asserted.** v4 cannot finalize unaided, but it needs only one honest vote
to cross the threshold and can pair with a *different* honest validator on each
of two conflicting blocks; the two quorums then intersect only in v4. If this
configuration ever passes, the weighted machinery has stopped distinguishing
stake from count and every weighted result here is worthless.

### The negative control matters

`WrfConsensus_NegativeControl.cfg` sets `ByzantineValidators = {v3, v4}` — two
faults at `n = 4`, beyond what a 2/3+ threshold tolerates. TLC reports:

```
Error: Invariant NoConflictingFinalization is violated.
```

**This failure is the point.** An invariant that passes because the model
cannot express the attack is worse than no invariant, and that failure mode is
invisible if you only ever run the passing configuration. Run the negative
control whenever `WrfConsensus.tla` is edited. If it ever passes, the model has
lost its teeth and the green safety run means nothing.

This is not hypothetical: the first draft of this spec produced a *false*
`NoConflictingFinalization` counterexample because `DropMessage` removed votes
from the same set the "an honest validator votes once" guard read, so losing a
message let an honest validator equivocate. The model was wrong, not the
protocol. Hence the split between `cast` (monotone, what the vote guards read)
and `inFlight` (droppable network soup).

### Liveness: currently violated, and that is informative

`EventuallyFinalizes` does **not** hold. Under `LiveSpec` — which already
assumes partial synchrony (eventually-reliable delivery, and a view timer long
enough not to pre-empt a reachable quorum) — TLC produces this counterexample:

1. An honest proposer proposes `ba`.
2. The Byzantine validator equivocates a second proposal `bb`.
3. Honest validators split: `v1 → bb`, `v3 → bb`, `v2 → ba`.
4. The Byzantine validator's votes land such that each block holds 2 of the
   required 3.
5. Nothing further is enabled. The height never decides.

**This is a limitation of the model's scope, not evidence of a defect in the
Rust.** The model has no proposer rotation, no view change and no locking
rule: honest validators may vote for any proposed block. Real BFT prevents
split votes with a single designated proposer per view plus a view change on
timeout — machinery that lives in `algorithms/hotstuff/` and
`selection/vrf.rs`, outside the finality gadget this spec models.

Extending the model with view change is the honest way to make a liveness
claim here, and it is deliberately out of scope for #2098. No issue is filed
against the protocol for this, because the counterexample does not demonstrate
a protocol defect — it demonstrates that this model cannot speak to liveness.
Safety, which is what the spec was built for, is unaffected: it holds across
all 4.26M reachable states.

### What the model deliberately abstracts away

| Abstracted | Why it is safe to abstract for *safety* |
|---|---|
| Cryptographic signatures | Modelled as unforgeable: a vote's `voter` field is authentic. Signature forgery is a different threat, covered by `THREAT_MODEL.md`. |
| Block contents and validity | Only block *identity* matters for conflicting finalization. Two distinct hashes at one height is the property, whatever the blocks contain. |
| Proposer selection / view change | Out of scope, and the reason liveness cannot be established here. Does not affect safety: safety must hold regardless of who proposes. |
| Non-uniform stake weight | Held uniform at 1, so quorum arithmetic is validator-count arithmetic. Non-uniform power is `ValidatorSet.tla`'s job (#2099). |
| Real time and timeouts | `Timeout` is an untimed nondeterministic action. Safety must not depend on timing; liveness does, which is why `LiveSpec` constrains it. |
| Vote persistence across restarts | Per-height state is cleared when a height closes, mirroring `submit_vote` step 2 (`AlreadyFinalized`), which rejects votes at or below `finalized_height`. |
| More than 2 observing nodes | `NoConflictingFinalization` is about *two* nodes disagreeing. A third adds state without adding reachable disagreements. |

### Relationship to the Rust invariant

`NoConflictingFinalization` here is the protocol-level statement of
INV-FINALITY-1, implemented at the process level in
`shared-protocol/src/invariants/finality.rs` (#2096). The TLA+ model asks
whether the *protocol* can produce a conflicting finalization; the Rust
invariant detects whether *this node* ever observes one. Both are needed: the
model cannot see implementation bugs, and the runtime check cannot see design
bugs that no execution happened to trigger.
