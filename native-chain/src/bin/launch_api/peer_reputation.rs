//! In-process peer reputation tracking driven by `P2pEvent::PeerPenalized`
//! (#1406, part of the #1739 wiring-gap closeout).
//!
//! Background: gossipsub peer scoring (`p2p/src/scoring.rs`) is already active
//! inside libp2p and surfaces graylisting / invalid-message penalties as
//! `P2pEvent::PeerPenalized` events. Before this change the `launch_api`
//! consumer (`p2p_events.rs`) only *logged* those events — the penalty outcomes
//! never accumulated anywhere the node could reason about, so a peer that was
//! penalized repeatedly looked identical to one penalized once.
//!
//! This tracker accumulates penalty outcomes per peer, derives a
//! monotonically-decreasing reputation score, and marks a peer *graylisted*
//! once it is banned or crosses the reputation floor. The runtime path (the P2P
//! event loop) updates it on every real `PeerPenalized` event via
//! [`apply_peer_penalty`], and escalates its log severity when a peer becomes
//! graylisted so operators see the outcome.
//!
//! Two outcomes are now wired on top of the tracker (the #1784 follow-ups):
//!   * **kademlia removal / disconnect** — when a penalty *newly* graylists a
//!     peer, [`apply_peer_penalty`] reports the transition and the event loop
//!     issues a `P2pCommand::DisconnectPeer`, which removes the peer from the
//!     kademlia routing table and drops the connection (`p2p/src/node.rs`);
//!   * **operator read route** — [`PeerReputationTracker::snapshot`] exposes a
//!     serializable per-peer view served by the node API.
//!
//! Everything here is operational/node-local: no peer-reputation state is
//! hashed into the consensus `state_root`.

use libp2p::PeerId;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Shared handle to the peer-reputation tracker: written by the P2P event loop
/// and read by the operator route. `tokio::sync::RwLock` so the route handler
/// can `.read().await` without blocking the runtime.
pub type SharedPeerReputation = Arc<RwLock<PeerReputationTracker>>;

/// Reputation deducted for a non-ban penalty (e.g. an invalid-message or
/// mesh-decay score event surfaced by gossipsub).
pub const PENALTY_DECREMENT: i32 = 20;

/// Reputation deducted for a penalty that libp2p already escalated to a ban.
/// A single ban alone is enough to graylist a peer (see [`GRAYLIST_THRESHOLD`]).
pub const BAN_DECREMENT: i32 = 100;

/// A peer at or below this reputation is treated as graylisted. Chosen so that
/// five accumulated non-ban penalties, or one ban, graylists a peer.
pub const GRAYLIST_THRESHOLD: i32 = -100;

/// Per-peer accumulated penalty outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerPenaltyRecord {
    /// Current reputation. Starts at 0 and only ever decreases as penalties
    /// accumulate (gossipsub scoring is the source of truth for *recovery*; this
    /// tracker deliberately does not rehabilitate, so a flapping peer cannot
    /// reset its standing by waiting).
    pub reputation: i32,
    /// Total penalty events seen for this peer.
    pub penalty_count: u64,
    /// Subset of `penalty_count` that arrived already escalated to a ban.
    pub ban_count: u64,
    /// Reason string from the most recent penalty event (for operator logs).
    pub last_reason: String,
    /// Whether the peer is currently graylisted (banned, or reputation has
    /// crossed [`GRAYLIST_THRESHOLD`]).
    pub graylisted: bool,
}

impl Default for PeerPenaltyRecord {
    fn default() -> Self {
        Self {
            reputation: 0,
            penalty_count: 0,
            ban_count: 0,
            last_reason: String::new(),
            graylisted: false,
        }
    }
}

/// Accumulates [`PeerPenaltyRecord`]s keyed by `PeerId`.
///
/// Lives loop-local inside `run_p2p_event_loop` (mirroring the fork/orphan
/// buffers), so it shares the single-threaded ownership of that task and needs
/// no locking.
#[derive(Debug, Default)]
pub struct PeerReputationTracker {
    peers: HashMap<PeerId, PeerPenaltyRecord>,
}

impl PeerReputationTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a penalty against `peer`, returning the updated record.
    ///
    /// Reputation is reduced by [`BAN_DECREMENT`] for a banned penalty and
    /// [`PENALTY_DECREMENT`] otherwise (saturating, so it cannot underflow), and
    /// the peer is graylisted if it was banned or has crossed the floor.
    pub fn record_penalty(
        &mut self,
        peer: &PeerId,
        reason: &str,
        banned: bool,
    ) -> PeerPenaltyRecord {
        let record = self.peers.entry(*peer).or_default();
        record.penalty_count = record.penalty_count.saturating_add(1);
        let decrement = if banned { BAN_DECREMENT } else { PENALTY_DECREMENT };
        record.reputation = record.reputation.saturating_sub(decrement);
        if banned {
            record.ban_count = record.ban_count.saturating_add(1);
        }
        record.last_reason = reason.to_string();
        record.graylisted = banned || record.reputation <= GRAYLIST_THRESHOLD;
        record.clone()
    }

    /// Current reputation for `peer`, or `None` if it has never been penalized.
    ///
    /// Read API consumed by tests today and by the operator read route in the
    /// #1406 follow-up; not yet called from the runtime path.
    #[allow(dead_code)]
    pub fn reputation_of(&self, peer: &PeerId) -> Option<i32> {
        self.peers.get(peer).map(|record| record.reputation)
    }

    /// Whether `peer` is currently graylisted. Unknown peers are not graylisted.
    pub fn is_graylisted(&self, peer: &PeerId) -> bool {
        self.peers.get(peer).is_some_and(|record| record.graylisted)
    }

    /// Number of distinct peers that have been penalized at least once.
    pub fn penalized_peer_count(&self) -> usize {
        self.peers.len()
    }

    /// Number of distinct peers currently graylisted.
    pub fn graylisted_peer_count(&self) -> usize {
        self.peers.values().filter(|record| record.graylisted).count()
    }

    /// Serializable per-peer view for the operator read route, ordered worst
    /// reputation first (then by peer id) for stable, useful output.
    pub fn snapshot(&self) -> Vec<PeerReputationEntry> {
        let mut entries: Vec<PeerReputationEntry> = self
            .peers
            .iter()
            .map(|(peer, record)| PeerReputationEntry {
                peer_id: peer.to_string(),
                reputation: record.reputation,
                penalty_count: record.penalty_count,
                ban_count: record.ban_count,
                last_reason: record.last_reason.clone(),
                graylisted: record.graylisted,
            })
            .collect();
        entries.sort_by(|a, b| {
            a.reputation
                .cmp(&b.reputation)
                .then_with(|| a.peer_id.cmp(&b.peer_id))
        });
        entries
    }
}

/// One peer's reputation as exposed by the operator read route (#1406).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PeerReputationEntry {
    pub peer_id: String,
    pub reputation: i32,
    pub penalty_count: u64,
    pub ban_count: u64,
    pub last_reason: String,
    pub graylisted: bool,
}

/// Apply a `PeerPenalized` outcome to `tracker`, emit an operator-visible log
/// line (escalating to `warn` once the peer is graylisted), and return whether
/// this penalty *newly* graylisted the peer.
///
/// The caller (the P2P event loop) uses the `true` return — the graylist edge —
/// to issue exactly one `P2pCommand::DisconnectPeer` per transition, instead of
/// re-issuing it on every subsequent penalty of an already-graylisted peer.
///
/// This is the exact body the event loop runs for `P2pEvent::PeerPenalized`,
/// factored out so it can be unit-tested without standing up the whole async
/// event loop (see the test proving the runtime path mutates reputation).
#[must_use]
pub fn apply_peer_penalty(
    tracker: &mut PeerReputationTracker,
    peer_id: &PeerId,
    reason: &str,
    banned: bool,
) -> bool {
    let was_graylisted = tracker.is_graylisted(peer_id);
    let record = tracker.record_penalty(peer_id, reason, banned);
    let newly_graylisted = record.graylisted && !was_graylisted;
    if record.graylisted {
        log::warn!(
            "P2P peer penalized and GRAYLISTED: peer={peer_id} reputation={} \
             penalties={} bans={} reason={reason} newly_graylisted={newly_graylisted} \
             (graylisted={}/{} tracked peers)",
            record.reputation,
            record.penalty_count,
            record.ban_count,
            tracker.graylisted_peer_count(),
            tracker.penalized_peer_count(),
        );
    } else {
        log::info!(
            "P2P peer penalized: peer={peer_id} reputation={} penalties={} \
             banned={banned} reason={reason}",
            record.reputation,
            record.penalty_count,
        );
    }
    newly_graylisted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_non_ban_penalty_decrements_reputation_without_graylisting() {
        let mut tracker = PeerReputationTracker::new();
        let peer = PeerId::random();

        let record = tracker.record_penalty(&peer, "invalid_message", false);

        assert_eq!(record.reputation, -PENALTY_DECREMENT);
        assert_eq!(record.penalty_count, 1);
        assert_eq!(record.ban_count, 0);
        assert_eq!(record.last_reason, "invalid_message");
        assert!(!record.graylisted, "one non-ban penalty must not graylist");
        assert_eq!(tracker.reputation_of(&peer), Some(-PENALTY_DECREMENT));
        assert!(!tracker.is_graylisted(&peer));
        assert_eq!(tracker.penalized_peer_count(), 1);
        assert_eq!(tracker.graylisted_peer_count(), 0);
    }

    #[test]
    fn accumulated_non_ban_penalties_eventually_graylist() {
        let mut tracker = PeerReputationTracker::new();
        let peer = PeerId::random();

        // Four penalties: reputation -80, still above the -100 floor.
        for _ in 0..4 {
            let record = tracker.record_penalty(&peer, "mesh_decay", false);
            assert!(!record.graylisted, "must not graylist above the floor");
        }
        assert_eq!(tracker.reputation_of(&peer), Some(-80));

        // Fifth penalty crosses the floor (-100) → graylisted.
        let record = tracker.record_penalty(&peer, "mesh_decay", false);
        assert_eq!(record.reputation, -100);
        assert!(record.graylisted, "crossing the floor must graylist");
        assert!(tracker.is_graylisted(&peer));
        assert_eq!(tracker.graylisted_peer_count(), 1);
    }

    #[test]
    fn a_single_ban_graylists_immediately() {
        let mut tracker = PeerReputationTracker::new();
        let peer = PeerId::random();

        let record = tracker.record_penalty(&peer, "peer_banned", true);

        assert_eq!(record.reputation, -BAN_DECREMENT);
        assert_eq!(record.ban_count, 1);
        assert!(record.graylisted, "a ban must graylist immediately");
        assert!(tracker.is_graylisted(&peer));
    }

    #[test]
    fn distinct_peers_are_tracked_independently() {
        let mut tracker = PeerReputationTracker::new();
        let noisy = PeerId::random();
        let quiet = PeerId::random();

        tracker.record_penalty(&noisy, "ban", true);
        tracker.record_penalty(&quiet, "invalid_message", false);

        assert!(tracker.is_graylisted(&noisy));
        assert!(!tracker.is_graylisted(&quiet));
        assert_eq!(tracker.penalized_peer_count(), 2);
        assert_eq!(tracker.graylisted_peer_count(), 1);
        assert_eq!(tracker.reputation_of(&quiet), Some(-PENALTY_DECREMENT));
    }

    #[test]
    fn unknown_peer_has_no_reputation_and_is_not_graylisted() {
        let tracker = PeerReputationTracker::new();
        let peer = PeerId::random();
        assert_eq!(tracker.reputation_of(&peer), None);
        assert!(!tracker.is_graylisted(&peer));
    }

    #[test]
    fn apply_peer_penalty_reports_graylist_transition_on_the_runtime_path() {
        // Proves the exact function the P2P event loop calls for
        // `P2pEvent::PeerPenalized` accumulates reputation, graylists, and
        // reports the transition edge exactly once (so the loop disconnects the
        // peer once, not on every subsequent penalty).
        let mut tracker = PeerReputationTracker::new();
        let peer = PeerId::random();

        let newly = apply_peer_penalty(&mut tracker, &peer, "invalid_message", false);
        assert!(!newly, "a single non-ban penalty must not graylist");
        assert_eq!(tracker.reputation_of(&peer), Some(-PENALTY_DECREMENT));
        assert!(!tracker.is_graylisted(&peer));

        let newly = apply_peer_penalty(&mut tracker, &peer, "peer_banned", true);
        assert!(newly, "the ban penalty is the graylist transition → disconnect once");
        assert!(tracker.is_graylisted(&peer));

        // A further penalty on an already-graylisted peer is NOT a new
        // transition: the loop must not re-issue a disconnect.
        let newly = apply_peer_penalty(&mut tracker, &peer, "still_bad", false);
        assert!(!newly, "already-graylisted peer must not re-trigger disconnect");
        assert!(tracker.is_graylisted(&peer));
        assert_eq!(tracker.penalized_peer_count(), 1);
    }

    #[test]
    fn snapshot_exposes_per_peer_state_worst_first() {
        let mut tracker = PeerReputationTracker::new();
        let banned = PeerId::random();
        let mild = PeerId::random();
        let _ = apply_peer_penalty(&mut tracker, &banned, "peer_banned", true);
        let _ = apply_peer_penalty(&mut tracker, &mild, "invalid_message", false);

        let snapshot = tracker.snapshot();
        assert_eq!(snapshot.len(), 2);
        // Worst reputation (banned: -100) is ordered first.
        assert_eq!(snapshot[0].peer_id, banned.to_string());
        assert!(snapshot[0].graylisted);
        assert_eq!(snapshot[0].ban_count, 1);
        assert_eq!(snapshot[1].peer_id, mild.to_string());
        assert!(!snapshot[1].graylisted);
        assert_eq!(snapshot[1].reputation, -PENALTY_DECREMENT);
    }
}
