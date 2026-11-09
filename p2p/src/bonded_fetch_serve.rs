//! Serve-side admission for the bonded snapshot fetch protocol
//! ([`crate::bonded_fetch`]).
//!
//! The node event loop answers rate-limited and overloaded requests itself,
//! before anything reaches the application. Authorization (is this a
//! registered bonded consumer?) needs chain state, so the application checks
//! it first thing, before any snapshot work.

use std::time::{Duration, Instant};

use libp2p::{
    PeerId,
    request_response::{self, InboundRequestId, ResponseChannel},
};
use log::{debug, warn};
use rustc_hash::FxHashMap;
use tokio::sync::mpsc;

use crate::bonded_fetch::{BondedFetchCodec, BondedFetchRequest, BondedFetchResponse};

/// A request the node admitted and handed to the application. The
/// application answers with [`crate::P2pCommand::RespondToBondedFetch`].
#[derive(Debug)]
pub struct InboundBondedFetch {
    pub peer_id: PeerId,
    pub request_id: InboundRequestId,
    pub request: BondedFetchRequest,
}

/// Capacity of the node → application queue. A full queue answers `BUSY`.
pub const BONDED_FETCH_QUEUE: usize = 16;

/// Serve-side ceilings. Defaults fit one consumer pulling a few hundred MiB
/// in 4 MiB pages: about 100 requests for a whole cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BondedFetchLimits {
    /// Requests one peer may start per window.
    pub per_peer_per_window: u32,
    pub window: Duration,
    /// Requests one peer may have unanswered at once.
    pub per_peer_in_flight: u32,
    /// Requests all peers together may have unanswered at once.
    pub global_in_flight: u32,
    /// Peers tracked at once; beyond this, idle entries are dropped first and
    /// a new peer is refused while every tracked peer is busy.
    pub max_tracked_peers: usize,
}

impl Default for BondedFetchLimits {
    fn default() -> Self {
        Self {
            per_peer_per_window: 240,
            window: Duration::from_secs(60),
            per_peer_in_flight: 2,
            global_in_flight: 8,
            max_tracked_peers: 1024,
        }
    }
}

/// Why a request was refused before reaching the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BondedFetchRefusal {
    /// The peer used its per-window budget; retry after the given delay.
    RateLimited(Duration),
    /// Too much is in flight (for this peer or for everyone).
    Busy,
}

impl BondedFetchRefusal {
    pub fn code(&self) -> &'static str {
        match self {
            BondedFetchRefusal::RateLimited(_) => "RATE_LIMITED",
            BondedFetchRefusal::Busy => "BUSY",
        }
    }

    pub fn retry_after_ms(&self) -> u64 {
        match self {
            BondedFetchRefusal::RateLimited(d) => u64::try_from(d.as_millis()).unwrap_or(u64::MAX),
            BondedFetchRefusal::Busy => 1_000,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct PeerWindow {
    started: Instant,
    count: u32,
    in_flight: u32,
}

/// Per-peer and global admission. Pure: the caller passes `now`.
#[derive(Debug)]
pub struct BondedFetchGovernor {
    limits: BondedFetchLimits,
    peers: FxHashMap<PeerId, PeerWindow>,
    in_flight: u32,
}

impl BondedFetchGovernor {
    pub fn new(limits: BondedFetchLimits) -> Self {
        Self { limits, peers: FxHashMap::default(), in_flight: 0 }
    }

    pub fn in_flight(&self) -> u32 {
        self.in_flight
    }

    /// Admit one request from `peer`, counting it in flight until [`Self::release`].
    pub fn admit(&mut self, peer: PeerId, now: Instant) -> Result<(), BondedFetchRefusal> {
        let limits = self.limits;
        if !self.peers.contains_key(&peer) && self.peers.len() >= limits.max_tracked_peers {
            self.peers.retain(|_, w| {
                w.in_flight > 0 || now.saturating_duration_since(w.started) < limits.window
            });
            if self.peers.len() >= limits.max_tracked_peers {
                return Err(BondedFetchRefusal::Busy);
            }
        }
        let window = self
            .peers
            .entry(peer)
            .or_insert(PeerWindow { started: now, count: 0, in_flight: 0 });
        if now.saturating_duration_since(window.started) >= limits.window {
            window.started = now;
            window.count = 0;
        }
        if window.count >= limits.per_peer_per_window {
            let wait = limits.window.saturating_sub(now.saturating_duration_since(window.started));
            return Err(BondedFetchRefusal::RateLimited(wait));
        }
        if window.in_flight >= limits.per_peer_in_flight || self.in_flight >= limits.global_in_flight {
            return Err(BondedFetchRefusal::Busy);
        }
        window.count += 1;
        window.in_flight += 1;
        self.in_flight += 1;
        Ok(())
    }

    /// The admitted request from `peer` was answered or failed.
    pub fn release(&mut self, peer: &PeerId) {
        if let Some(window) = self.peers.get_mut(peer) {
            if window.in_flight > 0 {
                window.in_flight -= 1;
                self.in_flight = self.in_flight.saturating_sub(1);
            }
        }
    }
}

/// Event-loop state for the protocol: admission, the unanswered response
/// channels, and the queue to the application (`None` when not serving).
pub(crate) struct BondedFetchServe {
    governor: BondedFetchGovernor,
    pending: FxHashMap<InboundRequestId, (PeerId, ResponseChannel<BondedFetchResponse>)>,
    app_tx: Option<mpsc::Sender<InboundBondedFetch>>,
}

impl BondedFetchServe {
    pub(crate) fn new(app_tx: Option<mpsc::Sender<InboundBondedFetch>>) -> Self {
        Self {
            governor: BondedFetchGovernor::new(BondedFetchLimits::default()),
            pending: FxHashMap::default(),
            app_tx,
        }
    }

    pub(crate) fn on_event(
        &mut self,
        event: request_response::Event<BondedFetchRequest, BondedFetchResponse>,
        behaviour: &mut request_response::Behaviour<BondedFetchCodec>,
        now: Instant,
    ) {
        match event {
            request_response::Event::Message {
                peer,
                message: request_response::Message::Request { request_id, request, channel },
                ..
            } => {
                if let Err(refusal) = self.governor.admit(peer, now) {
                    debug!("bonded fetch {} from {peer} refused: {}", request.label(), refusal.code());
                    let _ = behaviour.send_response(
                        channel,
                        BondedFetchResponse::retry_later(
                            refusal.code(),
                            "bonded fetch admission refused; retry later",
                            refusal.retry_after_ms(),
                        ),
                    );
                    return;
                }
                let Some(app_tx) = self.app_tx.as_ref() else {
                    self.governor.release(&peer);
                    let _ = behaviour.send_response(
                        channel,
                        BondedFetchResponse::error("UNAVAILABLE", "this node does not serve bonded snapshots"),
                    );
                    return;
                };
                match app_tx.try_send(InboundBondedFetch { peer_id: peer, request_id, request }) {
                    Ok(()) => {
                        self.pending.insert(request_id, (peer, channel));
                    },
                    Err(error) => {
                        self.governor.release(&peer);
                        let response = match error {
                            mpsc::error::TrySendError::Full(_) => BondedFetchResponse::retry_later(
                                "BUSY",
                                "bonded fetch queue full; retry later",
                                1_000,
                            ),
                            mpsc::error::TrySendError::Closed(_) => BondedFetchResponse::error(
                                "UNAVAILABLE",
                                "bonded snapshot service is not running",
                            ),
                        };
                        let _ = behaviour.send_response(channel, response);
                    },
                }
            },
            request_response::Event::InboundFailure { peer, request_id, error, .. } => {
                if let Some((owner, _)) = self.pending.remove(&request_id) {
                    self.governor.release(&owner);
                }
                debug!("bonded fetch inbound failure from {peer}: {error}");
            },
            // A node never sends bonded-fetch requests; only the CLI client does.
            request_response::Event::Message {
                message: request_response::Message::Response { .. },
                ..
            }
            | request_response::Event::OutboundFailure { .. }
            | request_response::Event::ResponseSent { .. } => {},
        }
    }

    /// Answer an admitted request. Unknown ids (already failed or timed out)
    /// are dropped.
    pub(crate) fn respond(
        &mut self,
        request_id: InboundRequestId,
        response: BondedFetchResponse,
        behaviour: &mut request_response::Behaviour<BondedFetchCodec>,
    ) {
        let Some((peer, channel)) = self.pending.remove(&request_id) else {
            debug!("bonded fetch answer for unknown request {request_id:?}");
            return;
        };
        self.governor.release(&peer);
        if behaviour.send_response(channel, response).is_err() {
            warn!("bonded fetch answer to {peer} could not be sent (connection closed)");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> BondedFetchLimits {
        BondedFetchLimits {
            per_peer_per_window: 3,
            window: Duration::from_secs(60),
            per_peer_in_flight: 2,
            global_in_flight: 3,
            max_tracked_peers: 2,
        }
    }

    #[test]
    fn per_peer_in_flight_and_window_budgets_hold_and_recover() {
        let mut g = BondedFetchGovernor::new(limits());
        let a = PeerId::random();
        let t = Instant::now();
        g.admit(a, t).unwrap();
        g.admit(a, t).unwrap();
        assert_eq!(g.admit(a, t), Err(BondedFetchRefusal::Busy), "two in flight");
        g.release(&a);
        g.admit(a, t).unwrap();
        g.release(&a);
        g.release(&a);
        match g.admit(a, t + Duration::from_secs(10)) {
            Err(BondedFetchRefusal::RateLimited(wait)) => assert_eq!(wait, Duration::from_secs(50)),
            other => panic!("expected rate limit, got {other:?}"),
        }
        // Refusals do not consume budget, and the window rolls over.
        g.admit(a, t + Duration::from_secs(60)).unwrap();
        assert_eq!(g.in_flight(), 1);
    }

    #[test]
    fn global_in_flight_and_tracked_peer_ceilings_are_shared() {
        let mut g = BondedFetchGovernor::new(limits());
        let (a, b, c) = (PeerId::random(), PeerId::random(), PeerId::random());
        let t = Instant::now();
        g.admit(a, t).unwrap();
        g.admit(a, t).unwrap();
        g.admit(b, t).unwrap();
        assert_eq!(g.admit(b, t), Err(BondedFetchRefusal::Busy), "global ceiling");
        assert_eq!(g.admit(c, t), Err(BondedFetchRefusal::Busy), "table full of busy peers");
        g.release(&a);
        g.release(&a);
        g.release(&b);
        // Idle but still inside their windows: still tracked.
        assert_eq!(g.admit(c, t), Err(BondedFetchRefusal::Busy));
        // Once their windows lapse they are dropped for a newcomer.
        g.admit(c, t + Duration::from_secs(61)).unwrap();
        // Releasing an unknown or idle peer is harmless.
        g.release(&PeerId::random());
        g.release(&a);
        assert_eq!(g.in_flight(), 1);
    }

    #[test]
    fn refusal_codes_and_retry_hints() {
        assert_eq!(BondedFetchRefusal::Busy.code(), "BUSY");
        assert_eq!(BondedFetchRefusal::RateLimited(Duration::from_millis(1500)).retry_after_ms(), 1500);
        assert_eq!(BondedFetchRefusal::RateLimited(Duration::ZERO).code(), "RATE_LIMITED");
    }
}
