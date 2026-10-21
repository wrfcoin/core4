//! Regression suite for mid-IBD peer disconnect / reconnect resume (#1596).
//!
//! Covers the desktop-peer hardening scenario where a home-network dropout or
//! node restart interrupts sync mid-IBD. The acceptance criteria are:
//!
//! - After reconnect the peer resumes from the last committed block, not from
//!   genesis — no wasted re-download of already-validated blocks.
//! - The SyncManager never re-requests a range that has already been fully
//!   applied to the local chain.
//! - If a range was in-flight when the peer disconnected, the range is
//!   requeued and re-assigned on reconnect without manual intervention.
//!
//! The tests drive `SyncManager` directly so they don't require a live swarm.
//! All three tests use only the public `wrfcoin-p2p` API so they also guard
//! against future refactors that accidentally break the event-loop wiring in
//! `native-chain/src/bin/launch_api/p2p_events.rs`.

use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use libp2p::{
    PeerId, StreamProtocol,
    request_response::{Behaviour as RequestResponseBehaviour, Config, ProtocolSupport},
};
use wrfcoin_p2p::sync::{
    BLOCK_SYNC_PROTOCOL, BlockRequest, BlockResponse, BlockSyncCodec, SyncManager, SyncState,
};
use wrfcoin_shared_protocol::{
    blockchain::{block::Block, chain::Blockchain, transaction::Transaction},
    crypto::hash::{Hash, hash_bytes},
};

fn make_block(height: u64, prev_hash: Hash, ts: DateTime<Utc>) -> Block {
    let mut b = Block::new(1, prev_hash, Vec::<Transaction>::new(), 1, height);
    b.timestamp = ts;
    let h = b.calculate_hash();
    b.set_hash(h);
    b
}

fn build_chain(count: u64) -> Vec<Block> {
    let base = Utc::now() - chrono::Duration::seconds(count as i64 + 60);
    let mut prev = hash_bytes(b"genesis");
    (1..=count)
        .map(|h| {
            let b = make_block(h, prev, base + chrono::Duration::seconds(h as i64));
            prev = b.hash.expect("hash set");
            b
        })
        .collect()
}

fn behaviour() -> RequestResponseBehaviour<BlockSyncCodec> {
    RequestResponseBehaviour::with_codec(
        BlockSyncCodec,
        std::iter::once((
            StreamProtocol::new(BLOCK_SYNC_PROTOCOL),
            ProtocolSupport::Full,
        )),
        Config::default(),
    )
}

/// Helper: register a request and return its id.
fn register(
    mgr: &mut SyncManager,
    beh: &mut RequestResponseBehaviour<BlockSyncCodec>,
    peer: PeerId,
    start: u64,
    count: u32,
    now: Instant,
) -> libp2p::request_response::OutboundRequestId {
    let req = BlockRequest { start_height: start, count };
    let id = beh.send_request(&peer, req.clone());
    mgr.register_request(id, peer, &req, now);
    id
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 1 — disconnect before any response, range requeued, reconnect replans
// ─────────────────────────────────────────────────────────────────────────────
//
// Simulates: desktop peer is partway through planning its first IBD window
// (range registered, request sent) when the network drops. `remove_peer` must
// put the in-flight range back so the next `plan_requests` after reconnect
// immediately redispatches it — without the operator restarting the node.

#[test]
fn disconnect_before_response_requeues_range_and_reconnect_replans() {
    const TARGET: u64 = 100;
    let peer = PeerId::random();
    let mut mgr = SyncManager::new(0);
    let mut beh = behaviour();
    let now = Instant::now();

    mgr.record_peer_status(peer, TARGET);
    assert_eq!(*mgr.state(), SyncState::Discovering);

    // IBD planning dispatches the first window (1..=100).
    let planned = mgr.plan_requests(now);
    assert_eq!(planned.len(), 1);
    assert_eq!(planned[0].request.start_height, 1);
    assert_eq!(planned[0].request.count, 100);

    // Register the request so the manager tracks it as in-flight.
    let req_id = beh.send_request(&peer, planned[0].request.clone());
    mgr.register_request(req_id, peer, &planned[0].request, now);

    // ── DISCONNECT ──────────────────────────────────────────────────────────
    mgr.remove_peer_at(&peer, now + Duration::from_millis(1));

    // With no peers left the state must become Idle — NOT stuck in Downloading.
    assert_eq!(
        *mgr.state(),
        SyncState::Idle,
        "state must revert to Idle after sole peer disconnects"
    );
    // The in-flight entry must be cancelled.
    // We verify indirectly: plan_requests with no known peers returns nothing.
    assert!(
        mgr.plan_requests(now).is_empty(),
        "no peers → plan_requests must return empty"
    );

    // ── RECONNECT ───────────────────────────────────────────────────────────
    mgr.record_peer_status_at(peer, TARGET, now + Duration::from_millis(2));
    assert_eq!(
        *mgr.state(),
        SyncState::Discovering,
        "state must advance to Discovering when the peer re-announces"
    );

    assert!(
        mgr.plan_requests(now + Duration::from_millis(2)).is_empty(),
        "reconnect must preserve the transport-failure cooldown"
    );

    // After the bounded first backoff, plan_requests redispatches the requeued
    // 1..=100 window without requiring a process restart.
    let replanned = mgr.plan_requests(now + Duration::from_secs(12));
    assert_eq!(
        replanned.len(),
        1,
        "exactly one range must be replanned after reconnect"
    );
    assert_eq!(
        replanned[0].request.start_height, 1,
        "reconnect must restart the in-progress window, not skip to a higher range"
    );
    assert_eq!(
        replanned[0].peer_id, peer,
        "the range must be assigned to the reconnected peer"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 — partial response applied → disconnect → reconnect resumes from tip
// ─────────────────────────────────────────────────────────────────────────────
//
// Simulates: desktop receives blocks 1–50 of a 1–100 window (network stall,
// not a disconnect), then the peer drops. On reconnect the sync must continue
// from height 51, not re-download from genesis.

#[test]
fn partial_ibd_resume_from_contiguous_tip_not_genesis() {
    const TARGET: u64 = 100;
    let blocks = build_chain(TARGET);
    let peer = PeerId::random();
    let mut mgr = SyncManager::new(0);
    let mut beh = behaviour();
    let mut local = Blockchain::default();
    let now = Instant::now();

    mgr.record_peer_status(peer, TARGET);

    // Register the full 1–100 window.
    let req_id = register(&mut mgr, &mut beh, peer, 1, 100, now);

    // Deliver only the first half (blocks 1–50).  handle_response applies
    // them and requeues the remainder (51–100) into pending_ranges.
    let applied = mgr
        .handle_response(
            req_id,
            BlockResponse { blocks: blocks[..50].to_vec(), retry_after_ms: None },
            now,
            &mut local,
        )
        .expect("first half must apply cleanly");
    assert_eq!(applied.len(), 50);
    assert_eq!(local.height, 50);
    assert_eq!(mgr.local_height(), 50);

    // ── DISCONNECT ──────────────────────────────────────────────────────────
    // The peer had no in-flight request at this point (the first one was
    // consumed by handle_response); only a pending range 51–100 remains.
    mgr.remove_peer(&peer);
    assert_eq!(*mgr.state(), SyncState::Idle);

    // ── RECONNECT ───────────────────────────────────────────────────────────
    mgr.record_peer_status(peer, TARGET);

    let resumed = mgr.plan_requests(now);
    assert_eq!(resumed.len(), 1, "exactly one pending range after reconnect");

    let r = &resumed[0].request;
    assert!(
        r.start_height > 50,
        "resume must start ABOVE the already-applied tip (height 50), got start={}",
        r.start_height,
    );
    assert_eq!(
        r.start_height, 51,
        "resume must start at exactly height 51 — the next missing block"
    );

    // Register and serve the remaining blocks.
    let req_id2 = beh.send_request(&peer, r.clone());
    mgr.register_request(req_id2, peer, r, now);
    let applied2 = mgr
        .handle_response(
            req_id2,
            BlockResponse { blocks: blocks[50..].to_vec(), retry_after_ms: None },
            now,
            &mut local,
        )
        .expect("second half must apply cleanly");
    assert_eq!(applied2.len(), 50);
    assert_eq!(local.height, TARGET);
    assert_eq!(mgr.local_height(), TARGET);
    assert_eq!(*mgr.state(), SyncState::Complete);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3 — idempotent re-download: already-applied blocks silently skipped
// ─────────────────────────────────────────────────────────────────────────────
//
// Simulates: a full range is in-flight while another path has already
// advanced the local chain (e.g., gossipsub during a connectivity gap). When
// the stale response finally arrives, it overlaps with blocks already applied
// locally. `ingest_block` must silently skip duplicates; the chain must not
// panic or corrupt.

#[test]
fn already_applied_blocks_in_redownloaded_range_are_idempotently_skipped() {
    const TARGET: u64 = 20;
    let blocks = build_chain(TARGET);
    let peer = PeerId::random();
    let mut mgr = SyncManager::new(0);
    let mut beh = behaviour();
    let mut local = Blockchain::default();
    let now = Instant::now();

    mgr.record_peer_status(peer, TARGET);

    // ── First partial sync (blocks 1–10) via a side channel ─────────────────
    // Represents blocks that arrived via gossipsub while IBD was paused.
    let side_id = register(&mut mgr, &mut beh, peer, 1, 10, now);
    let side_applied = mgr
        .handle_response(
            side_id,
            BlockResponse { blocks: blocks[..10].to_vec(), retry_after_ms: None },
            now,
            &mut local,
        )
        .expect("side-channel blocks 1–10 must apply cleanly");
    assert_eq!(side_applied.len(), 10);
    assert_eq!(local.height, 10);

    // ── Stale full-window response arrives after local progress ─────────────
    // This models an in-flight 1–20 request whose response arrives after blocks
    // 1–10 have already been committed through another path. The response length
    // still matches the registered request, but its first 10 blocks are now
    // duplicates and must be skipped idempotently.
    let full_id = register(&mut mgr, &mut beh, peer, 1, 20, now);

    let reapplied = mgr
        .handle_response(
            full_id,
            BlockResponse { blocks: blocks.clone(), retry_after_ms: None },
            now,
            &mut local,
        )
        .expect("overlapping 1–20 response must succeed without error or panic");

    assert_eq!(
        reapplied.len(),
        10,
        "exactly the 10 new blocks (11–20) must be reported as applied"
    );
    assert_eq!(local.height, TARGET, "chain must reach target height after reconnect");
    assert_eq!(mgr.local_height(), TARGET);
    assert_eq!(*mgr.state(), SyncState::Complete);

    // Calling plan_requests again must not produce spurious new windows.
    let extra = mgr.plan_requests(now + Duration::from_secs(1));
    assert!(
        extra.is_empty(),
        "no further requests must be planned once sync is complete"
    );
}
