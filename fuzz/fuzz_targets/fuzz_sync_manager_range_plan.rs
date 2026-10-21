#![no_main]

//! Fuzz target for SyncManager range-planning arithmetic.
//!
//! R100 surfaced two bugs on this surface:
//!   - `note_observed_height` not bumping per-peer heights (PR #1666) — caused no
//!     peer to be eligible for the updated range and IBD stalled.
//!   - `refill_pending_ranges` not covering heights below the in-memory window
//!     (PR #1667) — caused `InboundBlockSyncRequest` to serve empty responses
//!     during IBD, silently stalling sync.
//!
//! This target drives the full range-planning pipeline with arbitrary inputs:
//!
//!   SyncManager::new(local_height)
//!     → record_peer_status(peer, height) × N   (N ∈ [0, 4])
//!     → note_observed_height(observed_height)
//!     → plan_requests(Instant::now())
//!
//! Input layout (big-endian u64s):
//!   bytes  0..8   local_height
//!   bytes  8..16  observed_height
//!   bytes 16..    peer heights, up to MAX_PEERS × 8 bytes (remaining)
//!
//! All parsed heights are masked to HEIGHT_MASK (16 bits) — see that const for
//! why. Invariants checked after every call to plan_requests:
//!   1. No panic (implicit — libfuzzer treats any panic as a finding).
//!   2. Every PlannedRequest has count > 0 (no degenerate zero-width range).
//!   3. No two planned ranges overlap (SyncManager guarantees non-overlapping
//!      ranges; a violation would indicate a refill_pending_ranges regression).
//!
//! Run (requires cargo-fuzz / nightly):
//!   cargo +nightly fuzz run fuzz_sync_manager_range_plan -- -max_total_time=60

use std::sync::OnceLock;
use std::time::Instant;

use libfuzzer_sys::fuzz_target;
use libp2p_identity::PeerId;
use wrfcoin_p2p::sync::SyncManager;

const MAX_PEERS: usize = 4;
const HEADER_LEN: usize = 16; // 8 bytes local_height + 8 bytes observed_height

/// Bound every parsed height to a realistic chain-height domain (0..=65_535).
///
/// `local_height`/`observed_height`/peer heights are otherwise fully-arbitrary
/// u64s. A gap of up to ~1.8e19 between local and observed would make
/// `refill_pending_ranges`/`plan_requests` materialize an enormous pending-range
/// vector — a guaranteed libfuzzer OOM that reflects the input domain, not a
/// SyncManager defect. Masking to 16 bits keeps the target exercising the
/// planning *logic* (empty gaps, observed < local, dense multi-peer ranges,
/// boundary arithmetic) while staying well inside the allocator budget.
const HEIGHT_MASK: u64 = 0xFFFF;

/// Pool of 4 deterministic PeerIds, generated once per process.
///
/// The PeerId byte values do not influence range-planning arithmetic — only
/// their identity (are they equal?) matters for the SyncManager's peer map.
/// We initialise from fixed ed25519 seeds so corpus entries are reproducible.
static PEER_IDS: OnceLock<[PeerId; 4]> = OnceLock::new();

fn peer_ids() -> &'static [PeerId; 4] {
    PEER_IDS.get_or_init(|| {
        std::array::from_fn(|i| {
            // Each seed differs only in byte[0]; SHA-512 expansion inside ed25519
            // guarantees the resulting keypairs are distinct and well-formed for
            // any non-zero seed.
            let mut seed = [0u8; 32];
            seed[0] = (i as u8).wrapping_add(1);
            seed[16] = 0xAB;
            let secret = libp2p_identity::ed25519::SecretKey::try_from_bytes(&mut seed)
                .expect("32-byte ed25519 seed is always valid");
            let kp = libp2p_identity::ed25519::Keypair::from(secret);
            PeerId::from(libp2p_identity::PublicKey::from(kp.public()))
        })
    })
}

fuzz_target!(|data: &[u8]| {
    if data.len() < HEADER_LEN {
        return;
    }

    let local_height =
        u64::from_be_bytes(data[0..8].try_into().expect("8 bytes for local_height")) & HEIGHT_MASK;
    let observed_height =
        u64::from_be_bytes(data[8..16].try_into().expect("8 bytes for observed_height")) & HEIGHT_MASK;

    // Parse up to MAX_PEERS peer heights from the remaining input.
    let peer_bytes = &data[HEADER_LEN..];
    let n_peers = (peer_bytes.len() / 8).min(MAX_PEERS);

    let peers = peer_ids();
    let mut mgr = SyncManager::new(local_height);

    for i in 0..n_peers {
        let height =
            u64::from_be_bytes(peer_bytes[i * 8..(i + 1) * 8].try_into().expect("8 bytes")) & HEIGHT_MASK;
        mgr.record_peer_status(peers[i], height);
    }

    mgr.note_observed_height(observed_height);

    let plans = mgr.plan_requests(Instant::now());

    // --- Invariant checks ---

    for p in &plans {
        // Invariant 2: every planned range must be non-empty.
        assert!(
            p.request.count > 0,
            "degenerate zero-count BlockRequest at start_height={}",
            p.request.start_height
        );
    }

    // Invariant 3: no two planned ranges overlap.
    // refill_pending_ranges fills sequential non-overlapping segments; plan_requests
    // assigns each segment to at most one peer per call.
    for i in 0..plans.len() {
        let a = &plans[i].request;
        let a_end = a.start_height.saturating_add(a.count as u64);
        for j in (i + 1)..plans.len() {
            let b = &plans[j].request;
            let b_end = b.start_height.saturating_add(b.count as u64);
            let no_overlap = a_end <= b.start_height || b_end <= a.start_height;
            assert!(
                no_overlap,
                "overlapping planned ranges [{},{}) and [{},{})",
                a.start_height,
                a_end,
                b.start_height,
                b_end
            );
        }
    }
});
