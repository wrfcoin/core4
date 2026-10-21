//! Shared, cheaply-cloneable snapshot of per-peer sync status (#2820).
//!
//! The `SyncManager` lives inside the node event loop and cannot be borrowed
//! by the API layer, so the loop publishes a serializable snapshot here on the
//! same tick that already publishes the aggregate sync observation, and the
//! launch_api route reads it lock-free of the loop. Mirrors the
//! [`crate::discovered::DiscoveredPeers`] pattern (#1407).

use std::sync::{Arc, RwLock};

/// One peer's rotation-relevant sync state, flattened to plain data so the
/// API layer needs no access to `SyncManager` internals or `Instant`s.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SyncPeerSurfaceEntry {
    pub peer_id: String,
    pub advertised_height: u64,
    /// Height a peer must advertise to serve the next needed range.
    pub required_height: u64,
    /// Seconds since the advertised height was last refreshed; `None` when
    /// the entry predates timestamping.
    pub advertisement_age_secs: Option<u64>,
    /// Seconds since the peer's current connection was established.
    pub connection_age_secs: Option<u64>,
    pub in_flight_requests: u32,
    pub consecutive_transport_failures: u32,
    pub cooling_down: bool,
    pub retry_after_ms: u64,
    pub banned: bool,
    pub metered: bool,
    pub priority: i64,
    pub eligible: bool,
    /// How the peer qualifies when eligible: `strict` (normal height gate) or
    /// `fallback_probe` (qualifies only through the bounded stale-alternate
    /// probe, #2818, armed by a failure of the next needed range).
    pub eligible_via: Option<&'static str>,
    /// First disqualifier in selection-filter order: `banned` |
    /// `busy_in_flight` | `cooldown` | `failed_this_range` |
    /// `stale_advertisement`.
    pub ineligible_reason: Option<&'static str>,
}

/// Shared handle carried on `P2pHandle` / `P2pClient`.
#[derive(Debug, Clone, Default)]
pub struct SyncPeersSurface {
    inner: Arc<RwLock<Vec<SyncPeerSurfaceEntry>>>,
}

impl SyncPeersSurface {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the snapshot wholesale. Called from the node loop's sync tick;
    /// a poisoned lock is treated as an empty publish rather than a panic in
    /// the event loop.
    pub fn publish(&self, entries: Vec<SyncPeerSurfaceEntry>) {
        if let Ok(mut guard) = self.inner.write() {
            *guard = entries;
        }
    }

    /// Current snapshot (clone-out; the vector is bounded by connected-peer
    /// count).
    pub fn snapshot(&self) -> Vec<SyncPeerSurfaceEntry> {
        self.inner
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }
}
