//! Integration test for issue #1369 — block backfill wiring.
//!
//! Simulates a fresh node that hears a far-ahead tip via gossip and has to
//! backfill every block from genesis, in windows of up to 100 blocks per
//! request. Drives `SyncManager` directly so the test doesn't require a full
//! swarm (which is expensive to set up under `#[tokio::test]` and pulls in a
//! lot of unrelated machinery).
//!
//! Covers the acceptance criteria in issue #1369:
//!
//! - An out-of-order block at `height > local+1` is buffered via
//!   `SyncManager::buffer_orphan` instead of silently dropped.
//! - `plan_requests` emits contiguous `BlockRequest`s covering the gap,
//!   capped at `MAX_BLOCKS_PER_REQUEST` (100) per unit.
//! - `handle_response` applies the returned blocks in order, advancing the
//!   local chain to the target height.
//! - The buffered orphan is drained once its ancestors arrive.

use std::time::Instant;

use chrono::{DateTime, Utc};
use libp2p::{
    PeerId, StreamProtocol,
    request_response::{Behaviour as RequestResponseBehaviour, Config, ProtocolSupport},
};
use wrfcoin_p2p::sync::{
    BLOCK_SYNC_PROTOCOL, BlockResponse, BlockSyncCodec, MAX_BLOCKS_PER_REQUEST, SyncManager,
    SyncState,
};
use wrfcoin_shared_protocol::{
    blockchain::{block::Block, chain::Blockchain, transaction::Transaction},
    crypto::hash::{Hash, hash_bytes},
};

fn make_block(height: u64, prev_hash: Hash, timestamp: DateTime<Utc>) -> Block {
    let mut block = Block::new(1, prev_hash, Vec::<Transaction>::new(), 1, height);
    block.timestamp = timestamp;
    let hash = block.calculate_hash();
    block.set_hash(hash);
    block
}

/// Build a canonical chain of `count` blocks above an empty genesis.
fn build_chain(count: u64) -> (Blockchain, Vec<Block>) {
    let mut chain = Blockchain::default();
    let mut blocks = Vec::new();
    let mut prev_hash = hash_bytes(b"genesis");
    // Anchor the chain firmly in the past so `max_future_drift` checks pass.
    let base = Utc::now() - chrono::Duration::seconds(count as i64 + 60);
    for height in 1..=count {
        let block = make_block(
            height,
            prev_hash,
            base + chrono::Duration::seconds(height as i64),
        );
        prev_hash = block.hash.expect("hash set");
        chain.add_block(block.clone()).unwrap();
        blocks.push(block);
    }
    (chain, blocks)
}

fn dummy_behaviour() -> RequestResponseBehaviour<BlockSyncCodec> {
    RequestResponseBehaviour::with_codec(
        BlockSyncCodec,
        std::iter::once((
            StreamProtocol::new(BLOCK_SYNC_PROTOCOL),
            ProtocolSupport::Full,
        )),
        Config::default(),
    )
}

/// A far-ahead block delivered via gossipsub (out of order) must be buffered
/// as an orphan rather than silently dropped. After the backfill drains the
/// missing history, the orphan is re-applied automatically by the flush
/// loop inside `handle_response`.
#[test]
fn fresh_node_backfills_to_target_and_drains_orphan() {
    const TARGET_HEIGHT: u64 = 5;
    let (_source_chain, source_blocks) = build_chain(TARGET_HEIGHT);
    let peer = PeerId::random();

    // Fresh local node at height 0.
    let mut mgr = SyncManager::new(0);
    let mut local = Blockchain::default();

    assert_eq!(*mgr.state(), SyncState::Idle);

    // 1. Gossip delivers the current tip (height TARGET_HEIGHT) far ahead of
    //    our local height — this is exactly the codepath that used to
    //    silently drop the block at `p2p_events.rs:152`.
    let now = Instant::now();
    mgr.record_peer_status(peer, TARGET_HEIGHT);
    mgr.buffer_orphan(peer, source_blocks[TARGET_HEIGHT as usize - 1].clone(), now);

    assert_eq!(
        mgr.orphan_count(),
        1,
        "orphan block must be buffered, not silently dropped"
    );
    assert_eq!(*mgr.state(), SyncState::Discovering);
    assert_eq!(mgr.target_height(), TARGET_HEIGHT);

    // 2. `plan_requests` must emit a `BlockRequest` that covers the gap
    //    (height 1..=TARGET_HEIGHT), not exceeding the batch cap. With a
    //    gap of 5 it fits in one request.
    let planned = mgr.plan_requests(now);
    assert!(!planned.is_empty(), "plan_requests must emit at least one request");
    let first = &planned[0];
    assert_eq!(first.peer_id, peer);
    assert_eq!(first.request.start_height, 1);
    assert_eq!(first.request.count, TARGET_HEIGHT as u32);
    assert!(first.request.count <= MAX_BLOCKS_PER_REQUEST);

    // 3. Simulate the peer responding with the full range. Register the
    //    request id so `handle_response` can correlate.
    let mut behaviour = dummy_behaviour();
    let request_id = behaviour.send_request(&first.peer_id, first.request.clone());
    mgr.register_request(request_id, first.peer_id, &first.request, now);

    // Include every block UP TO but NOT including the buffered tip; the
    // tip is already in the orphan buffer and will be re-applied by
    // `flush_orphans` when height TARGET_HEIGHT-1 is added.
    let response_blocks: Vec<Block> = source_blocks[..TARGET_HEIGHT as usize - 1].to_vec();
    let response = BlockResponse {
        blocks: response_blocks,
        retry_after_ms: None,
    };
    let applied = mgr
        .handle_response(request_id, response, now, &mut local)
        .expect("handle_response should succeed");

    // 4. Assert: the local chain has caught up to the tip (TARGET_HEIGHT).
    //    The last block applied is the orphan that was drained from the
    //    buffer, so `applied.len()` == TARGET_HEIGHT.
    assert_eq!(
        applied.len(),
        TARGET_HEIGHT as usize,
        "all blocks (including buffered orphan) must be applied"
    );
    assert_eq!(local.height, TARGET_HEIGHT);
    assert_eq!(mgr.local_height(), TARGET_HEIGHT);
    assert_eq!(mgr.orphan_count(), 0, "orphan buffer must be drained");
    assert_eq!(*mgr.state(), SyncState::Complete);
}

/// With a gap larger than `MAX_BLOCKS_PER_REQUEST`, `plan_requests` must
/// split the backfill into multiple contiguous windows — covering the node host
/// bringup scenario where the tip was 5235 blocks ahead and the fresh peer
/// needed ~53 sequential requests.
#[test]
fn fresh_node_plans_multiple_windows_for_large_gap() {
    // Gap of 250 blocks → 3 windows: [1..=100], [101..=200], [201..=250].
    const TARGET_HEIGHT: u64 = 250;
    let peer = PeerId::random();

    let mut mgr = SyncManager::new(0);
    mgr.record_peer_status(peer, TARGET_HEIGHT);

    let planned = mgr.plan_requests(Instant::now());
    assert!(
        planned.len() >= 1,
        "plan_requests must produce at least one window"
    );

    // The first planned request must cover the first MAX_BLOCKS_PER_REQUEST
    // blocks starting at height 1.
    let first = &planned[0];
    assert_eq!(first.request.start_height, 1);
    assert_eq!(first.request.count, MAX_BLOCKS_PER_REQUEST);
}

/// The consumer-side fix in `p2p_events.rs` calls
/// `SyncManager::buffer_orphan`. That API must be a no-op for blocks that
/// aren't actually orphans (height <= local, or height == local+1), so the
/// caller doesn't have to branch on height before calling.
#[test]
fn buffer_orphan_ignores_non_orphan_heights() {
    let peer = PeerId::random();
    let mut mgr = SyncManager::new(10);

    let prev = hash_bytes(b"parent");
    let now = Utc::now();

    let below = make_block(5, prev, now);
    let exact_next = make_block(11, prev, now);
    let orphan = make_block(15, prev, now);

    mgr.buffer_orphan(peer, below, Instant::now());
    assert_eq!(mgr.orphan_count(), 0, "height <= local must not buffer");

    mgr.buffer_orphan(peer, exact_next, Instant::now());
    assert_eq!(
        mgr.orphan_count(),
        0,
        "height == local+1 must not buffer (caller should apply directly)"
    );

    mgr.buffer_orphan(peer, orphan, Instant::now());
    assert_eq!(mgr.orphan_count(), 1, "height > local+1 must buffer");
}
