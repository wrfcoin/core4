//! #2117 — bounding what an unauthenticated peer can make this node commit.
//!
//! # What this bounds, and what it does NOT
//!
//! #2117 asks for this section explicitly, and #2112's red-team pass names the
//! risk it exists to prevent: *"'memory allocated before authentication' is hard
//! to attribute precisely in an async libp2p stack without instrumenting the
//! allocator. Risk of shipping a proxy metric that does not bound the real
//! thing."* So, plainly:
//!
//! **Bounded here:** the number of entries a peer can cause this node to retain
//! in an admission map keyed by data that peer chooses. Entry *count*, per peer.
//!
//! **NOT bounded here:** total allocator bytes attributable to pre-authentication
//! work. There is no honest way to measure that from inside the application
//! without an instrumented allocator, and a `max_bytes_total` field that
//! approximated it would be exactly the proxy metric the story warns against —
//! a number that looks like a guard while bounding nothing. The story's
//! `max_bytes_total` / `max_bytes_per_conn` are therefore **deliberately not
//! implemented**; see the PR body for the argument.
//!
//! **Already bounded elsewhere, so not rebuilt here:**
//! - *Concurrent unauthenticated connections.* `max_pending_incoming` (default
//!   32) is handed to libp2p's `connection_limits::Behaviour`, which rejects
//!   before the Noise handshake — i.e. before authentication, which is the
//!   definition the story wants.
//! - *Signature-verification concurrency.* The story asks for a semaphore sized
//!   by `max_verify_concurrency`. Verification runs inline on the single p2p
//!   event loop, so concurrency is already exactly 1; adding a semaphore would
//!   bound nothing that is not already bounded. (The event loop being the bound
//!   is its own problem — see #2522 — but it is a scheduling problem, not an
//!   unbounded-concurrency one.)
//!
//! # The defect this was written for
//!
//! `WeatherDataAdmission` keys two maps by attacker-chosen data —
//! `last_peer_station_verify` by `(PeerId, station_id)` and `seen_nonces` by
//! `(station_id, nonce)` — where `station_id` arrives in the gossip payload and
//! is used **before the station signature is verified** (deliberately: cheap
//! checks first). Both maps were pruned by TTL alone, never by count.
//!
//! Measured, not estimated: 50 000 distinct `station_id`s from **one** peer
//! produced 50 000 entries in each map, and took **~16 s of CPU** — because
//! `prune()` ran a full `retain` over both maps on every single `admit`, so per
//! message cost grows with the number of entries already held. The memory growth
//! and the CPU amplification are the same bug.
//!
//! # Why the cap is PER PEER, and why that is the load-bearing decision
//!
//! A global count cap would trade an OOM for a **security regression**. Both maps
//! carry guarantees that eviction weakens: evicting from `seen_nonces` re-opens
//! replay, and evicting from `last_peer_station_verify` re-opens rate limiting.
//! With a global cap, one attacker can flush *other peers'* entries and reopen
//! those guarantees for everyone — strictly worse than the memory growth it fixes.
//!
//! Capping **per peer** contains the damage to the peer causing it: an attacker
//! can only evict their own entries, so they can only weaken rate limiting
//! against themselves. See `one_peers_flood_cannot_evict_another_peers_entries`,
//! which is the test that would catch a well-intentioned "just cap the map"
//! change.

use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

/// Per-peer ceiling on retained admission entries.
///
/// Sized so honest traffic never evicts: a real station emits on the order of
/// one observation per minute, and `WEATHER_DATA_NONCE_TTL` is 10 minutes, so a
/// peer relaying for even a few hundred stations stays far below this. It binds
/// only on something generating distinct station identifiers in bulk, which no
/// honest peer does.
pub const DEFAULT_MAX_ENTRIES_PER_PEER: usize = 4_096;

/// Why an entry was dropped, for metrics and logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvictionReason {
    /// Older than the TTL — the intended path, no guarantee weakened.
    Expired,
    /// The owning peer exceeded its entry quota. **Observable on purpose**: this
    /// means that peer has weakened its own replay/rate guarantees, and a peer
    /// doing it repeatedly is behaving like an attacker.
    QuotaExceeded,
}

/// A TTL-and-count bounded map whose capacity is enforced **per owner**.
///
/// `O`: the owner the quota applies to (a `PeerId`, as a string).
/// `K`: the full key, which may include attacker-chosen components.
#[derive(Debug)]
pub struct PerOwnerBoundedMap<K: Ord + Clone, V> {
    ttl: Duration,
    max_per_owner: usize,
    entries: BTreeMap<K, (V, Instant)>,
    /// Insertion order per owner, so eviction is oldest-first and O(1) amortised
    /// rather than a scan.
    order: BTreeMap<String, VecDeque<K>>,
    evicted_expired: u64,
    evicted_quota: u64,
}

impl<K: Ord + Clone, V> PerOwnerBoundedMap<K, V> {
    pub fn new(ttl: Duration, max_per_owner: usize) -> Self {
        Self {
            ttl,
            max_per_owner,
            entries: BTreeMap::new(),
            order: BTreeMap::new(),
            evicted_expired: 0,
            evicted_quota: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Entries currently attributed to one owner.
    pub fn len_for(&self, owner: &str) -> usize {
        self.order.get(owner).map_or(0, VecDeque::len)
    }

    pub const fn evicted_expired(&self) -> u64 {
        self.evicted_expired
    }

    /// Count of quota evictions — the signal that a peer is flooding.
    pub const fn evicted_quota(&self) -> u64 {
        self.evicted_quota
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.entries.contains_key(key)
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        self.entries.get(key).map(|(v, _)| v)
    }

    /// When this key was last inserted.
    ///
    /// The map already tracks this for TTL, and re-inserting refreshes it — which
    /// is exactly "last seen" semantics — so callers doing rate limiting read it
    /// here instead of storing a second copy that could drift from the TTL clock.
    pub fn inserted_at(&self, key: &K) -> Option<Instant> {
        self.entries.get(key).map(|(_, at)| *at)
    }

    /// Insert, evicting this owner's oldest entry if the owner is at quota.
    ///
    /// Returns the eviction that happened, if any, so the caller can count it.
    pub fn insert(&mut self, owner: &str, key: K, value: V, now: Instant) -> Option<EvictionReason> {
        let mut evicted = None;

        let queue = self.order.entry(owner.to_string()).or_default();
        if !self.entries.contains_key(&key) {
            while queue.len() >= self.max_per_owner {
                if let Some(oldest) = queue.pop_front() {
                    self.entries.remove(&oldest);
                    self.evicted_quota += 1;
                    evicted = Some(EvictionReason::QuotaExceeded);
                } else {
                    break;
                }
            }
            queue.push_back(key.clone());
        }
        self.entries.insert(key, (value, now));
        evicted
    }

    /// Drop expired entries.
    ///
    /// **Not called from the hot path.** The original defect was a full `retain`
    /// on every admission, making per-message cost grow with entries held; the
    /// per-owner quota is what bounds memory, so this only needs to run
    /// periodically to reclaim idle entries.
    pub fn prune_expired(&mut self, now: Instant) {
        let ttl = self.ttl;
        let before = self.entries.len();
        self.entries
            .retain(|_, (_, seen_at)| now.saturating_duration_since(*seen_at) <= ttl);
        self.evicted_expired += (before - self.entries.len()) as u64;
        let live: std::collections::BTreeSet<&K> = self.entries.keys().collect();
        let live: std::collections::BTreeSet<K> = live.into_iter().cloned().collect();
        self.order.retain(|_, queue| {
            queue.retain(|k| live.contains(k));
            !queue.is_empty()
        });
    }
}

// ---------------------------------------------------------------------------
// #2117, second half. `PerOwnerBoundedMap` above bounds how much an owner may
// ACCUMULATE; the types below bound how much work it may make us DO. Same
// issue, disjoint mechanisms — kept in one file so there is one place to read
// the pre-authentication budget rather than two that sound alike
// (#2531 + #2547).
// ---------------------------------------------------------------------------

// Explicit ceilings on what an unauthenticated peer can make this node commit
// (#2117).
//
// # What this bounds
//
// Stated first and plainly, because #2117 requires it and because a budget
// whose units are vague is a budget nobody can reason about.
//
// This bounds **declared sizes of in-flight work we accept on behalf of a peer
// that has not proven chain identity**, and **the number of signature
// verifications such a peer can make us perform per time window**.
//
// It does **not** bound:
//
// - Total allocator usage. Nothing here calls into the allocator or observes
//   it. A reservation of *n* bytes is a statement that we accepted a frame
//   declaring *n* bytes, not a measurement of heap growth.
// - Memory held by libp2p beneath us — transport buffers, Noise state, Yamux
//   windows. Those are bounded by libp2p's own configuration and by the
//   connection ceilings in [`crate::quota`], not here.
// - Fragmentation, amortised growth in maps we keep, or anything the allocator
//   does with the bytes after we admit them.
//
// So the honest description is: this is an **admission-control budget over
// declared work**, not a memory accountant. Where the two diverge — a peer
// declaring a small frame that expands into a large structure — the defence is
// the per-message size ceilings in [`crate::message`], not this module. Saying
// "memory budget" without that distinction would imply a guarantee the code
// cannot make.
//
// # `max_verify_concurrency` cannot be implemented as specified
//
// #2117 asks for a semaphore bounding concurrent signature verification "so
// invalid-signature CPU burn cannot saturate the runtime". There is no
// concurrent verification to bound: `verify_weather_gossip_signature` and its
// V2 twin are called **inline** in `handle_behaviour_event`, which runs on the
// single task spawned for `event_loop`. No `spawn_blocking`, no per-message
// task. Verification concurrency is therefore already exactly 1, and a
// semaphore over it could never block — a guard that cannot fire.
//
// The underlying exposure is real, and worse than the issue describes.
// Because verification is serial *on the event loop*, an attacker flooding
// weather messages with bogus signatures does not "saturate the runtime" — it
// **stalls the P2P event loop entirely**, taking block gossip, sync and every
// other branch of the `select!` down with it. The attacker supplies the public
// key in the message, so each attempt costs a full ed25519 verification and
// costs them nothing but bytes. This is the #1832/#1833 shape.
//
// A concurrency limit cannot help a serial loop. What helps is bounding *how
// much verification work an unproven peer may cause per unit time*, which is
// what [`VerifyBudget`] does. That is the same intent — an explicit, measured
// ceiling on committed resources — expressed on the axis that exists.

// (`Duration`/`Instant` already imported above.)

use libp2p::PeerId;
use rustc_hash::FxHashMap;

/// Ceilings on unauthenticated work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreAuthLimits {
    /// Total inbound gossip bytes accepted across **all** peers per
    /// [`Self::verify_window`]. `0` disables.
    ///
    /// There is deliberately no per-connection twin here: that ceiling already
    /// exists as `InboundRateLimit` in `node.rs`, applied per peer per window.
    /// Adding a second per-connection byte limit would give one ceiling two
    /// independent sources of truth, which is how they drift.
    ///
    /// The aggregate is the part that was missing, and it is the part that
    /// matters for #2077's failure: N peers each individually *under* the
    /// per-peer limit still sum to an unbounded total. A per-peer ceiling
    /// cannot bound a sum over an attacker-chosen N.
    pub max_bytes_total: usize,
    /// Signature verifications one unproven peer may cause per
    /// [`Self::verify_window`]. `0` disables.
    pub max_verify_ops_per_window: u32,
    /// Window over which [`Self::max_verify_ops_per_window`] is counted.
    pub verify_window: Duration,
}

impl Default for PreAuthLimits {
    /// Sized so that legitimate traffic never notices and a flood is bounded
    /// well below the point where the event loop stalls.
    ///
    /// `max_bytes_total = 64 MiB` per second across every peer. On a ~12-node
    /// network carrying blocks, transactions and weather data this is orders of
    /// magnitude above real traffic, so it never binds in normal operation —
    /// but it caps the sum that #2077 showed can otherwise grow without bound.
    ///
    /// `max_verify_ops_per_window = 64` per second. A weather station publishes
    /// on the order of once per block, so 64/s is ~three orders of magnitude
    /// above legitimate use, while capping an attacker at 64 ed25519
    /// verifications per second of event-loop time instead of unbounded.
    fn default() -> Self {
        Self {
            max_bytes_total: 64 * 1024 * 1024,
            max_verify_ops_per_window: 64,
            verify_window: Duration::from_secs(1),
        }
    }
}

/// Why a reservation was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetRefusal {
    /// The shared pre-authentication budget is exhausted.
    TotalExhausted,
    /// This peer has caused its allowance of verifications this window.
    VerifyRateExceeded,
}

impl BudgetRefusal {
    pub fn as_str(self) -> &'static str {
        match self {
            BudgetRefusal::TotalExhausted => "total_exhausted",
            BudgetRefusal::VerifyRateExceeded => "verify_rate_exceeded",
        }
    }
}

impl std::fmt::Display for BudgetRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Aggregate inbound-byte ceiling across all peers, per window.
///
/// Refusal is total rather than degrading: when the budget is exhausted we drop
/// the message outright. Degrading — accepting the bytes but doing less with
/// them — keeps the allocation while losing the guarantee, which is the worst
/// of both.
#[derive(Debug)]
pub struct PreAuthBudget {
    limits: PreAuthLimits,
    window_started: Option<Instant>,
    used: usize,
}

impl PreAuthBudget {
    pub fn new(limits: PreAuthLimits) -> Self {
        Self {
            limits,
            window_started: None,
            used: 0,
        }
    }

    pub fn limits(&self) -> &PreAuthLimits {
        &self.limits
    }

    /// Account `bytes` against the shared ceiling.
    ///
    /// All-or-nothing: a refusal accounts nothing, so a refused message cannot
    /// consume the budget it was refused for.
    pub fn try_reserve(&mut self, bytes: usize, now: Instant) -> Result<(), BudgetRefusal> {
        if self.limits.max_bytes_total == 0 {
            return Ok(()); // disabled
        }

        match self.window_started {
            Some(started) if now.duration_since(started) < self.limits.verify_window => {},
            _ => {
                self.window_started = Some(now);
                self.used = 0;
            },
        }

        if self.used.saturating_add(bytes) > self.limits.max_bytes_total {
            return Err(BudgetRefusal::TotalExhausted);
        }
        self.used += bytes;
        Ok(())
    }

    /// Bytes accounted in the current window.
    pub fn used_in_window(&self) -> usize {
        self.used
    }
}

/// Per-peer ceiling on signature verifications per time window.
///
/// This is the replacement for #2117's `max_verify_concurrency`; see the module
/// docs for why a concurrency limit cannot apply to a serial event loop.
///
/// Time is passed in rather than read from the clock so the window can be
/// tested by advancing a value rather than sleeping.
#[derive(Debug)]
pub struct VerifyBudget {
    limits: PreAuthLimits,
    windows: FxHashMap<PeerId, Window>,
}

#[derive(Debug, Clone, Copy)]
struct Window {
    started: Instant,
    used: u32,
}

impl VerifyBudget {
    pub fn new(limits: PreAuthLimits) -> Self {
        Self {
            limits,
            windows: FxHashMap::default(),
        }
    }

    /// Claim one signature verification for `peer`.
    ///
    /// `Err` means the caller must **skip the verification entirely** — not
    /// perform it and discard the result. The whole point is that the ed25519
    /// work never happens.
    pub fn try_claim(&mut self, peer: PeerId, now: Instant) -> Result<(), BudgetRefusal> {
        if self.limits.max_verify_ops_per_window == 0 {
            return Ok(()); // disabled
        }

        let window = self.windows.entry(peer).or_insert(Window {
            started: now,
            used: 0,
        });

        if now.duration_since(window.started) >= self.limits.verify_window {
            window.started = now;
            window.used = 0;
        }

        if window.used >= self.limits.max_verify_ops_per_window {
            return Err(BudgetRefusal::VerifyRateExceeded);
        }

        window.used += 1;
        Ok(())
    }

    /// Forget a peer's window.
    pub fn release_peer(&mut self, peer: &PeerId) {
        self.windows.remove(peer);
    }

    /// Drop windows that have fully expired.
    ///
    /// Without this the map is keyed by peer identity and grows for every peer
    /// ever seen — which is #2077's bug shape exactly, and is what #2117's
    /// "no unbounded map keyed by unauthenticated peer identity" criterion is
    /// about. `release_peer` on disconnect is not sufficient on its own: a peer
    /// that is never cleanly disconnected would otherwise persist forever.
    pub fn evict_expired(&mut self, now: Instant) {
        let window = self.limits.verify_window;
        self.windows
            .retain(|_, w| now.duration_since(w.started) < window);
    }

    /// Peers with a live window.
    pub fn tracked_peers(&self) -> usize {
        self.windows.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn map() -> PerOwnerBoundedMap<(String, String), ()> {
        PerOwnerBoundedMap::new(Duration::from_secs(600), 4)
    }

    fn key(owner: &str, k: &str) -> (String, String) {
        (owner.to_string(), k.to_string())
    }

    /// The defect, as a bound: one owner flooding distinct keys stays at quota
    /// instead of growing without limit.
    #[test]
    fn one_owner_cannot_grow_past_its_quota() {
        let mut m = map();
        let now = Instant::now();
        for i in 0..10_000 {
            m.insert("peer-a", key("peer-a", &format!("s{i}")), (), now);
        }
        assert_eq!(m.len_for("peer-a"), 4, "quota must bind");
        assert_eq!(m.len(), 4, "and total must not exceed it for a single owner");
        assert!(m.evicted_quota() > 0, "the flood must be observable");
    }

    /// THE load-bearing property. A global cap would let one attacker flush other
    /// peers' entries, re-opening replay and rate limiting for everyone — trading
    /// an OOM for a security regression. Per-owner quotas contain the damage to
    /// the peer causing it.
    #[test]
    fn one_peers_flood_cannot_evict_another_peers_entries() {
        let mut m = map();
        let now = Instant::now();
        for i in 0..3 {
            m.insert("honest", key("honest", &format!("h{i}")), (), now);
        }
        assert_eq!(m.len_for("honest"), 3);

        for i in 0..10_000 {
            m.insert("attacker", key("attacker", &format!("a{i}")), (), now);
        }

        assert_eq!(
            m.len_for("honest"),
            3,
            "an honest peer's entries must survive another peer's flood — losing \
             them would re-open replay protection and rate limiting for that peer"
        );
        for i in 0..3 {
            assert!(
                m.contains_key(&key("honest", &format!("h{i}"))),
                "honest entry h{i} must still be present"
            );
        }
    }

    /// Eviction is oldest-first, so the most recent (and therefore most
    /// rate-limit-relevant) entries survive.
    #[test]
    fn quota_eviction_drops_the_oldest_first() {
        let mut m = map();
        let now = Instant::now();
        for i in 0..6 {
            m.insert("p", key("p", &format!("k{i}")), (), now);
        }
        assert!(!m.contains_key(&key("p", "k0")), "k0 was oldest, must be gone");
        assert!(!m.contains_key(&key("p", "k1")));
        assert!(m.contains_key(&key("p", "k5")), "k5 is newest, must remain");
    }

    /// Re-inserting an existing key must refresh it, not consume a second slot —
    /// otherwise an honest station reporting repeatedly evicts itself.
    #[test]
    fn reinserting_the_same_key_does_not_consume_quota() {
        let mut m = map();
        let now = Instant::now();
        for _ in 0..100 {
            m.insert("p", key("p", "same"), (), now);
        }
        assert_eq!(m.len_for("p"), 1);
        assert_eq!(m.evicted_quota(), 0, "no eviction should have been needed");
    }

    #[test]
    fn expired_entries_are_reclaimed_and_counted() {
        let mut m = map();
        let t0 = Instant::now();
        m.insert("p", key("p", "a"), (), t0);
        m.insert("p", key("p", "b"), (), t0);
        assert_eq!(m.len(), 2);

        m.prune_expired(t0 + Duration::from_secs(601));
        assert_eq!(m.len(), 0, "TTL must still reclaim");
        assert_eq!(m.evicted_expired(), 2);
        assert_eq!(m.len_for("p"), 0, "owner order must be reclaimed too, not leaked");
    }

    /// The order index must not outlive its entries, or the fix leaks the very
    /// thing it bounds.
    #[test]
    fn the_owner_index_does_not_leak_after_expiry() {
        let mut m = map();
        let t0 = Instant::now();
        for i in 0..4 {
            m.insert("p", key("p", &format!("k{i}")), (), t0);
        }
        m.prune_expired(t0 + Duration::from_secs(601));
        assert!(m.is_empty());
        // A fresh insert must get a full quota again, not a stale queue.
        for i in 0..4 {
            m.insert("p", key("p", &format!("n{i}")), (), t0 + Duration::from_secs(602));
        }
        assert_eq!(m.len_for("p"), 4);
        assert_eq!(
            m.evicted_quota(),
            0,
            "a stale order queue would have forced an eviction here"
        );
    }

    /// Many owners each within quota is fine — the bound is per owner by design,
    /// so total memory scales with *connected peers*, which is itself capped by
    /// `max_established_total`.
    #[test]
    fn total_scales_with_owners_which_are_separately_capped() {
        let mut m = map();
        let now = Instant::now();
        for p in 0..10 {
            for i in 0..4 {
                m.insert(&format!("p{p}"), key(&format!("p{p}"), &format!("k{i}")), (), now);
            }
        }
        assert_eq!(m.len(), 40, "10 owners x quota 4");
        assert_eq!(m.evicted_quota(), 0);
    }

    fn peer(n: u8) -> PeerId {
        let mut bytes = [0u8; 32];
        bytes[0] = n;
        let key = libp2p::identity::ed25519::SecretKey::try_from_bytes(bytes).unwrap();
        libp2p::identity::Keypair::from(libp2p::identity::ed25519::Keypair::from(key))
            .public()
            .to_peer_id()
    }

    fn limits() -> PreAuthLimits {
        PreAuthLimits::default()
    }

    // -- Aggregate byte budget -----------------------------------------------

    #[test]
    fn the_aggregate_budget_bounds_a_sum_no_per_peer_limit_could() {
        // The #2077 shape: every contributor is individually modest, and the
        // total is what kills the node.
        let mut budget = PreAuthBudget::new(PreAuthLimits {
            max_bytes_total: 1000,
            ..limits()
        });
        let now = Instant::now();

        // Ten "peers" each sending a small, individually reasonable message.
        for _ in 0..10 {
            assert_eq!(budget.try_reserve(100, now), Ok(()));
        }
        assert_eq!(budget.used_in_window(), 1000);

        assert_eq!(
            budget.try_reserve(1, now),
            Err(BudgetRefusal::TotalExhausted),
            "the sum must be bounded even though every contribution was small"
        );
    }

    #[test]
    fn a_refused_reservation_accounts_nothing() {
        let mut budget = PreAuthBudget::new(PreAuthLimits {
            max_bytes_total: 100,
            ..limits()
        });
        let now = Instant::now();
        assert_eq!(budget.try_reserve(90, now), Ok(()));

        assert_eq!(
            budget.try_reserve(50, now),
            Err(BudgetRefusal::TotalExhausted)
        );
        assert_eq!(
            budget.used_in_window(),
            90,
            "the refused 50 bytes must not have been charged"
        );
        // The remaining headroom is still usable.
        assert_eq!(budget.try_reserve(10, now), Ok(()));
    }

    #[test]
    fn the_aggregate_budget_refills_each_window() {
        let mut budget = PreAuthBudget::new(PreAuthLimits {
            max_bytes_total: 100,
            verify_window: Duration::from_secs(1),
            ..limits()
        });
        let start = Instant::now();
        assert_eq!(budget.try_reserve(100, start), Ok(()));
        assert_eq!(
            budget.try_reserve(1, start),
            Err(BudgetRefusal::TotalExhausted)
        );

        assert_eq!(
            budget.try_reserve(100, start + Duration::from_secs(1)),
            Ok(()),
            "a budget that never refills is a one-way ratchet to a dead node"
        );
    }

    #[test]
    fn an_oversized_single_message_is_refused_rather_than_wrapping() {
        // `saturating_add` must not let a huge value wrap into a small one.
        let mut budget = PreAuthBudget::new(PreAuthLimits {
            max_bytes_total: 1000,
            ..limits()
        });
        assert_eq!(
            budget.try_reserve(usize::MAX, Instant::now()),
            Err(BudgetRefusal::TotalExhausted)
        );
        assert_eq!(budget.used_in_window(), 0);
    }

    #[test]
    fn zero_disables_the_aggregate_budget() {
        let mut budget = PreAuthBudget::new(PreAuthLimits {
            max_bytes_total: 0,
            ..limits()
        });
        let now = Instant::now();
        for _ in 0..1000 {
            assert_eq!(budget.try_reserve(usize::MAX, now), Ok(()));
        }
    }

    // -- Verification budget -------------------------------------------------

    #[test]
    fn a_peer_is_capped_on_verifications_per_window() {
        let mut budget = VerifyBudget::new(PreAuthLimits {
            max_verify_ops_per_window: 3,
            verify_window: Duration::from_secs(1),
            ..limits()
        });
        let start = Instant::now();

        for i in 0..3 {
            assert_eq!(budget.try_claim(peer(1), start), Ok(()), "claim {i}");
        }
        assert_eq!(
            budget.try_claim(peer(1), start),
            Err(BudgetRefusal::VerifyRateExceeded),
            "the fourth ed25519 verification this second must not happen"
        );
    }

    #[test]
    fn one_peers_verification_flood_does_not_starve_another() {
        // The budget is per peer precisely so that an attacker exhausting its
        // own allowance cannot deny verification to honest peers.
        let mut budget = VerifyBudget::new(PreAuthLimits {
            max_verify_ops_per_window: 2,
            ..limits()
        });
        let start = Instant::now();

        budget.try_claim(peer(1), start).unwrap();
        budget.try_claim(peer(1), start).unwrap();
        assert_eq!(
            budget.try_claim(peer(1), start),
            Err(BudgetRefusal::VerifyRateExceeded)
        );

        assert_eq!(
            budget.try_claim(peer(2), start),
            Ok(()),
            "an honest peer must be unaffected by the attacker's exhaustion"
        );
    }

    #[test]
    fn the_verification_window_refills() {
        let mut budget = VerifyBudget::new(PreAuthLimits {
            max_verify_ops_per_window: 1,
            verify_window: Duration::from_secs(1),
            ..limits()
        });
        let start = Instant::now();
        budget.try_claim(peer(1), start).unwrap();
        assert!(budget.try_claim(peer(1), start).is_err());

        assert_eq!(
            budget.try_claim(peer(1), start + Duration::from_secs(1)),
            Ok(())
        );
    }

    #[test]
    fn zero_disables_the_verification_budget() {
        let mut budget = VerifyBudget::new(PreAuthLimits {
            max_verify_ops_per_window: 0,
            ..limits()
        });
        let start = Instant::now();
        for _ in 0..10_000 {
            assert_eq!(budget.try_claim(peer(1), start), Ok(()));
        }
    }

    // -- The unbounded-map criterion -----------------------------------------

    /// #2117: "No unbounded map or vec is keyed by unauthenticated peer
    /// identity." `VerifyBudget` keys by `PeerId`, so it is exactly the shape
    /// that OOM'd the node in #2077 if left to grow.
    #[test]
    fn expired_verification_windows_are_evicted() {
        let mut budget = VerifyBudget::new(PreAuthLimits {
            max_verify_ops_per_window: 10,
            verify_window: Duration::from_secs(1),
            ..limits()
        });
        let start = Instant::now();

        for i in 0..200 {
            budget.try_claim(peer(i as u8), start).unwrap();
        }
        assert_eq!(budget.tracked_peers(), 200);

        budget.evict_expired(start + Duration::from_secs(2));

        assert_eq!(
            budget.tracked_peers(),
            0,
            "a map keyed by unauthenticated peer identity must not grow \
             without bound — this is #2077's failure shape"
        );
    }

    #[test]
    fn eviction_keeps_peers_whose_window_is_still_live() {
        // Eviction must not discard live accounting, or an attacker could reset
        // its own budget just by waiting for a sweep.
        let mut budget = VerifyBudget::new(PreAuthLimits {
            max_verify_ops_per_window: 1,
            verify_window: Duration::from_secs(10),
            ..limits()
        });
        let start = Instant::now();
        budget.try_claim(peer(1), start).unwrap();

        budget.evict_expired(start + Duration::from_secs(1));

        assert_eq!(budget.tracked_peers(), 1);
        assert_eq!(
            budget.try_claim(peer(1), start + Duration::from_secs(1)),
            Err(BudgetRefusal::VerifyRateExceeded),
            "the peer's spent allowance must survive a sweep of its live window"
        );
    }

    #[test]
    fn disconnecting_releases_a_peers_window() {
        let mut budget = VerifyBudget::new(limits());
        let start = Instant::now();
        budget.try_claim(peer(1), start).unwrap();
        assert_eq!(budget.tracked_peers(), 1);
        budget.release_peer(&peer(1));
        assert_eq!(budget.tracked_peers(), 0);
    }

    #[test]
    fn refusal_labels_are_distinct() {
        assert_ne!(
            BudgetRefusal::TotalExhausted.as_str(),
            BudgetRefusal::VerifyRateExceeded.as_str()
        );
    }

    #[test]
    fn defaults_sit_far_above_legitimate_traffic() {
        // A ceiling that binds in normal operation is an outage, not a defence.
        let d = PreAuthLimits::default();
        assert!(d.max_bytes_total >= 16 * 1024 * 1024, "{}", d.max_bytes_total);
        assert!(d.max_verify_ops_per_window >= 16);
        assert!(!d.verify_window.is_zero(), "a zero window would refill every call");
    }
}
