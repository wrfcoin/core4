//! INV-FINALITY-1 — no conflicting finalization.
//!
//! The safety property of a BFT chain, stated as code rather than prose:
//!
//! > Two honest nodes must never finalize conflicting blocks.
//!
//! Locally that reduces to: **this node must never finalize two different
//! block hashes at the same height**, and must never finalize a *new* block
//! at a height below the finality frontier it has already established.
//!
//! # Relationship to the existing guard in `finality_events.rs`
//!
//! The BFT finality consumer already cross-checks an incoming
//! [`FinalitySignal`](crate::consensus::finality::FinalitySignal) against the
//! block currently stored locally at that height. That guard is necessary but
//! not sufficient: it compares against *present* local state, which a reorg
//! can change underneath it. It therefore cannot detect that height `N` was
//! finalized as hash `A` ten minutes ago and is being finalized as hash `B`
//! now — by then the local block at `N` may legitimately be `B`.
//!
//! [`FinalityInvariant`] closes that gap by remembering every finalization it
//! has ever observed. It is the only component that can answer "have we
//! already finalized something else here?".
//!
//! # Why safety violations are sticky
//!
//! A safety violation is not recoverable by subsequent good behaviour. Once
//! two conflicting finalizations have been observed, the chain has forked
//! under finality and no later well-formed observation makes that untrue. So
//! the first one is latched and [`FinalityInvariant::check`] keeps reporting
//! it forever. This is what lets a caller that only polls `check()` — such as
//! the invariant harness — see a violation that happened between polls.
//!
//! Ordering anomalies are **not** latched. See
//! [`FinalityViolation::is_safety_violation`]: this channel is documented to
//! deliver out of order, so a below-frontier finalization is expected traffic,
//! and latching it would make `check()` permanently red on a healthy node.

use core::fmt;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use super::StatefulInvariant;

/// A finalized block identity, as carried by
/// [`FinalitySignal::block_hash`](crate::consensus::finality::FinalitySignal::block_hash).
///
/// Deliberately the raw 32-byte form rather than
/// [`Hash`](crate::crypto::hash::Hash): the finalization path speaks
/// `[u8; 32]`, and an invariant that needs a conversion at its only call site
/// is an invariant that will be called with the wrong thing eventually.
pub type BlockHash = [u8; 32];

/// A detected breach of INV-FINALITY-1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinalityViolation {
    /// Two different block hashes were finalized at the same height. This is
    /// the safety violation proper: the chain has forked below finality.
    ConflictingFinalization {
        height: u64,
        seen: BlockHash,
        incoming: BlockHash,
    },
    /// A *new* height below the established finality frontier was finalized.
    ///
    /// An **ordering anomaly, not a safety violation** — see
    /// [`FinalityViolation::is_safety_violation`]. Finalizing an ancestor of
    /// an already-finalized block is implied and harmless; the finality
    /// channel is explicitly documented to deliver out of order, and the
    /// chain layer clamps monotonically. Reported so callers can observe it,
    /// not so they can halt on it.
    NonMonotonicHeight { last: u64, incoming: u64 },
}

impl FinalityViolation {
    /// Whether this breach means the chain has actually forked under finality.
    ///
    /// Only [`ConflictingFinalization`](FinalityViolation::ConflictingFinalization)
    /// qualifies. A safety violation is unrecoverable and is cause to halt;
    /// [`NonMonotonicHeight`](FinalityViolation::NonMonotonicHeight) is an
    /// out-of-order delivery that the surrounding plumbing already handles, so
    /// halting on it would take down healthy nodes during normal catch-up.
    ///
    /// This distinction is why only safety violations are latched by
    /// [`FinalityInvariant::check`].
    pub fn is_safety_violation(&self) -> bool {
        matches!(self, FinalityViolation::ConflictingFinalization { .. })
    }
}

impl fmt::Display for FinalityViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FinalityViolation::ConflictingFinalization {
                height,
                seen,
                incoming,
            } => write!(
                f,
                "INV-FINALITY-1 violated: conflicting finalization at height {height} — \
                 already finalized {}, now asked to finalize {}",
                hex_32(seen),
                hex_32(incoming),
            ),
            FinalityViolation::NonMonotonicHeight { last, incoming } => write!(
                f,
                "INV-FINALITY-1 violated: non-monotonic finalization — highest finalized \
                 height is {last}, but a new block was finalized at {incoming}",
            ),
        }
    }
}

impl std::error::Error for FinalityViolation {}

/// Lower-case hex of a 32-byte hash, without pulling in a hex dependency at
/// this layer.
fn hex_32(bytes: &BlockHash) -> String {
    let mut s = String::with_capacity(64);
    for b in bytes {
        s.push(char::from_digit((b >> 4) as u32, 16).unwrap_or('?'));
        s.push(char::from_digit((b & 0x0f) as u32, 16).unwrap_or('?'));
    }
    s
}

/// Tracks every finalization this node has observed and rejects conflicts.
///
/// Cheap to keep for the lifetime of a process: one `u64` + 32 bytes per
/// finalized height.
#[derive(Debug, Clone, Default)]
pub struct FinalityInvariant {
    finalized: BTreeMap<u64, BlockHash>,
    /// First violation ever observed. Latched — see the module docs.
    violation: Option<FinalityViolation>,
}

impl FinalityInvariant {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that `hash` was finalized at `height`.
    ///
    /// Returns `Err` if this observation breaches INV-FINALITY-1. Callers must
    /// consult [`FinalityViolation::is_safety_violation`] before reacting: a
    /// conflicting finalization is fatal and is latched into
    /// [`check`](Self::check), while a below-frontier height is an ordering
    /// anomaly that is reported but neither latched nor cause to halt.
    ///
    /// The observation is recorded either way — including a below-frontier
    /// one — so that a genuinely conflicting finalization arriving at that
    /// height later is still caught.
    ///
    /// # Accepted, deliberately
    ///
    /// Re-observing an **identical** `(height, hash)` pair is `Ok`. The
    /// finality plumbing explicitly tolerates duplicate and out-of-order
    /// delivery — see the "Stale-finality ordering" section of
    /// [`crate::consensus::finality`] — so a replayed signal must not be
    /// mistaken for a fork. Only a *new* claim about an already-finalized
    /// height is a violation.
    pub fn observe_finalized(
        &mut self,
        height: u64,
        hash: BlockHash,
    ) -> Result<(), FinalityViolation> {
        if let Some(seen) = self.finalized.get(&height) {
            if *seen == hash {
                // Idempotent replay of a signal we already accepted.
                return Ok(());
            }
            return Err(self.latch(FinalityViolation::ConflictingFinalization {
                height,
                seen: *seen,
                incoming: hash,
            }));
        }

        // A height we have not finalized before. Record it either way — a
        // below-frontier height must still be remembered, or a genuinely
        // conflicting finalization arriving there later would go unnoticed.
        let below_frontier = self
            .finalized
            .keys()
            .next_back()
            .copied()
            .filter(|&last| height < last);

        self.finalized.insert(height, hash);

        match below_frontier {
            Some(last) => Err(FinalityViolation::NonMonotonicHeight {
                last,
                incoming: height,
            }),
            None => Ok(()),
        }
    }

    /// Report the latched **safety** violation, if any.
    ///
    /// Ordering anomalies are not latched — only a genuine
    /// [`ConflictingFinalization`](FinalityViolation::ConflictingFinalization)
    /// makes this return `Err`, and once it does it always will.
    ///
    /// Pure — safe to call from a polling harness.
    pub fn check(&self) -> Result<(), FinalityViolation> {
        match &self.violation {
            Some(v) => Err(v.clone()),
            None => Ok(()),
        }
    }

    /// Highest height finalized so far.
    pub fn highest_finalized(&self) -> Option<u64> {
        self.finalized.keys().next_back().copied()
    }

    /// Hash finalized at `height`, if any.
    pub fn finalized_at(&self, height: u64) -> Option<BlockHash> {
        self.finalized.get(&height).copied()
    }

    /// Number of distinct heights finalized.
    pub fn len(&self) -> usize {
        self.finalized.len()
    }

    pub fn is_empty(&self) -> bool {
        self.finalized.is_empty()
    }

    fn latch(&mut self, violation: FinalityViolation) -> FinalityViolation {
        if self.violation.is_none() {
            self.violation = Some(violation.clone());
        }
        violation
    }
}

/// Shareable [`FinalityInvariant`], so one instance can be **fed** at the
/// finalization event site and **polled** by a
/// [`StatefulInvariantRegistry`](super::StatefulInvariantRegistry).
///
/// This is the pattern every stateful invariant should follow. The tracker
/// itself needs `&mut` to record an observation, but a registry can only hold
/// shared references — so the mutability lives behind a `Mutex` and every
/// method takes `&self`.
///
/// A `std::sync::Mutex` rather than a `tokio` one on purpose: the critical
/// section is a `BTreeMap` lookup and insert with no `.await` inside, so an
/// async mutex would add a scheduler round-trip and a runtime dependency for
/// no benefit. Lock poisoning is recovered from rather than propagated — a
/// panic elsewhere must not silently disable the safety check.
#[derive(Clone, Default)]
pub struct SharedFinalityInvariant {
    inner: Arc<Mutex<FinalityInvariant>>,
}

impl SharedFinalityInvariant {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a finalization. See [`FinalityInvariant::observe_finalized`].
    pub fn observe_finalized(&self, height: u64, hash: BlockHash) -> Result<(), FinalityViolation> {
        self.locked().observe_finalized(height, hash)
    }

    /// Latched safety violation, if any.
    pub fn violation(&self) -> Option<FinalityViolation> {
        self.locked().check().err()
    }

    pub fn highest_finalized(&self) -> Option<u64> {
        self.locked().highest_finalized()
    }

    pub fn finalized_at(&self, height: u64) -> Option<BlockHash> {
        self.locked().finalized_at(height)
    }

    /// Recover from poisoning instead of propagating it. A panic in an
    /// unrelated task must not take the finality safety check offline.
    fn locked(&self) -> std::sync::MutexGuard<'_, FinalityInvariant> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl StatefulInvariant for SharedFinalityInvariant {
    fn id(&self) -> &'static str {
        "INV-FINALITY-1: No Conflicting Finalization"
    }

    fn check(&self) -> Result<(), String> {
        self.locked().check().map_err(|v| v.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(byte: u8) -> BlockHash {
        [byte; 32]
    }

    /// duplicate-ok: re-observing the same (height, hash) is not a violation.
    #[test]
    fn duplicate_ok() {
        let mut inv = FinalityInvariant::new();
        inv.observe_finalized(10, hash(0xaa))
            .expect("first observation");
        inv.observe_finalized(10, hash(0xaa))
            .expect("identical replay must be accepted");
        inv.observe_finalized(10, hash(0xaa))
            .expect("repeated replay must stay accepted");

        assert!(inv.check().is_ok(), "duplicates must not latch a violation");
        assert_eq!(inv.len(), 1);
    }

    /// conflict: two distinct hashes at one height.
    #[test]
    fn conflict() {
        let mut inv = FinalityInvariant::new();
        inv.observe_finalized(7, hash(0x01))
            .expect("first observation");

        let err = inv
            .observe_finalized(7, hash(0x02))
            .expect_err("distinct hash at a finalized height must be rejected");

        assert_eq!(
            err,
            FinalityViolation::ConflictingFinalization {
                height: 7,
                seen: hash(0x01),
                incoming: hash(0x02),
            }
        );
        assert!(err.is_safety_violation(), "a fork under finality is fatal");

        // Latched: still reported later, and not cleared by good observations.
        assert!(inv.check().is_err(), "violation must latch");
        inv.observe_finalized(8, hash(0x03))
            .expect("later well-formed finalization still records");
        assert!(
            inv.check().is_err(),
            "a safety violation is not recoverable by subsequent good behaviour"
        );

        // The original finalization is retained, not overwritten by the conflict.
        assert_eq!(inv.finalized_at(7), Some(hash(0x01)));
    }

    /// non-monotonic: a NEW height finalized below the frontier is reported…
    #[test]
    fn non_monotonic() {
        let mut inv = FinalityInvariant::new();
        inv.observe_finalized(100, hash(0x10)).expect("frontier");

        let err = inv
            .observe_finalized(99, hash(0x11))
            .expect_err("new finalization below the frontier must be reported");

        assert_eq!(
            err,
            FinalityViolation::NonMonotonicHeight {
                last: 100,
                incoming: 99,
            }
        );
    }

    /// …but it is an ordering anomaly, not a safety violation, so it must not
    /// latch. Halting on this would take down healthy nodes during the
    /// out-of-order delivery that `consensus::finality` documents as normal.
    #[test]
    fn non_monotonic_is_not_a_safety_violation_and_does_not_latch() {
        let mut inv = FinalityInvariant::new();
        inv.observe_finalized(100, hash(0x10)).expect("frontier");

        let err = inv.observe_finalized(99, hash(0x11)).expect_err("reported");
        assert!(
            !err.is_safety_violation(),
            "out-of-order finality is not a fork"
        );
        assert!(
            inv.check().is_ok(),
            "an ordering anomaly must not latch a violation"
        );
    }

    /// A below-frontier height is still *recorded*, so a genuinely conflicting
    /// finalization arriving there later is still caught. Dropping it would
    /// open a hole exactly where the ordering is already suspect.
    #[test]
    fn below_frontier_height_is_recorded_so_later_conflicts_are_caught() {
        let mut inv = FinalityInvariant::new();
        inv.observe_finalized(100, hash(0x10)).expect("frontier");
        inv.observe_finalized(99, hash(0x11))
            .expect_err("ordering anomaly reported");

        assert_eq!(
            inv.finalized_at(99),
            Some(hash(0x11)),
            "the observation must still be remembered"
        );

        let err = inv
            .observe_finalized(99, hash(0x12))
            .expect_err("a different hash at 99 is a real conflict");
        assert!(err.is_safety_violation());
        assert!(inv.check().is_err(), "the conflict must latch");
    }

    /// A stale *replay* below the frontier is explicitly not a violation —
    /// the finality channel is documented to deliver out of order.
    #[test]
    fn stale_replay_below_frontier_is_not_a_violation() {
        let mut inv = FinalityInvariant::new();
        inv.observe_finalized(5, hash(0x05)).expect("h5");
        inv.observe_finalized(6, hash(0x06)).expect("h6");
        inv.observe_finalized(7, hash(0x07)).expect("h7");

        inv.observe_finalized(5, hash(0x05))
            .expect("out-of-order redelivery of an already-accepted signal is normal");

        assert!(inv.check().is_ok());
    }

    /// long-sequence: a long monotonic run stays clean.
    #[test]
    fn long_sequence() {
        let mut inv = FinalityInvariant::new();
        for height in 0..2_000u64 {
            let mut h = [0u8; 32];
            h[..8].copy_from_slice(&height.to_le_bytes());
            inv.observe_finalized(height, h)
                .unwrap_or_else(|e| panic!("height {height} must finalize cleanly: {e}"));
        }

        assert!(inv.check().is_ok());
        assert_eq!(inv.len(), 2_000);
        assert_eq!(inv.highest_finalized(), Some(1_999));
    }

    /// Gaps are legal: finality can skip heights.
    #[test]
    fn sparse_heights_are_accepted() {
        let mut inv = FinalityInvariant::new();
        inv.observe_finalized(1, hash(1)).expect("h1");
        inv.observe_finalized(50, hash(2)).expect("h50");
        inv.observe_finalized(5_000, hash(3)).expect("h5000");

        assert!(inv.check().is_ok());
        assert_eq!(inv.highest_finalized(), Some(5_000));
    }

    /// Re-finalizing the frontier height itself with the same hash is fine.
    #[test]
    fn refinalizing_frontier_is_ok() {
        let mut inv = FinalityInvariant::new();
        inv.observe_finalized(42, hash(0xff)).expect("h42");
        inv.observe_finalized(42, hash(0xff)).expect("same again");
        assert!(inv.check().is_ok());
    }

    #[test]
    fn violation_messages_name_the_invariant_and_the_height() {
        let conflict = FinalityViolation::ConflictingFinalization {
            height: 12,
            seen: hash(0xab),
            incoming: hash(0xcd),
        };
        let text = conflict.to_string();
        assert!(text.contains("INV-FINALITY-1"), "got: {text}");
        assert!(text.contains("height 12"), "got: {text}");
        assert!(text.contains(&"ab".repeat(32)), "got: {text}");

        let non_mono = FinalityViolation::NonMonotonicHeight {
            last: 9,
            incoming: 4,
        };
        let text = non_mono.to_string();
        assert!(text.contains("INV-FINALITY-1"), "got: {text}");
        assert!(text.contains('9') && text.contains('4'), "got: {text}");
    }

    #[test]
    fn fresh_invariant_is_clean_and_empty() {
        let inv = FinalityInvariant::new();
        assert!(inv.check().is_ok());
        assert!(inv.is_empty());
        assert_eq!(inv.highest_finalized(), None);
    }

    #[test]
    fn shared_handle_feeds_and_polls_the_same_instance() {
        let shared = SharedFinalityInvariant::new();
        let feeder = shared.clone();

        feeder.observe_finalized(1, hash(0xa1)).expect("h1");
        assert_eq!(shared.highest_finalized(), Some(1));
        assert!(shared.check().is_ok());

        feeder
            .observe_finalized(1, hash(0xa2))
            .expect_err("conflict at h1");

        // Polled through the trait, from the other clone.
        let err = shared.check().expect_err("the poll must see the violation");
        assert!(err.contains("INV-FINALITY-1"), "got: {err}");
        assert!(shared.violation().is_some());
    }

    #[test]
    fn shared_handle_registers_and_is_polled_by_the_registry() {
        use super::super::StatefulInvariantRegistry;

        let shared = SharedFinalityInvariant::new();
        let registry = StatefulInvariantRegistry::new().with(std::sync::Arc::new(shared.clone()));
        registry.assert_unique_ids().expect("unique");
        assert_eq!(registry.len(), 1);
        assert_eq!(registry.check_all(), Ok(()));

        shared.observe_finalized(5, hash(0x01)).expect("h5");
        shared
            .observe_finalized(5, hash(0x02))
            .expect_err("conflict");

        let violations = registry
            .check_all()
            .expect_err("the registry must surface the violation");
        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].0,
            "INV-FINALITY-1: No Conflicting Finalization"
        );
    }

    #[test]
    fn registry_reports_every_violation_not_just_the_first() {
        use super::super::{StatefulInvariant, StatefulInvariantRegistry};

        struct AlwaysBroken(&'static str);
        impl StatefulInvariant for AlwaysBroken {
            fn id(&self) -> &'static str {
                self.0
            }
            fn check(&self) -> Result<(), String> {
                Err("broken".to_string())
            }
        }

        let registry = StatefulInvariantRegistry::new()
            .with(std::sync::Arc::new(AlwaysBroken("INV-A-1")))
            .with(std::sync::Arc::new(AlwaysBroken("INV-B-1")));

        let violations = registry.check_all().expect_err("both must be reported");
        assert_eq!(violations.len(), 2, "got {violations:?}");
    }

    #[test]
    fn registry_rejects_duplicate_ids() {
        use super::super::StatefulInvariantRegistry;
        let registry = StatefulInvariantRegistry::new()
            .with(std::sync::Arc::new(SharedFinalityInvariant::new()))
            .with(std::sync::Arc::new(SharedFinalityInvariant::new()));
        assert!(registry.assert_unique_ids().is_err());
    }
}
