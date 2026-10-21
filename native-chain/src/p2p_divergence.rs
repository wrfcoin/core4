use std::sync::atomic::{AtomicBool, Ordering};

use wrfcoin_shared_protocol::monitoring::CORE_METRICS;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChainStateView {
    pub height: u64,
    pub tip_hash: [u8; 32],
    pub state_root: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChainDivergenceObservation {
    /// Peer agrees (or is at a different height); divergence flag untouched.
    NoDivergence,
    /// Peer agrees on tip/root at our height AND we were previously latched
    /// diverged, so the latch was cleared. This is the recovery signal.
    Recovered,
    /// Peer sent an all-zero state root but agrees on tip/height. Ambiguous:
    /// neither a divergence nor strong enough evidence to clear a latch.
    OmittedStateRoot,
    /// Peer at our height disagrees on tip or state root; flag is latched.
    Diverged { newly_latched: bool },
    /// `LocallyVerifiedRoot` policy only: a same-height peer agrees on tip and
    /// a non-zero state root and *would* have cleared the latch under the
    /// default `PeerAgreement` policy, but the node is still latched because it
    /// has not yet produced a locally-verified canonical root. The peer's
    /// self-reported root is not accepted as a clear signal (#1724).
    AwaitingLocalProof,
    /// The node's own canonical-root recompute disagreed with its live state
    /// root: an internal (producer/applier/replay) integrity failure rather
    /// than a peer-observed fork. The latch is left/forced set.
    LocalRootMismatch,
}

/// Policy governing how the `state_diverged` readiness latch is *cleared*.
///
/// Detection (latching `true`) is identical under every policy — the change is
/// purely in what evidence is accepted as proof the node has re-converged.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub enum StateDivergenceRecoveryPolicy {
    /// #1717 default: a same-height peer that agrees on **both** the tip hash
    /// and a **non-zero** state root clears the latch. Recovery trusts the
    /// peer's self-reported state root (the same operands that latch detection).
    #[default]
    PeerAgreement,
    /// #1724 (stricter): the peer path can **never** clear the latch. Only a
    /// locally-verified canonical root — the node's own deterministic recompute
    /// / WAL-replay parity root, post-#1681 producer/applier parity — clears it.
    ///
    /// This removes any dependence on a peer's *self-reported* root, so a
    /// colluding or malicious same-height peer cannot clear the latch merely by
    /// echoing a matching tip+root back to us. It is the more robust long-term
    /// design at the cost of requiring the node to make local forward progress
    /// (apply/replay a block) before `/readyz` returns ready again.
    LocallyVerifiedRoot,
}

impl StateDivergenceRecoveryPolicy {
    /// Parse the policy from `WRFCOIN_STATE_DIVERGENCE_RECOVERY`.
    ///
    /// Unset, empty, or unrecognized values fall back to the backwards-
    /// compatible [`PeerAgreement`](Self::PeerAgreement) default. Accepted
    /// (case-insensitive) values:
    ///   * `peer-agreement`, `peer`, `agreement` -> `PeerAgreement`
    ///   * `local-root`, `local`, `locally-verified` -> `LocallyVerifiedRoot`
    pub fn from_env() -> Self {
        match std::env::var("WRFCOIN_STATE_DIVERGENCE_RECOVERY") {
            Ok(raw) => Self::parse(&raw),
            Err(_) => Self::default(),
        }
    }

    /// Pure parse helper (env-independent, so it is unit-testable).
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "local-root" | "local" | "locally-verified" | "local_root" => {
                Self::LocallyVerifiedRoot
            }
            // Empty / unknown / explicit peer-agreement all use the default.
            _ => Self::PeerAgreement,
        }
    }

    /// Whether the latch can only be cleared by a locally-verified canonical
    /// root (i.e. peer agreement alone is insufficient).
    pub fn requires_local_proof(self) -> bool {
        matches!(self, Self::LocallyVerifiedRoot)
    }
}

/// A node's independently-recomputed canonical state root paired with the root
/// it is currently advertising, used by [`observe_local_canonical_root`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalCanonicalRoot {
    /// Root recomputed deterministically from materialized state — a fresh
    /// `recalculate_state_root()` on a state clone (or a full WAL replay).
    /// Post-#1681 this is the canonical producer/applier/replay parity root.
    pub recomputed_root: [u8; 32],
    /// The node's currently-stored live state root.
    pub live_root: [u8; 32],
}

/// Apply the app-level same-height divergence rule used by launch_api.
///
/// # Latch / reset semantics (#1528 detection, #1713 recovery)
///
/// The `state_diverged` flag gates `/readyz`. It is latched `true` whenever a
/// peer at our exact height reports a tip hash or non-zero state root that
/// differs from ours — strong evidence the node forked or partitioned.
///
/// Prior to #1713 the flag was monotonic: nothing ever cleared it, so a single
/// height-aligned mismatch from one peer (e.g. a legacy pre-#1681 binary)
/// wedged the node `not_ready` forever, which also flunked the container
/// Docker healthcheck. This made it a launch blocker.
///
/// The reset is intentionally *conservative*. The flag is cleared **only** on a
/// genuine re-convergence signal: a peer at our exact height that agrees on
/// **both** the tip hash and a **non-zero** state root. That is the same
/// equality test, with the same operands, that latches divergence in the first
/// place — so it is exactly as trustworthy as the detection it reverses. We do
/// NOT clear the flag merely because:
///   * time passed (no timer-based reset),
///   * a block was produced/applied (local progress is not peer agreement),
///   * a peer omitted its state root (`OmittedStateRoot` — ambiguous; a legacy
///     peer that doesn't publish roots must not be able to mask a real fork),
///   * a peer is simply at a different height (says nothing about our tip).
///
/// Because both detection and recovery require a same-height peer with a
/// non-zero state root, a genuine unresolved fork stays latched: the diverging
/// peer keeps reporting a mismatching root and never satisfies the clear
/// condition. Only after the node has actually re-aligned (via reorg / IBD /
/// WAL replay) such that a same-height peer agrees on tip *and* root does the
/// node report ready again.
///
/// # Recovery policy (#1724)
///
/// `policy` selects what evidence clears the latch. Under the default
/// [`PeerAgreement`](StateDivergenceRecoveryPolicy::PeerAgreement) the above
/// peer-agreement signal clears it (the #1717 behavior). Under
/// [`LocallyVerifiedRoot`](StateDivergenceRecoveryPolicy::LocallyVerifiedRoot)
/// peer agreement never clears — it yields
/// [`AwaitingLocalProof`](ChainDivergenceObservation::AwaitingLocalProof) while
/// latched — and the latch is cleared only via
/// [`observe_local_canonical_root`], so a colluding same-height peer cannot
/// clear it by echoing a matching tip+root. **Detection is identical under
/// both policies**: a same-height tip/root mismatch always latches.
pub fn observe_peer_chain_status(
    peer_id: &str,
    peer: ChainStateView,
    local: ChainStateView,
    state_diverged: &AtomicBool,
    policy: StateDivergenceRecoveryPolicy,
) -> ChainDivergenceObservation {
    if peer.height != local.height {
        return ChainDivergenceObservation::NoDivergence;
    }

    let peer_root_is_zero = peer.state_root == [0u8; 32];
    let tip_matches = peer.tip_hash == local.tip_hash;
    let root_matches = peer.state_root == local.state_root;

    if peer_root_is_zero && tip_matches {
        return ChainDivergenceObservation::OmittedStateRoot;
    }

    if !tip_matches || !root_matches {
        let newly_latched = !state_diverged.swap(true, Ordering::SeqCst);
        if newly_latched {
            CORE_METRICS.inc_state_divergence_detected(peer_id);
        }
        return ChainDivergenceObservation::Diverged { newly_latched };
    }

    // Full agreement at the same height on both tip hash and a non-zero state
    // root. `peer_root_is_zero` is necessarily false here (it would have
    // returned OmittedStateRoot when the tip matched, or Diverged when it did
    // not), so this is a real non-zero matching root — never a legacy omitted
    // root.
    match policy {
        StateDivergenceRecoveryPolicy::PeerAgreement => {
            // #1713/#1717: peer agreement is genuine re-convergence. If we were
            // latched diverged, clear the latch so /readyz can return ready.
            let was_diverged = state_diverged.swap(false, Ordering::SeqCst);
            if was_diverged {
                CORE_METRICS.inc_state_divergence_cleared();
                ChainDivergenceObservation::Recovered
            } else {
                ChainDivergenceObservation::NoDivergence
            }
        }
        StateDivergenceRecoveryPolicy::LocallyVerifiedRoot => {
            // #1724: the peer's self-reported root is NOT accepted as a clear
            // signal. Leave the latch untouched; recovery must come from
            // observe_local_canonical_root. Surface AwaitingLocalProof while
            // still latched so operators can see the node is converging.
            if state_diverged.load(Ordering::SeqCst) {
                ChainDivergenceObservation::AwaitingLocalProof
            } else {
                ChainDivergenceObservation::NoDivergence
            }
        }
    }
}

/// #2734: [`observe_peer_chain_status`] with a recovery path that does not
/// require an exact-height coincidence.
///
/// The plain function can only *clear* the latch on an observation where the
/// peer's height exactly equals local height. On a continuously producing
/// chain that equality is a race outcome, not a guarantee — the measured
/// incident (#2734) saw a fully re-converged fleet stay `not_ready`
/// indefinitely because every chain status arrived while the peer was a few
/// blocks behind, and only a process restart cleared the latch.
///
/// This variant additionally accepts `local_at_peer_height`: the local
/// chain's **own** view at the peer's (lower) height — the hash of our block
/// at that height and the post-state root that block committed (#2219). If
/// the peer fully agrees with our own chain at its height (tip hash AND a
/// non-zero state root), the peer is demonstrably on our chain, which is the
/// same strength of evidence the same-height clearing branch accepts — the
/// same operands, compared at a height we have proof for.
///
/// Semantics are otherwise unchanged, deliberately:
///   * **Detection stays same-height only.** A mismatch at an ancestor height
///     does NOT latch: a peer mid-catch-up or mid-reorg legitimately reports
///     heights we have already moved past, and the plain function never
///     latched on those either. This variant only widens *recovery*.
///   * A zero peer root at an ancestor height never clears (same
///     legacy-peer rule as [`ChainDivergenceObservation::OmittedStateRoot`]).
///   * Under [`StateDivergenceRecoveryPolicy::LocallyVerifiedRoot`] ancestor
///     agreement yields [`ChainDivergenceObservation::AwaitingLocalProof`]
///     while latched, exactly like same-height agreement — a peer echo still
///     cannot clear the strict policy's latch.
///   * A peer AHEAD of local is unchanged (`NoDivergence`): we hold no block
///     at its height to compare against.
///   * `local_at_peer_height = None` (block evicted from the in-memory
///     window, or heights equal/ahead) degrades to the plain function's
///     behavior.
pub fn observe_peer_chain_status_with_history(
    peer_id: &str,
    peer: ChainStateView,
    local_tip: ChainStateView,
    local_at_peer_height: Option<ChainStateView>,
    state_diverged: &AtomicBool,
    policy: StateDivergenceRecoveryPolicy,
) -> ChainDivergenceObservation {
    if peer.height == local_tip.height {
        return observe_peer_chain_status(peer_id, peer, local_tip, state_diverged, policy);
    }

    let Some(hist) = local_at_peer_height else {
        return ChainDivergenceObservation::NoDivergence;
    };

    // The caller resolves the view at the peer's height; a view at any other
    // height (or for a peer ahead of us) proves nothing — ignore it.
    if peer.height >= local_tip.height || hist.height != peer.height {
        return ChainDivergenceObservation::NoDivergence;
    }

    let peer_root_is_zero = peer.state_root == [0u8; 32];
    let agrees_with_our_chain = !peer_root_is_zero
        && peer.tip_hash == hist.tip_hash
        && peer.state_root == hist.state_root;
    if !agrees_with_our_chain {
        // Ancestor-height disagreement is NOT a divergence signal (see doc);
        // detection remains the plain function's same-height rule.
        return ChainDivergenceObservation::NoDivergence;
    }

    match policy {
        StateDivergenceRecoveryPolicy::PeerAgreement => {
            let was_diverged = state_diverged.swap(false, Ordering::SeqCst);
            if was_diverged {
                CORE_METRICS.inc_state_divergence_cleared();
                ChainDivergenceObservation::Recovered
            } else {
                ChainDivergenceObservation::NoDivergence
            }
        }
        StateDivergenceRecoveryPolicy::LocallyVerifiedRoot => {
            if state_diverged.load(Ordering::SeqCst) {
                ChainDivergenceObservation::AwaitingLocalProof
            } else {
                ChainDivergenceObservation::NoDivergence
            }
        }
    }
}

/// Clear the `state_diverged` latch on a **locally-verified canonical root**.
///
/// This is the local, peer-independent recovery path requested in #1724. The
/// caller recomputes its canonical state root deterministically — a fresh
/// `recalculate_state_root()` on a clone of materialized state, or a full WAL
/// replay — and passes it alongside the live root it is advertising. Post-#1681
/// these are the producer/applier/replay parity root, so equality is positive
/// local evidence that the node's stored root is the canonical root of its own
/// accepted blocks, computed without consulting any peer.
///
/// Semantics:
///   * `recomputed == live` and non-zero -> the latch is cleared
///     ([`Recovered`](ChainDivergenceObservation::Recovered) if it was set,
///     otherwise [`NoDivergence`](ChainDivergenceObservation::NoDivergence)).
///   * `recomputed != live` -> the node's own recompute disagrees with its
///     stored root: an internal integrity failure. The latch is forced/left set
///     and [`LocalRootMismatch`](ChainDivergenceObservation::LocalRootMismatch)
///     is returned.
///   * `recomputed == 0` -> there is no materialized state to verify against
///     (e.g. genesis-only); not strong enough to clear. Reports
///     [`AwaitingLocalProof`](ChainDivergenceObservation::AwaitingLocalProof)
///     while latched, else `NoDivergence`.
///
/// Recovery here is intentionally policy-independent: a locally-verified root is
/// strictly stronger evidence than peer agreement, so it clears the latch under
/// either [`StateDivergenceRecoveryPolicy`] — but it is the *only* clear path
/// when [`LocallyVerifiedRoot`](StateDivergenceRecoveryPolicy::LocallyVerifiedRoot)
/// is selected.
pub fn observe_local_canonical_root(
    verification: LocalCanonicalRoot,
    state_diverged: &AtomicBool,
) -> ChainDivergenceObservation {
    let LocalCanonicalRoot {
        recomputed_root,
        live_root,
    } = verification;

    if recomputed_root == [0u8; 32] {
        // Nothing materialized to verify against — cannot assert canonicality.
        return if state_diverged.load(Ordering::SeqCst) {
            ChainDivergenceObservation::AwaitingLocalProof
        } else {
            ChainDivergenceObservation::NoDivergence
        };
    }

    if recomputed_root != live_root {
        // Our own deterministic recompute disagrees with the stored root: the
        // node's state materialization is corrupt or non-deterministic. This is
        // a divergence, so make sure the latch is set.
        state_diverged.store(true, Ordering::SeqCst);
        return ChainDivergenceObservation::LocalRootMismatch;
    }

    // recomputed == live, non-zero: a locally-verified canonical root.
    let was_diverged = state_diverged.swap(false, Ordering::SeqCst);
    if was_diverged {
        CORE_METRICS.inc_state_divergence_cleared();
        ChainDivergenceObservation::Recovered
    } else {
        ChainDivergenceObservation::NoDivergence
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes tests that assert exact deltas on the process-global
    /// `CORE_METRICS` divergence counters. `cargo test` runs tests in parallel
    /// within one process, so two such tests can otherwise interleave between a
    /// before/after read and observe an inflated delta.
    static COUNTER_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    const ROOT_A: [u8; 32] = [0xAA; 32];
    const ROOT_B: [u8; 32] = [0xBB; 32];
    const TIP_A: [u8; 32] = [0x11; 32];
    const TIP_B: [u8; 32] = [0x22; 32];
    const ZERO: [u8; 32] = [0u8; 32];
    const PEER_ID: &str = "12D3KooWStateDivergenceTestPeer";

    fn view(height: u64, tip_hash: [u8; 32], state_root: [u8; 32]) -> ChainStateView {
        ChainStateView { height, tip_hash, state_root }
    }

    #[test]
    fn latches_on_nonzero_root_mismatch_at_same_height() {
        let flag = AtomicBool::new(false);
        let obs = observe_peer_chain_status(
            PEER_ID,
            view(100, TIP_A, ROOT_B),
            view(100, TIP_A, ROOT_A),
            &flag,
            StateDivergenceRecoveryPolicy::PeerAgreement,
        );
        assert_eq!(obs, ChainDivergenceObservation::Diverged { newly_latched: true });
        assert!(flag.load(Ordering::SeqCst), "flag must latch true on mismatch");
    }

    #[test]
    fn latches_on_tip_mismatch_at_same_height() {
        let flag = AtomicBool::new(false);
        let obs = observe_peer_chain_status(
            PEER_ID,
            view(100, TIP_B, ROOT_A),
            view(100, TIP_A, ROOT_A),
            &flag,
            StateDivergenceRecoveryPolicy::PeerAgreement,
        );
        assert_eq!(obs, ChainDivergenceObservation::Diverged { newly_latched: true });
        assert!(flag.load(Ordering::SeqCst));
    }

    #[test]
    fn second_mismatch_does_not_report_newly_latched() {
        let flag = AtomicBool::new(true);
        let obs = observe_peer_chain_status(
            PEER_ID,
            view(100, TIP_A, ROOT_B),
            view(100, TIP_A, ROOT_A),
            &flag,
            StateDivergenceRecoveryPolicy::PeerAgreement,
        );
        assert_eq!(obs, ChainDivergenceObservation::Diverged { newly_latched: false });
        assert!(flag.load(Ordering::SeqCst));
    }

    #[test]
    fn resets_on_reconvergence_when_previously_latched() {
        let _g = COUNTER_GUARD.lock().unwrap();
        let cleared_before = CORE_METRICS.state_diverged_cleared_total.get();

        // Node is latched diverged; a same-height peer now agrees on tip AND a
        // non-zero state root -> genuine re-convergence clears the latch.
        let flag = AtomicBool::new(true);
        let obs = observe_peer_chain_status(
            PEER_ID,
            view(200, TIP_A, ROOT_A),
            view(200, TIP_A, ROOT_A),
            &flag,
            StateDivergenceRecoveryPolicy::PeerAgreement,
        );
        assert_eq!(obs, ChainDivergenceObservation::Recovered);
        assert!(!flag.load(Ordering::SeqCst), "latch must clear on re-convergence");
        assert_eq!(
            CORE_METRICS.state_diverged_cleared_total.get(),
            cleared_before + 1,
            "recovery must increment the cleared counter exactly once"
        );
    }

    #[test]
    fn full_agreement_when_not_latched_is_noop() {
        let _g = COUNTER_GUARD.lock().unwrap();
        let cleared_before = CORE_METRICS.state_diverged_cleared_total.get();

        let flag = AtomicBool::new(false);
        let obs = observe_peer_chain_status(
            PEER_ID,
            view(200, TIP_A, ROOT_A),
            view(200, TIP_A, ROOT_A),
            &flag,
            StateDivergenceRecoveryPolicy::PeerAgreement,
        );
        assert_eq!(obs, ChainDivergenceObservation::NoDivergence);
        assert!(!flag.load(Ordering::SeqCst));
        assert_eq!(
            CORE_METRICS.state_diverged_cleared_total.get(),
            cleared_before,
            "no recovery event should fire when the node was not latched"
        );
    }

    #[test]
    fn stays_latched_on_unresolved_divergence() {
        // The genuine-fork scenario: node is latched, and the same-height peer
        // KEEPS reporting a mismatching non-zero root. The latch must not clear.
        let flag = AtomicBool::new(true);
        for _ in 0..5 {
            let obs = observe_peer_chain_status(
                PEER_ID,
                view(300, TIP_A, ROOT_B),
                view(300, TIP_A, ROOT_A),
                &flag,
                StateDivergenceRecoveryPolicy::PeerAgreement,
            );
            assert_eq!(obs, ChainDivergenceObservation::Diverged { newly_latched: false });
            assert!(flag.load(Ordering::SeqCst), "unresolved fork must remain latched");
        }
    }

    #[test]
    fn omitted_state_root_never_clears_latch() {
        // A legacy peer that omits its state root (all-zero) but agrees on tip
        // must NOT be able to clear a real divergence latch — that would let a
        // pre-fix peer mask a genuine fork.
        let flag = AtomicBool::new(true);
        let obs = observe_peer_chain_status(
            PEER_ID,
            view(400, TIP_A, ZERO),
            view(400, TIP_A, ROOT_A),
            &flag,
            StateDivergenceRecoveryPolicy::PeerAgreement,
        );
        assert_eq!(obs, ChainDivergenceObservation::OmittedStateRoot);
        assert!(flag.load(Ordering::SeqCst), "omitted root must leave the latch untouched");
    }

    #[test]
    fn different_height_never_clears_latch() {
        // A peer at a different height says nothing about our tip and must not
        // clear the latch even if everything else would have matched.
        let flag = AtomicBool::new(true);
        let obs = observe_peer_chain_status(
            PEER_ID,
            view(199, TIP_A, ROOT_A),
            view(200, TIP_A, ROOT_A),
            &flag,
            StateDivergenceRecoveryPolicy::PeerAgreement,
        );
        assert_eq!(obs, ChainDivergenceObservation::NoDivergence);
        assert!(flag.load(Ordering::SeqCst), "off-height peer must leave the latch untouched");
    }

    #[test]
    fn latch_then_reconverge_round_trip() {
        // End-to-end: clean -> latched (mismatch) -> recovered (re-aligned).
        let flag = AtomicBool::new(false);

        let latch = observe_peer_chain_status(
            PEER_ID,
            view(500, TIP_A, ROOT_B),
            view(500, TIP_A, ROOT_A),
            &flag,
            StateDivergenceRecoveryPolicy::PeerAgreement,
        );
        assert_eq!(latch, ChainDivergenceObservation::Diverged { newly_latched: true });
        assert!(flag.load(Ordering::SeqCst));

        // Local node reorgs/replays so its root now equals the peer's (ROOT_B).
        let _g = COUNTER_GUARD.lock().unwrap();
        let recover = observe_peer_chain_status(
            PEER_ID,
            view(500, TIP_A, ROOT_B),
            view(500, TIP_A, ROOT_B),
            &flag,
            StateDivergenceRecoveryPolicy::PeerAgreement,
        );
        assert_eq!(recover, ChainDivergenceObservation::Recovered);
        assert!(!flag.load(Ordering::SeqCst), "node must report ready after re-converging");
    }

    // ----- #2734: recovery must not require an exact-height coincidence -----

    /// **The #2734 incident test.** A latched node whose peer has fully
    /// re-converged but reports its status while one block BEHIND (the normal
    /// state on a continuously producing chain) must clear the latch. On
    /// pre-fix code there is no API through which this observation can clear:
    /// `observe_peer_chain_status` early-returns on the height mismatch, and
    /// only a process restart recovers `/readyz`.
    #[test]
    fn behind_peer_agreeing_with_our_chain_at_its_height_clears_latch_2734() {
        let _g = COUNTER_GUARD.lock().unwrap();
        let cleared_before = CORE_METRICS.state_diverged_cleared_total.get();

        let flag = AtomicBool::new(true);
        // Local tip is height 36; the peer reports height 35 — and its tip and
        // root are exactly OUR block 35's hash and committed post-state root.
        let obs = observe_peer_chain_status_with_history(
            PEER_ID,
            view(35, TIP_A, ROOT_A),
            view(36, TIP_B, ROOT_B),
            Some(view(35, TIP_A, ROOT_A)),
            &flag,
            StateDivergenceRecoveryPolicy::PeerAgreement,
        );
        assert_eq!(obs, ChainDivergenceObservation::Recovered);
        assert!(
            !flag.load(Ordering::SeqCst),
            "a re-converged fleet must not stay not_ready until a height coincidence (#2734)"
        );
        assert_eq!(
            CORE_METRICS.state_diverged_cleared_total.get(),
            cleared_before + 1,
            "ancestor-height recovery must increment the cleared counter exactly once"
        );
    }

    #[test]
    fn behind_peer_on_a_different_chain_never_clears_latch_2734() {
        // A genuinely forked peer has a DIFFERENT block at its height. The
        // latch must hold.
        let flag = AtomicBool::new(true);
        let obs = observe_peer_chain_status_with_history(
            PEER_ID,
            view(35, TIP_B, ROOT_B),
            view(36, TIP_A, ROOT_A),
            Some(view(35, TIP_A, ROOT_A)),
            &flag,
            StateDivergenceRecoveryPolicy::PeerAgreement,
        );
        assert_eq!(obs, ChainDivergenceObservation::NoDivergence);
        assert!(flag.load(Ordering::SeqCst), "a forked behind-peer must not clear the latch");
    }

    #[test]
    fn behind_peer_ancestor_mismatch_does_not_latch_2734() {
        // Detection stays same-height only: a mid-catch-up peer disagreeing at
        // an ancestor height must not LATCH a clean node.
        let flag = AtomicBool::new(false);
        let obs = observe_peer_chain_status_with_history(
            PEER_ID,
            view(35, TIP_B, ROOT_B),
            view(36, TIP_A, ROOT_A),
            Some(view(35, TIP_A, ROOT_A)),
            &flag,
            StateDivergenceRecoveryPolicy::PeerAgreement,
        );
        assert_eq!(obs, ChainDivergenceObservation::NoDivergence);
        assert!(!flag.load(Ordering::SeqCst), "ancestor mismatch must not latch (#2734)");
    }

    #[test]
    fn behind_peer_with_zero_root_never_clears_latch_2734() {
        // The legacy-peer rule carries over: an omitted (all-zero) root is not
        // strong enough to clear, at any height.
        let flag = AtomicBool::new(true);
        let obs = observe_peer_chain_status_with_history(
            PEER_ID,
            view(35, TIP_A, ZERO),
            view(36, TIP_B, ROOT_B),
            Some(view(35, TIP_A, ROOT_A)),
            &flag,
            StateDivergenceRecoveryPolicy::PeerAgreement,
        );
        assert_eq!(obs, ChainDivergenceObservation::NoDivergence);
        assert!(flag.load(Ordering::SeqCst));
    }

    #[test]
    fn behind_peer_without_local_history_leaves_latch_untouched_2734() {
        // Block already evicted from the in-memory window -> no proof -> no
        // clear. Degrades to the pre-#2734 behavior.
        let flag = AtomicBool::new(true);
        let obs = observe_peer_chain_status_with_history(
            PEER_ID,
            view(35, TIP_A, ROOT_A),
            view(500, TIP_B, ROOT_B),
            None,
            &flag,
            StateDivergenceRecoveryPolicy::PeerAgreement,
        );
        assert_eq!(obs, ChainDivergenceObservation::NoDivergence);
        assert!(flag.load(Ordering::SeqCst));
    }

    #[test]
    fn ahead_peer_still_never_clears_latch_2734() {
        // We hold no block at an AHEAD peer's height; nothing to verify.
        // A stale/wrong historical view must also be ignored.
        let flag = AtomicBool::new(true);
        let obs = observe_peer_chain_status_with_history(
            PEER_ID,
            view(37, TIP_A, ROOT_A),
            view(36, TIP_A, ROOT_A),
            Some(view(36, TIP_A, ROOT_A)),
            &flag,
            StateDivergenceRecoveryPolicy::PeerAgreement,
        );
        assert_eq!(obs, ChainDivergenceObservation::NoDivergence);
        assert!(flag.load(Ordering::SeqCst));
    }

    #[test]
    fn behind_peer_agreement_under_strict_policy_awaits_local_proof_2734() {
        // LocallyVerifiedRoot: a peer echo cannot clear the latch through the
        // ancestor path either — same rule as the same-height branch.
        let flag = AtomicBool::new(true);
        let obs = observe_peer_chain_status_with_history(
            PEER_ID,
            view(35, TIP_A, ROOT_A),
            view(36, TIP_B, ROOT_B),
            Some(view(35, TIP_A, ROOT_A)),
            &flag,
            StateDivergenceRecoveryPolicy::LocallyVerifiedRoot,
        );
        assert_eq!(obs, ChainDivergenceObservation::AwaitingLocalProof);
        assert!(flag.load(Ordering::SeqCst), "strict policy must not trust a peer echo (#1724)");
    }

    #[test]
    fn same_height_observation_delegates_unchanged_2734() {
        // With equal heights the variant is exactly the plain function,
        // historical view ignored.
        let _g = COUNTER_GUARD.lock().unwrap();
        let flag = AtomicBool::new(true);
        let obs = observe_peer_chain_status_with_history(
            PEER_ID,
            view(200, TIP_A, ROOT_A),
            view(200, TIP_A, ROOT_A),
            Some(view(200, TIP_B, ROOT_B)),
            &flag,
            StateDivergenceRecoveryPolicy::PeerAgreement,
        );
        assert_eq!(obs, ChainDivergenceObservation::Recovered);
        assert!(!flag.load(Ordering::SeqCst));
    }

    // ----- #1724: recovery-policy parsing -----

    #[test]
    fn recovery_policy_defaults_to_peer_agreement() {
        assert_eq!(
            StateDivergenceRecoveryPolicy::default(),
            StateDivergenceRecoveryPolicy::PeerAgreement
        );
        assert!(!StateDivergenceRecoveryPolicy::default().requires_local_proof());
    }

    #[test]
    fn recovery_policy_parses_known_aliases() {
        for raw in ["local-root", "LOCAL", " Locally-Verified ", "local_root"] {
            assert_eq!(
                StateDivergenceRecoveryPolicy::parse(raw),
                StateDivergenceRecoveryPolicy::LocallyVerifiedRoot,
                "{raw:?} should select LocallyVerifiedRoot"
            );
        }
        for raw in ["", "peer", "peer-agreement", "agreement", "nonsense", "  "] {
            assert_eq!(
                StateDivergenceRecoveryPolicy::parse(raw),
                StateDivergenceRecoveryPolicy::PeerAgreement,
                "{raw:?} should fall back to PeerAgreement"
            );
        }
        assert!(StateDivergenceRecoveryPolicy::LocallyVerifiedRoot.requires_local_proof());
    }

    // ----- #1724: peer agreement no longer clears under LocallyVerifiedRoot -----

    #[test]
    fn local_policy_peer_agreement_does_not_clear_latch() {
        let _g = COUNTER_GUARD.lock().unwrap();
        let cleared_before = CORE_METRICS.state_diverged_cleared_total.get();

        // Latched, and a same-height peer echoes a matching tip + non-zero root.
        // Under PeerAgreement this would clear; under LocallyVerifiedRoot it must
        // NOT — a malicious peer must not be able to clear the latch.
        let flag = AtomicBool::new(true);
        let obs = observe_peer_chain_status(
            PEER_ID,
            view(700, TIP_A, ROOT_A),
            view(700, TIP_A, ROOT_A),
            &flag,
            StateDivergenceRecoveryPolicy::LocallyVerifiedRoot,
        );
        assert_eq!(obs, ChainDivergenceObservation::AwaitingLocalProof);
        assert!(flag.load(Ordering::SeqCst), "peer agreement must not clear under local policy");
        assert_eq!(
            CORE_METRICS.state_diverged_cleared_total.get(),
            cleared_before,
            "no clear event may fire on peer agreement under local policy"
        );
    }

    #[test]
    fn local_policy_still_latches_on_mismatch() {
        // Detection is identical under both policies: a same-height mismatch
        // latches even when recovery is gated on local proof.
        let flag = AtomicBool::new(false);
        let obs = observe_peer_chain_status(
            PEER_ID,
            view(710, TIP_A, ROOT_B),
            view(710, TIP_A, ROOT_A),
            &flag,
            StateDivergenceRecoveryPolicy::LocallyVerifiedRoot,
        );
        assert_eq!(obs, ChainDivergenceObservation::Diverged { newly_latched: true });
        assert!(flag.load(Ordering::SeqCst));
    }

    #[test]
    fn local_policy_full_agreement_when_not_latched_is_noop() {
        let flag = AtomicBool::new(false);
        let obs = observe_peer_chain_status(
            PEER_ID,
            view(720, TIP_A, ROOT_A),
            view(720, TIP_A, ROOT_A),
            &flag,
            StateDivergenceRecoveryPolicy::LocallyVerifiedRoot,
        );
        assert_eq!(obs, ChainDivergenceObservation::NoDivergence);
        assert!(!flag.load(Ordering::SeqCst));
    }

    // ----- #1724: observe_local_canonical_root -----

    fn local_root(recomputed: [u8; 32], live: [u8; 32]) -> LocalCanonicalRoot {
        LocalCanonicalRoot { recomputed_root: recomputed, live_root: live }
    }

    #[test]
    fn local_canonical_root_clears_latch_on_match() {
        let _g = COUNTER_GUARD.lock().unwrap();
        let cleared_before = CORE_METRICS.state_diverged_cleared_total.get();

        let flag = AtomicBool::new(true);
        let obs = observe_local_canonical_root(local_root(ROOT_A, ROOT_A), &flag);
        assert_eq!(obs, ChainDivergenceObservation::Recovered);
        assert!(!flag.load(Ordering::SeqCst), "locally-verified root must clear the latch");
        assert_eq!(
            CORE_METRICS.state_diverged_cleared_total.get(),
            cleared_before + 1,
            "local recovery must increment the cleared counter exactly once"
        );
    }

    #[test]
    fn local_canonical_root_match_when_not_latched_is_noop() {
        let _g = COUNTER_GUARD.lock().unwrap();
        let cleared_before = CORE_METRICS.state_diverged_cleared_total.get();

        let flag = AtomicBool::new(false);
        let obs = observe_local_canonical_root(local_root(ROOT_A, ROOT_A), &flag);
        assert_eq!(obs, ChainDivergenceObservation::NoDivergence);
        assert!(!flag.load(Ordering::SeqCst));
        assert_eq!(
            CORE_METRICS.state_diverged_cleared_total.get(),
            cleared_before,
            "no clear event should fire when the node was not latched"
        );
    }

    #[test]
    fn local_canonical_root_mismatch_forces_latch() {
        // The node's own recompute disagrees with its stored root: internal
        // integrity failure. Must report LocalRootMismatch and force the latch
        // set even if it was previously clear.
        let flag = AtomicBool::new(false);
        let obs = observe_local_canonical_root(local_root(ROOT_B, ROOT_A), &flag);
        assert_eq!(obs, ChainDivergenceObservation::LocalRootMismatch);
        assert!(flag.load(Ordering::SeqCst), "internal root mismatch must latch divergence");
    }

    #[test]
    fn local_canonical_root_zero_recompute_never_clears() {
        // A zero recompute (e.g. genesis-only, nothing materialized) is not
        // strong enough to clear a latch.
        let flag = AtomicBool::new(true);
        let obs = observe_local_canonical_root(local_root(ZERO, ZERO), &flag);
        assert_eq!(obs, ChainDivergenceObservation::AwaitingLocalProof);
        assert!(flag.load(Ordering::SeqCst), "zero recompute must leave the latch untouched");
    }

    #[test]
    fn local_proof_clears_latch_that_peer_agreement_could_not() {
        // End-to-end under the strict policy: latch set, peer agreement is
        // rebuffed (AwaitingLocalProof), then a locally-verified canonical root
        // clears it without ever trusting the peer's reported root.
        let flag = AtomicBool::new(true);

        let peer = observe_peer_chain_status(
            PEER_ID,
            view(800, TIP_A, ROOT_A),
            view(800, TIP_A, ROOT_A),
            &flag,
            StateDivergenceRecoveryPolicy::LocallyVerifiedRoot,
        );
        assert_eq!(peer, ChainDivergenceObservation::AwaitingLocalProof);
        assert!(flag.load(Ordering::SeqCst));

        let _g = COUNTER_GUARD.lock().unwrap();
        let local = observe_local_canonical_root(local_root(ROOT_A, ROOT_A), &flag);
        assert_eq!(local, ChainDivergenceObservation::Recovered);
        assert!(!flag.load(Ordering::SeqCst), "local proof must clear the latch");
    }
}
