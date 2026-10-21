# WRFCoin Consensus Safety & BFT Analysis

**Issue:** #720  
**Author:** Alice (marathon sprint, 2026-06-19)  
**Status:** Draft — awaiting Tony sign-off  
**Codebase refs:** `shared-protocol/src/consensus/proof_of_publishing.rs`,
`shared-protocol/src/consensus/finality.rs`,
`shared-protocol/src/blockchain/chain.rs`,
`shared-protocol/src/common/constants.rs`,
`shared-protocol/src/blockchain/state.rs`

---

## 1. Threat Model and Assumptions

### 1.1 Network Model

- **Partial synchrony**: The network is partially synchronous (periods of asynchrony are bounded
  but the bound is unknown in advance). Safety holds under full asynchrony; liveness requires
  eventual synchrony.
- **Point-to-point links**: Honest nodes can always eventually communicate. An adversary may
  delay but not forge messages.

### 1.2 Adversary Model

- **Byzantine threshold**: Up to `f` oracle validators may be Byzantine (malicious, crashed, or
  equivocating). Safety requires `N ≥ 3f + 1` for the 2/3+ quorum formula.
- **Bounded adversary**: The adversary cannot break SHA3-256 or ed25519 (computational
  assumption). All oracle signatures are verified before a vote is counted.
- **No long-range attacks**: The finalized height is monotonic and enforced at the chain layer
  (`set_finalized_height` in `chain.rs`). Once finalized, a block cannot be reverted regardless
  of accumulated stake from a historical key.

### 1.3 Trust Assumptions

| Component | Trust Level | Rationale |
|-----------|-------------|-----------|
| Oracle validator set | Byzantine-tolerated (up to f < N/3) | Dynamic quorum enforces 2/3+ |
| Block producer | Untrusted — produces proposal only | Applier re-validates every block |
| API callers | Untrusted — validated at boundary | RBAC + ed25519 signature check |
| N5 testnet hardware | Trusted execution environment | Local-only deploy, no external exposure |

---

## 2. BFT Safety Argument

### 2.1 Quorum Formula

Defined in `shared-protocol/src/common/constants.rs`:

```rust
pub const DEFAULT_ORACLE_QUORUM: usize = 2;
pub const DYNAMIC_QUORUM_ACTIVATION_HEIGHT: u64 = 0;

pub fn compute_oracle_quorum(oracle_set_size: usize) -> usize {
    (oracle_set_size * 2 / 3 + 1).max(DEFAULT_ORACLE_QUORUM)
}
```

The formula `⌊N × 2/3⌋ + 1` is the standard BFT 2f+1 threshold. Concrete values:

| Oracle set size N | Quorum Q | Tolerated Byzantine f = N − Q |
|-------------------|----------|-------------------------------|
| 2 | 2 | 0 |
| 3 | 3 | 0 |
| 6 | 5 | 1 |
| 9 | 7 | 2 |
| 12 | 9 | 3 |
| 100 | 67 | 33 |

The quorum is evaluated fresh at every block using the live oracle set
(`compute_oracle_quorum(chain.state.oracle_set.len())`), so oracle set changes take effect
immediately without a configuration lag.

### 2.2 Safety Proof Sketch

**Claim:** No two conflicting blocks at the same height can both accumulate quorum attestations
from an honest majority.

**Argument:**

1. A block is only considered finalized if `≥ Q` distinct oracle IDs have submitted
   `OracleAttestationPayload { approved: true, commit_id: block_hash }` for that hash.
2. Each oracle submission is gated by ed25519 signature verification against the registered
   signing key for that oracle's `SigningRole::Oracle` epoch key
   (`verify_signature_for_identity_role` in `proof_of_publishing.rs`).
3. The attestation set for each commit is stored as a set keyed by `(commit_id, oracle_id)` —
   a single oracle cannot vote twice on the same block to inflate the count.
4. Two conflicting blocks B₁ and B₂ at the same height have distinct `commit_id` hashes (SHA3-256
   of block content). For both to reach quorum, `Q` votes would be needed for each. The union
   of voters would require `2Q > N` distinct oracle IDs. Since `Q > N/2`, the two quorum sets
   must overlap by at least one oracle. An honest oracle signs at most one hash per height; a
   Byzantine oracle may equivocate, but the overlap of 1 means at least one oracle is Byzantine.
   With `Q = ⌊2N/3⌋ + 1`, we need `2(⌊2N/3⌋ + 1) > N`, i.e. at least `⌈N/3⌉` Byzantine nodes
   for a safety violation — exactly the BFT threshold.
5. Therefore, with `f < N/3` Byzantine oracles, no two conflicting blocks can simultaneously
   reach quorum. **Safety holds.**

### 2.3 Producer/Applier Parity (PR #1857)

A prerequisite for the safety argument is that the producer and applier execute identical
state transitions. Prior to #1857, the applier called `apply_block_finalization` but the
producer's re-apply path did not, causing state root divergence:

- **Applier path** (`proof_of_publishing.rs` ~line 1531): calls `apply_block_finalization(block.height)` after all reward transitions.
- **Producer path** (`proof_of_publishing.rs` ~line 2746): now also calls `apply_block_finalization(block.height)` at the equivalent point.

`apply_block_finalization` executes: `flush_deferred_slashing` → `prune_commit_reveal_expired`
→ `prune_expired_state` → `recalculate_state_root`. Without parity, the producer's proposed
state root could not be validated by the applier, causing quorum attestations to reference
different state roots at the same height — a silent fork.

---

## 3. BFT Liveness Argument

### 3.1 Conditions for Block Production

The `PopConsensusAdapter::create_next_block` method guards block production with a partition-mode
check:

```rust
if !current_mode.allows_block_production() {
    // Refuses to produce a block when the node is partitioned or in safe mode
}
```

Under partition, the node stops producing blocks rather than risking a minority fork. Liveness
is sacrificed when the node cannot confirm majority connectivity — the standard CAP theorem trade-off.

### 3.2 Progress Under Honest Majority

When `≥ Q` honest oracles are online and connected:

1. The block producer constructs a candidate block and gossips it.
2. Each honest oracle attests independently, submitting via `submit_oracle_attestation`.
3. After `≥ Q` distinct approvals accumulate, the block is considered ready for finalization.
4. The finality consumer (`run_finality_event_loop` in `p2p_events.rs`) delivers the
   `FinalitySignal` to `set_finalized_height`, advancing the finality frontier.
5. The next block producer can build on the finalized tip.

Liveness holds as long as:
- The network is eventually synchronous.
- At least `Q` honest oracles can receive and process blocks within the synchrony bound.
- The finality channel buffer (`FINALITY_SIGNAL_CHANNEL_CAPACITY = 256`) is not saturated.

### 3.3 Finality Wiring (PR #1216)

`FinalitySignal` crossing from consensus to chain is defined in `finality.rs`:

```rust
pub struct FinalitySignal {
    pub height: u64,
    pub block_hash: [u8; 32],
    pub source: &'static str,
}
```

`set_finalized_height` is **monotonic** — it only advances the finality frontier, never
retreats. Out-of-order delivery (e.g. two signals where the first is delayed) is handled
by the monotonic clamp: whichever arrives later only takes effect if its height exceeds
the current `finalized_height`. No reorder buffer is needed.

---

## 4. State Root Integrity

### 4.1 Canonical Pre-image

The state root is SHA3-256 over a deterministically ordered byte sequence prefixed with the
version tag `"wrfcoin_state_root_v8"`. The 26 fields that contribute to the pre-image are
pinned by `STATE_ROOT_MATERIAL_FIELD_NAMES` (a `const &[&str]` array). The field order is
enforced by the `state_root_field_pin_guard` test.

Accounts are sorted by `address.0` (the raw bytes of the address), ensuring consistent
ordering regardless of insertion order.

### 4.2 Field-Pin Guard (PR #1855)

The `state_root_field_pin_guard` test ensures that adding or reordering fields in
`state_root_material()` without updating the pin array causes a compile-time or test failure.
This prevents accidental state root changes across a validator upgrade — any intentional field
change requires a version bump to `"wrfcoin_state_root_v9"` and a deliberate pin update.

### 4.3 Float Canonicalization

Floating-point values in the state root material are encoded via `canonical_f64_bits(v)`:

- **NaN** → `u64::MAX` (logged as a warning)
- **+Inf / -Inf** → their IEEE 754 bit patterns (logged as warnings)  
- **-0.0** → `0u64` (normalized to +0.0 canonical bits, per issue #1850)
- **All other values** → `v.to_bits()`

This ensures x86_64 and ARM validators produce identical state roots even when near-zero
arithmetic produces `-0.0` on one architecture but not the other.

### 4.4 Governance Parameter Canonicalization (PR #1851)

`push_governance_parameter_update` previously used `serde_json::to_vec`, which routed f64
values through serde_json's float serializer rather than `canonical_f64_bits`. This bypass
allowed -0.0 and NaN to produce different JSON byte sequences on different architectures.
Fixed by directly routing all governance parameter f64 values through `canonical_f64_bits`.

---

## 5. Fork and Reorg Handling

### 5.1 Finality Guard

`plan_reorg_with_loader` in `chain.rs` enforces two hard limits before accepting a reorg:

```rust
if fork_height < self.finalized_height {
    return Err(/* "cannot reorg below finalized height" */);
}
if blocks_to_revert > MAX_REORG_DEPTH {
    return Err(/* "reorg depth exceeds maximum" */);
}
```

`MAX_REORG_DEPTH = 100` caps how many blocks can be reverted in a single reorg, bounding the
computational cost and limiting the window for long-range reorg attacks.

`finalized_height` starts at 0 and advances monotonically as `FinalitySignal`s are consumed.
Any fork whose split point is at or below `finalized_height` is unconditionally rejected —
the finalized chain is immutable.

### 5.2 In-Memory vs. Persistent Reorg

`plan_reorg` (no loader) searches only the in-memory block cache. `plan_reorg_with_loader`
fixes issue #1277: when a fork point is within `MAX_REORG_DEPTH` but has been evicted from
the in-memory cache, the loader fetches it from storage, allowing the full depth check to
proceed correctly rather than failing on a cache miss.

### 5.3 Absent Finality Consumer

If the `run_finality_event_loop` consumer is not wired (e.g. during early testnet bootstrap
before P2P is fully active), `finalized_height` stays at 0. In this state, the `plan_reorg`
finality guard is inert — it will not reject any fork below a non-existent finality frontier.
**The reorg depth cap (`MAX_REORG_DEPTH = 100`) still applies**, bounding the worst-case
reorg to 100 blocks. Operators should ensure the finality consumer is wired before enabling
block production on the live network.

---

## 6. Nondeterminism Guards (PRs #1849–#1853)

Cross-node state divergence caused by floating-point nondeterminism is a silent safety hazard:
nodes compute identical logical transitions but arrive at different state roots, causing honest
validators to reject each other's blocks as invalid.

### 6.1 BPS Truncation, Not Rounding (PR #1849)

Reward multiplier-to-BPS conversion previously used `.round() as u128`. On some platforms,
midpoint values (e.g. `0.5005`) can round up or down depending on the rounding mode register.
Fixed by using truncation (`as u128` / `as u32` directly), which is unambiguous: any value
`v` produces `⌊v⌋`, identical on all IEEE 754 platforms.

Regression guard in `proof_of_publishing.rs` (function `reward_multiplier_to_bps_truncates_not_rounds`):
```rust
// e.g. (0.9995 * 10_000) as u128 == 9994  (truncate)
// vs   (0.9995 * 10_000).round() == 10000  (rounds at midpoint — platform-dependent)
```

### 6.2 -0.0 Canonicalization in State Root (PR #1850)

`canonical_f64_bits` normalises `-0.0` to `0u64` before encoding into the state root
pre-image. Without this, near-zero arithmetic on ARM can produce `-0.0` where x86_64 produces
`+0.0`; their IEEE 754 bit patterns differ (`0x8000_0000_0000_0000` vs `0`), causing a hash
mismatch and an intra-epoch fork.

### 6.3 Governance Parameter f64 Canonical Encoding (PR #1851)

Previously `push_governance_parameter_update` called `serde_json::to_vec(&value)`, which
does not route f64 through `canonical_f64_bits`. This bypassed -0.0 and NaN normalization
for governance proposals. Now every f64 in a `GovernanceParameterUpdate` is encoded via
`canonical_f64_bits`.

### 6.4 Haversine via libm (PR #1852)

`haversine_km` (in `wrfcoin_geo_grid`) uses `libm` sin/cos/atan2/sqrt — software
implementations with identical rounding on all platforms, unlike the hardware FPU
instructions used by `f64::sin` etc. which can produce 1-ULP differences between x86_64
and ARM. The raw distance result is only used for spatial pre-filtering
(not hashed into the state root), so 1-ULP differences in the raw km value would propagate
to `spatial_scores` in the state root, causing a fork. The comment in `spatial.rs` notes
that the residual ~10⁻¹³ km variation after libm is negligible and not state-root-visible.

### 6.5 Attestation Outcome Determinism (PR #1853)

`apply_block_finalization` in `state.rs` calls `prune_commit_reveal_expired` and
`flush_deferred_slashing` in a deterministic order before `recalculate_state_root`.
Prior to #1853, the ordering of these cleanup passes could vary between producer and
applier depending on call-site differences, causing the resulting state root to differ even
when the logical outcome was the same (issue #1853: "state root reflects attestation
outcomes").

---

## 7. Known Limitations and Open Items

### 7.1 Finality Consumer Must Be Wired for Full Safety

The `FinalitySignal` mechanism is wired in `p2p_events.rs` only when the `p2p` feature flag is
active. In single-node testnet mode (no P2P), `finalized_height` remains 0, leaving the reorg
finality guard inert. The `MAX_REORG_DEPTH = 100` cap still limits reorg depth, but a long-range
attack against a non-finalized chain is theoretically possible up to 100 blocks.

**Mitigation for testnet launch:** Single-node operation has no peers to fork against. The
primary risk is operational (accidental restart from a stale snapshot). Operators should use
checkpointing to establish an out-of-band finality floor.

### 7.2 Bootstrap Oracle Set Must Reach Quorum Before Block Height 1

`DEFAULT_ORACLE_QUORUM = 2` and `DYNAMIC_QUORUM_ACTIVATION_HEIGHT = 0` mean the quorum is
enforced from genesis. With a single oracle registered, `compute_oracle_quorum(1) = max(2, 1)
= 2` — more oracles are required than exist. Block production stalls until at least 2 oracles
are registered and both attest.

**Mitigation:** The operator runbook (`docs/launch/key-ceremony-runbook.md`) requires
registration of at least 2 oracle operators before `launch_api` is started in production mode.
The `oracle_admin_api_enabled` gate (Dan's #1869) prevents bootstrap mutations after height > 0.

### 7.3 Oracle Set Mutation After Height 0 Is Blocked

Once `blockchain.height > 0`, `register_oracle_operator` and `update_oracle_config` return
`409 Conflict` (Dan's #1869 live-network guard). This prevents runtime re-configuration of
the oracle set, which is correct for safety but means oracle key rotation requires a planned
restart and state migration. There is no online re-keying path at this time.

### 7.4 `FINALITY_SIGNAL_CHANNEL_CAPACITY = 256`

If the finality consumer stalls (e.g. the consumer task panics), the channel buffer fills in
256 blocks. The sender uses `try_send`; overflow signals are dropped silently. `finalized_height`
stops advancing but block production continues. This does not violate safety (blocks are still
attested by quorum), but the reorg depth protection degrades over time.

**Mitigation:** The channel drop is logged at `warn!` level via `emit_finality_signal` in
`finality.rs`. There is no dedicated Prometheus counter for this event; operators should
monitor the `warn!` log line ("BFT finality signal channel full") via log-based alerting.
A dedicated metric is an open item post-launch.

### 7.5 Quorum Dynamic Scaling Is Not Rate-Limited

Adding a large oracle set in a single genesis configuration step immediately raises the quorum
threshold. If the new threshold exceeds the number of live oracles, block production stalls
immediately. There is no gradual ramp-up or grace period.

### 7.6 Post-Quantum Cryptography

Oracle signatures use ed25519. Post-quantum algorithms (DILITHIUM, FALCON, SPHINCS+) are
implemented in `wrfcoin/blockchain-core` (archived) but not yet integrated into the consensus
oracle path. This is a known gap for long-term security; it is out of scope for the 2026-06-19
testnet launch.

---

## 8. Summary

| Property | Status | Code Reference |
|----------|--------|----------------|
| BFT Safety (2f+1 quorum) | Implemented | `constants.rs:compute_oracle_quorum` |
| State root determinism | Implemented | `state.rs:canonical_f64_bits`, #1850, #1851 |
| Producer/applier parity | Fixed (#1857) | `proof_of_publishing.rs:apply_block_finalization` |
| Field-order pin guard | Implemented (#1855) | `state.rs:STATE_ROOT_MATERIAL_FIELD_NAMES` |
| Reorg depth cap | Implemented | `chain.rs:MAX_REORG_DEPTH = 100` |
| Finality monotonicity | Implemented (#1216) | `chain.rs:set_finalized_height` |
| BPS truncation guard | Implemented (#1849) | `proof_of_publishing.rs:reward_multiplier_to_bps_truncates_not_rounds` |
| Haversine libm | Implemented (#1852) | `geo_grid::haversine_km` |
| Governance f64 canonical | Implemented (#1851) | `state.rs:push_governance_parameter_update` |
| Attestation cleanup order | Fixed (#1853) | `state.rs:apply_block_finalization` |
| Finality consumer wiring | **Partial** — P2P-only | `p2p_events.rs:run_finality_event_loop` |
| Online oracle key rotation | **Not implemented** | Open item post-launch |
| Post-quantum signatures | **Not implemented** | Deferred (blockchain-core archived) |

---

_Document prepared for Tony's review and sign-off as required by issue #720. Code references
are against the `wrfcoin/core4` main branch as of the 2026-06-19 marathon launch sprint._
