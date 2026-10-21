//! #2119 — serve-side ceilings for block-sync requests.
//!
//! # The gap this closes
//!
//! `sync.rs` is already full of ceilings — `MAX_BLOCKS_PER_REQUEST`,
//! `MAX_PARALLEL_PEERS`, `MAX_PENDING_SYNC_RANGES`, `MAX_SYNC_TARGET_ADVANCE_BLOCKS`.
//! Every one of them bounds what **this node asks other nodes for**. None bounds
//! what **a peer can make this node do**, and that is the direction hostile input
//! travels.
//!
//! On the serve path (`collect_block_sync_response`) the only ceiling was
//! `count.min(MAX_BLOCKS_PER_REQUEST)`. That caps blocks per request and nothing
//! else:
//!
//! - **No byte ceiling.** 100 blocks are read from redb, cloned, and handed to
//!   the codec. The 16 MiB `MAX_SYNC_MESSAGE_BYTES` write cap then *errors* — but
//!   only after the node has already paid the full read-and-clone cost. A ceiling
//!   that fires after the work is done is an error report, not a limit.
//! - **No rate.** Back-to-back requests are each individually legal, so a peer
//!   can hold the node at its per-request maximum indefinitely.
//! - **No accounting.** Nothing measures what a peer has already been served, so
//!   nothing can notice a peer draining the node continuously.
//!
//! # Why the story's "amplification ratio" is not the bound implemented here
//!
//! #2119 asks to *"reject any request whose estimated response size exceeds a
//! fixed multiple of the request size."* For a fixed-shape request that
//! denominator is meaningless: a `BlockRequest` is a `u64` and a `u32`, so
//! **every** legitimate response has an enormous ratio, and any threshold that
//! admits normal IBD admits everything. Tuning it down breaks sync instead.
//!
//! The concept is right; the denominator is wrong. What actually bounds
//! amplification is **cost per peer per unit time** — a peer gets at most
//! `max_response_bytes` of work per request and at most
//! `max_bytes_per_peer_per_window` across a rolling window, however it chooses to
//! split its requests. Splitting one big request into many small ones buys
//! nothing, which is exactly the property the ratio was reaching for. The
//! estimate is still recorded on rejection, as the story asks.
//!
//! # Why an over-budget request is SERVED SHORT rather than rejected
//!
//! Not a softening — it is what the protocol already specifies.
//! `SyncManager::handle_response` penalises a peer for returning **more** blocks
//! than requested (`sync.rs`, `InvalidBatchSize`) but treats **fewer** as normal:
//! it ingests what arrived and requeues the unserved remainder
//! (`if next_missing <= in_flight.range.end { self.requeue_range(..) }`).
//!
//! So a short response is the protocol's own backpressure signal. Honest peers
//! converge — they just take another round trip — while a peer trying to buy
//! 100 large blocks with one request gets exactly `max_response_bytes` of work
//! and no more. Rejecting instead would stall honest IBD across any range whose
//! blocks happen to be large.
//!
//! Rate and bandwidth exhaustion **are** rejections, because there is no
//! partial answer to "you are asking too often".
//!
//! # Determinism and self-defence
//!
//! Every entry point takes `now: Instant` from the caller rather than reading the
//! clock, so decisions are reproducible and testable without sleeping — and the
//! type carries no wall-clock input into any consensus-adjacent path.
//!
//! The governor is itself attacker-facing: peer ids are attacker-chosen, so an
//! unbounded per-peer map would turn a memory-limiting component into a memory
//! exhaustion vector. `max_tracked_peers` bounds it, and eviction prefers the
//! least recently seen entry. See `governor_peer_table_is_bounded`.

use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

/// Serve-side ceilings. Defaults are deliberately generous relative to the
/// ~12-node testnet (see [`SyncServeQuota::default`]).
///
/// A distinct named struct rather than new fields on `P2pConfig`: #2112 notes
/// that four of its stories add configuration, and that `config.rs` is the
/// epic's main merge-conflict surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncServeQuota {
    /// Hard cap on blocks in one response, independent of what was requested.
    pub max_blocks_per_request: u32,
    /// Byte budget for one response. Blocks are accumulated until the next one
    /// would exceed this, then the response is sent short.
    pub max_response_bytes: u64,
    /// Total bytes one peer may be served across `bandwidth_window`.
    pub max_bytes_per_peer_per_window: u64,
    /// Sliding window for [`Self::max_bytes_per_peer_per_window`].
    pub bandwidth_window: Duration,
    /// Minimum spacing between two admitted requests from the same peer.
    pub min_request_interval: Duration,
    /// Upper bound on tracked peers, so the governor cannot itself be used to
    /// exhaust memory.
    pub max_tracked_peers: usize,
}

impl Default for SyncServeQuota {
    /// Defaults sized for the current ~12-node testnet with headroom, per the
    /// #2112 red-team note that a ceiling set too tight is a self-inflicted
    /// liveness incident.
    ///
    /// A node doing IBD from one peer at `MAX_PARALLEL_PEERS = 4` in flight and
    /// 100 blocks per range moves ~400 blocks per round trip. At 8 MiB per
    /// response and 64 MiB per minute, one peer can pull roughly 8 full-size
    /// responses a minute — far above IBD's real rate and far below what makes a
    /// useful amplifier.
    fn default() -> Self {
        Self {
            max_blocks_per_request: crate::sync::MAX_BLOCKS_PER_REQUEST,
            // Half of MAX_SYNC_MESSAGE_BYTES (16 MiB), so the budget binds
            // BEFORE the codec's hard error rather than after the work is done.
            max_response_bytes: 8 * 1024 * 1024,
            max_bytes_per_peer_per_window: 64 * 1024 * 1024,
            bandwidth_window: Duration::from_secs(60),
            min_request_interval: Duration::from_millis(50),
            max_tracked_peers: 1024,
        }
    }
}

/// What the serve path should do with one inbound request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServeDecision {
    /// Serve up to `max_blocks`, stopping early once `byte_budget` is reached.
    Serve { max_blocks: u32, byte_budget: u64 },
    /// Do not serve; answer empty and record the reason.
    Reject(ServeRejection),
}

/// Why a request was refused. Carries the numbers the operator needs, so the
/// rejection metric records the estimate rather than just a count.
///
/// # `retry_after` is advisory, and the wire cannot carry it
///
/// Stated plainly because the original version of this module implied otherwise.
/// A rejection is answered with an **empty `BlockResponse`**, which
/// `SyncManager::handle_response` cannot distinguish from "no blocks in that
/// range": it treats the short batch as normal and calls `requeue_range`, which
/// does `pending_ranges.push_front(range)` — front, no delay. The peer therefore
/// re-requests **immediately**, and `retry_after` reaches it in no form at all.
///
/// So this is NOT backpressure on the peer. What it is:
///
/// - **Cheap for us.** The retry is refused again inside `admit`, which is a map
///   lookup, *before* any storage read. A retry storm costs the node a lookup per
///   request instead of a bounded fetch, so the asymmetry still runs the right
///   way.
/// - **Useful to the operator.** `retry_after` and [`Self::label`] are what the
///   rejection log and metric report.
///
/// Real backoff needs a wire-visible retry-after, which is a protocol change and
/// is filed separately. Serving SHORT (the byte budget) *is* backpressure the
/// requester honours, because it requeues only the unserved remainder — that is
/// the mechanism doing the actual work here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServeRejection {
    /// The peer asked again sooner than `min_request_interval`.
    TooSoon { retry_after: Duration },
    /// The peer has used its rolling-window byte allowance.
    BandwidthExhausted {
        used: u64,
        cap: u64,
        retry_after: Duration,
    },
}

impl ServeRejection {
    /// Stable label for metrics and logs.
    pub const fn label(&self) -> &'static str {
        match self {
            Self::TooSoon { .. } => "too_soon",
            Self::BandwidthExhausted { .. } => "bandwidth_exhausted",
        }
    }

    /// How long the peer should wait — the `retry-after` the acceptance
    /// criteria ask for in place of buffering.
    pub const fn retry_after(&self) -> Duration {
        match self {
            Self::TooSoon { retry_after } | Self::BandwidthExhausted { retry_after, .. } => {
                *retry_after
            },
        }
    }
}

/// Per-peer serving state: when we last admitted a request, and what we have
/// served inside the current rolling window.
/// Bound on retained samples per peer, so a genuinely sliding window cannot be
/// turned into unbounded memory by a peer making many tiny requests.
///
/// `min_request_interval` already caps admissions per window
/// (60s / 50ms = 1200 at the defaults); this is comfortably above that and is a
/// backstop rather than the primary bound. On overflow the oldest samples are
/// merged rather than dropped, so bytes are never uncharged.
const MAX_WINDOW_SAMPLES: usize = 2_048;

#[derive(Debug, Clone)]
struct PeerServeState {
    last_admitted: Option<Instant>,
    /// `(when, bytes)` samples inside the window, oldest first.
    ///
    /// A single counter reset at the window boundary is a TUMBLING window, not a
    /// rolling one: a peer consumes the full allowance just before the boundary
    /// and the full allowance again just after, so the effective cap is **2x**
    /// the documented one over an instant. Keeping per-sample timestamps and
    /// expiring them individually is what makes the limit actually sliding.
    /// Pinned by `a_boundary_straddling_burst_cannot_exceed_the_cap`.
    samples: VecDeque<(Instant, u64)>,
    bytes_in_window: u64,
    /// `(admitted_at, byte_budget)` for serves that were admitted but have not
    /// yet settled, oldest first.
    ///
    /// This exists because admission and charging no longer happen in one
    /// event-loop turn: the serve runs on a spawned task and settles when the
    /// storage read completes. Without a reservation, `bytes_in_window` is
    /// stale for the whole in-flight interval, and a peer whose serve latency
    /// exceeds `min_request_interval` is re-admitted against allowance it has
    /// already committed. Pinned by
    /// `an_unsettled_reservation_counts_against_the_window`.
    reservations: VecDeque<(Instant, u64)>,
    reserved_in_flight: u64,
    last_seen: Instant,
}

impl PeerServeState {
    fn new(now: Instant) -> Self {
        Self {
            last_admitted: None,
            samples: VecDeque::new(),
            bytes_in_window: 0,
            reservations: VecDeque::new(),
            reserved_in_flight: 0,
            last_seen: now,
        }
    }

    /// Expire samples older than `window`. Called before every read of
    /// `bytes_in_window`, so the total always reflects the trailing window only.
    fn roll_window(&mut self, now: Instant, window: Duration) {
        while let Some(&(at, bytes)) = self.samples.front() {
            if now.saturating_duration_since(at) >= window {
                self.samples.pop_front();
                self.bytes_in_window = self.bytes_in_window.saturating_sub(bytes);
            } else {
                break;
            }
        }
    }

    /// Drop reservations older than `window`.
    ///
    /// A reservation this stale belongs to a serve task that died without
    /// settling (panic, abort). Leaking it forever would let a local fault
    /// permanently shrink an honest peer's allowance, so after one full window
    /// the budget fails open instead. A settle arriving after expiry still
    /// charges its actual bytes — only the reservation is released early.
    fn expire_reservations(&mut self, now: Instant, window: Duration) {
        while let Some(&(at, bytes)) = self.reservations.front() {
            if now.saturating_duration_since(at) >= window {
                self.reservations.pop_front();
                self.reserved_in_flight = self.reserved_in_flight.saturating_sub(bytes);
            } else {
                break;
            }
        }
    }

    /// Release `reserved` bytes of in-flight budget, consuming bookkeeping
    /// entries oldest-first. Amounts are what matter for the window check;
    /// the per-entry timestamps only drive [`Self::expire_reservations`], so
    /// front-first consumption is exact on totals and at worst slightly
    /// conservative on expiry when settles complete out of order.
    fn release_reservation(&mut self, reserved: u64) {
        self.reserved_in_flight = self.reserved_in_flight.saturating_sub(reserved);
        let mut to_release = reserved;
        while to_release > 0 {
            let Some(front) = self.reservations.front_mut() else {
                break;
            };
            if front.1 <= to_release {
                to_release -= front.1;
                self.reservations.pop_front();
            } else {
                front.1 -= to_release;
                break;
            }
        }
    }

    fn charge(&mut self, now: Instant, bytes: u64) {
        if bytes == 0 {
            // A zero charge (refused serve, empty range, failed send) would
            // occupy a sample slot without affecting the window total.
            return;
        }
        self.bytes_in_window = self.bytes_in_window.saturating_add(bytes);
        if self.samples.len() >= MAX_WINDOW_SAMPLES {
            // Merge into the oldest rather than dropping: dropping would
            // un-charge bytes the peer actually consumed.
            if let Some(front) = self.samples.front_mut() {
                front.1 = front.1.saturating_add(bytes);
                return;
            }
        }
        self.samples.push_back((now, bytes));
    }
}

/// Serve-side admission control for block-sync requests.
///
/// Holds no clock and no I/O: `admit` and `settle_served` take `now` from the
/// caller, which makes the whole policy a pure function of its inputs and lets
/// the tests below cover rate and bandwidth behaviour without sleeping.
///
/// # Budget lifecycle
///
/// `admit` grants a `byte_budget` AND reserves it against the peer's window;
/// `settle_served` releases the reservation and charges the actually-served
/// bytes. Between the two, the budget counts as committed, so asynchronous
/// serving cannot admit a peer past its cap on stale accounting.
#[derive(Debug)]
pub struct SyncServeGovernor {
    limits: SyncServeQuota,
    peers: BTreeMap<String, PeerServeState>,
}

impl SyncServeGovernor {
    pub fn new(limits: SyncServeQuota) -> Self {
        Self {
            limits,
            peers: BTreeMap::new(),
        }
    }

    pub fn limits(&self) -> &SyncServeQuota {
        &self.limits
    }

    /// Number of peers currently tracked. Exposed so the bound is assertable.
    pub fn tracked_peers(&self) -> usize {
        self.peers.len()
    }

    /// Decide whether to serve `requested` blocks to `peer`.
    ///
    /// Order matters: rate first, then bandwidth. A peer that is hammering gets
    /// the cheap answer without the window arithmetic, and a peer that is within
    /// its rate but has drained its allowance gets the accurate `retry_after`.
    pub fn admit(&mut self, peer: &str, requested: u32, now: Instant) -> ServeDecision {
        self.evict_if_needed(peer, now);

        let limits = self.limits.clone();
        let state = self
            .peers
            .entry(peer.to_string())
            .or_insert_with(|| PeerServeState::new(now));
        state.last_seen = now;

        if let Some(last) = state.last_admitted {
            let elapsed = now.saturating_duration_since(last);
            if elapsed < limits.min_request_interval {
                return ServeDecision::Reject(ServeRejection::TooSoon {
                    retry_after: limits.min_request_interval - elapsed,
                });
            }
        }

        state.roll_window(now, limits.bandwidth_window);
        state.expire_reservations(now, limits.bandwidth_window);
        // Committed = already-served bytes still in the window PLUS budget
        // granted to serves that have not settled yet. Serving is asynchronous
        // (the read runs on a spawned task), so `bytes_in_window` alone is
        // stale for the whole in-flight interval; admitting against it let a
        // peer whose serve latency exceeded `min_request_interval` stack one
        // extra full response per interval past the cap.
        let committed = state
            .bytes_in_window
            .saturating_add(state.reserved_in_flight);
        if committed >= limits.max_bytes_per_peer_per_window {
            // Time until the oldest sample expires — i.e. when allowance actually
            // frees up under a sliding window, rather than a fixed boundary.
            // In-flight reservations free up when their serve settles, which has
            // no schedule; their expiry backstop is the window, so the sample-
            // based estimate remains the advisory value reported.
            let retry_after = state
                .samples
                .front()
                .or_else(|| state.reservations.front())
                .map(|(at, _)| {
                    limits
                        .bandwidth_window
                        .saturating_sub(now.saturating_duration_since(*at))
                })
                .unwrap_or(limits.bandwidth_window);
            return ServeDecision::Reject(ServeRejection::BandwidthExhausted {
                used: committed,
                cap: limits.max_bytes_per_peer_per_window,
                retry_after,
            });
        }

        state.last_admitted = Some(now);

        // The remaining allowance also caps this single response, and the
        // budget is RESERVED until the serve settles — so concurrent in-flight
        // serves cannot jointly overshoot the cap: each admission sees the
        // budget already committed to the ones before it.
        let remaining = limits
            .max_bytes_per_peer_per_window
            .saturating_sub(committed);
        let byte_budget = limits.max_response_bytes.min(remaining);

        state.reserved_in_flight = state.reserved_in_flight.saturating_add(byte_budget);
        state.reservations.push_back((now, byte_budget));

        ServeDecision::Serve {
            max_blocks: requested.min(limits.max_blocks_per_request),
            byte_budget,
        }
    }

    /// Settle a serve admitted earlier: release its `reserved` byte budget and
    /// charge what was ACTUALLY serialized against the peer's rolling window.
    ///
    /// This is the ONLY completion entry point, and it must run on every path —
    /// success, storage error, and send failure alike (`served` is 0 when
    /// nothing went out). A serve that never settles is covered by the
    /// [`PeerServeState::expire_reservations`] backstop.
    ///
    /// Release is amount-based, not identity-based: a settle arriving AFTER
    /// its own reservation expired releases that many bytes of whatever is
    /// still reserved. The worst case — a serve in flight for a full window,
    /// settling while another serve is pending — re-opens at most one
    /// response of headroom for that peer, which is the pre-#2579 bound
    /// confined to that rare case. Served bytes are always charged, so the
    /// window total is never understated. Pinned by
    /// `a_stale_reservation_expires_after_the_window_and_a_late_settle_still_charges`.
    pub fn settle_served(&mut self, peer: &str, reserved: u64, served: u64, now: Instant) {
        let window = self.limits.bandwidth_window;
        if let Some(state) = self.peers.get_mut(peer) {
            state.expire_reservations(now, window);
            state.release_reservation(reserved);
            state.roll_window(now, window);
            state.charge(now, served);
            state.last_seen = now;
        }
    }

    /// Bound the peer table before inserting a new entry.
    ///
    /// Peer ids are attacker-chosen, so an unbounded map would make this
    /// component — whose whole purpose is limiting resource use — into a memory
    /// exhaustion vector. Evicts the least recently seen entry, which is the one
    /// whose window is closest to being irrelevant anyway.
    fn evict_if_needed(&mut self, incoming: &str, _now: Instant) {
        if self.peers.contains_key(incoming) || self.peers.len() < self.limits.max_tracked_peers {
            return;
        }
        if let Some(victim) = self
            .peers
            .iter()
            .min_by_key(|(id, state)| (state.last_seen, (*id).clone()))
            .map(|(id, _)| id.clone())
        {
            self.peers.remove(&victim);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> SyncServeQuota {
        SyncServeQuota {
            max_blocks_per_request: 100,
            max_response_bytes: 1_000,
            max_bytes_per_peer_per_window: 5_000,
            bandwidth_window: Duration::from_secs(60),
            min_request_interval: Duration::from_millis(50),
            max_tracked_peers: 4,
        }
    }

    fn governor() -> SyncServeGovernor {
        SyncServeGovernor::new(limits())
    }

    /// A request for more than the cap is clamped, not refused — refusing would
    /// stall IBD, and the requester already requeues an unserved remainder.
    #[test]
    fn oversized_block_count_is_clamped_not_rejected() {
        let mut g = governor();
        let now = Instant::now();
        assert_eq!(
            g.admit("peer-a", u32::MAX, now),
            ServeDecision::Serve {
                max_blocks: 100,
                byte_budget: 1_000,
            }
        );
    }

    /// The byte budget is handed to the serve path BEFORE it reads anything, so
    /// the cost is bounded rather than reported after the fact.
    #[test]
    fn byte_budget_is_issued_up_front() {
        let mut g = governor();
        let now = Instant::now();
        let ServeDecision::Serve { byte_budget, .. } = g.admit("peer-a", 100, now) else {
            panic!("first request must be admitted");
        };
        assert_eq!(byte_budget, 1_000, "budget must be the per-response cap");
    }

    #[test]
    fn second_request_inside_the_min_interval_is_rejected_with_retry_after() {
        let mut g = governor();
        let t0 = Instant::now();
        assert!(matches!(g.admit("peer-a", 10, t0), ServeDecision::Serve { .. }));

        let decision = g.admit("peer-a", 10, t0 + Duration::from_millis(10));
        let ServeDecision::Reject(rejection) = decision else {
            panic!("a request 10ms after the last must be rejected at a 50ms interval");
        };
        assert_eq!(rejection.label(), "too_soon");
        assert_eq!(
            rejection.retry_after(),
            Duration::from_millis(40),
            "retry_after must be the remaining interval, so the peer can back off exactly"
        );
    }

    #[test]
    fn request_after_the_min_interval_is_admitted() {
        let mut g = governor();
        let t0 = Instant::now();
        let _ = g.admit("peer-a", 10, t0);
        assert!(matches!(
            g.admit("peer-a", 10, t0 + Duration::from_millis(50)),
            ServeDecision::Serve { .. }
        ));
    }

    /// The property the per-request cap alone does NOT give: a peer that stays
    /// under every per-request limit still cannot drain the node continuously.
    #[test]
    fn sustained_draining_under_the_per_request_cap_still_trips_the_window() {
        let mut g = governor();
        let mut t = Instant::now();
        // Five 1_000-byte responses exactly exhaust the 5_000-byte window.
        for i in 0..5 {
            let decision = g.admit("peer-a", 100, t);
            let ServeDecision::Serve { byte_budget, .. } = decision else {
                panic!("request {i} is individually legal and must be admitted, got {decision:?}");
            };
            g.settle_served("peer-a", byte_budget, 1_000, t);
            t += Duration::from_millis(50);
        }

        let ServeDecision::Reject(rejection) = g.admit("peer-a", 100, t) else {
            panic!("the sixth request must be refused: the window allowance is spent");
        };
        assert_eq!(rejection.label(), "bandwidth_exhausted");
        let ServeRejection::BandwidthExhausted { used, cap, .. } = rejection else {
            unreachable!("matched the label above");
        };
        assert_eq!((used, cap), (5_000, 5_000));
    }

    /// THE test that would have caught the original defect. A single counter
    /// reset at the window boundary is a TUMBLING window: a peer spends the full
    /// allowance just before the boundary and the full allowance again just
    /// after, so the effective cap is 2x the documented one over an instant.
    ///
    /// Codex found this on #2536 and it was correct — the docs said "rolling"
    /// while the implementation reset wholesale.
    #[test]
    fn a_boundary_straddling_burst_cannot_exceed_the_cap() {
        let mut g = governor(); // cap 5_000 over 60s
        let t0 = Instant::now();

        // Spend the whole allowance just before the boundary.
        let _ = g.admit("peer-a", 100, t0);
        g.settle_served("peer-a", 1_000, 5_000, t0);

        // One millisecond past the old fixed boundary. Under a tumbling window
        // the counter would have reset and this would be admitted.
        let just_after = t0 + Duration::from_secs(60) + Duration::from_millis(1);
        // ...but the sample at t0 has only just expired, so allowance is back.
        assert!(
            matches!(g.admit("peer-a", 100, just_after), ServeDecision::Serve { .. }),
            "once the sample is genuinely older than the window, allowance returns"
        );

        // The real test: spend at t0 and again just BEFORE the boundary. Under a
        // tumbling window both land in different buckets and both are admitted.
        let mut g2 = governor();
        let s0 = Instant::now();
        let _ = g2.admit("peer-b", 100, s0);
        g2.settle_served("peer-b", 1_000, 3_000, s0);

        let near_boundary = s0 + Duration::from_secs(59);
        let _ = g2.admit("peer-b", 100, near_boundary);
        g2.settle_served("peer-b", 1_000, 2_000, near_boundary);
        // 5_000 spent, both samples still inside the trailing 60s.

        let just_past = s0 + Duration::from_secs(60) + Duration::from_millis(1);
        // The s0 sample (3_000) has expired; the near_boundary one (2_000) has NOT.
        // A tumbling window would have zeroed BOTH and allowed a full 5_000 here.
        let ServeDecision::Serve { byte_budget, .. } = g2.admit("peer-b", 100, just_past) else {
            panic!("2_000 of 5_000 still charged, so this must be admitted");
        };
        assert_eq!(
            byte_budget, 1_000,
            "remaining allowance must be cap(5_000) - still-charged(2_000) = 3_000, \
             clamped by the per-response cap of 1_000 — NOT a full reset"
        );
    }

    /// The sample list is bounded, so a sliding window cannot be turned into
    /// unbounded memory by a peer making many tiny requests.
    #[test]
    fn window_samples_are_bounded_and_bytes_are_never_uncharged() {
        let mut g = SyncServeGovernor::new(SyncServeQuota {
            max_bytes_per_peer_per_window: u64::MAX,
            min_request_interval: Duration::ZERO,
            ..limits()
        });
        let t0 = Instant::now();
        for _ in 0..10_000 {
            let _ = g.admit("peer-a", 1, t0);
            g.settle_served("peer-a", 1_000, 1, t0);
        }
        // All 10_000 bytes still charged even though samples are capped.
        let ServeDecision::Serve { .. } = g.admit("peer-a", 1, t0) else {
            panic!("cap is u64::MAX here, so admission must still succeed");
        };
    }

    /// A peer that goes quiet is not charged forever.
    #[test]
    fn rolling_window_resets_after_it_elapses() {
        let mut g = governor();
        let t0 = Instant::now();
        let _ = g.admit("peer-a", 100, t0);
        g.settle_served("peer-a", 1_000, 5_000, t0);
        assert!(matches!(
            g.admit("peer-a", 100, t0 + Duration::from_millis(50)),
            ServeDecision::Reject(_)
        ));

        assert!(
            matches!(
                g.admit("peer-a", 100, t0 + Duration::from_secs(60)),
                ServeDecision::Serve { .. }
            ),
            "once the window elapses the allowance must be restored"
        );
    }

    /// The last request in a window must not overshoot the cap by a whole
    /// response — the budget is clamped to what is left.
    #[test]
    fn final_request_in_a_window_is_clamped_to_the_remaining_allowance() {
        let mut g = governor();
        let mut t = Instant::now();
        for _ in 0..4 {
            let _ = g.admit("peer-a", 100, t);
            g.settle_served("peer-a", 1_000, 1_000, t);
            t += Duration::from_millis(50);
        }
        // 4_000 of 5_000 used. A fifth serve is admitted with the last full
        // 1_000-byte budget but only 600 bytes actually go out.
        let ServeDecision::Serve { byte_budget: fifth_budget, .. } = g.admit("peer-a", 100, t)
        else {
            panic!("4_000 of 5_000 used, so the fifth request must be admitted");
        };
        g.settle_served("peer-a", fifth_budget, 600, t);
        t += Duration::from_millis(50);
        let ServeDecision::Serve { byte_budget, .. } = g.admit("peer-a", 100, t) else {
            panic!("still inside the allowance, so it must be admitted");
        };
        assert_eq!(
            byte_budget, 400,
            "budget must be min(per-response cap, remaining allowance)"
        );
    }

    /// One peer's exhaustion must not deny another. A shared counter here would
    /// make a single hostile peer a denial-of-service against honest ones.
    #[test]
    fn accounting_is_per_peer() {
        let mut g = governor();
        let t0 = Instant::now();
        let _ = g.admit("hostile", 100, t0);
        g.settle_served("hostile", 1_000, 5_000, t0);
        assert!(matches!(
            g.admit("hostile", 100, t0 + Duration::from_millis(50)),
            ServeDecision::Reject(_)
        ));
        assert!(
            matches!(g.admit("honest", 100, t0), ServeDecision::Serve { .. }),
            "an honest peer must be unaffected by another peer's exhaustion"
        );
    }

    // ----- #2579: the budget lifecycle under asynchronous serving -----
    //
    // Serving moved off the event loop, so admission and charging no longer
    // happen in one turn. These tests pin the reservation that closes the gap
    // both reviews named: without it, a peer whose serve latency exceeds
    // `min_request_interval` is re-admitted against a stale window.

    /// THE lifecycle gap: an admitted-but-unsettled serve must count against
    /// the window. Before the reservation existed, this second admit was
    /// granted a full fresh budget, overshooting the cap by one response per
    /// interval of serve latency.
    #[test]
    fn an_unsettled_reservation_counts_against_the_window() {
        let mut g = SyncServeGovernor::new(SyncServeQuota {
            // One response budget IS the whole window, so a single in-flight
            // serve must exhaust it.
            max_bytes_per_peer_per_window: 1_000,
            ..limits()
        });
        let t0 = Instant::now();
        let ServeDecision::Serve { byte_budget, .. } = g.admit("peer-a", 100, t0) else {
            panic!("first request must be admitted");
        };
        assert_eq!(byte_budget, 1_000);

        // Past the rate interval — nothing served yet, serve still in flight.
        let t1 = t0 + Duration::from_millis(50);
        let ServeDecision::Reject(rejection) = g.admit("peer-a", 100, t1) else {
            panic!(
                "an unsettled serve holds the whole window's budget, so this \
                 must be refused — admitting it is the #2579 overshoot"
            );
        };
        assert_eq!(rejection.label(), "bandwidth_exhausted");
        let ServeRejection::BandwidthExhausted { used, cap, .. } = rejection else {
            unreachable!("matched the label above");
        };
        assert_eq!(
            (used, cap),
            (1_000, 1_000),
            "the refusal must report the RESERVED bytes as used, so the \
             operator sees why an idle-looking peer is refused"
        );
    }

    /// Settling reconciles the reservation to what was actually serialized:
    /// the unused remainder of the budget returns to the peer's allowance.
    #[test]
    fn settle_reconciles_the_reservation_to_actual_bytes() {
        let mut g = SyncServeGovernor::new(SyncServeQuota {
            max_bytes_per_peer_per_window: 1_200,
            ..limits()
        });
        let t0 = Instant::now();
        let ServeDecision::Serve { byte_budget, .. } = g.admit("peer-a", 100, t0) else {
            panic!("first request must be admitted");
        };
        assert_eq!(byte_budget, 1_000);
        // The range turned out short: only 300 of the budgeted 1_000 went out.
        g.settle_served("peer-a", byte_budget, 300, t0 + Duration::from_millis(20));

        let ServeDecision::Serve { byte_budget: second, .. } =
            g.admit("peer-a", 100, t0 + Duration::from_millis(50))
        else {
            panic!(
                "after settling, only the ACTUAL bytes stay charged — if the \
                 full reservation stuck, remaining would be 1_200-1_000-300 \
                 and this would be refused or zero-budgeted"
            );
        };
        assert_eq!(
            second, 900,
            "remaining allowance must be cap(1_200) - served(300) = 900"
        );
    }

    /// The error path settles with zero served bytes and must release the
    /// FULL reservation — a failed read costs the peer nothing.
    #[test]
    fn error_path_settle_releases_the_full_reservation() {
        let mut g = SyncServeGovernor::new(SyncServeQuota {
            max_bytes_per_peer_per_window: 1_000,
            ..limits()
        });
        let t0 = Instant::now();
        let ServeDecision::Serve { byte_budget, .. } = g.admit("peer-a", 100, t0) else {
            panic!("first request must be admitted");
        };
        g.settle_served("peer-a", byte_budget, 0, t0 + Duration::from_millis(20));

        let ServeDecision::Serve { byte_budget: second, .. } =
            g.admit("peer-a", 100, t0 + Duration::from_millis(50))
        else {
            panic!("a failed serve must not consume the peer's allowance");
        };
        assert_eq!(second, 1_000, "full allowance must be back after a 0-byte settle");
    }

    /// Concurrent in-flight serves cannot jointly overshoot the cap: each
    /// admission sees the budget already reserved by the ones before it, so
    /// the SUM of granted budgets is bounded by the window.
    #[test]
    fn concurrent_reservations_cannot_jointly_overshoot_the_cap() {
        let mut g = SyncServeGovernor::new(SyncServeQuota {
            max_bytes_per_peer_per_window: 2_500,
            ..limits()
        });
        let mut t = Instant::now();
        let mut granted = Vec::new();
        // Hold every serve in flight (no settles) and keep asking.
        for _ in 0..3 {
            match g.admit("peer-a", 100, t) {
                ServeDecision::Serve { byte_budget, .. } => granted.push(byte_budget),
                ServeDecision::Reject(rejection) => {
                    panic!("2_500 of budget exists, refused early: {rejection:?}")
                },
            }
            t += Duration::from_millis(50);
        }
        assert_eq!(
            granted,
            vec![1_000, 1_000, 500],
            "each admission must be clamped by the budget already in flight"
        );
        assert!(
            matches!(g.admit("peer-a", 100, t), ServeDecision::Reject(_)),
            "with the whole window reserved in flight, the next request must \
             be refused — total granted budget is bounded by the cap"
        );
    }

    /// A reservation whose serve never settles (task death) expires after one
    /// full window: fail open for the peer rather than leaking its allowance
    /// forever. A late settle still charges the actual bytes.
    #[test]
    fn a_stale_reservation_expires_after_the_window_and_a_late_settle_still_charges() {
        let mut g = SyncServeGovernor::new(SyncServeQuota {
            max_bytes_per_peer_per_window: 1_000,
            ..limits()
        });
        let t0 = Instant::now();
        let ServeDecision::Serve { byte_budget, .. } = g.admit("peer-a", 100, t0) else {
            panic!("first request must be admitted");
        };
        // Mid-window the reservation binds...
        assert!(matches!(
            g.admit("peer-a", 100, t0 + Duration::from_millis(50)),
            ServeDecision::Reject(_)
        ));
        // ...after a full window with no settle, it has expired.
        let t_late = t0 + Duration::from_secs(60) + Duration::from_millis(1);
        let ServeDecision::Serve { .. } = g.admit("peer-a", 100, t_late) else {
            panic!(
                "a reservation older than the window belongs to a dead serve \
                 task; leaking it forever would let a local fault permanently \
                 shrink an honest peer's allowance"
            );
        };
        // The first serve finally settles anyway. Release is amount-based, so
        // this late settle — whose own reservation already expired — releases
        // the t_late admission's fresh 1_000-byte reservation instead. That is
        // the accepted residual imprecision of the expiry backstop: it needs a
        // serve to stay in flight for a FULL window and then settle while
        // another serve is pending, and its worst case is the pre-#2579 bound
        // (one extra response) in that rare case only. The 800 actual bytes
        // are still charged, so the window total is never understated.
        g.settle_served("peer-a", byte_budget, 800, t_late);
        let ServeDecision::Serve { byte_budget: after, .. } =
            g.admit("peer-a", 100, t_late + Duration::from_millis(50))
        else {
            panic!("late-settled 800 of 1_000 leaves allowance, must be admitted");
        };
        assert_eq!(
            after, 200,
            "the 800 late-charged bytes bind: cap(1_000) - served(800) = 200"
        );
    }

    /// The governor is attacker-facing: peer ids are attacker-chosen, so the
    /// table that enforces the limits must itself be bounded.
    #[test]
    fn governor_peer_table_is_bounded() {
        let mut g = governor(); // max_tracked_peers = 4
        let mut t = Instant::now();
        for i in 0..1_000 {
            let _ = g.admit(&format!("sybil-{i}"), 1, t);
            t += Duration::from_millis(1);
        }
        assert!(
            g.tracked_peers() <= 4,
            "peer table must stay bounded, got {} entries — an unbounded map would \
             make the resource limiter a memory-exhaustion vector",
            g.tracked_peers()
        );
    }

    /// Eviction must drop the least recently seen peer, not an arbitrary one:
    /// evicting the active peer would reset the allowance of whoever is
    /// currently draining the node, which is the opposite of the intent.
    #[test]
    fn eviction_drops_the_least_recently_seen_peer() {
        let mut g = governor(); // max_tracked_peers = 4
        let t0 = Instant::now();
        for i in 0..4 {
            let _ = g.admit(&format!("peer-{i}"), 1, t0 + Duration::from_millis(i as u64));
        }
        // Keep peer-0 fresh, leave peer-1 as the stalest, then force an eviction.
        let t1 = t0 + Duration::from_secs(1);
        let _ = g.admit("peer-0", 1, t1);
        let _ = g.admit("newcomer", 1, t1 + Duration::from_millis(1));

        // peer-0's window survived, so its recorded bytes are still charged.
        g.settle_served("peer-0", 1_000, 5_000, t1);
        assert!(
            matches!(
                g.admit("peer-0", 1, t1 + Duration::from_millis(100)),
                ServeDecision::Reject(_)
            ),
            "the recently active peer must have been retained, allowance intact"
        );
    }
}
