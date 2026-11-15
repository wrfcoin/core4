//! Silent-gossip watchdog.
//!
//! Sync only asks for blocks when it learns a peer is ahead, and it learns
//! that from `ChainStatus` / `NewBlock` gossip. If gossip stops while the
//! connection stays up, the node stops moving and nothing reports an error
//! (ws node, 2026-11-08: 9.5 hours behind a connected peer).
//!
//! The watchdog notices the silence and asks directly. When no chain gossip
//! has arrived from any proven peer for `silence_timeout`, it probes every
//! proven peer with a one-block request for `local_height + 1` over the
//! existing block-sync protocol. The answer decides what happens:
//!
//! * **Level** (empty answer) or **Refused** (`retry_after_ms`): the peer is
//!   alive and has nothing newer for us. The chain is quiet, not broken. Keep
//!   the connection and start a new silence window.
//! * **Ahead** (a block): the peer has a newer chain and gossip did not tell us.
//!   Gossip on that connection is broken, so drop it and let bootstrap redial
//!   build a fresh one. A new connection re-announces our subscriptions and
//!   carries the peer's eager `ChainStatus`, which restarts sync.
//! * **Failed** (transport failure, or no answer by `probe_timeout`): drop the
//!   connection the same way.
//!
//! This module is pure state; the node event loop does the I/O.

use std::time::{Duration, Instant};

use libp2p::PeerId;
use rustc_hash::FxHashSet;

/// What a probe answer showed about the peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProbeAnswer {
    /// The peer returned at least one block above our height.
    Ahead,
    /// The peer returned no blocks: it has nothing above our height.
    Level,
    /// The peer is alive but rate-limited us (`retry_after_ms`).
    Refused,
    /// The request failed or timed out.
    Failed,
}

/// What the event loop should do after a watchdog tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WatchdogAction {
    None,
    /// Send a one-block probe to each of these peers.
    Probe(Vec<PeerId>),
    /// Disconnect these peers so they are redialled.
    Reconnect(Vec<PeerId>),
}

#[derive(Debug)]
struct Probe {
    deadline: Instant,
    outstanding: FxHashSet<PeerId>,
}

#[derive(Debug)]
pub(crate) struct GossipWatchdog {
    silence_timeout: Duration,
    probe_timeout: Duration,
    last_gossip: Instant,
    last_gossip_count: u64,
    had_peers: bool,
    probe: Option<Probe>,
}

impl GossipWatchdog {
    /// A zero `silence_timeout` disables the watchdog.
    pub(crate) fn new(silence_timeout: Duration, probe_timeout: Duration, now: Instant) -> Self {
        Self {
            silence_timeout,
            probe_timeout,
            last_gossip: now,
            last_gossip_count: 0,
            had_peers: false,
            probe: None,
        }
    }

    pub(crate) fn enabled(&self) -> bool {
        !self.silence_timeout.is_zero()
    }

    /// Feed the running count of chain gossip messages received. Any change
    /// means gossip is alive: the silence window restarts and an open probe is
    /// dropped.
    pub(crate) fn observe_gossip_count(&mut self, count: u64, now: Instant) {
        if count != self.last_gossip_count {
            self.last_gossip_count = count;
            self.last_gossip = now;
            self.probe = None;
        }
    }

    /// One tick. `proven_peers` are the connected peers whose genesis proof
    /// landed this connection epoch.
    pub(crate) fn tick(&mut self, now: Instant, proven_peers: &[PeerId]) -> WatchdogAction {
        if !self.enabled() {
            return WatchdogAction::None;
        }
        if proven_peers.is_empty() {
            // Nothing to hear from. Bootstrap redial owns this case; restart
            // the window when a peer is proven again.
            self.had_peers = false;
            self.probe = None;
            return WatchdogAction::None;
        }
        if !self.had_peers {
            self.had_peers = true;
            self.last_gossip = now;
            return WatchdogAction::None;
        }
        if let Some(probe) = &mut self.probe {
            probe.outstanding.retain(|peer| proven_peers.contains(peer));
            if probe.outstanding.is_empty() {
                self.probe = None;
                self.last_gossip = now;
                return WatchdogAction::None;
            }
            if now < probe.deadline {
                return WatchdogAction::None;
            }
            let unanswered: Vec<PeerId> = probe.outstanding.drain().collect();
            self.probe = None;
            self.last_gossip = now;
            return WatchdogAction::Reconnect(unanswered);
        }
        if now.saturating_duration_since(self.last_gossip) < self.silence_timeout {
            return WatchdogAction::None;
        }
        self.probe = Some(Probe {
            deadline: now + self.probe_timeout,
            outstanding: proven_peers.iter().copied().collect(),
        });
        WatchdogAction::Probe(proven_peers.to_vec())
    }

    /// Record a probe answer. Returns `true` when that peer's connection
    /// should be dropped and redialled.
    pub(crate) fn on_probe_answer(&mut self, peer: PeerId, answer: ProbeAnswer, now: Instant) -> bool {
        let Some(probe) = &mut self.probe else {
            // Gossip came back (or the probe expired) first; nothing to do.
            return false;
        };
        if !probe.outstanding.remove(&peer) {
            return false;
        }
        if probe.outstanding.is_empty() {
            self.probe = None;
            self.last_gossip = now;
        }
        matches!(answer, ProbeAnswer::Ahead | ProbeAnswer::Failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SILENCE: Duration = Duration::from_secs(300);
    const PROBE: Duration = Duration::from_secs(60);

    fn armed(peers: &[PeerId]) -> (GossipWatchdog, Instant) {
        let t0 = Instant::now();
        let mut w = GossipWatchdog::new(SILENCE, PROBE, t0);
        assert_eq!(w.tick(t0, peers), WatchdogAction::None, "first proven peer starts the window");
        (w, t0)
    }

    #[test]
    fn silence_shorter_than_the_timeout_does_nothing() {
        let peer = PeerId::random();
        let (mut w, t0) = armed(&[peer]);
        assert_eq!(w.tick(t0 + SILENCE - Duration::from_secs(1), &[peer]), WatchdogAction::None);
    }

    #[test]
    fn silence_probes_every_proven_peer_once() {
        let (a, b) = (PeerId::random(), PeerId::random());
        let (mut w, t0) = armed(&[a, b]);
        let WatchdogAction::Probe(mut probed) = w.tick(t0 + SILENCE, &[a, b]) else {
            panic!("silence must probe");
        };
        probed.sort();
        let mut want = vec![a, b];
        want.sort();
        assert_eq!(probed, want);
        assert_eq!(w.tick(t0 + SILENCE + Duration::from_secs(1), &[a, b]), WatchdogAction::None);
    }

    #[test]
    fn gossip_resets_the_window_and_cancels_a_probe() {
        let peer = PeerId::random();
        let (mut w, t0) = armed(&[peer]);
        w.observe_gossip_count(1, t0 + SILENCE - Duration::from_secs(1));
        assert_eq!(w.tick(t0 + SILENCE, &[peer]), WatchdogAction::None);
        let t1 = t0 + SILENCE * 2;
        assert!(matches!(w.tick(t1, &[peer]), WatchdogAction::Probe(_)));
        w.observe_gossip_count(2, t1 + Duration::from_secs(1));
        assert!(!w.on_probe_answer(peer, ProbeAnswer::Failed, t1 + Duration::from_secs(2)));
        assert_eq!(w.tick(t1 + PROBE * 2, &[peer]), WatchdogAction::None);
    }

    #[test]
    fn ahead_or_failed_answers_reconnect_level_or_refused_keep() {
        for (answer, reconnect) in [
            (ProbeAnswer::Ahead, true),
            (ProbeAnswer::Failed, true),
            (ProbeAnswer::Level, false),
            (ProbeAnswer::Refused, false),
        ] {
            let peer = PeerId::random();
            let (mut w, t0) = armed(&[peer]);
            assert!(matches!(w.tick(t0 + SILENCE, &[peer]), WatchdogAction::Probe(_)));
            assert_eq!(w.on_probe_answer(peer, answer, t0 + SILENCE), reconnect, "{answer:?}");
            // The answer closes the probe and starts a fresh window.
            assert_eq!(w.tick(t0 + SILENCE + PROBE * 2, &[peer]), WatchdogAction::None, "{answer:?}");
            assert!(matches!(w.tick(t0 + SILENCE * 2, &[peer]), WatchdogAction::Probe(_)), "{answer:?}");
        }
    }

    #[test]
    fn an_unanswered_probe_reconnects_at_its_deadline() {
        let (a, b) = (PeerId::random(), PeerId::random());
        let (mut w, t0) = armed(&[a, b]);
        let t1 = t0 + SILENCE;
        assert!(matches!(w.tick(t1, &[a, b]), WatchdogAction::Probe(_)));
        assert!(!w.on_probe_answer(a, ProbeAnswer::Level, t1));
        assert_eq!(w.tick(t1 + PROBE - Duration::from_secs(1), &[a, b]), WatchdogAction::None);
        assert_eq!(w.tick(t1 + PROBE, &[a, b]), WatchdogAction::Reconnect(vec![b]));
        // A fresh window follows, so it does not reconnect again at once.
        assert_eq!(w.tick(t1 + PROBE + Duration::from_secs(1), &[a, b]), WatchdogAction::None);
    }

    #[test]
    fn no_peers_means_no_probe_and_a_new_peer_gets_a_full_window() {
        let peer = PeerId::random();
        let t0 = Instant::now();
        let mut w = GossipWatchdog::new(SILENCE, PROBE, t0);
        assert_eq!(w.tick(t0 + SILENCE * 10, &[]), WatchdogAction::None);
        let t1 = t0 + SILENCE * 10;
        assert_eq!(w.tick(t1, &[peer]), WatchdogAction::None);
        assert_eq!(w.tick(t1 + SILENCE - Duration::from_secs(1), &[peer]), WatchdogAction::None);
        assert!(matches!(w.tick(t1 + SILENCE, &[peer]), WatchdogAction::Probe(_)));
    }

    #[test]
    fn a_peer_that_leaves_mid_probe_is_not_reconnected() {
        let (a, b) = (PeerId::random(), PeerId::random());
        let (mut w, t0) = armed(&[a, b]);
        let t1 = t0 + SILENCE;
        assert!(matches!(w.tick(t1, &[a, b]), WatchdogAction::Probe(_)));
        assert_eq!(w.tick(t1 + PROBE, &[a]), WatchdogAction::Reconnect(vec![a]));
    }

    #[test]
    fn zero_timeout_disables_it() {
        let peer = PeerId::random();
        let t0 = Instant::now();
        let mut w = GossipWatchdog::new(Duration::ZERO, PROBE, t0);
        for n in 0..5 {
            assert_eq!(w.tick(t0 + SILENCE * n, &[peer]), WatchdogAction::None);
        }
    }
}
