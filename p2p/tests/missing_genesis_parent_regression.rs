//! Regression suite for the `MissingGenesisParent` IBD halt (#1482).
//!
//! These tests exercise the **public** `SyncManager` API only — they live
//! outside the `wrfcoin-p2p` crate to catch regressions that would let a
//! downstream consumer (e.g. `launch_api::p2p_events`, `/network/info`,
//! `/readyz`) see stale or misleading sync state during the halt.
//!
//! The bug class we are guarding against:
//!
//! * the IBD orphan-genesis hot loop (#1466 / node host 2026-04-27 outage) —
//!   re-introducing it would manifest as `plan_requests` dispatching the
//!   same non-converging range repeatedly while `chain.height` stays at 0
//! * silent recovery on peer churn — losing peers must NOT clear the halt
//!   because the underlying condition is a storage bug on the seed peer,
//!   not a peer-availability issue
//! * trusting peer-supplied height-0 material — the only sanctioned way
//!   out of the halt is a later batch carrying the exact missing parent
//!
//! The in-crate unit tests in `p2p/src/sync.rs` cover the same logic at a
//! lower level; this file is the external-API contract that downstream
//! plumbing can rely on.

use std::time::Instant;

use chrono::{DateTime, Utc};
use libp2p::{
    PeerId, StreamProtocol,
    request_response::{Behaviour as RequestResponseBehaviour, Config, ProtocolSupport},
};
use wrfcoin_p2p::sync::{
    BLOCK_SYNC_PROTOCOL, BlockRequest, BlockResponse, BlockSyncCodec, SyncError, SyncManager,
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

/// Build a canonical chain of `count` blocks above an empty genesis. The
/// first element of the returned vec is the "real" height-1 block whose
/// hash is what the desktop peer needs to recover from the halt.
fn build_real_chain(count: u64) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut prev_hash = hash_bytes(b"genesis");
    let base = Utc::now() - chrono::Duration::seconds(count as i64 + 60);
    for height in 1..=count {
        let block = make_block(
            height,
            prev_hash,
            base + chrono::Duration::seconds(height as i64),
        );
        prev_hash = block.hash.expect("hash set");
        blocks.push(block);
    }
    blocks
}

/// Mint a height-1 block with `prev=zeros` whose hash is guaranteed distinct
/// from `real_h1.hash`. This is the production-bug shape: a stale orphan
/// left over in the seed peer's `wrfcoin.redb` from a prior chain reset.
fn make_orphan_seed_distinct_from(real_h1: &Block, offset_secs: i64) -> Block {
    let orphan = make_block(
        1,
        Hash::default(),
        real_h1.timestamp + chrono::Duration::seconds(offset_secs),
    );
    assert_ne!(
        orphan.hash, real_h1.hash,
        "orphan must have a distinct hash from the real height-1 block",
    );
    orphan
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

/// Drive `SyncManager` into the halted state and return everything a test
/// needs to follow up. Asserts the halt fired and surfaces the `Stalled`
/// state before returning.
fn drive_into_halt(
    peer: PeerId,
    mgr: &mut SyncManager,
    behaviour: &mut RequestResponseBehaviour<BlockSyncCodec>,
    local: &mut Blockchain,
    real: &[Block],
) -> Hash {
    let orphan = make_orphan_seed_distinct_from(&real[0], -30);
    let required_parent = real[1].prev_block_hash;

    mgr.record_peer_status(peer, real.len() as u64);
    let request = BlockRequest {
        start_height: 1,
        count: real.len() as u32,
    };
    let request_id = behaviour.send_request(&peer, request.clone());
    mgr.register_request(request_id, peer, &request, Instant::now());

    let err = mgr
        .handle_response(
            request_id,
            BlockResponse {
                blocks: vec![
                    orphan,
                    real[1].clone(),
                    real[2].clone(),
                    real[3].clone(),
                ],
                retry_after_ms: None,
            },
            Instant::now(),
            local,
        )
        .expect_err("orphan + descendants without real parent must halt sync");
    assert_eq!(err, SyncError::MissingGenesisParent { required_parent });
    assert_eq!(*mgr.state(), SyncState::Stalled { required_parent });
    assert_eq!(mgr.missing_genesis_parent(), Some(required_parent));
    required_parent
}

/// `SyncState::Stalled` and `missing_genesis_parent()` must both be
/// observable from outside the `wrfcoin-p2p` crate. This is the contract
/// that downstream `launch_api` plumbing relies on to surface the halt in
/// `/network/info`, `/readyz`, and operator logs — without ever holding a
/// reference to `SyncManager`'s private fields.
#[test]
fn missing_genesis_parent_halt_surfaces_in_public_state() {
    let real = build_real_chain(4);
    let peer = PeerId::random();
    let mut mgr = SyncManager::new(0);
    let mut behaviour = dummy_behaviour();
    let mut local = Blockchain::default();

    let required = drive_into_halt(peer, &mut mgr, &mut behaviour, &mut local, &real);

    // Pattern-match on the public variant to confirm the field shape.
    let SyncState::Stalled { required_parent } = mgr.state().clone() else {
        panic!("public SyncState must expose Stalled, got {:?}", mgr.state());
    };
    assert_eq!(required_parent, required);
    assert_eq!(
        required_parent,
        real[1].prev_block_hash,
        "the surfaced hash must be exactly the prev_block_hash of the real height-2 block",
    );
    // The local chain must not have been poisoned by the dropped orphan.
    assert_eq!(local.height, 0);
    assert_eq!(local.tip, Hash::default());
}

/// While halted, `plan_requests` must stay within the bounded-probe contract
/// even when many peers advertise heights far ahead of local: exactly one
/// single-block probe for the missing parent, then silence under backoff —
/// never a `refill_pending_ranges` fan-out. Regression targets: dropping the
/// `missing_genesis_parent.is_some()` short-circuit brings the orphan-genesis
/// hot loop back; dropping the probe (#2071) brings back the silent stall
/// that stranded cold-start joiners at height 0 (#2036).
#[test]
fn plan_requests_stays_bounded_while_stalled_even_with_many_peers() {
    let real = build_real_chain(4);
    let peer = PeerId::random();
    let mut mgr = SyncManager::new(0);
    let mut behaviour = dummy_behaviour();
    let mut local = Blockchain::default();

    let _ = drive_into_halt(peer, &mut mgr, &mut behaviour, &mut local, &real);

    // Add multiple peers all advertising heights well above the local tip.
    // A range refill here would fan requests out across peers; the latched
    // contract allows only the single parent probe.
    for _ in 0..4 {
        mgr.record_peer_status(PeerId::random(), 1_000);
    }

    let now = Instant::now();
    let planned = mgr.plan_requests(now);
    assert_eq!(
        planned.len(),
        1,
        "latched planning must yield exactly the bounded parent probe; got {}",
        planned.len(),
    );
    assert_eq!(planned[0].request.start_height, 1, "probe targets the missing parent");
    assert_eq!(planned[0].request.count, 1, "probe is a single block, not a range");

    // Repeated calls within the backoff interval are no-ops — guarding
    // against state mutation that would let a later call fan out.
    for _ in 0..5 {
        assert!(mgr.plan_requests(now).is_empty());
    }
    assert_eq!(*mgr.state(), SyncState::Stalled { required_parent: real[1].prev_block_hash });
}

/// A new peer announcement during the halt must not transition the public
/// state out of `Stalled`. Regression target: `record_peer_status` at
/// `sync.rs:312-314` short-circuits on `missing_genesis_parent.is_some()`;
/// if that guard is removed, a fresh peer's status would flip state to
/// `Discovering` and downstream consumers would believe sync had resumed.
#[test]
fn record_peer_status_does_not_reset_stalled_state() {
    let real = build_real_chain(4);
    let peer = PeerId::random();
    let mut mgr = SyncManager::new(0);
    let mut behaviour = dummy_behaviour();
    let mut local = Blockchain::default();

    let required = drive_into_halt(peer, &mut mgr, &mut behaviour, &mut local, &real);

    // A new peer joining with a much higher tip must not change state.
    let fresh_peer = PeerId::random();
    mgr.record_peer_status(fresh_peer, 9_999);
    assert_eq!(
        *mgr.state(),
        SyncState::Stalled { required_parent: required },
        "fresh peer announcement during halt must not reset state",
    );
    assert_eq!(mgr.missing_genesis_parent(), Some(required));

    // Even a peer announcement at exactly the required-parent height must
    // not clear the halt — only an actual block batch carrying the parent
    // hash is sufficient (peer-supplied height advertisements are not
    // trusted material).
    mgr.record_peer_status(PeerId::random(), 1);
    assert_eq!(*mgr.state(), SyncState::Stalled { required_parent: required });
}

/// A failed in-flight request (peer stall / disconnect) during the halt must
/// not unlatch the halt or resume RANGE planning. Since #2071 the latched
/// path is no longer silent: `plan_requests` issues a single bounded probe
/// for the missing parent (`start = local+1, count = 1`, backed off by
/// `MISSING_PARENT_RETRY_INTERVAL`) instead of an empty vec — the silent
/// version stranded cold-start joiners at height 0 forever (#2036). What
/// `note_request_failure`'s unconditional `requeue_range` must NOT do is
/// leak the failed range back into planning while latched: everything
/// planned during the halt stays within the bounded-probe contract.
#[test]
fn note_request_failure_during_halt_plans_only_the_bounded_parent_probe() {
    let real = build_real_chain(4);
    let peer = PeerId::random();
    let mut mgr = SyncManager::new(0);
    let mut behaviour = dummy_behaviour();
    let mut local = Blockchain::default();

    let required = drive_into_halt(peer, &mut mgr, &mut behaviour, &mut local, &real);

    // Register a fresh in-flight request (as if we had been talking to a
    // second peer when the halt was tripped on the first), then signal a
    // peer-side failure for it.
    let req = BlockRequest {
        start_height: 1,
        count: 4,
    };
    let request_id = behaviour.send_request(&peer, req.clone());
    mgr.register_request(request_id, peer, &req, Instant::now());
    mgr.note_request_failure(request_id);

    // The halt must persist.
    assert_eq!(*mgr.state(), SyncState::Stalled { required_parent: required });

    // Planning while latched yields AT MOST the single bounded parent probe —
    // never the requeued 4-block range the failure tried to hand back.
    let now = Instant::now();
    let planned = mgr.plan_requests(now);
    assert!(
        planned.len() <= 1,
        "latched planning must never fan out; got {} requests",
        planned.len(),
    );
    if let Some(probe) = planned.first() {
        assert_eq!(probe.request.start_height, 1, "probe targets the missing parent");
        assert_eq!(probe.request.count, 1, "probe is a single block, not a range");
    }

    // Within the retry interval a second tick is silent (backoff holds), and
    // the halt still persists — the failure did not reset either.
    assert!(
        mgr.plan_requests(now).is_empty(),
        "probe must back off within MISSING_PARENT_RETRY_INTERVAL",
    );
    assert_eq!(*mgr.state(), SyncState::Stalled { required_parent: required });
}

/// Full lifecycle: halt → external operator recovery (real-genesis batch
/// arrives from a peer that has the correct height-1 block) → both signals
/// clear → IBD advances to `Complete`. Critically, the recovery path must
/// only fire on a block whose hash matches `required_parent` exactly; any
/// other height-1 candidate stays rejected (covered by the unit tests).
#[test]
fn recovery_clears_halt_and_advances_to_complete() {
    let real = build_real_chain(4);
    let peer = PeerId::random();
    let mut mgr = SyncManager::new(0);
    let mut behaviour = dummy_behaviour();
    let mut local = Blockchain::default();

    let required = drive_into_halt(peer, &mut mgr, &mut behaviour, &mut local, &real);

    // Peer churn must not clear the halt.
    mgr.remove_peer(&peer);
    assert_eq!(*mgr.state(), SyncState::Stalled { required_parent: required });

    // A peer with the correct chain reconnects and serves the full real
    // chain in one batch. `recover_missing_genesis_parent` should detect
    // the exact required parent and clear the halt before ingestion runs.
    mgr.record_peer_status(peer, 4);
    let req = BlockRequest {
        start_height: 1,
        count: 4,
    };
    let request_id = behaviour.send_request(&peer, req.clone());
    mgr.register_request(request_id, peer, &req, Instant::now());
    let applied = mgr
        .handle_response(
            request_id,
            BlockResponse {
                blocks: real.clone(),
                retry_after_ms: None,
            },
            Instant::now(),
            &mut local,
        )
        .expect("real-genesis batch must apply and clear halt");

    // Halt signals both clear.
    assert_eq!(mgr.missing_genesis_parent(), None);
    assert!(
        !matches!(*mgr.state(), SyncState::Stalled { .. }),
        "state must leave Stalled, got {:?}",
        mgr.state(),
    );

    // And the chain caught up.
    assert_eq!(applied.len(), 4);
    assert_eq!(local.height, 4);
    assert_eq!(mgr.local_height(), 4);
    assert_eq!(*mgr.state(), SyncState::Complete);
}
