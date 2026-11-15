//! Re-delivering our genesis handshake and chain status to a newly connected peer.
//!
//! A peer only becomes a sync source after we have received its handshake, so a
//! lost handshake leaves the other side connected, at tip 0, with no error.
//! `event_loop` publishes ours when a peer connects and again when it
//! subscribes. With flood publish off, both can miss the peer:
//!
//! - gossipsub publishes to the mesh, plus random extras only while the mesh is
//!   below `mesh_n`, so a seed with a full mesh skips the newcomer; and
//! - the newcomer grafts within a round trip, and gossipsub sends IHAVE only to
//!   peers outside the mesh, so lazy gossip does not recover it either. The
//!   message went out just before the peer joined the mesh that would have
//!   carried it.
//!
//! So publish again a little later, once the peer should have grafted.
//!
//! The chain status goes with it. A peer becomes a sync source only with both
//! the proof and a status, and the one eager status we send when we receive the
//! peer's handshake can miss it the same way. The next periodic status can be
//! minutes away. Receivers hold a status that beats the proof, so the two need
//! no ordering.
//!
//! # Why a topic publish rather than a direct send
//!
//! Gossipsub has no send-to-one-peer call. A new request/response protocol would
//! only help when both ends run this code, and a stalled newcomer may be an
//! older build. A gossip republish is what every released node already accepts,
//! so upgrading only the seed fixes the stall. Each repeat is two small messages
//! to the mesh, and [`MIN_PUBLISH_GAP`] coalesces repeats across peers.
//!
//! # Why receiving the peer's handshake does not stop the retries
//!
//! Receiving *their* handshake says nothing about whether they received *ours*:
//! in the stall above the seed has the newcomer's proof before the first retry
//! is due. The protocol has no acknowledgement, so the stop signal is the closest
//! thing we can observe: a publish went out while the peer was in our mesh for
//! the topic, which delivers it directly.
//!
//! # Bounds
//!
//! There are no per-peer timers: one map, swept by one tick in `event_loop`. An
//! entry is removed on disconnect, on that mesh publish, or after the last of
//! [`REDELIVERY_DELAYS`], so the map never exceeds the connected-peer count and
//! connection churn leaves nothing behind.

use std::time::{Duration, Instant};

use libp2p::PeerId;
use rustc_hash::FxHashMap;

/// When to publish again, measured from the peer's first connection.
pub(crate) const REDELIVERY_DELAYS: [Duration; 3] = [
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(15),
];

/// Least time between re-delivery publishes, across all peers.
///
/// A publish serves every peer waiting on it, so peers connecting together share
/// one. This also keeps the extra traffic well below what receivers tolerate:
/// `limit_for(GenesisHandshake)` allows 16 per 10 s window per forwarding peer,
/// and over-rate traffic is penalised, so churn must not spend that allowance.
pub(crate) const MIN_PUBLISH_GAP: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy)]
struct Pending {
    connected_at: Instant,
    /// Index into [`REDELIVERY_DELAYS`] of the next attempt.
    next_attempt: usize,
}

impl Pending {
    fn is_due(&self, now: Instant) -> bool {
        REDELIVERY_DELAYS
            .get(self.next_attempt)
            .is_some_and(|delay| now.saturating_duration_since(self.connected_at) >= *delay)
    }
}

/// Peers we may still need to re-deliver our handshake to.
#[derive(Debug, Default)]
pub(crate) struct HandshakeRedelivery {
    pending: FxHashMap<PeerId, Pending>,
    last_publish: Option<Instant>,
}

impl HandshakeRedelivery {
    /// A peer's first connection was established.
    pub(crate) fn peer_connected(&mut self, peer_id: PeerId, now: Instant) {
        self.pending.insert(
            peer_id,
            Pending {
                connected_at: now,
                next_attempt: 0,
            },
        );
    }

    /// A peer's last connection closed.
    pub(crate) fn peer_disconnected(&mut self, peer_id: &PeerId) {
        self.pending.remove(peer_id);
    }

    /// Whether a re-delivery publish should go out now.
    pub(crate) fn should_publish(&self, now: Instant) -> bool {
        let gap_elapsed = self
            .last_publish
            .is_none_or(|last| now.saturating_duration_since(last) >= MIN_PUBLISH_GAP);
        gap_elapsed && self.pending.values().any(|pending| pending.is_due(now))
    }

    /// A re-delivery publish went out. Peers it reached through the mesh are
    /// finished; every other peer uses up the attempts that were due.
    pub(crate) fn record_publish(&mut self, now: Instant, in_mesh: impl Fn(&PeerId) -> bool) {
        self.last_publish = Some(now);
        self.pending.retain(|peer_id, pending| {
            if in_mesh(peer_id) {
                return false;
            }
            while pending.is_due(now) {
                pending.next_attempt += 1;
            }
            pending.next_attempt < REDELIVERY_DELAYS.len()
        });
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn secs(value: u64) -> Duration {
        Duration::from_secs(value)
    }

    /// Run the production decision once a second, returning publish times.
    fn drive(
        tracker: &mut HandshakeRedelivery,
        start: Instant,
        total_secs: u64,
        in_mesh: impl Fn(&PeerId, u64) -> bool,
    ) -> Vec<u64> {
        let mut published = Vec::new();
        for second in 0..=total_secs {
            let now = start + secs(second);
            if tracker.should_publish(now) {
                tracker.record_publish(now, |peer| in_mesh(peer, second));
                published.push(second);
            }
        }
        published
    }

    #[test]
    fn a_peer_outside_the_mesh_gets_every_attempt_then_is_forgotten() {
        let start = Instant::now();
        let mut tracker = HandshakeRedelivery::default();
        tracker.peer_connected(PeerId::random(), start);

        let published = drive(&mut tracker, start, 60, |_, _| false);

        assert_eq!(
            published,
            vec![2, 7, 15],
            "the 5 s attempt waits out the publish gap"
        );
        assert_eq!(tracker.len(), 0, "an exhausted peer must not stay tracked");
    }

    #[test]
    fn nothing_is_published_before_the_first_delay() {
        let start = Instant::now();
        let mut tracker = HandshakeRedelivery::default();
        tracker.peer_connected(PeerId::random(), start);

        assert!(!tracker.should_publish(start));
        assert!(!tracker.should_publish(start + secs(1)));
        assert!(tracker.should_publish(start + secs(2)));
    }

    #[test]
    fn a_publish_with_the_peer_in_the_mesh_finishes_it() {
        let start = Instant::now();
        let mut tracker = HandshakeRedelivery::default();
        tracker.peer_connected(PeerId::random(), start);

        let published = drive(&mut tracker, start, 60, |_, _| true);

        assert_eq!(published, vec![2], "one mesh publish is enough");
        assert_eq!(tracker.len(), 0);
    }

    /// The stall this exists for: the peer is outside the mesh at +2 s and grafts
    /// afterwards. It must still get the publish that follows.
    #[test]
    fn a_peer_that_grafts_late_is_still_served_after_grafting() {
        let start = Instant::now();
        let mut tracker = HandshakeRedelivery::default();
        tracker.peer_connected(PeerId::random(), start);

        let published = drive(&mut tracker, start, 60, |_, second| second >= 3);

        assert_eq!(published, vec![2, 7]);
        assert_eq!(tracker.len(), 0);
    }

    #[test]
    fn a_disconnect_cancels_further_attempts() {
        let start = Instant::now();
        let mut tracker = HandshakeRedelivery::default();
        let peer = PeerId::random();
        tracker.peer_connected(peer, start);
        tracker.peer_disconnected(&peer);

        assert_eq!(tracker.len(), 0);
        assert!(drive(&mut tracker, start, 60, |_, _| false).is_empty());
    }

    #[test]
    fn a_reconnect_starts_a_fresh_schedule_without_a_second_entry() {
        let start = Instant::now();
        let mut tracker = HandshakeRedelivery::default();
        let peer = PeerId::random();
        tracker.peer_connected(peer, start);
        tracker.peer_connected(peer, start + secs(1));

        assert_eq!(tracker.len(), 1);
        assert!(!tracker.should_publish(start + secs(2)));
        assert!(tracker.should_publish(start + secs(3)));
    }

    /// Churn: thousands of peers that connect and leave before their first
    /// attempt must leave no state and cause no traffic.
    #[test]
    fn connection_churn_leaves_nothing_behind() {
        let start = Instant::now();
        let mut tracker = HandshakeRedelivery::default();
        for second in 0..10_000u64 {
            let now = start + secs(second);
            let peer = PeerId::random();
            tracker.peer_connected(peer, now);
            assert_eq!(tracker.len(), 1);
            assert!(!tracker.should_publish(now));
            tracker.peer_disconnected(&peer);
        }
        assert_eq!(tracker.len(), 0);
    }

    /// Peers that stay connected, outside the mesh, arriving as fast as possible:
    /// entries expire and publishes stay under what receivers accept.
    #[test]
    fn sustained_arrivals_stay_bounded_and_within_the_receiver_rate_limit() {
        use crate::message::{MessageKind, limit_for};

        let start = Instant::now();
        let mut tracker = HandshakeRedelivery::default();
        let mut published = Vec::new();
        let arrivals_per_second = 50;
        let total_secs = 600u64;
        for second in 0..total_secs {
            let now = start + secs(second);
            for _ in 0..arrivals_per_second {
                tracker.peer_connected(PeerId::random(), now);
            }
            if tracker.should_publish(now) {
                tracker.record_publish(now, |_| false);
                published.push(second);
            }
            // Longest possible stay: the last delay plus one publish gap.
            let lifetime = REDELIVERY_DELAYS[2].as_secs() + MIN_PUBLISH_GAP.as_secs() + 1;
            assert!(tracker.len() <= (lifetime * arrivals_per_second) as usize);
        }

        let window = 10u64;
        let max = limit_for(MessageKind::GenesisHandshake).max_per_window as usize;
        for start_second in 0..total_secs {
            let in_window = published
                .iter()
                .filter(|&&at| at >= start_second && at < start_second + window)
                .count();
            assert!(
                in_window <= 2,
                "{in_window} publishes in a {window} s window"
            );
            assert!(in_window * 4 < max, "must leave most of the allowance free");
        }
    }
}
