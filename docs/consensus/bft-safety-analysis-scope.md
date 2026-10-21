# BFT Consensus Safety Analysis — External Review Scope

**Status:** Draft scope for human review
**Target:** Pre-mainnet external formal safety analysis of the consensus layer
**Refs:** #720

---

## 1. Analysis target and motivation

Launch blocker #720 requires an independent formal safety analysis of the
WRFCoin consensus layer (BFT guarantees, partition recovery, long-range
attack resistance, nothing-at-stake, liveness vs safety tradeoffs). An
internal analysis authored by Alice already exists at
[`docs/architecture/consensus-safety-analysis.md`](../architecture/consensus-safety-analysis.md)
(556 lines) and enumerates six gaps (GAP-1 .. GAP-6), four of which have
been closed by PRs #1238 (GAP-1, issue #1216), #1237 (GAP-3, issue #1218),
and #1239 (GAP-5, issue #1217). That internal analysis is a starting
point — it has not been reviewed by a distributed-systems expert, uses no
formal methods, and its "informal theorem" in §2 is a proof sketch, not a
machine-checked proof. This scope doc enumerates the exact code surface,
properties, attacker model, and deliverables that an external analyst
(firm or academic reviewer with distributed-systems / BFT expertise) needs
in order to quote and perform the formal review.

A later bounded internal testnet-readiness analysis exists at
[`docs/consensus/testnet-consensus-safety-analysis.md`](testnet-consensus-safety-analysis.md).
It is useful reviewer input, but it does not replace this external-review
scope or close the human-required gate.

---

## 2. In-scope consensus paths

All paths are relative to the `wrfcoin/core4` workspace root.

### `consensus-engines/` — `wrfcoin-consensus-engines` crate

Block-production algorithms and coordination layer.

| File | LOC | Purpose |
|------|-----|---------|
| `consensus-engines/src/algorithms/pow.rs` | 411 | Double-SHA256 proof-of-work |
| `consensus-engines/src/algorithms/pos.rs` | 522 | Stake-weighted PoS with double-sign detection + durable evidence store hook |
| `consensus-engines/src/algorithms/dpos.rs` | 761 | 21-delegate DPoS with 21-day default unbonding |
| `consensus-engines/src/algorithms/pop_adapter.rs` | 617 | Proof-of-Publishing adapter (wraps `shared-protocol` PoP into the `ConsensusEngine` trait); holds the primary partition-mode guard |
| `consensus-engines/src/algorithms/hotstuff/mod.rs` | 1678 | HotStuff three-phase BFT: prepare / pre-commit / commit with chained QCs, three-chain commit rule |
| `consensus-engines/src/algorithms/hotstuff/pipelined_voting.rs` | 817 | Pipelined voting across views |
| `consensus-engines/src/algorithms/hotstuff/types.rs` | 574 | `QuorumCertificate`, `ViewNumber`, `NewView` messages |
| `consensus-engines/src/algorithms/hotstuff/crypto.rs` | 442 | Vote / QC signature aggregation helpers |
| `consensus-engines/src/algorithms/dag.rs` | 1072 | GHOSTDAG-inspired DAG consensus, k-parameter blue-set scoring |
| `consensus-engines/src/algorithms/raft.rs` | 1408 | Raft (CFT, not Byzantine) — in scope for liveness under crash but out of scope for BFT guarantees |
| `consensus-engines/src/selection/bft_finality.rs` | 984 | BFT finality gadget: 2/3+ supermajority rule (`accumulated * 3 > total * 2`, see `is_supermajority` at L311), per-height vote accumulation, equivocation detection, `FinalitySignal` emission |
| `consensus-engines/src/selection/finality.rs` | 1068 | Checkpoint finality, probabilistic exponential-depth finality, economic finality |
| `consensus-engines/src/selection/vrf.rs` | 685 | VRF-based proposer selection |
| `consensus-engines/src/coordination/adaptive_fault_tolerance.rs` | 1312 | Partition detection (30-missed-heartbeat threshold), topology selection (Hierarchical / Mesh / Ring / Hybrid), graceful degradation levels |
| `consensus-engines/src/coordination/byzantine.rs` | 412 | BFT config types, malicious actor detection, `ByzantineFaultToleranceConfig { max_byzantine_nodes, total_nodes, consensus_timeout_ms }` |
| `consensus-engines/src/coordination/byzantine_coordinator.rs` | 1474 | Byzantine consensus coordinator state machine |
| `consensus-engines/src/coordination/hard_fork.rs` | 946 | Hard-fork coordination — in scope for safety at fork boundaries |
| `consensus-engines/src/security/attack_prevention.rs` | 788 | Attack detection / mitigation hooks |
| `consensus-engines/src/security/audit.rs` | 791 | Enumerates `VulnerabilityType::LongRangeAttack`, `NothingAtStake`, `GrindingAttack` (types only; detection logic partial) |
| `consensus-engines/src/security/correlation_penalties.rs` | 614 | Correlated-slashing penalty calculator |
| `consensus-engines/src/equivocation.rs` | ~180 | `EquivocationEvidence` type + `EvidenceStore` trait (`#720 GAP-3`, closes #1218) |
| `consensus-engines/src/validator.rs` | 252 | Validator set management |

### `shared-protocol/src/consensus/` — `wrfcoin-shared-protocol` consensus module

Lower-level consensus primitives shared by every algorithm and the mempool.

| File | LOC | Purpose |
|------|-----|---------|
| `shared-protocol/src/consensus/proof_of_publishing.rs` | 11258 | Full PoP implementation: block construction, reward calculation, state transitions, partition-mode defense-in-depth guard (L775-789) |
| `shared-protocol/src/consensus/partition_policy.rs` | 403 | Partition classification (`Supermajority` / `Middle` / `Minority`) and operating mode (`Normal` / `BlockProductionOnly` / `ReadOnly`), with cite `Refs #720 GAP-5, closes #1217` at L56 |
| `shared-protocol/src/consensus/finality.rs` | 215 | `FinalitySignal` type, bounded mpsc channel (`FINALITY_SIGNAL_CHANNEL_CAPACITY = 256`), stale-finality monotonic ordering |
| `shared-protocol/src/consensus/capability.rs` | 911 | Contributor capability scoring |
| `shared-protocol/src/consensus/task_orchestrator.rs` | 741 | Task orchestrator |
| `shared-protocol/src/consensus/zk_verification.rs` | 92 | ZK proof verification hooks — **out of scope; covered by #716** |
| `shared-protocol/src/consensus/hyper_local.rs` | 228 | Hyper-local reward multipliers |
| `shared-protocol/src/consensus/data_desert.rs` | 120 | Data-desert reward multipliers |

### Chain layer — `shared-protocol/src/blockchain/chain.rs`

The fork-choice + finalized-height guard surface that BFT finality feeds.

| Item | Location | Purpose |
|------|----------|---------|
| `MAX_REORG_DEPTH = 100` | `chain.rs:25` | Hard cap on reorg depth regardless of finality |
| `Blockchain::finalized_height` field | `chain.rs:57` | BFT-finalized height, starts at 0 |
| `finalized_height()` getter | `chain.rs:91-93` | Read current finalized height |
| `set_finalized_height()` setter | `chain.rs:101-105` | Monotonically clamped advance |
| `plan_reorg` guard | `chain.rs:377-391` (per #1238 analysis) | Refuses any reorg whose fork point is at or below `finalized_height` |

### Chain-layer BFT consumer — `native-chain/`

| File | Purpose |
|------|---------|
| `native-chain/src/bin/launch_api/finality_events.rs` | `run_finality_event_loop` — drains the `FinalitySignal` channel and forwards each signal into `Blockchain::set_finalized_height` under a write lock |
| `native-chain/src/bin/launch_api/routes/mining.rs` (L57-121) | HTTP `/blocks/mine` outer 503 envelope for `PARTITION_READ_ONLY` |
| `native-chain/src/bin/launch_api/routes/network.rs` | `/node/partition-status` read and `/operator/partition-mode` push |
| `native-chain/src/consensus/evidence_store.rs` (~620 LOC) | `DurableEvidenceStore` — redb-backed equivocation evidence persistence, fail-closed on open error (#1218) |
| `native-chain/tests/partition_mode_block_production.rs` | HTTP integration tests pinning block-production refusal in `ReadOnly` |
| `native-chain/tests/partition_mode_routes.rs` | HTTP contract tests for partition-mode endpoints |

---

## 3. Safety properties to verify

For each property the external analyst should produce a formal statement,
a proof (or counterexample), and a confidence rating tied to the specific
code locations below.

| Property | Code location | Current status |
|---|---|---|
| **Agreement** — no two conflicting blocks can both be BFT-final at the same height | `bft_finality.rs:308-322` (`is_supermajority`, `required_weight`), `bft_finality.rs::submit_vote` (equivocation check at L352-360) | Internal proof sketch in `consensus-safety-analysis.md` §2; no formal proof |
| **Validity** — a finalized block is a well-formed block in the chain | `Blockchain::add_block` (`chain.rs:111+`), `ConsensusEngine::validate_block` per-algorithm | Internal review only |
| **Integrity** — once finalized, a block is never reverted | `plan_reorg` guard + `set_finalized_height` monotonicity (`chain.rs:101-105`) | Armed end-to-end by #1238; internal tests `consumer_armed_guard_rejects_deep_reorg` |
| **Accountable safety** — any safety violation yields cryptographic proof of ≥ 1/3 Byzantine stake | `equivocation.rs::EquivocationEvidence`, `DurableEvidenceStore` (fail-closed at startup) | Evidence is durable (#1218); cross-fork voting **not** yet penalized — see GAP-4 in internal analysis |
| **State-root determinism** — honest validators that apply the same transitions from the same starting state compute identical state roots | `shared-protocol/src/blockchain/state.rs:2770+` (`wrfcoin_state_root_v6`, canonical f64 bits, sorted collection hashing) | Verified by `shared-protocol/tests/partition_convergence.rs` |

---

## 4. Liveness properties to verify

| Property | Code location | Current status |
|---|---|---|
| **Progress under partial synchrony** — after GST (Global Stabilization Time), finality advances in bounded rounds | HotStuff view-change path (`hotstuff/mod.rs`, `hotstuff/types.rs::NewView`), BFT gadget vote accumulation (`bft_finality.rs::submit_vote`) | No explicit GST bound stated in code; internal review only |
| **View-change timeout handling** — stalled views recover in bounded time | `hotstuff/mod.rs` view machinery, Raft election timeout 150-300 ms (`raft.rs`), `ByzantineFaultToleranceConfig::consensus_timeout_ms` | Timeouts present; no formal liveness argument |
| **Leader liveness** — honest validators eventually become leaders under fair scheduling | `vrf.rs` VRF-based proposer selection, DPoS round-robin (`dpos.rs`), HotStuff leader rotation | TBD — depends on randomness-bias analysis of VRF setup |
| **Finality under middle-partition** — `BlockProductionOnly` mode preserves availability while halting finality | `partition_policy.rs:92-103` (`PartitionMode`), `allows_block_production()` / `allows_finality_voting()` accessors | Implemented and tested; the deliberate trade — availability over finality in the middle regime — needs expert confirmation that it's the right call for this network model |

The external analyst should explicitly state the **GST assumption**, the
**adversary's network power** (can the adversary delay messages? how
long?), and the **synchrony model** (synchronous / partially synchronous
/ asynchronous) under which each liveness claim is expected to hold.

---

## 5. Partition recovery

Partition policy lives at
`shared-protocol/src/consensus/partition_policy.rs` (403 LOC, closes
#1217). Classification:

| Local stake fraction | `PartitionQuorumStatus` | `PartitionMode` | Behavior |
|---|---|---|---|
| `local * 3 >= total * 2` | `Supermajority` | `Normal` | Produce + finalize |
| in between | `Middle` | `BlockProductionOnly` | Produce, no finality votes |
| `local * 3 <= total` | `Minority` | `ReadOnly` | Refuse blocks + mempool writes, serve queries |

The external analyst should verify:

1. **Classification correctness:** that the integer arithmetic
   `local * 3 ?? total * 2` at `partition_policy.rs:140-147` actually
   matches the intended 2/3 / 1/3 boundaries without off-by-one error or
   overflow vulnerability (u128 chosen specifically to avoid overflow; see
   `classify_handles_large_stake_without_overflow` test at L285-293).
2. **Guard completeness:** that the block-production guard chain
   (mempool ingress → adapter `PopConsensusAdapter::create_next_block` →
   shared-protocol `ProofOfPublishing::create_next_block` → HTTP outer
   envelope) has no bypass. Deliberate non-guard on `Blockchain::add_block`
   is documented — catch-up from peers must keep working — and the
   analyst should confirm that distinction is sound.
3. **Convergence after heal:** that two partitions which both advanced
   state while split arrive at a consistent state after reconnection.
   `shared-protocol/tests/partition_convergence.rs` exercises
   deterministic transition application; a formal proof that the fork
   choice (longest-chain + finality guard) reliably selects the correct
   history is **not** yet produced.
4. **Liveness after heal:** that a minority partition which healed is
   able to catch up via p2p sync without getting stuck in `ReadOnly`
   forever. The catch-up path goes through `Blockchain::add_block` (not
   guarded) which is intended behavior — the analyst should verify the
   intended fault-free healing sequence actually works.

---

## 6. Long-range attack resistance

Relevant code:
- `consensus-engines/src/selection/finality.rs` (1068 LOC) — checkpoint
  finality with configurable interval (default 100 blocks), types
  `Periodic` / `Triggered` / `Emergency`, 2/3+ validator signatures
  required, post-checkpoint blocks claim absolute finality (P = 1.0)
- `consensus-engines/src/security/audit.rs:54-65` — `LongRangeAttack` and
  `NothingAtStake` vulnerability type definitions (enumeration only; the
  runtime does not implement detection for these)
- PoS double-sign detection at `consensus-engines/src/algorithms/pos.rs`
  (`slashing_records` + `evidence_store`), durable via #1218

**Gaps flagged by the internal analysis (`consensus-safety-analysis.md` §5):**

- **GAP-2: Weak subjectivity period undefined** — the code claims
  post-checkpoint blocks are "immune" to long-range rewrites but no
  maximum sync age is enforced for nodes syncing from scratch. A node
  starting from genesis has no trusted recent checkpoint.
- **GAP-4: No cross-fork voting penalty** — equivocation detection
  catches same-height double-signing inside a single node's view; a
  validator that votes on multiple fork candidates across partitions is
  not detected until partitions reconnect.

The external analyst should:

1. Verify the internal claim that the 100-block `MAX_REORG_DEPTH` plus
   2/3+ checkpoint signatures together protect against long-range attacks
   beyond the most-recent checkpoint. State the assumptions formally
   (specifically: what fraction of historical validator keys an attacker
   may possess, and over what time horizon).
2. Propose a concrete weak-subjectivity period (block count or wall-clock
   duration) given the network's block time and reasonable assumptions
   about key-compromise lifetime.
3. Assess whether the lack of cross-fork voting penalty (GAP-4) is
   exploitable in practice given the partition-mode guards.

---

## 7. BFT fault-tolerance bounds

Which algorithms claim which bounds in the current code:

| Algorithm | File | Claimed bound | Evidence |
|---|---|---|---|
| BFT finality gadget | `consensus-engines/src/selection/bft_finality.rs` | f < n/3 by stake | `is_supermajority`: `accumulated * 3 > total * 2` (L311-315) |
| HotStuff | `consensus-engines/src/algorithms/hotstuff/mod.rs` | f < n/3 (standard BFT) | Three-phase commit with QC aggregation; see module docs L1-27 |
| PoS (base algorithm) | `consensus-engines/src/algorithms/pos.rs` | Not specified — probabilistic via longest chain + BFT overlay | No explicit bound; relies on the BFT gadget overlay |
| DPoS | `consensus-engines/src/algorithms/dpos.rs` | 21 delegates, unspecified Byzantine bound | `DEFAULT_VALIDATOR_COUNT`; see algorithm docs |
| PoP (`ProofOfPublishing`) | `shared-protocol/src/consensus/proof_of_publishing.rs` | Not specified — quality-weighted block proposals + BFT overlay | Relies on BFT gadget overlay |
| PoW | `consensus-engines/src/algorithms/pow.rs` | f < n/2 honest hashrate | Standard Nakamoto assumption; not called out explicitly in code |
| Raft | `consensus-engines/src/algorithms/raft.rs` | CFT only (f < n/2 crash faults, **not Byzantine**) | Module docs: "ByzantineConsensusCoordinator integration from blockchain-core has been removed" |
| DAG | `consensus-engines/src/algorithms/dag.rs` | Probabilistic via k-parameter blue set | No formal bound in comments |

**For the analyst:** verify each claim, flag any algorithm whose code
diverges from the claim in its module docs, and explicitly state which
combinations of base algorithm + BFT overlay yield a real f < n/3
guarantee at the *chain* level vs only at the finality-gadget level.

---

## 8. Attacker model

The analysis should be performed against an attacker with the following
capabilities. The analyst may tighten or relax any dimension but must
state the final assumption explicitly.

| Dimension | Assumption |
|---|---|
| **Byzantine fraction** | f < n/3 by stake (matches BFT gadget threshold) |
| **Adaptive corruption** | Adaptive: adversary may corrupt validators mid-execution up to the f bound |
| **Rushing adversary** | Yes: adversary may delay / reorder messages after seeing honest messages in the same round |
| **Network model** | Partial synchrony: messages delivered within unknown but finite `Δ` after GST |
| **Cryptographic primitives** | Standard: ed25519 signatures and BLAKE3 / SHA-256 hashes are unforgeable (post-quantum attackers **out of scope**) |
| **Key compromise** | Static keys — long-range attack via compromise of historical validator keys is in scope for §6 |
| **Equivocation cost** | Caught same-height double-signing incurs slashing; cross-fork voting across partitions is currently uncatchable (GAP-4) |
| **Economic rationality** | Out of scope — economic / game-theoretic modeling is covered by #718 |

---

## 9. Out of scope

- **Privacy / ZK layer** — covered by #716 audit scope doc (merged as #1250)
- **Economic / game-theoretic modeling** — covered by #718
- **Smart contract execution safety** (`execution-engine/`, EVM compat) — separate concern
- **L2 scaling solutions** (`layer2/`) — separate concern
- **Post-quantum attackers** — covered by separate planned PQ migration
- **Sensor attestation / oracle trust bootstrap** — covered by #724
- **Validator key ceremony / OPSEC** — covered by #721

---

## 10. Methodology recommendations

The issue body suggests TLA+ or similar formal methods. The analyst
should choose based on the property being verified:

1. **Pen-and-paper proofs** — agreement and integrity theorems for the
   BFT gadget against the f < n/3 attacker (Alice's §2 sketch is a
   starting point; needs expert reinforcement).
2. **TLA+ model checking** — view-change and HotStuff three-chain
   commit liveness under partial synchrony. Existing public HotStuff
   TLA+ specs can be adapted rather than rebuilt.
3. **Isabelle/HOL or Coq** — if budget allows, machine-checked proof of
   the agreement theorem. This is the gold standard but may be
   disproportionate to the single-BFT-gadget scope.
4. **Property-based testing with `proptest` / `quickcheck`** — the
   partition-policy boundaries and `is_supermajority` arithmetic are
   trivial to property-test and should be added regardless of the formal
   work.
5. **Trace-based model checking (Stateright)** — feasible for the BFT
   gadget state machine and produces concrete counterexamples.

---

## 11. Known weaknesses and self-flagged concerns

Honest disclosure to set expectations for the analyst. These are already
in the internal analysis (`docs/architecture/consensus-safety-analysis.md`):

- **GAP-2 (open, Medium):** Weak subjectivity period not defined. Nodes
  syncing from genesis cannot distinguish a long-range-rewritten chain
  from the honest one without an out-of-band trusted checkpoint.
- **GAP-4 (open, Low):** Cross-fork voting is not penalized. A validator
  that votes on multiple fork candidates across partitions currently
  walks away unslashed after partitions reconnect.
- **GAP-6 (open, Medium):** Slashing circuit breaker caps penalties at
  100 events per block; excess events are silently dropped. This weakens
  accountability precisely during correlated attacks.
- **No explicit GST statement** in the code — partial-synchrony
  assumptions are used implicitly but not documented.
- **`raft.rs`'s `ByzantineConsensusCoordinator` integration was
  removed during extraction** — Raft is CFT-only in the current tree;
  any claim that Raft contributes to BFT guarantees is incorrect and
  should be flagged.
- **Fork-choice rule is longest-chain + hash tiebreak**, not
  stake-weighted. Equal-height forks are resolved by lexicographic tip
  hash (`consensus-safety-analysis.md` §4). Integrity is preserved by
  the `plan_reorg` finality guard, but this is a non-standard choice
  worth expert scrutiny.
- **BFT gadget's `total_weight` is a plain `u64` sum** at construction
  (`bft_finality.rs:245-262`). Dynamic validator-set changes that modify
  stake weights need separate review — the gadget does not itself
  expose a rekeying path.
- **Partition detection threshold** (30 missed heartbeats, per
  `adaptive_fault_tolerance.rs`) is a tunable; the analyst should
  comment on whether the default is safe given reasonable block times.

---

## 12. Deliverables

The external review should produce:

1. **Formal safety theorem** for the BFT finality gadget (agreement +
   integrity) under the attacker model in §8, with either a
   pen-and-paper proof or a machine-checked proof.
2. **Formal liveness theorem** for one or more block-production paths
   (HotStuff preferred as the strongest BFT claim) under partial
   synchrony, with an explicit GST statement.
3. **Counterexamples or proofs of safety** for each of the known
   weaknesses in §11 — in particular, whether GAP-2, GAP-4, and GAP-6
   are exploitable under the stated attacker model.
4. **Severity ratings** (Critical / High / Medium / Low / Informational)
   for every finding, with concrete exploit scenarios where applicable.
5. **Recommendations** for each finding — either a code fix at a
   specific file/line or a documented assumption the deployment must
   adopt.
6. **Confidence rating** per property in the §9 summary table of the
   internal analysis, replacing the current self-assessed ratings.
7. **TLA+ / Stateright / other formal-method artifacts** produced
   during the review, committed to `docs/consensus/formal-models/` (path
   to be created).

---

## 13. Reference artifacts

### Internal analysis (starting point)

- `docs/architecture/consensus-safety-analysis.md` (556 lines) — Alice's
  automated internal analysis, enumerates GAP-1 .. GAP-6 and their
  resolution status as of 2026-04-10.

### Test coverage (pre-existing, to be verified by the analyst)

- `shared-protocol/tests/partition_convergence.rs` — state-root
  determinism tests
- `shared-protocol/src/consensus/partition_policy.rs` tests (inline,
  19 tests) — classification boundaries, u128 overflow safety
- `consensus-engines/src/selection/bft_finality.rs` tests — vote
  accumulation, equivocation detection, `finality_sink_*`
- `native-chain/src/bin/launch_api/finality_events.rs` tests — armed
  guard, stale ordering, shutdown
- `native-chain/tests/partition_mode_block_production.rs` — HTTP
  integration tests across all three partition modes
- `native-chain/src/consensus/evidence_store.rs` tests — durable
  evidence persistence across restart

### Resolved GAP PRs (for context on the fix diffs)

- [#1238](https://github.com/wrfcoin/core4/pull/1238) — GAP-1 fork
  choice / finality guard wiring (#1216)
- [#1237](https://github.com/wrfcoin/core4/pull/1237) — GAP-3 durable
  evidence store (#1218)
- [#1239](https://github.com/wrfcoin/core4/pull/1239) — GAP-5 partition
  mode block-production guard (#1217)

---

## 14. TBD — items that need human decision before the review starts

1. **Reviewer selection** — academic vs commercial firm vs both?
   Distributed-systems labs with BFT expertise (e.g. groups at UIUC,
   NUS, Yale, EPFL) vs firms with formal-methods audit practices.
   TBD — depends on budget and timeline.
2. **Methodology choice** — pen-and-paper only, or machine-checked? See
   §10. TBD — depends on budget.
3. **GST / synchrony assumption** — partial synchrony is proposed in §8
   but the analyst may want to formalize under stricter (synchronous)
   or looser (asynchronous + common coin) models. TBD.
4. **Scope of Raft coverage** — Raft is CFT-only in the current tree;
   include it for CFT completeness or exclude entirely? TBD.
5. **Weak subjectivity period value** — GAP-2 is unresolved. The analyst
   may propose a number but adoption requires a governance decision
   about how strict initial-sync trust-on-first-use should be.
6. **Budget / timeline** — audit length (weeks vs months) and whether
   the review runs sequentially with or in parallel to the #716
   privacy/ZK audit.
7. **Disclosure policy** — findings embargo period, coordinated
   disclosure channel, public report format. TBD.

---

Refs #720
