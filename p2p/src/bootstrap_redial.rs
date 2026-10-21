//! Bootstrap-peer auto-redial state machine (#1668).
//!
//! After a peer disconnect (e.g. the operator restarts the remote container)
//! libp2p does not, on its own, redial entries listed in
//! `WRFCOIN_P2P_BOOTSTRAP`. This module owns both the initial and periodic
//! dial state per
//! bootstrap entry: it tracks whether each configured peer is currently
//! connected, and when to attempt the next redial.
//!
//! The backoff is jittered exponential, base = `interval`, multiplier 2x,
//! capped at `max_backoff`. Jitter is a random 0..25% bonus applied on top
//! of the bounded delay to avoid synchronised dial storms when multiple
//! peers come back at once.

use std::time::{Duration, Instant};

use libp2p::{Multiaddr, PeerId};

/// Per-bootstrap-peer redial state.
#[derive(Debug)]
pub(crate) struct BootstrapPeerRedial {
    /// Full multiaddr (including `/p2p/<peer_id>` if present).
    pub addr: Multiaddr,
    /// Extracted peer id from the multiaddr, if available. Required for
    /// "is this peer currently connected?" checks. Entries without a peer
    /// id fall back to "dial unconditionally on a fixed cadence" since we
    /// can't observe the connection state.
    pub peer_id: Option<PeerId>,
    /// Earliest time we may attempt the next dial. Initialised by the caller;
    /// startup uses zero delay so its first dial reaches the same policy gate
    /// as every retry.
    pub next_attempt: Instant,
    /// Number of consecutive failed redials. Reset to 0 on
    /// `ConnectionEstablished`.
    pub consecutive_failures: u32,
}

impl BootstrapPeerRedial {
    pub fn new(addr: Multiaddr, peer_id: Option<PeerId>, now: Instant, initial_delay: Duration) -> Self {
        Self {
            addr,
            peer_id,
            next_attempt: now + initial_delay,
            consecutive_failures: 0,
        }
    }

    pub fn mark_connected(&mut self) {
        self.consecutive_failures = 0;
    }

    pub fn schedule_after_disconnect(&mut self, now: Instant, base_interval: Duration) {
        self.consecutive_failures = 0;
        self.next_attempt = now + base_interval;
    }

    pub fn schedule_after_failure(
        &mut self,
        now: Instant,
        base_interval: Duration,
        max_backoff: Duration,
    ) {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        let delay = compute_backoff(self.consecutive_failures, base_interval, max_backoff);
        self.next_attempt = now + delay;
    }
}

/// Compute the jittered exponential backoff for the next redial attempt.
///
/// `failures = 0` returns the base interval (no extra delay). Each
/// additional failure doubles the delay up to `max_backoff`. A random
/// 0..25% jitter is added on top of the bounded value to spread out dial
/// attempts across a cluster of failing peers.
pub(crate) fn compute_backoff(
    failures: u32,
    base: Duration,
    max_backoff: Duration,
) -> Duration {
    // Cap the exponent so 2^n doesn't overflow. 2^20 * 30s = ~365 days,
    // far past max_backoff for any sane config.
    let exp = 2u64.saturating_pow(failures.min(20));
    let base_secs = base.as_secs_f64();
    let raw_secs = base_secs * (exp as f64);
    let bounded = Duration::from_secs_f64(raw_secs.min(max_backoff.as_secs_f64()));
    let jitter_max_ms = bounded.as_millis() as u64 / 4;
    let jitter_ms = if jitter_max_ms == 0 {
        0
    } else {
        use rand::Rng;
        rand::thread_rng().gen_range(0..=jitter_max_ms)
    };
    bounded + Duration::from_millis(jitter_ms)
}

/// Extract the `/p2p/<peer_id>` component from a multiaddr, if present.
pub(crate) fn peer_id_from_multiaddr(addr: &Multiaddr) -> Option<PeerId> {
    addr.iter().find_map(|p| match p {
        libp2p::multiaddr::Protocol::P2p(peer_id) => Some(peer_id),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_zero_failures_returns_base_with_jitter() {
        let base = Duration::from_secs(30);
        let max = Duration::from_secs(300);
        let backoff = compute_backoff(0, base, max);
        assert!(backoff >= base, "backoff {backoff:?} < base {base:?}");
        assert!(
            backoff <= base + base / 4,
            "backoff {backoff:?} exceeded base + 25% jitter"
        );
    }

    #[test]
    fn backoff_doubles_until_cap() {
        let base = Duration::from_secs(10);
        let cap = Duration::from_secs(80);

        // failures=0 -> 10s
        // failures=1 -> 20s
        // failures=2 -> 40s
        // failures=3 -> 80s (cap)
        // failures=10 -> 80s (cap)
        for n in 3..=10 {
            let backoff = compute_backoff(n, base, cap);
            assert!(
                backoff >= cap && backoff <= cap + cap / 4,
                "failures={n}: backoff {backoff:?} not within [cap, cap+25%]"
            );
        }
    }

    #[test]
    fn backoff_respects_max_cap_for_extreme_values() {
        let backoff =
            compute_backoff(u32::MAX, Duration::from_secs(30), Duration::from_secs(300));
        assert!(
            backoff <= Duration::from_secs(300) + Duration::from_secs(75),
            "extreme failure count produced runaway backoff: {backoff:?}"
        );
    }

    #[test]
    fn peer_id_from_multiaddr_extracts_p2p_component() {
        let peer_id = PeerId::random();
        let addr: Multiaddr =
            format!("/ip4/127.0.0.1/tcp/8555/p2p/{peer_id}").parse().unwrap();
        assert_eq!(peer_id_from_multiaddr(&addr), Some(peer_id));
    }

    #[test]
    fn peer_id_from_multiaddr_missing_returns_none() {
        let addr: Multiaddr = "/ip4/127.0.0.1/tcp/8555".parse().unwrap();
        assert_eq!(peer_id_from_multiaddr(&addr), None);
    }

    #[test]
    fn disconnect_resets_backoff_to_base_interval() {
        let now = Instant::now();
        let mut state = BootstrapPeerRedial::new(
            "/ip4/127.0.0.1/tcp/8555".parse().unwrap(),
            None,
            now,
            Duration::from_secs(300),
        );
        state.consecutive_failures = 7;

        state.schedule_after_disconnect(now, Duration::from_secs(30));

        assert_eq!(state.consecutive_failures, 0);
        assert_eq!(state.next_attempt, now + Duration::from_secs(30));
    }

    #[test]
    fn async_failure_reschedules_from_incremented_failure_count() {
        let now = Instant::now();
        let mut state = BootstrapPeerRedial::new(
            "/ip4/127.0.0.1/tcp/8555".parse().unwrap(),
            None,
            now,
            Duration::from_secs(30),
        );

        state.schedule_after_failure(now, Duration::from_secs(30), Duration::from_secs(300));

        assert_eq!(state.consecutive_failures, 1);
        assert!(state.next_attempt >= now + Duration::from_secs(60));
        assert!(state.next_attempt <= now + Duration::from_secs(75));
    }
}
