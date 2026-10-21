//! Network partition operating-mode policy (#1217).
//!
//! Defines the behavior of a node that finds itself in a network
//! partition. The
//! [`consensus_engines::coordination::adaptive_fault_tolerance`](https://docs.rs/wrfcoin-consensus-engines)
//! module detects partitions and selects recovery topologies but does
//! NOT specify whether a partition should keep producing blocks,
//! finalize them, or enter a read-only safety mode.
//!
//! This module fills that gap with a small, focused decision function:
//!
//! 1. Classify the local partition by its **stake weight** relative to
//!    the full validator set
//! 2. Map the classification to an operating mode
//! 3. Expose both the classification and the mode so that block
//!    production, finality voting, and RPC layers can gate their
//!    behavior consistently.
//!
//! ## Thresholds
//!
//! The thresholds mirror the BFT safety proof (f < n/3) and the
//! supermajority rule used by the finality gadget (≥ 2n/3 + 1):
//!
//! | Local stake fraction | Classification | Mode |
//! |---|---|---|
//! | ≥ 2/3 | `Supermajority` | `Normal` — produce + finalize |
//! | > 1/3, < 2/3 | `Middle` | `BlockProductionOnly` — produce, no BFT finality |
//! | ≤ 1/3 | `Minority` | `ReadOnly` — refuse new blocks |
//!
//! ## Why a minority must not produce blocks
//!
//! A minority partition that continues to produce blocks will, upon
//! reconnection, present the majority with a conflicting history.
//! Because it holds < 1/3 of stake, none of its blocks can have BFT
//! finality (the gadget requires 2/3+ attestation). The only outcome
//! for its blocks is to be discarded during reorg. Producing them in
//! the first place wastes work, bloats mempool, and introduces user-
//! visible state that will later be rolled back — a strictly worse UX
//! than refusing new writes until the partition heals.
//!
//! A middle partition (1/3–2/3) is allowed to produce blocks for
//! availability but cannot finalize them. Operators should treat blocks
//! from this mode as probabilistic until the partition clears.
//!
//! ## Location (moved from `consensus-engines`)
//!
//! The module originally lived at
//! `consensus-engines::coordination::partition_policy` where it could be
//! reached from the partition detection state machine. It was moved into
//! `shared-protocol` so the mempool and other lower-level modules can
//! import [`PartitionMode`] without a circular dependency — shared-protocol
//! must not depend on consensus-engines. `consensus-engines` still
//! re-exports the symbols for backward compatibility with existing
//! callers.
//!
//! Refs #720 GAP-5, closes #1217.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

/// How the local partition sits relative to the full validator set by stake.
///
/// Derived via [`classify_partition`] from a local stake weight and the
/// total validator stake.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartitionQuorumStatus {
    /// Local partition holds ≥ 2/3 of total stake.
    ///
    /// Can finalize blocks via the BFT gadget without help from any
    /// other partition. Safe to operate normally.
    Supermajority,

    /// Local partition holds > 1/3 but < 2/3 of total stake.
    ///
    /// Can produce blocks (enough live stake for block production) but
    /// cannot unilaterally finalize them — the BFT gadget requires 2/3+
    /// attestation which is unavailable by definition in this split.
    Middle,

    /// Local partition holds ≤ 1/3 of total stake.
    ///
    /// By the BFT safety proof, no block this partition produces can
    /// ever achieve finality, so producing blocks wastes work and
    /// creates state that will be reverted on reconnection.
    Minority,
}

/// Operating mode that a node should assume given its partition status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartitionMode {
    /// Block production and finality voting both active.
    Normal,

    /// Produce blocks, but finality is stalled (no quorum).
    BlockProductionOnly,

    /// Refuse new blocks. Read-only: queries are served, mempool
    /// accepts no new transactions, block production is halted.
    ReadOnly,
}

impl Default for PartitionMode {
    /// Default to [`PartitionMode::Normal`].
    ///
    /// A freshly-initialized node assumes it is operating on the
    /// full network until the partition detection layer reports
    /// otherwise. This matches the invariant that unaware callers
    /// (e.g. tests that construct a `Mempool` without an orchestrator)
    /// behave as they did before partition-mode gating existed.
    fn default() -> Self {
        Self::Normal
    }
}

// ---------------------------------------------------------------------------
// Decision functions
// ---------------------------------------------------------------------------

/// Classify the local partition by stake fraction.
///
/// `local_stake` is the sum of stake held by validators in the local
/// partition. `total_stake` is the full validator set stake (including
/// validators not currently reachable). Returns the classification
/// described by [`PartitionQuorumStatus`].
///
/// A `total_stake` of 0 returns [`PartitionQuorumStatus::Supermajority`]
/// on the theory that an empty validator set means "no one can veto
/// anything" — this is an edge case for tests and genesis bootstrapping
/// and should not occur on a live network.
pub fn classify_partition(local_stake: u128, total_stake: u128) -> PartitionQuorumStatus {
    if total_stake == 0 {
        return PartitionQuorumStatus::Supermajority;
    }

    // Supermajority: local * 3 >= total * 2  (i.e. local >= 2/3 * total)
    // Use u128 to avoid overflow for large stake totals.
    if local_stake.saturating_mul(3) >= total_stake.saturating_mul(2) {
        PartitionQuorumStatus::Supermajority
    // Minority: local * 3 <= total  (i.e. local <= 1/3 * total)
    } else if local_stake.saturating_mul(3) <= total_stake {
        PartitionQuorumStatus::Minority
    } else {
        PartitionQuorumStatus::Middle
    }
}

/// Map a partition classification to the operating mode a node should
/// assume.
pub fn mode_for_status(status: PartitionQuorumStatus) -> PartitionMode {
    match status {
        PartitionQuorumStatus::Supermajority => PartitionMode::Normal,
        PartitionQuorumStatus::Middle => PartitionMode::BlockProductionOnly,
        PartitionQuorumStatus::Minority => PartitionMode::ReadOnly,
    }
}

/// Convenience wrapper: classify and map in one call.
///
/// ```
/// use wrfcoin_shared_protocol::consensus::partition_policy::{
///     partition_mode, PartitionMode,
/// };
///
/// // 80% of stake → supermajority → normal operation
/// assert_eq!(partition_mode(800, 1000), PartitionMode::Normal);
///
/// // 50% of stake → middle → produce without finality
/// assert_eq!(partition_mode(500, 1000), PartitionMode::BlockProductionOnly);
///
/// // 20% of stake → minority → read-only
/// assert_eq!(partition_mode(200, 1000), PartitionMode::ReadOnly);
/// ```
pub fn partition_mode(local_stake: u128, total_stake: u128) -> PartitionMode {
    mode_for_status(classify_partition(local_stake, total_stake))
}

impl PartitionMode {
    /// Whether a node in this mode may propose or append new blocks.
    pub fn allows_block_production(&self) -> bool {
        matches!(self, Self::Normal | Self::BlockProductionOnly)
    }

    /// Whether a node in this mode may cast BFT finality votes.
    ///
    /// Only `Normal` allows finality because `BlockProductionOnly`
    /// partitions cannot reach the 2/3+ threshold and voting on blocks
    /// that will be reverted would create inconsistent local finality
    /// state.
    pub fn allows_finality_voting(&self) -> bool {
        matches!(self, Self::Normal)
    }

    /// Whether a node in this mode accepts new mempool transactions.
    ///
    /// `ReadOnly` refuses writes to prevent user-visible state that
    /// will be rolled back on reconnection.
    pub fn accepts_mempool_writes(&self) -> bool {
        !matches!(self, Self::ReadOnly)
    }

    /// Whether read-only RPC queries are served.
    ///
    /// All modes serve queries; this accessor exists for symmetry with
    /// the write accessors and as a defensive guard for code paths that
    /// may want to refuse queries during emergency degradation.
    pub fn serves_queries(&self) -> bool {
        true
    }

    /// Short lowercase label for metrics and logs.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::BlockProductionOnly => "block_production_only",
            Self::ReadOnly => "read_only",
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── classify_partition boundaries ─────────────────────────────────

    #[test]
    fn exactly_two_thirds_is_supermajority() {
        // 2/3 should be the minimum supermajority (inclusive lower bound).
        assert_eq!(
            classify_partition(666, 999),
            PartitionQuorumStatus::Supermajority
        );
        assert_eq!(
            classify_partition(2, 3),
            PartitionQuorumStatus::Supermajority
        );
    }

    #[test]
    fn just_below_two_thirds_is_middle() {
        // 665/999 < 2/3
        assert_eq!(classify_partition(665, 999), PartitionQuorumStatus::Middle);
    }

    #[test]
    fn exactly_one_third_is_minority() {
        // local * 3 == total → local == total / 3 → minority (inclusive upper bound)
        assert_eq!(classify_partition(333, 999), PartitionQuorumStatus::Minority);
        assert_eq!(classify_partition(1, 3), PartitionQuorumStatus::Minority);
    }

    #[test]
    fn just_above_one_third_is_middle() {
        // 334/999 > 1/3
        assert_eq!(classify_partition(334, 999), PartitionQuorumStatus::Middle);
    }

    #[test]
    fn zero_local_stake_is_minority() {
        assert_eq!(classify_partition(0, 1000), PartitionQuorumStatus::Minority);
    }

    #[test]
    fn full_stake_is_supermajority() {
        assert_eq!(
            classify_partition(1000, 1000),
            PartitionQuorumStatus::Supermajority
        );
    }

    #[test]
    fn empty_validator_set_is_supermajority() {
        // Edge case: genesis or empty set — no quorum to fail.
        assert_eq!(classify_partition(0, 0), PartitionQuorumStatus::Supermajority);
    }

    #[test]
    fn classify_handles_large_stake_without_overflow() {
        // u128::MAX / 3 is near u128::MAX; verify no overflow panic.
        let huge = u128::MAX / 3;
        // local = huge, total = huge → 100% → Supermajority
        assert_eq!(
            classify_partition(huge, huge),
            PartitionQuorumStatus::Supermajority
        );
    }

    // ── mode_for_status / partition_mode ──────────────────────────────

    #[test]
    fn supermajority_maps_to_normal() {
        assert_eq!(
            mode_for_status(PartitionQuorumStatus::Supermajority),
            PartitionMode::Normal
        );
    }

    #[test]
    fn middle_maps_to_block_production_only() {
        assert_eq!(
            mode_for_status(PartitionQuorumStatus::Middle),
            PartitionMode::BlockProductionOnly
        );
    }

    #[test]
    fn minority_maps_to_read_only() {
        assert_eq!(
            mode_for_status(PartitionQuorumStatus::Minority),
            PartitionMode::ReadOnly
        );
    }

    #[test]
    fn partition_mode_wrapper_combines_both_steps() {
        assert_eq!(partition_mode(900, 1000), PartitionMode::Normal);
        assert_eq!(partition_mode(500, 1000), PartitionMode::BlockProductionOnly);
        assert_eq!(partition_mode(100, 1000), PartitionMode::ReadOnly);
    }

    // ── mode capability accessors ─────────────────────────────────────

    #[test]
    fn normal_mode_allows_everything() {
        let m = PartitionMode::Normal;
        assert!(m.allows_block_production());
        assert!(m.allows_finality_voting());
        assert!(m.accepts_mempool_writes());
        assert!(m.serves_queries());
    }

    #[test]
    fn block_production_only_refuses_finality_voting() {
        let m = PartitionMode::BlockProductionOnly;
        assert!(m.allows_block_production());
        assert!(
            !m.allows_finality_voting(),
            "middle partition can't reach 2/3 quorum — voting would create \
             inconsistent local finality state"
        );
        assert!(m.accepts_mempool_writes());
        assert!(m.serves_queries());
    }

    #[test]
    fn read_only_refuses_writes_and_production() {
        let m = PartitionMode::ReadOnly;
        assert!(
            !m.allows_block_production(),
            "minority must not produce blocks that will be reverted"
        );
        assert!(!m.allows_finality_voting());
        assert!(!m.accepts_mempool_writes());
        assert!(
            m.serves_queries(),
            "read-only still serves queries — that's the whole point"
        );
    }

    // ── Default impl ──────────────────────────────────────────────────

    #[test]
    fn default_partition_mode_is_normal() {
        // Pre-existing callers that construct a Mempool without wiring
        // up the partition orchestrator rely on this default to keep
        // behaving as they did before partition-mode gating existed.
        assert_eq!(PartitionMode::default(), PartitionMode::Normal);
    }

    #[test]
    fn mode_label_covers_all_variants() {
        assert_eq!(PartitionMode::Normal.label(), "normal");
        assert_eq!(
            PartitionMode::BlockProductionOnly.label(),
            "block_production_only"
        );
        assert_eq!(PartitionMode::ReadOnly.label(), "read_only");
    }

    // ── serde round trip ──────────────────────────────────────────────

    #[test]
    fn status_and_mode_serialize_to_json() {
        let status = PartitionQuorumStatus::Middle;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"Middle\"");
        let recovered: PartitionQuorumStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered, status);

        let mode = PartitionMode::ReadOnly;
        let json = serde_json::to_string(&mode).unwrap();
        assert_eq!(json, "\"ReadOnly\"");
        let recovered: PartitionMode = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered, mode);
    }
}
