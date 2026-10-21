# WRFCoin Consensus Safety Analysis

> **Issue:** #720 — [Launch Blocker] Formal consensus safety analysis
>
> **Author:** Alice (automated analysis)
>
> **Date:** 2026-04-10
>
> **Scope:** BFT guarantees, partition recovery, long-range attacks,
> nothing-at-stake, liveness vs safety tradeoffs.

> **2026-05-30 update:** For the current testnet launch-readiness record, see
> [`docs/consensus/testnet-consensus-safety-analysis.md`](../consensus/testnet-consensus-safety-analysis.md).
> That document updates this internal analysis against current `origin/main`
> and preserves the distinction between controlled-testnet readiness and the
> still-open external `human-required` review gate.

---

## 1. Consensus Architecture Overview

WRFCoin uses a **multi-algorithm consensus layer** with a **BFT finality
gadget** overlay. The base layer produces blocks via one of several
pluggable algorithms; the finality gadget provides deterministic finality
via 2/3+ supermajority attestation.

### Available Algorithms

| Algorithm | File | Block Production | Finality |
|-----------|------|-----------------|----------|
| PoW | `consensus-engines/src/algorithms/pow.rs` | Double-SHA256 mining | Probabilistic (depth) |
| PoS | `consensus-engines/src/algorithms/pos.rs` | Stake-weighted selection | Probabilistic + BFT overlay |
| DPoS | `consensus-engines/src/algorithms/dpos.rs` | 21 delegates, round-robin | Probabilistic + BFT overlay |
| PoP | `consensus-engines/src/algorithms/pop_adapter.rs` | Weather data quality scoring | Probabilistic + BFT overlay |
| HotStuff | `consensus-engines/src/algorithms/hotstuff/mod.rs` | O(n) BFT, 3-phase commit | Deterministic (built-in) |
| DAG | `consensus-engines/src/algorithms/dag.rs` | GHOSTDAG parallel blocks | Probabilistic (k-parameter) |
| Raft | `consensus-engines/src/algorithms/raft.rs` | Leader-elected log replication | CFT (crash, not Byzantine) |

**Active testnet configuration:** PoP (Proof of Publishing) with BFT finality
gadget overlay.

---

## 2. BFT Fault Tolerance

### Threshold

The BFT finality gadget (`consensus-engines/src/selection/bft_finality.rs`)
uses the standard **2/3+ supermajority** rule:

```
finalized iff accumulated_weight * 3 > total_weight * 2
```

This provides **Byzantine fault tolerance for up to f < n/3 validators**
(by stake weight), where f is the number of Byzantine validators and n is
the total validator set.

### Voting Protocol

1. Validator submits `FinalityVote(block_hash, block_height, validator_id, signature)`
2. Gadget verifies: validator is registered, block not already finalized,
   no equivocation at this height
3. Accumulates stake weight for (height, hash) pair
4. At 2/3+ threshold: emits `FinalityEvent::BlockFinalized`, updates
   `finalized_height` (monotonically increasing)

### Equivocation Detection

Double voting (same height, different hash) is detected and rejected
(`bft_finality.rs:294-310`). The vote is refused but the validator is
**not currently slashed** by the finality gadget itself.

### Safety Guarantee

> **Theorem (informal):** If fewer than 1/3 of validators (by stake) are
> Byzantine, no two conflicting blocks can both receive BFT finality.
>
> **Proof sketch:** Finalizing two conflicting blocks at the same height
> requires 2/3+ votes for each. Since honest validators never equivocate,
> the two voting sets must each contain >2/3 of total stake. Their
> intersection exceeds 1/3 of total stake, which exceeds the assumed
> Byzantine fraction — contradiction.

---

## 3. State Root Determinism

**File:** `shared-protocol/src/blockchain/state.rs:2770+`

The state root is computed as SHA3-256 over deterministically-ordered state:

1. All collections are **sorted by key** before hashing (accounts by
   address, NFTs by ID, commits by commit ID, etc.)
2. Floating-point values use `canonical_f64_bits()` for deterministic
   IEEE-754 representation
3. State root is versioned (`wrfcoin_state_root_v8`) for upgrade safety
4. Integer arithmetic is used throughout reward calculations (#1190 converted
   f64 math to deterministic integer arithmetic)

**Verdict:** State roots are deterministic across all validators. No
floating-point non-determinism vectors remain.

---

## 4. Fork Choice Rule

**File:** `shared-protocol/src/blockchain/chain.rs:309-350`

### Selection Criteria

1. **Longest chain wins** (candidate must be longer than local chain)
2. **Tie-break:** Equal height resolved by lower tip hash (lexicographic)
3. **Reorg depth limit:** Maximum 100 blocks (`MAX_REORG_DEPTH`)

### Limitations

- **No economic weighting:** Equal-height forks are resolved by hash
  comparison, not by accumulated stake or work
- **BFT finality guard boundary:** The fork choice rule now refuses
  reorgs whose fork point is below `finalized_height`. A fork exactly at
  `finalized_height` is allowed because it preserves the finalized block
  and replaces only unfinalized descendants.

### GAP-1: Fork Choice Should Respect BFT Finality

> **Recommendation:** Add a guard in `plan_reorg()` that rejects any reorg
> whose fork point is at or below `finalized_height`. This closes the
> theoretical gap between probabilistic fork choice and deterministic BFT
> finality.

#### Resolution — [wrfcoin/core4#1238](https://github.com/wrfcoin/core4/pull/1238) (issue #1216)

The rejection guard at `shared-protocol/src/blockchain/chain.rs:377-391`
(refusing any reorg whose fork height is below `self.finalized_height`)
existed from earlier work but was **inert** because
`Blockchain::set_finalized_height` (`chain.rs:95-105`) was never called in
production — `finalized_height` stayed at 0 forever and every reorg within
`MAX_REORG_DEPTH` was allowed. PR #1238 wires the BFT finality sources to
the consumer, arming the guard end-to-end:

1. **Signal type** — `shared-protocol/src/consensus/finality.rs` defines
   `FinalitySignal { height, block_hash, source }` and a bounded mpsc
   channel (`FINALITY_SIGNAL_CHANNEL_CAPACITY = 256`). Placed in
   `shared-protocol` because `consensus-engines` depends on it; a type in
   the upstream crate cannot appear in shared-protocol signatures.
2. **BFT gadget emit** — `BftFinalityGadget::submit_vote` calls
   `emit_finality_signal` the moment a block crosses the 2/3+ threshold
   (`consensus-engines/src/selection/bft_finality.rs:421-426`).
3. **HotStuff emit** — the HotStuff commit-phase handler
   (`consensus-engines/src/algorithms/hotstuff/mod.rs:470-474`) emits a
   `FinalitySignal` tagged `"hotstuff"` as soon as a block reaches the
   Commit phase with a commitQC (BFT-final by the three-chain rule).
4. **Consumer** —
   `native-chain/src/bin/launch_api/finality_events.rs::run_finality_event_loop`
   drains the channel and forwards each signal into
   `Blockchain::set_finalized_height` under a write lock. The consumer is
   not gated on `#[cfg(feature = "p2p")]` so single-node dev deployments
   also arm the guard.
5. **Startup wiring** — `launch_api/main.rs` creates the channel, passes
   the sender to the BFT sources via `AppState::finality_signal_tx`, and
   spawns the consumer task alongside the HTTP server.
6. **Runtime monitoring fix** — `routes/chain.rs::build_runtime_monitoring_snapshot`
   previously reported `chain_finalized_height` as
   `tip - MAX_REORG_DEPTH` (a probabilistic floor); it now reports the
   max of the probabilistic floor and the real `Blockchain::finalized_height()`
   when BFT finality has advanced (`routes/chain.rs:540-579`).

**Stale-finality safety:** finality signals may arrive out of order (a
slow p2p path can deliver height `N-2` after `N`). `set_finalized_height`
is monotonic — a lower height is silently dropped — so the consumer
needs no reordering buffer.

**End-to-end coverage:**
`consumer_armed_guard_rejects_deep_reorg`
(`finality_events.rs:178-234`) builds a 30-block chain, emits a
`FinalitySignal { height: 20 }`, then constructs a candidate fork that
branches at height 10 and extends past the local tip. Before #1238 this
reorg would be accepted (depth 20 ≤ MAX_REORG_DEPTH=100); with the
guard armed, `plan_reorg` refuses it. Supporting tests:
`shared-protocol::consensus::finality` (5), `consensus-engines::selection::bft_finality`
(3 new `finality_sink_*` tests), `consensus-engines::algorithms::hotstuff`
(5 new `emit_finality_signal_*` tests),
`native-chain::launch_api::finality_events` (4 tests incl. stale /
shutdown / armed-guard).

**Invariant now enforced:** once a block reaches BFT finality (2/3+
attestation via the gadget, or a commitQC via HotStuff), no accepted reorg
can remove a block at or below that finalized height. The implementation
rejects forks whose fork point is below `finalized_height`; a fork exactly
at `finalized_height` keeps the finalized block and replaces only
unfinalized descendants.

---

## 5. Long-Range Attack Protection

### Checkpoint System

**File:** `consensus-engines/src/selection/finality.rs`

- Checkpoints at configurable intervals (default: 100 blocks)
- Types: Periodic, Triggered (high-value tx), Emergency (anomaly)
- Require 2/3+ validator signatures by stake
- Blocks at or below checkpoint height have **absolute finality** (P=1.0)

### Probabilistic Finality (Between Checkpoints)

Exponential model: `P(final) = 1 - q^depth` where
`q = adversary_fraction / (1 - adversary_fraction)`

With default `adversary_threshold = 0.33`:
- Depth 6: ~98.5% finality
- Depth 30: ~99.99% finality (DEFAULT_DEEP_CONFIRMATION_DEPTH)

### Economic Finality

Cost to revert = `slashing_fraction * total_staked / min_colluding_validators`

Example with 1M staked, 10% slash, 4 colluders (BFT 1/3+1 of 10):
**100,000 tokens** to revert one block.

### Long-Range Attack Verdict

Post-checkpoint blocks are immune. Pre-checkpoint blocks are protected
by the 100-block reorg limit and probabilistic finality. An attacker with
old validator keys cannot rewrite history beyond the most recent checkpoint.

### GAP-2: Weak Subjectivity Period Not Defined

> **Recommendation:** Define and document a **weak subjectivity period**
> (maximum age at which a node trusts its local chain without external
> checkpoint verification). Nodes syncing from scratch should verify against
> a recent trusted checkpoint to prevent long-range attacks during initial
> sync.

---

## 6. Nothing-at-Stake (PoS Variants)

### Current Protections

1. **Double-signing detection** (`pos.rs:127-148`): Tracks
   `(validator_address, block_height) -> hash`; rejects conflicting blocks
   at same height
2. **Slashing:** Validators caught double-signing face stake penalties
   (5% major, 10% critical)
3. **DPoS unbonding period:** 21-day default prevents immediate withdrawal
   after misbehavior

### GAP-3: In-Memory Slashing Evidence

> Double-signing records are stored in `Arc<RwLock<FxHashMap>>` — **lost on
> node restart**. Evidence should be persisted to disk for durability and
> external audit.

#### Resolution — [wrfcoin/core4#1237](https://github.com/wrfcoin/core4/pull/1237) (issue #1218)

Before #1237, `consensus-engines::equivocation` defined the
`EquivocationEvidence` struct and the `EvidenceStore` trait, and
`ProofOfStake::record_double_sign` already called
`self.evidence_store.persist(evidence)` on equivocation detection
(`consensus-engines/src/algorithms/pos.rs:187-194`) — but the default
implementation was `InMemoryEvidenceStore`, which is lost on restart.
A malicious validator whose equivocation was detected could still walk
away unslashed if the detecting node rebooted before the evidence was
exported or the slashing transaction was broadcast.

PR #1237 adds a durable `redb`-backed implementation and wires it at
node startup:

1. **`DurableEvidenceStore`** — new module at
   `native-chain/src/consensus/evidence_store.rs` (~620 lines incl.
   tests). Backed by [redb](https://crates.io/crates/redb) 2.4 with
   `bincode` serialization. Stores each piece of evidence under its
   canonical `storage_key()` (format `"{validator}:{height:020}"`) so
   lexicographic range scans over a validator's evidence return results
   in numeric height order. Persistence is idempotent — persisting the
   same evidence twice overwrites the row with identical bytes.
2. **PoS engine hook** — `ProofOfStake` now holds
   `evidence_store: Arc<dyn EvidenceStore>`
   (`consensus-engines/src/algorithms/pos.rs:30-35`) with a builder
   (`with_evidence_store`, L65-68) and an accessor (`evidence_store()`,
   L72-74). Default is still `InMemoryEvidenceStore` to preserve
   pre-#1218 behavior for callers that don't opt in.
3. **Factory helper** —
   `ConsensusFactory::create_pos_with_evidence_store` gives launch-side
   code a single call-site for attaching the durable store.
4. **Startup wiring (fail-closed)** — `launch_api/main.rs:384-402`
   opens the durable store at `<data_dir>/equivocation_evidence.redb`
   on startup. **On open error the process aborts** rather than
   silently falling back to in-memory — durable slashing evidence is
   treated as a launch-readiness requirement. The `Arc<dyn EvidenceStore>`
   is held on `AppState::evidence_store` so multiple engines or audit
   tooling can share the same backend.
5. **Database file isolation** — the evidence DB is separate from the
   main blockchain DB so operator tooling can back it up independently
   or open it read-only while the node is live.

**End-to-end coverage:**
- `durable_store_survives_restart`
  (`evidence_store.rs:329`) — opens the store, persists 3 evidence
  records, drops the handle, reopens the DB, and asserts all 3 records
  are still loadable.
- `pos_engine_double_sign_evidence_survives_restart`
  (`evidence_store.rs:458`) — end-to-end: constructs a `ProofOfStake`
  engine with a `DurableEvidenceStore`, triggers a double-sign
  detection, drops the engine and the store, reopens the DB, and
  asserts the evidence is retrievable via the fresh handle.
- 8 total tests in the new module cover open/create, persist/load,
  idempotency, range scans, and the restart path.

**Invariant now enforced:** every double-sign detected by
`ProofOfStake::record_double_sign` produces an `EquivocationEvidence`
row that is `fsync`ed to the redb file before the `MaliciousValidator`
error returns to the caller. A node restart between detection and
slashing tx broadcast no longer loses the proof.

### GAP-4: No Cross-Fork Voting Penalty

> Validators can vote on multiple fork candidates pre-checkpoint without
> penalty. The equivocation detection only catches same-height double-signing
> within a single node's view. Multi-fork voting across partitions is not
> detected until partitions reconnect.

---

## 7. Network Partition Recovery

### Detection

**File:** `consensus-engines/src/coordination/adaptive_fault_tolerance.rs`

Partition detection threshold: 30 missed heartbeats. Partitions tracked
as `NetworkPartition { nodes, is_majority, leader }`.

### Recovery Topologies

- **Hierarchical:** Central leader with backups; fast recovery, single
  point of failure
- **Mesh:** Peer-to-peer; resilient but O(n^2) communication
- **Ring:** Sequential; ordered processing, single-link fragile
- **Hybrid:** Primary + secondary with transition conditions

### Graceful Degradation Levels

| Level | Availability | Behavior |
|-------|-------------|----------|
| Full | ~100% | Normal consensus |
| Minor | ~85% | Normal, slower finality |
| Moderate | ~70% | Reduced throughput |
| Severe | ~50% | Limited finality guarantees |
| Emergency | <50% | Recovery mode |

### GAP-5: Minority Partition Behavior Undefined

> The code defines detection and topology structures but does **not
> specify whether a minority partition should halt or continue with
> reduced guarantees**. In a standard BFT system, a minority partition
> (<1/3 stake) cannot produce finalized blocks and should halt block
> production to prevent forks.
>
> **Recommendation:** Minority partitions should enter a read-only mode
> (accept queries, reject new blocks) until they rejoin the majority.
> The majority partition (>2/3 stake) continues normally. Partitions
> with 1/3-2/3 stake should continue block production but cannot
> finalize (BFT gadget stalls).

#### Resolution — [wrfcoin/core4#1239](https://github.com/wrfcoin/core4/pull/1239) (issue #1217), on top of commit `d295f66d33`

The partition-mode policy was landed in two commits. The baseline
wiring in commit `d295f66d33` added the policy type, the mempool
ingress guard, the shared-state field, and the HTTP status endpoints;
PR #1239 closes the remaining block-production half.

**Policy** —
`shared-protocol/src/consensus/partition_policy.rs` defines three
modes corresponding to the three stake-fraction regimes identified in
the recommendation above:

| Local stake fraction | `PartitionQuorumStatus` | `PartitionMode` | Behavior |
|---|---|---|---|
| ≥ 2/3 | `Supermajority` | `Normal` | Produce blocks, cast finality votes, accept mempool writes |
| > 1/3, < 2/3 | `Middle` | `BlockProductionOnly` | Produce blocks, no finality votes, accept mempool writes |
| ≤ 1/3 | `Minority` | `ReadOnly` | Refuse new blocks, refuse mempool writes, serve queries |

The `partition_mode(local_stake, total_stake) -> PartitionMode`
function and the `allows_block_production()` /
`allows_finality_voting()` / `accepts_mempool_writes()` /
`serves_queries()` accessors on `PartitionMode` give every subsystem a
single decision source. The module lives in `shared-protocol` rather
than `consensus-engines` so the mempool can import it without a
circular dependency.

**Guard chain (defense-in-depth at every block-production entrypoint):**

1. **Mempool ingress** (`shared-protocol/src/mempool/pool.rs`,
   merged in `d295f66d33`) — refuses new transactions when the
   mempool's cached `PartitionMode` is `ReadOnly`. This stops new
   user-visible state from accumulating, but does **not** stop block
   production from already-pooled transactions.
2. **`PopConsensusAdapter::create_next_block` — primary guard**
   (`consensus-engines/src/algorithms/pop_adapter.rs:123-146`). Reads
   the mempool's current mode under a short read lock before any work,
   and returns the new typed
   `ConsensusError::PartitionReadOnly(label)` variant
   (`consensus-engines/src/types.rs`) if `allows_block_production()`
   is false. All `create_pop_block_with_state` calls — the only
   production chokepoint for PoP block creation — flow through here.
3. **`ProofOfPublishing::create_next_block` — defense-in-depth guard**
   (`shared-protocol/src/consensus/proof_of_publishing.rs:775-789`).
   Catches direct callers that bypass the adapter (integration tests,
   future gadgets). Returns `anyhow::Error` tagged with the sentinel
   string `SP_CONS_PARTITION_READ_ONLY`. The adapter's error-translation
   branch (L151-164) maps the sentinel back to
   `ConsensusError::PartitionReadOnly` so HTTP handlers match on a
   single variant.
4. **HTTP `/blocks/mine` — outer 503 envelope**
   (`native-chain/src/bin/launch_api/routes/mining.rs:57-77`) returns
   `SERVICE_UNAVAILABLE` with error code `PARTITION_READ_ONLY` before
   taking the block-production lock, avoiding wasted contention on a
   doomed request. The error-mapping branch (L98-121) also translates
   adapter `PartitionReadOnly` / shared-protocol `SP_CONS_PARTITION_READ_ONLY`
   errors to the same 503 envelope, so a race (mempool flips to
   `ReadOnly` between the outer check and the inner consensus call)
   is still refused cleanly with a single error code.

**Deliberate non-guard on `Blockchain::add_block`:** the p2p sync path
in `p2p/src/sync.rs` calls `Blockchain::add_block` to **apply** blocks
received from peers. A node in a minority partition still wants —
and in fact *especially* needs — to receive blocks from the majority
so it can detect when it is safe to re-join and catch up. Guarding
`add_block` would break catch-up and make the partition permanent.
The guards therefore sit on the **production** side only:
`create_next_block` is the single entry point that builds a new
block, and the two inner guards plus the HTTP outer guard cover every
caller currently in the codebase. This is a deliberate safety
distinction documented in PR #1239.

**`BlockProductionOnly` is preserved:** the middle-partition case
(1/3 < local stake < 2/3) **must** keep producing blocks for
availability even though BFT finality stalls. Every unit and
integration test covers all three modes to pin this — a regression
that conflated `BlockProductionOnly` with `ReadOnly` would defeat the
point of the three-mode distinction.

**End-to-end coverage:**
- `native-chain/tests/partition_mode_block_production.rs` — 5 HTTP
  integration tests driving `/blocks/mine` across all three modes:
  `ReadOnly` refused with 503 + `PARTITION_READ_ONLY`,
  `BlockProductionOnly` allowed, `Normal` allowed,
  Normal→ReadOnly→Normal transition, mempool-only drift.
- `consensus-engines::algorithms::pop_adapter::tests` — 4 adapter-level
  unit tests covering each mode plus the sentinel-translation path.
- `shared-protocol::consensus::proof_of_publishing::tests` — 3
  shared-protocol-level tests covering each mode on the direct-call
  path.
- `shared-protocol::consensus::partition_policy::tests` — 19 tests
  pinning the classification boundaries (exact 1/3 is minority, exact
  2/3 is supermajority, u128 overflow safety, etc.).

**Invariant now enforced:** a node whose local stake is ≤ 1/3 of
total stake will refuse every block-production request from every
entrypoint with a 503 / `PARTITION_READ_ONLY` response, preventing
the minority-partition fork-then-revert UX hazard. A node in the
middle-partition case (1/3 < local < 2/3) continues producing blocks
but its votes are silently refused by the BFT gadget (no 2/3+ quorum
available by definition). Blocks from either mode still arrive via
p2p sync so the partitioned node can re-join the majority.

---

## 8. Liveness vs Safety Tradeoffs

### Safety-First Design

WRFCoin's current design **favors safety over liveness**:

- BFT finality requires 2/3+ — if >1/3 validators are offline, finality
  stalls (but block production may continue)
- 100-block reorg limit prevents deep reorganizations even at the cost
  of rejecting legitimate longer chains
- Slashing circuit breaker caps penalties at 100/block, potentially
  allowing misbehavior during correlated events

### Liveness Guarantees

- Block production continues as long as the base consensus algorithm
  can select a producer (varies by algorithm)
- HotStuff has built-in view-change for leader failure
- Raft handles leader failure with re-election (150-300ms timeout)
- PoP/PoW have no single leader dependency

### GAP-6: Slashing Circuit Breaker Drops Events

> When >100 slashing events occur in a single block (e.g., correlated
> sensor failure during a hurricane), excess events are **silently
> dropped**. This weakens incentive alignment during precisely the
> scenarios where it matters most.
>
> **Recommendation:** Buffer dropped events and process them in
> subsequent blocks, or log them for manual review.

---

## 9. Summary of Safety Guarantees

| Property | Status | Confidence |
|----------|--------|------------|
| BFT finality (f < n/3) | Implemented, guard armed end-to-end ([#1238](https://github.com/wrfcoin/core4/pull/1238)) | High |
| Finalized blocks never reverted | Implemented ([#1238](https://github.com/wrfcoin/core4/pull/1238)) | High |
| Deterministic state roots | Verified | High |
| Reorg depth bounded (100 blocks) | Implemented | High |
| Checkpoint finality (absolute) | Implemented | High |
| Probabilistic finality (exponential) | Implemented | High |
| Double-signing detection | Implemented | High |
| Double-signing evidence durability | Implemented (redb), fail-closed startup ([#1237](https://github.com/wrfcoin/core4/pull/1237)) | High |
| Long-range attack prevention | Partial (checkpoint-based) | Medium |
| Partition recovery | Structural + production-side guards ([#1239](https://github.com/wrfcoin/core4/pull/1239) + `d295f66d33`) | High |
| Minority partition halts block production | Implemented ([#1239](https://github.com/wrfcoin/core4/pull/1239)) | High |
| Nothing-at-stake prevention | Detection durable; cross-fork voting still open (GAP-4) | Medium |
| Liveness under partition | `BlockProductionOnly` preserved for middle partition | High |

---

## 10. Recommendations (Priority Order)

| # | Gap | Severity | Status | Recommendation |
|---|-----|----------|--------|---------------|
| 1 | Fork choice ignores BFT finality | **High** | **Resolved** — [#1238](https://github.com/wrfcoin/core4/pull/1238) (issue #1216) | Guard `plan_reorg()` against reverting finalized blocks |
| 2 | Minority partition behavior undefined | **High** | **Resolved** — [#1239](https://github.com/wrfcoin/core4/pull/1239) (issue #1217) + `d295f66d33` | Implement read-only mode for <1/3 partitions |
| 3 | Slashing evidence not persisted | **Medium** | **Resolved** — [#1237](https://github.com/wrfcoin/core4/pull/1237) (issue #1218) | Write double-signing proofs to redb storage |
| 4 | Slashing circuit breaker drops events | **Medium** | Open | Buffer and defer excess events to next block |
| 5 | Weak subjectivity period undefined | **Medium** | Open | Define max sync age; require checkpoint for initial sync |
| 6 | Cross-fork voting not penalized | **Low** | Open | Extend equivocation detection to cross-partition views |

---

## Appendix: Files Analyzed

| Component | File | Lines |
|-----------|------|-------|
| PoW | `consensus-engines/src/algorithms/pow.rs` | 24-116 |
| PoS | `consensus-engines/src/algorithms/pos.rs` | 30-204 |
| DPoS | `consensus-engines/src/algorithms/dpos.rs` | 31-150 |
| PoP adapter | `consensus-engines/src/algorithms/pop_adapter.rs` | 40-166 |
| PoP block production (guard) | `shared-protocol/src/consensus/proof_of_publishing.rs` | 755-789 |
| HotStuff | `consensus-engines/src/algorithms/hotstuff/mod.rs` | 1-490 |
| DAG | `consensus-engines/src/algorithms/dag.rs` | 1-150 |
| Raft | `consensus-engines/src/algorithms/raft.rs` | 1-150 |
| BFT Finality | `consensus-engines/src/selection/bft_finality.rs` | 1-439 |
| Finality/Checkpoints | `consensus-engines/src/selection/finality.rs` | 1-1069 |
| Finality signal plumbing | `shared-protocol/src/consensus/finality.rs` | 1-215 |
| Finality signal consumer | `native-chain/src/bin/launch_api/finality_events.rs` | 1-250 |
| Fork Choice + finalized_height | `shared-protocol/src/blockchain/chain.rs` | 40-105, 342-406 |
| Partition Policy | `shared-protocol/src/consensus/partition_policy.rs` | 1-403 |
| HTTP block production guard | `native-chain/src/bin/launch_api/routes/mining.rs` | 57-121 |
| State Root | `shared-protocol/src/blockchain/state.rs` | 2770-3000+ |
| Slashing | `shared-protocol/src/security/slashing.rs` | 1-300 |
| Equivocation evidence (trait) | `consensus-engines/src/equivocation.rs` | — |
| Durable evidence store | `native-chain/src/consensus/evidence_store.rs` | 1-620 |
| Partition Recovery | `consensus-engines/src/coordination/adaptive_fault_tolerance.rs` | 1-150 |
| Validator Mgmt | `consensus-engines/src/validator.rs` | 1-252 |
