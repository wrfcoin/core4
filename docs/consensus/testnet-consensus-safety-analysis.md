# Testnet Consensus Safety Analysis

**Issue:** [core4#720](https://github.com/wrfcoin/core4/issues/720)  
**Date:** 2026-05-30  
**Scope:** Internal launch-readiness analysis for the current PoP testnet path  
**Decision:** Sufficient for a controlled multi-operator testnet, not sufficient
to close the external `human-required` review gate

This document is the bounded consensus-safety answer needed before inviting
additional testnet operators. It updates the earlier internal analysis in
[`docs/architecture/consensus-safety-analysis.md`](../architecture/consensus-safety-analysis.md)
and complements the external-review scope in
[`docs/consensus/bft-safety-analysis-scope.md`](bft-safety-analysis-scope.md).

It is not a third-party audit, a TLA+ model, or a machine-checked proof. It is
a code-evidence-backed internal analysis of the properties most relevant to
the current launch blocker: BFT bounds, partition recovery, long-range attack
posture, nothing-at-stake posture, and the liveness-versus-safety tradeoff.

## Current Testnet Consensus Path

The current testnet path is PoP block production with a BFT finality overlay:

- PoP constructs blocks through `PopConsensusAdapter::create_next_block` and
  `ProofOfPublishing::create_next_block`.
- The BFT finality gadget finalizes a block only when accumulated validator
  stake satisfies `accumulated * 3 > total * 2`.
- Finality signals are consumed by `run_finality_event_loop`, which advances
  `Blockchain::finalized_height`.
- Fork choice remains longest-chain plus lower-hash tie-break for unfinalized
  history, but it refuses any reorg whose fork point is below the finalized
  height.
- Partition mode can force a node into `ReadOnly` or `BlockProductionOnly`
  before new local block production occurs.

The analysis therefore covers two distinct surfaces:

| Surface | Safety role | Evidence |
|---|---|---|
| BFT finality gadget | Agreement for finalized blocks under f < n/3 Byzantine stake | `consensus-engines/src/selection/bft_finality.rs` |
| Chain finality guard | Prevents finalized blocks from being reverted by probabilistic fork choice | `shared-protocol/src/blockchain/chain.rs` and `native-chain/src/bin/launch_api/finality_events.rs` |
| Partition policy | Prevents minority partitions from producing local rollback-prone blocks | `shared-protocol/src/consensus/partition_policy.rs` |
| PoP production guards | Enforces partition mode at HTTP, adapter, and shared-protocol chokepoints | `native-chain/src/bin/launch_api/routes/mining.rs`, `consensus-engines/src/algorithms/pop_adapter.rs`, `shared-protocol/src/consensus/proof_of_publishing.rs` |
| State-root computation | Makes deterministic replay possible after partitions heal | `shared-protocol/src/blockchain/state.rs` |
| Equivocation evidence | Preserves same-height double-sign evidence across restart | `consensus-engines/src/equivocation.rs`, `native-chain/src/consensus/evidence_store.rs` |

## BFT Fault-Tolerance Bound

For finalized blocks, the implemented bound is the standard BFT bound:

> If less than one third of total validator stake is Byzantine and honest
> validators do not sign two conflicting blocks at the same height, two
> conflicting blocks at the same height cannot both become BFT-final.

The code uses a strict supermajority rule:

```text
finalized iff accumulated_weight * 3 > total_weight * 2
required_weight = floor(total_weight * 2 / 3) + 1
```

This matters at small validator counts. With three equal-weight validators,
two votes are exactly 2/3 and do not finalize; all three are required. With
four equal-weight validators, three votes finalize. The tests in
`bft_finality.rs` already pin the exact-boundary behavior and large-weight
arithmetic.

The guarantee is finality-gadget scoped, not a blanket statement that every
base consensus algorithm has a BFT proof. PoP, PoS, DPoS, DAG, and PoW remain
probabilistic before BFT finality. Raft is crash-fault tolerant only and must
not be described as Byzantine-fault tolerant.

**Readiness judgment:** acceptable for controlled testnet finalized blocks.
The f < n/3 statement is reasonable for this phase when validator weights are
static and the validator set is operator-controlled. Dynamic validator-set
changes still need explicit reconfiguration analysis.

## Finality And Fork Choice

`Blockchain::finalized_height` is monotonic. Once a finality signal advances
that height, stale or duplicate signals cannot roll it backward. The launch
API finality consumer also persists finalized-height advances, so a restart no
longer silently disarms the guard.

One boundary is easy to misstate: the code rejects reorgs with
`fork_height < finalized_height`, not `fork_height <= finalized_height`.
That is the correct safety boundary. A candidate chain whose fork point is
exactly the finalized height keeps the finalized block and replaces only
unfinalized descendants. The existing `shared-protocol/tests/chain_reorg.rs`
boundary test documents that behavior.

**Invariant:**

```text
No accepted reorg may remove a block at height <= finalized_height.
```

**Readiness judgment:** acceptable for testnet. The guard is correctly scoped
to preserve finalized blocks while allowing liveness for forks above finality.

## Partition Recovery

The partition policy maps local stake fraction to operating mode:

| Local stake fraction | Mode | Behavior |
|---|---|---|
| `>= 2/3` | `Normal` | Produce blocks, accept mempool writes, cast finality votes |
| `> 1/3` and `< 2/3` | `BlockProductionOnly` | Produce blocks and accept writes, but do not cast finality votes |
| `<= 1/3` | `ReadOnly` | Refuse new mempool writes and local block production; serve reads |

This is the right safety-first split for the current testnet model:

- A minority partition cannot finalize, so producing local blocks only creates
  state that should later be reverted.
- A middle partition may preserve availability, but its blocks remain
  probabilistic until the network heals.
- A supermajority partition can continue normally because it can satisfy the
  BFT finality threshold without the missing validators.

The guards are defense in depth:

1. Mempool ingress refuses new writes in `ReadOnly`.
2. `PopConsensusAdapter::create_next_block` refuses local PoP block production
   in `ReadOnly`.
3. `ProofOfPublishing::create_next_block` repeats the production guard for
   direct callers.
4. `/blocks/mine` returns a 503 `PARTITION_READ_ONLY` envelope before taking
   the production lock, and maps inner races to the same error code.

`Blockchain::add_block` is intentionally not guarded by partition mode. A
partitioned node must still be able to apply blocks received from peers while
catching up after heal.

State-root determinism has strong local evidence: `State::recalculate_state_root`
uses `wrfcoin_state_root_v8`, sorted map material, deterministic BTreeMap
iteration for consensus maps, and canonical f64 bit encoding. The partition
and weather replay tests already check insertion-order and replay determinism.

**Readiness judgment:** acceptable for testnet with operator monitoring. The
remaining risk is not deterministic replay from the same transition sequence;
it is choosing and disseminating the right post-heal history when middle
partitions produced unfinalized competing blocks.

## Producer/Applier State Parity ([#1680](https://github.com/wrfcoin/core4/issues/1680))

Deterministic state-root computation is necessary but not sufficient for
convergence. The same transition *sequence* must also be produced when a node
*builds* a block and when a peer *applies* that same block. The
same-tip/different-state divergence observed on early multi-node testnet
([#1680](https://github.com/wrfcoin/core4/issues/1680)) was exactly this class
of bug: two nodes agreed on the block tip hash but computed different state
roots, because the block-producer apply path and the peer-applier apply path
diverged in two places.

The merged fixes close that gap:

- **Coinbase live-credit + telemetry excluded from `state_root`**
  ([#1681](https://github.com/wrfcoin/core4/pull/1681)). The coinbase reward is
  credited on both the producer and applier paths, and node-local telemetry /
  diagnostic event logs are no longer hashed into the consensus state root, so
  observability noise cannot perturb consensus.
- **Producer stale-`state_root` recompute + a producer/applier parity test**
  ([#1682](https://github.com/wrfcoin/core4/pull/1682) /
  [#1683](https://github.com/wrfcoin/core4/pull/1683)). The producer recomputes
  the state root after applying all transitions rather than trusting a stale
  pre-apply value.
- **Replay low-quality streaks on the applier path**
  ([#1684](https://github.com/wrfcoin/core4/pull/1684)). Low-quality-streak
  accounting is now re-derived from block contents on the applier path instead
  of relying on untrusted block metadata, so the streak counters that feed the
  state root match on both sides.
- **Shared batch transition apply**
  ([#1688](https://github.com/wrfcoin/core4/pull/1688),
  [#1690](https://github.com/wrfcoin/core4/pull/1690),
  [#1691](https://github.com/wrfcoin/core4/pull/1691)). Producer launch
  transitions and reward state mutations are routed through one shared batch
  apply, removing the structural opportunity for the two paths to drift.
- **State-root parity guard** ([#1686](https://github.com/wrfcoin/core4/pull/1686)).
  `producer_applier_diff_covers_state_root_material_fields_1682` asserts that
  the producer/applier diff harness actually inspects every field that feeds the
  state root, so a newly added state-root field cannot silently escape parity
  coverage.

**Invariant:**

```text
For the same block, the producer's post-apply state root and any honest
applier's post-apply state root are identical. Node-local telemetry never
contributes to the state root.
```

**Test evidence (re-verified on current `origin/main`):**

| Property | Test |
|---|---|
| Coinbase-only block parity | `producer_applier_block_apply_parity_coinbase_only` |
| Attested-reward block parity | `producer_applier_block_apply_parity_attested_reward` |
| Low-quality streak applier replay parity | `producer_applier_low_quality_streak_divergence_known_1682`, `producer_applier_low_quality_slash_failure_preserves_streak_1682` |
| Parity harness covers all state-root fields | `producer_applier_diff_covers_state_root_material_fields_1682` |
| Telemetry excluded from state root | `telemetry_event_logs_do_not_affect_state_root`, `state_root_ignores_radio_metadata` |
| Untrusted streak metadata ignored on extract | `extract_transitions_ignores_untrusted_low_quality_streak_metadata` |
| Same-tip / different-root divergence detection | `peer_chain_status_same_height_different_tip_sets_state_diverged`, `peer_chain_status_nonzero_root_mismatch_sets_state_diverged` |

All of the above pass on `origin/main` (the `p2p_events::tests` suite reports
15 passed / 0 failed with `--features p2p`).

**Readiness judgment:** acceptable for testnet. The original divergence class is
closed, the two apply paths share a single batch-apply primitive, and a guard
test prevents new state-root fields from regressing parity. The residual risk is
the same one noted under Partition Recovery — *which* unfinalized history wins
after a heal — not whether replaying the same block yields the same state.

## Long-Range Attack Posture

The current code has useful local protections:

- Finalized height is persisted and restored across restart.
- Block storage has checkpoint and WAL replay support.
- Missing or corrupt checkpoint anchors fail closed instead of replaying from
  an unsafe partial state.
- Snapshot sync has deterministic snapshot roots and trusted-signer signature
  checks.

Those protections do not fully solve weak subjectivity. A brand-new node
syncing from genesis still needs an out-of-band recent trusted checkpoint or a
defined maximum sync age. Without that, an attacker with historical validator
keys could present an alternative old history to a fresh node.

**Readiness judgment:** acceptable for a controlled testnet where operators
bootstrap from a known current checkpoint or trusted peer set. Not acceptable
as a final mainnet posture until weak-subjectivity rules are documented and
enforced.

## Nothing-At-Stake And Equivocation

Same-height double signing is detected and represented as
`EquivocationEvidence`. Production launch code can use the durable redb-backed
evidence store so detected evidence survives node restart, and the evidence
store tests cover restart recovery.

The gap is cross-fork voting. A validator voting on different fork candidates
across a partition is not necessarily caught until the views meet, and the
current evidence shape is centered on same-height conflicts observed by one
node. The slashing circuit breaker also caps applied slashing events per block,
which protects against correlated false positives but can delay accountability
during a mass event.

**Readiness judgment:** acceptable for controlled testnet only because the
validator set is small and operator-coordinated. It remains a material
pre-mainnet audit item.

## Liveness Versus Safety

The current design prefers safety over liveness:

- BFT finality stalls without more than two thirds of stake voting for the
  same block.
- Minority partitions halt local block production instead of creating
  rollback-prone user state.
- Finalized blocks cannot be reverted by longer probabilistic forks.
- Storage fails closed on missing or corrupt checkpoint anchors.

Liveness is preserved where it does not compromise finalized safety:

- Unfinalized fork choice can still adopt better forks above
  `finalized_height`.
- Middle partitions can continue producing probabilistic blocks.
- `Blockchain::add_block` remains open for peer catch-up during heal.
- HotStuff and other engines retain their own leader/view-change or
  probabilistic progress mechanisms, but those are not the primary PoP testnet
  safety claim.

**Readiness judgment:** the halt-vs-accept-invalid rule is clear. The system
should halt finality, or halt minority local production, rather than accept a
block that would violate finalized safety.

## Test Evidence Reviewed

Existing tests already cover the cheap invariants this sprint lane would add:

| Property | Test evidence |
|---|---|
| BFT finality threshold and certificate boundary | `consensus-engines/src/selection/bft_finality.rs` tests, plus `consensus-engines/tests/bft_finality_aggregator_e2e.rs` |
| Finalized-height reorg guard | `shared-protocol/tests/chain_reorg.rs` |
| Finality signal consumer and stale-signal monotonicity | `native-chain/src/bin/launch_api/finality_events.rs` tests |
| Persisted finalized height across restart | `native-chain/src/storage.rs` tests |
| Partition classification boundaries | `shared-protocol/src/consensus/partition_policy.rs` tests |
| ReadOnly production refusal | `native-chain/tests/partition_mode_block_production.rs`, `consensus-engines/src/algorithms/pop_adapter.rs`, `shared-protocol/src/consensus/proof_of_publishing.rs` tests |
| Mempool write refusal in ReadOnly | `shared-protocol/src/mempool/pool.rs` tests |
| State-root determinism | `shared-protocol/tests/partition_convergence.rs`, `shared-protocol/tests/weather_reward_e2e.rs`, and state module tests |
| Producer/applier state parity (#1680) | `p2p_events::tests::producer_applier_*` (5 tests), `transition_extractor::tests::extract_transitions_ignores_untrusted_low_quality_streak_metadata`, `state::tests::telemetry_event_logs_do_not_affect_state_root` |
| Same-tip / different-root divergence detection | `p2p_events::tests::peer_chain_status_same_height_different_tip_sets_state_diverged`, `..._nonzero_root_mismatch_sets_state_diverged` |
| Durable equivocation evidence | `native-chain/src/consensus/evidence_store.rs` and `consensus-engines/src/algorithms/pos.rs` tests |
| Snapshot root determinism and trusted signer checks | `shared-protocol/src/blockchain/snapshot_sync.rs` tests |

No new invariant test is required for this documentation slice because the
obvious arithmetic, restart, and partition-mode invariants are already covered.

## Open Items

These should remain visible after this document merges:

| Gap | Testnet disposition | Required before mainnet or public external-operator scale |
|---|---|---|
| External BFT / distributed-systems review | Internal analysis is enough only for controlled testnet | Commission or bundle with the security audit scope |
| Weak subjectivity period | Operators use trusted current bootstrap material | Define and enforce max checkpoint age / trusted checkpoint policy |
| Cross-fork voting evidence | Monitor manually in small validator set | Extend evidence and slashing model across partition views |
| Dynamic validator-set changes | Keep validator set tightly controlled | Formalize reconfiguration and weight-change safety |
| Middle-partition history choice | Accept probabilistic rollback risk above finality | Add operator runbook and, ideally, model-check fork-choice behavior after heal |
| Slashing circuit breaker backlog | Accept delayed accountability | Buffer or explicitly report skipped events |

## Final Disposition For Core4 #720

For the 2026-05-30 testnet launch-blocker round, the consensus safety posture
is bounded but adequate for a controlled multi-operator testnet:

- finalized blocks have a standard f < n/3 by-stake BFT agreement argument;
- finalized-height persistence and the fork-choice guard protect finalized
  history across reorgs and restarts;
- minority partitions halt local production instead of creating guaranteed
  rollback state;
- deterministic state-root tests support replay convergence;
- the same-tip/different-state replay divergence ([#1680](https://github.com/wrfcoin/core4/issues/1680))
  is closed: producer and applier paths now share one batch-apply primitive,
  telemetry is excluded from the state root, and a parity guard test prevents
  new state-root fields from regressing;
- same-height double-sign evidence can survive restart.

The posture is not adequate to close the `human-required` portion of #720 for
mainnet. The issue should remain open, or be split, until a human distributed
systems reviewer accepts this analysis or replaces it with a formal external
review.
