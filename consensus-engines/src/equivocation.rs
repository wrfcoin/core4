//! Equivocation evidence persistence (#1218).
//!
//! Double-signing evidence was previously kept in an in-memory map on the
//! [`ProofOfStake`](crate::algorithms::ProofOfStake) engine, which meant
//! it was lost on node restart. This module provides a persistent
//! evidence store abstraction so that equivocation proofs can survive
//! restarts and be exported for external audit.
//!
//! The store is trait-based:
//! - [`EvidenceStore`] is the async trait that backends implement
//! - [`InMemoryEvidenceStore`] is the default backend (no persistence,
//!   backwards-compatible with pre-#1218 behavior)
//! - A redb-backed implementation lives in `native-chain` and is wired in
//!   by the consensus adapter.
//!
//! Refs #720 GAP-3, closes #1218.
//!
//! ## Why a trait instead of directly writing to disk?
//!
//! `consensus-engines` is a library crate with no storage dependencies.
//! Pulling in `redb` here would couple every consumer (tests, benchmarks,
//! external embedders) to a specific storage backend. The trait lets
//! production nodes plug in disk persistence while keeping the library
//! lightweight.

use async_trait::async_trait;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

use wrfcoin_shared_protocol::crypto::hash::Hash;

use crate::types::ConsensusResult;

// ---------------------------------------------------------------------------
// Evidence types
// ---------------------------------------------------------------------------

/// Durable proof that a validator signed two different blocks at the same
/// height — the standard PoS "nothing-at-stake" violation.
///
/// Carries enough material that an external auditor (or a rejoining node)
/// can verify the equivocation without trusting the node that reported it:
/// the validator address, the height, and both conflicting block hashes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EquivocationEvidence {
    /// Address of the validator that double-signed.
    pub validator: String,
    /// Block height at which the equivocation occurred.
    pub height: u64,
    /// Hash of the first block observed at this height.
    pub first_block_hash: Hash,
    /// Hash of the second (conflicting) block observed at this height.
    pub second_block_hash: Hash,
    /// Unix timestamp (seconds) when the equivocation was detected.
    ///
    /// This is wall-clock time on the detecting node, not a consensus
    /// timestamp. It is advisory only — the proof itself is complete
    /// without it.
    pub detected_at: u64,
}

impl EquivocationEvidence {
    /// Deterministic key for the evidence, suitable for use as a primary
    /// key in a key-value store. Format: `"{validator}:{height:020}"`.
    ///
    /// The zero-padded height means lexicographic and numeric ordering
    /// agree, so range scans over a validator's evidence return results
    /// in height order without extra sorting.
    pub fn storage_key(&self) -> String {
        format!("{}:{:020}", self.validator, self.height)
    }
}

// ---------------------------------------------------------------------------
// Store trait
// ---------------------------------------------------------------------------

/// Persistent backend for equivocation evidence.
///
/// Implementations must be `Send + Sync` and may block on disk I/O inside
/// the async methods (the caller is responsible for using
/// `tokio::task::spawn_blocking` if appropriate).
#[async_trait]
pub trait EvidenceStore: Send + Sync + std::fmt::Debug {
    /// Persist a new piece of evidence. Idempotent: persisting the same
    /// evidence twice must succeed without error and must not create
    /// duplicates.
    async fn persist(&self, evidence: EquivocationEvidence) -> ConsensusResult<()>;

    /// Load all evidence for a given validator, sorted by height ascending.
    async fn load_for_validator(&self, validator: &str) -> ConsensusResult<Vec<EquivocationEvidence>>;

    /// Total count of evidence records across all validators.
    async fn count(&self) -> ConsensusResult<usize>;

    /// List every evidence record in the store whose height is strictly
    /// greater than the per-validator watermark carried by `cursor`. A
    /// validator not present in the cursor is treated as having a
    /// watermark of `0` — i.e. all of its records are returned.
    ///
    /// Used by the on-chain slashing submitter (#1393) to scan for new
    /// evidence since the last persisted cursor and turn it into
    /// `SlashingReport` transactions.
    ///
    /// Per-validator watermarks (vs. the earlier single-string cursor)
    /// are needed because a naïve `storage_key > cursor` compare drops
    /// evidence whose validator id sorts *before* the validator the
    /// cursor last advanced past, even when the dropped record has a
    /// *later* height. Example: cursor `"v_xyz:…100"`, new evidence
    /// `"v_abc:…200"` — lexicographic compare says the new record is
    /// "older" than the cursor and silently skips it. A per-validator
    /// watermark cannot have that failure mode: every validator's cursor
    /// advances independently.
    ///
    /// Default implementation returns an empty vec so existing backends
    /// that do not need global iteration are not forced to implement it;
    /// real backends (in-memory + redb) override it below.
    async fn list_since(
        &self,
        _cursor: &SubmitterCursor,
    ) -> ConsensusResult<Vec<EquivocationEvidence>> {
        Ok(Vec::new())
    }
}

// ---------------------------------------------------------------------------
// Submitter cursor — per-validator height watermark
// ---------------------------------------------------------------------------

/// Per-validator watermark used by the slashing submitter to remember
/// which equivocation records have already been turned into
/// `SlashingReport` transactions.
///
/// The earlier design stored a single `"{validator}:{height}"` string
/// and compared candidates lexicographically. That drops evidence for
/// validators whose ids sort before the cursor's validator, even when
/// the new evidence has a strictly larger height — validator ids are
/// arbitrary strings, so the lexicographic order of `"v_abc"` and
/// `"v_xyz"` has nothing to do with which evidence is "newer". The
/// per-validator map removes that coupling: each validator carries its
/// own independent height watermark.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SubmitterCursor {
    heights: FxHashMap<String, u64>,
}

impl SubmitterCursor {
    /// New empty cursor — no validator has a watermark, so every stored
    /// record is treated as un-processed on the next `list_since` call.
    pub fn new() -> Self {
        Self::default()
    }

    /// Highest height already processed for `validator`, or `None` if
    /// the validator has never been observed.
    pub fn get(&self, validator: &str) -> Option<u64> {
        self.heights.get(validator).copied()
    }

    /// Advance `validator`'s watermark to `height` if (a) the validator
    /// has no previous watermark, or (b) `height` is strictly greater
    /// than the current watermark. Older or equal heights are ignored so
    /// out-of-order submitter iteration cannot regress a watermark.
    pub fn advance(&mut self, validator: &str, height: u64) {
        use std::collections::hash_map::Entry;
        match self.heights.entry(validator.to_string()) {
            Entry::Vacant(v) => {
                v.insert(height);
            },
            Entry::Occupied(mut o) => {
                if height > *o.get() {
                    o.insert(height);
                }
            },
        }
    }

    /// Returns true if every validator mentioned in `other` has a
    /// watermark at least as advanced in `self`. Used by tests to
    /// assert cursor monotonicity.
    pub fn covers(&self, other: &SubmitterCursor) -> bool {
        other
            .heights
            .iter()
            .all(|(v, h)| self.heights.get(v).copied().unwrap_or(0) >= *h)
    }

    /// Number of validators with a recorded watermark.
    pub fn len(&self) -> usize {
        self.heights.len()
    }

    /// True iff no validator has a watermark.
    pub fn is_empty(&self) -> bool {
        self.heights.is_empty()
    }

    /// Returns `true` iff this cursor's watermark for `evidence.validator`
    /// is at least `evidence.height`, i.e. the submitter has already
    /// processed this record. Filter used by `list_since`.
    pub fn has_processed(&self, evidence: &EquivocationEvidence) -> bool {
        self.heights
            .get(&evidence.validator)
            .copied()
            .is_some_and(|last| last >= evidence.height)
    }
}

// ---------------------------------------------------------------------------
// In-memory default backend
// ---------------------------------------------------------------------------

/// Non-persistent backend. Stores evidence in an `FxHashMap` keyed by
/// `storage_key()`. Used as the default when no disk-backed store is
/// configured, preserving pre-#1218 behavior for callers that do not
/// opt in to persistence.
#[derive(Debug, Default, Clone)]
pub struct InMemoryEvidenceStore {
    records: Arc<RwLock<FxHashMap<String, EquivocationEvidence>>>,
}

impl InMemoryEvidenceStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl EvidenceStore for InMemoryEvidenceStore {
    async fn persist(&self, evidence: EquivocationEvidence) -> ConsensusResult<()> {
        let mut guard = self.records.write().await;
        // Idempotent: overwrite with the same key is a no-op.
        guard.insert(evidence.storage_key(), evidence);
        Ok(())
    }

    async fn load_for_validator(&self, validator: &str) -> ConsensusResult<Vec<EquivocationEvidence>> {
        let guard = self.records.read().await;
        let mut out: Vec<EquivocationEvidence> = guard
            .values()
            .filter(|ev| ev.validator == validator)
            .cloned()
            .collect();
        out.sort_by_key(|ev| ev.height);
        Ok(out)
    }

    async fn count(&self) -> ConsensusResult<usize> {
        Ok(self.records.read().await.len())
    }

    async fn list_since(
        &self,
        cursor: &SubmitterCursor,
    ) -> ConsensusResult<Vec<EquivocationEvidence>> {
        let guard = self.records.read().await;
        let mut out: Vec<EquivocationEvidence> = guard
            .values()
            .filter(|ev| !cursor.has_processed(ev))
            .cloned()
            .collect();
        out.sort_by(|a, b| a.storage_key().cmp(&b.storage_key()));
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_evidence(validator: &str, height: u64, tag: u8) -> EquivocationEvidence {
        let mut first = [0u8; 32];
        first[0] = tag;
        let mut second = [0u8; 32];
        second[0] = tag ^ 0xFF;
        EquivocationEvidence {
            validator: validator.to_string(),
            height,
            first_block_hash: Hash::from(first),
            second_block_hash: Hash::from(second),
            detected_at: 1_700_000_000 + height,
        }
    }

    #[test]
    fn storage_key_is_deterministic_and_height_padded() {
        let ev = sample_evidence("alice", 42, 1);
        assert_eq!(ev.storage_key(), "alice:00000000000000000042");
    }

    #[test]
    fn storage_key_orders_by_height_lexically() {
        let low = sample_evidence("bob", 5, 1).storage_key();
        let high = sample_evidence("bob", 100, 1).storage_key();
        assert!(
            low < high,
            "zero-padded height means lexicographic order == numeric order"
        );
    }

    #[tokio::test]
    async fn in_memory_store_persists_and_loads() {
        let store = InMemoryEvidenceStore::new();
        let ev = sample_evidence("alice", 10, 1);
        store.persist(ev.clone()).await.unwrap();

        let loaded = store.load_for_validator("alice").await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0], ev);
    }

    #[tokio::test]
    async fn in_memory_store_persist_is_idempotent() {
        let store = InMemoryEvidenceStore::new();
        let ev = sample_evidence("alice", 10, 1);
        store.persist(ev.clone()).await.unwrap();
        store.persist(ev.clone()).await.unwrap();
        store.persist(ev).await.unwrap();
        assert_eq!(store.count().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn in_memory_store_isolates_by_validator() {
        let store = InMemoryEvidenceStore::new();
        store.persist(sample_evidence("alice", 1, 1)).await.unwrap();
        store.persist(sample_evidence("alice", 2, 2)).await.unwrap();
        store.persist(sample_evidence("bob", 1, 3)).await.unwrap();

        let alice_evidence = store.load_for_validator("alice").await.unwrap();
        assert_eq!(alice_evidence.len(), 2);
        assert_eq!(alice_evidence[0].height, 1);
        assert_eq!(alice_evidence[1].height, 2);

        let bob_evidence = store.load_for_validator("bob").await.unwrap();
        assert_eq!(bob_evidence.len(), 1);
        assert_eq!(bob_evidence[0].validator, "bob");
    }

    #[tokio::test]
    async fn in_memory_store_load_returns_sorted_by_height() {
        let store = InMemoryEvidenceStore::new();
        // Insert out of order.
        store.persist(sample_evidence("alice", 100, 1)).await.unwrap();
        store.persist(sample_evidence("alice", 5, 2)).await.unwrap();
        store.persist(sample_evidence("alice", 50, 3)).await.unwrap();

        let loaded = store.load_for_validator("alice").await.unwrap();
        let heights: Vec<u64> = loaded.iter().map(|e| e.height).collect();
        assert_eq!(heights, vec![5, 50, 100]);
    }

    #[tokio::test]
    async fn in_memory_store_count_across_validators() {
        let store = InMemoryEvidenceStore::new();
        store.persist(sample_evidence("alice", 1, 1)).await.unwrap();
        store.persist(sample_evidence("bob", 2, 2)).await.unwrap();
        store.persist(sample_evidence("carol", 3, 3)).await.unwrap();
        assert_eq!(store.count().await.unwrap(), 3);
    }

    #[test]
    fn evidence_round_trips_through_json() {
        let ev = sample_evidence("alice", 42, 7);
        let json = serde_json::to_string(&ev).unwrap();
        let recovered: EquivocationEvidence = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered, ev);
    }

    // ── #1393 Blocker 2 — per-validator cursor must not drop evidence ──
    //
    // The earlier `list_since` compared a single concatenated-string
    // cursor lexicographically, so evidence from a validator whose id
    // sorted before the cursor's validator was dropped even when the new
    // record had a strictly larger height. These tests pin the invariant
    // that no un-processed evidence is ever silently skipped just because
    // a different validator's cursor advanced past it.
    #[tokio::test]
    async fn list_since_emits_evidence_from_earlier_sorted_validator() {
        let store = InMemoryEvidenceStore::new();
        // Populate v_xyz at height 100 first, then v_abc at height 200.
        // `"v_abc:…200" < "v_xyz:…100"` lexicographically, so the old
        // string-cursor implementation dropped v_abc's evidence.
        store
            .persist(sample_evidence("v_xyz", 100, 1))
            .await
            .unwrap();
        store
            .persist(sample_evidence("v_abc", 200, 2))
            .await
            .unwrap();

        // Advance cursor as if the submitter already queued v_xyz@100.
        let mut cursor = SubmitterCursor::new();
        cursor.advance("v_xyz", 100);

        let out = store.list_since(&cursor).await.unwrap();
        let observed: Vec<(String, u64)> =
            out.iter().map(|ev| (ev.validator.clone(), ev.height)).collect();

        assert!(
            observed.contains(&("v_abc".to_string(), 200)),
            "v_abc@200 must be emitted even though its id sorts before v_xyz \
             (this was the naive-string-compare regression)"
        );
        assert!(
            !observed.contains(&("v_xyz".to_string(), 100)),
            "already-watermarked v_xyz@100 must not be re-emitted"
        );
    }

    #[tokio::test]
    async fn list_since_respects_per_validator_watermarks_independently() {
        // Build up multi-validator state and verify each validator's
        // watermark is independent: advancing v1's watermark must not
        // hide v2's or v3's un-processed records.
        let store = InMemoryEvidenceStore::new();
        store.persist(sample_evidence("v1", 10, 1)).await.unwrap();
        store.persist(sample_evidence("v1", 20, 2)).await.unwrap();
        store.persist(sample_evidence("v2", 5, 3)).await.unwrap();
        store.persist(sample_evidence("v2", 15, 4)).await.unwrap();
        store.persist(sample_evidence("v3", 50, 5)).await.unwrap();

        // Cursor: v1 processed up to 20, v2 processed up to 5, v3 unseen.
        let mut cursor = SubmitterCursor::new();
        cursor.advance("v1", 20);
        cursor.advance("v2", 5);

        let out = store.list_since(&cursor).await.unwrap();
        let pairs: Vec<(String, u64)> =
            out.iter().map(|ev| (ev.validator.clone(), ev.height)).collect();

        // v1@10 and v1@20 already processed.
        assert!(!pairs.contains(&("v1".to_string(), 10)));
        assert!(!pairs.contains(&("v1".to_string(), 20)));
        // v2@5 already processed, but v2@15 is un-processed.
        assert!(!pairs.contains(&("v2".to_string(), 5)));
        assert!(pairs.contains(&("v2".to_string(), 15)));
        // v3 not mentioned in cursor → all records un-processed.
        assert!(pairs.contains(&("v3".to_string(), 50)));
    }

    #[tokio::test]
    async fn list_since_empty_cursor_returns_everything() {
        let store = InMemoryEvidenceStore::new();
        store.persist(sample_evidence("v1", 10, 1)).await.unwrap();
        store.persist(sample_evidence("v2", 5, 2)).await.unwrap();

        let cursor = SubmitterCursor::new();
        let out = store.list_since(&cursor).await.unwrap();
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn submitter_cursor_advance_is_monotonic() {
        let mut cursor = SubmitterCursor::new();
        cursor.advance("alice", 10);
        cursor.advance("alice", 5); // lower — must not regress
        assert_eq!(cursor.get("alice"), Some(10));
        cursor.advance("alice", 20); // higher — must win
        assert_eq!(cursor.get("alice"), Some(20));
        cursor.advance("alice", 20); // equal — no-op
        assert_eq!(cursor.get("alice"), Some(20));
    }

    #[test]
    fn submitter_cursor_has_processed_matches_watermark() {
        let mut cursor = SubmitterCursor::new();
        cursor.advance("alice", 10);
        assert!(cursor.has_processed(&sample_evidence("alice", 5, 1)));
        assert!(cursor.has_processed(&sample_evidence("alice", 10, 1)));
        assert!(!cursor.has_processed(&sample_evidence("alice", 11, 1)));
        // Different validator → never processed until explicitly advanced.
        assert!(!cursor.has_processed(&sample_evidence("bob", 5, 1)));
    }

    #[test]
    fn submitter_cursor_round_trips_through_json() {
        let mut cursor = SubmitterCursor::new();
        cursor.advance("alice", 10);
        cursor.advance("bob", 42);
        let json = serde_json::to_string(&cursor).unwrap();
        let recovered: SubmitterCursor = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.get("alice"), Some(10));
        assert_eq!(recovered.get("bob"), Some(42));
    }
}
