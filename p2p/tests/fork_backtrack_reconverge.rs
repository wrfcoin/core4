//! A follower holding a losing one-block branch must be able to fetch the
//! competing branch's ancestor and hand it to the authoritative apply path,
//! instead of repeatedly asking for the range after its own tip.

use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use libp2p::{
    PeerId, StreamProtocol,
    request_response::{Behaviour as RequestResponseBehaviour, Config, ProtocolSupport},
};
use wrfcoin_p2p::sync::{
    BLOCK_SYNC_PROTOCOL, BlockRequest, BlockResponse, BlockSyncCodec, SyncError, SyncManager,
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

#[test]
fn follower_on_losing_branch_requests_and_forwards_the_competing_ancestor() {
    let base = Utc::now() - chrono::Duration::seconds(600);
    let genesis = hash_bytes(b"genesis");
    let b1 = make_block(1, genesis, base + chrono::Duration::seconds(1));
    let h1 = b1.hash.unwrap();
    // Local (losing) branch: 2'.
    let b2_local = make_block(2, h1, base + chrono::Duration::seconds(2));
    // Canonical branch: 2, 3 (different timestamp => different hash).
    let b2 = make_block(2, h1, base + chrono::Duration::seconds(5));
    let b3 = make_block(3, b2.hash.unwrap(), base + chrono::Duration::seconds(6));
    assert_ne!(b2.hash, b2_local.hash);

    let mut local = Blockchain::default();
    local.add_block(b1.clone()).unwrap();
    local.add_block(b2_local).unwrap();

    let peer = PeerId::random();
    let mut mgr = SyncManager::new(2);
    mgr.record_peer_status(peer, 3);
    let mut behaviour = dummy_behaviour();
    let now = Instant::now();

    // Round 1: the ordinary forward range (3..=3) returns a block whose
    // parent is not our tip.
    let planned = mgr.plan_requests(now);
    assert_eq!(planned[0].request.start_height, 3);
    let id = behaviour.send_request(&planned[0].peer_id, planned[0].request.clone());
    mgr.register_request(id, planned[0].peer_id, &planned[0].request, now);
    let forwarded = mgr
        .handle_response(
            id,
            BlockResponse { blocks: vec![b3.clone()], retry_after_ms: None },
            now,
            &mut local,
        )
        .expect("a competing next-height block is fork evidence, not a peer fault");
    assert_eq!(forwarded.len(), 1, "competing block goes to the authoritative path");
    assert_eq!(
        mgr.peer_metrics(&peer).unwrap().invalid_blocks,
        0,
        "serving the canonical chain must not be penalised"
    );

    // Round 2: the manager must now ask for the ancestors, not 3..=3 again.
    let planned = mgr.plan_requests(now);
    assert!(!planned.is_empty(), "a backtrack request must be planned");
    assert!(
        planned[0].request.start_height <= 2,
        "backtrack must cover the competing height-2 block, got start={}",
        planned[0].request.start_height
    );
    let id = behaviour.send_request(&planned[0].peer_id, planned[0].request.clone());
    mgr.register_request(id, planned[0].peer_id, &planned[0].request, now);
    let forwarded = mgr
        .handle_response(
            id,
            BlockResponse {
                blocks: vec![b1, b2.clone(), b3],
                retry_after_ms: None,
            },
            now,
            &mut local,
        )
        .expect("backtrack response must not error");
    assert!(
        forwarded.iter().any(|b| b.hash == b2.hash),
        "canonical height-2 block must reach the fork buffer"
    );
    assert_eq!(mgr.peer_metrics(&peer).unwrap().invalid_blocks, 0);
}

/// Chain of `count` blocks; `seed` varies timestamps so two branches differ.
fn branch(parent: Hash, from: u64, to: u64, seed: i64) -> Vec<Block> {
    let base = Utc::now() - chrono::Duration::seconds(5000);
    let mut prev = parent;
    let mut out = Vec::new();
    for h in from..=to {
        let b = make_block(h, prev, base + chrono::Duration::seconds(h as i64 * 10 + seed));
        prev = b.hash.unwrap();
        out.push(b);
    }
    out
}

struct Rig {
    mgr: SyncManager,
    peer: PeerId,
    behaviour: RequestResponseBehaviour<BlockSyncCodec>,
    local: Blockchain,
}

impl Rig {
    fn new(local_blocks: &[Block], advertised: u64) -> Self {
        let mut local = Blockchain::default();
        for b in local_blocks {
            local.add_block(b.clone()).unwrap();
        }
        let peer = PeerId::random();
        let mut mgr = SyncManager::new(local.height);
        mgr.record_peer_status(peer, advertised);
        Rig { mgr, peer, behaviour: dummy_behaviour(), local }
    }

    /// Send a hand-made request and answer it.
    fn answer(&mut self, start: u64, count: u32, blocks: Vec<Block>) -> Result<Vec<Block>, SyncError> {
        let request = BlockRequest { start_height: start, count };
        let id = self.behaviour.send_request(&self.peer, request.clone());
        let now = Instant::now();
        self.mgr.register_request(id, self.peer, &request, now);
        self.mgr.handle_response(
            id,
            BlockResponse { blocks, retry_after_ms: None },
            now,
            &mut self.local,
        )
    }

    /// Answer the request the manager itself plans next.
    fn answer_planned(&mut self, blocks: Vec<Block>) -> (u64, Result<Vec<Block>, SyncError>) {
        let now = Instant::now();
        let planned = self.mgr.plan_requests(now);
        assert!(!planned.is_empty(), "a request must be planned");
        let request = planned[0].request.clone();
        let id = self.behaviour.send_request(&planned[0].peer_id, request.clone());
        self.mgr.register_request(id, planned[0].peer_id, &request, now);
        let r = self.mgr.handle_response(
            id,
            BlockResponse { blocks, retry_after_ms: None },
            now,
            &mut self.local,
        );
        (request.start_height, r)
    }

    fn invalid(&self) -> u32 {
        self.mgr.peer_metrics(&self.peer).unwrap().invalid_blocks as u32
    }
}

#[test]
fn repeated_ancestor_plus_junk_tip_pairs_are_penalised() {
    let local = branch(hash_bytes(b"genesis"), 1, 5, 0);
    let junk_tip = make_block(6, hash_bytes(b"junk-parent"), Utc::now() - chrono::Duration::seconds(10));
    let pair = vec![local[3].clone(), junk_tip];
    let mut rig = Rig::new(&local, 6);

    let first = rig.answer(5, 2, pair.clone()).expect("one free pass per (peer, height, tip)");
    assert_eq!(first.len(), 1, "the ancestor must not ride along with the free pass");
    assert_eq!(rig.invalid(), 0);

    let second = rig.answer(5, 2, pair);
    assert!(matches!(second, Err(SyncError::WrongParentHash)), "{second:?}");
    assert_eq!(rig.invalid(), 1);
}

#[test]
fn ancestor_flood_outside_a_backtrack_reply_is_not_forwarded() {
    let local = branch(hash_bytes(b"genesis"), 1, 60, 0);
    let mut rig = Rig::new(&local, 61);
    // Open a backtrack (the free pass), so ancestor forwarding is armed.
    let junk_tip = make_block(61, hash_bytes(b"junk-parent"), Utc::now() - chrono::Duration::seconds(10));
    let first = rig.answer(61, 1, vec![junk_tip]).expect("free pass");
    assert_eq!(first.len(), 1);
    // A reply to some OTHER request must not get ancestors forwarded.
    let forwarded = rig.answer(1, 60, local.clone()).expect("benign overlap");
    assert!(forwarded.is_empty(), "got {} forwarded", forwarded.len());
    assert_eq!(rig.invalid(), 0);
}

#[test]
fn only_the_first_tip_plus_one_block_of_a_response_is_free() {
    let local = branch(hash_bytes(b"genesis"), 1, 5, 0);
    let mk = |n: &[u8]| make_block(6, hash_bytes(n), Utc::now() - chrono::Duration::seconds(10));
    let mut rig = Rig::new(&local, 6);
    let r = rig.answer(6, 5, vec![mk(b"a"), mk(b"b"), mk(b"c")]);
    assert!(matches!(r, Err(SyncError::WrongParentHash)), "{r:?}");
    assert_eq!(rig.invalid(), 1);
}

#[test]
fn canonical_branch_two_blocks_longer_is_forwarded_whole() {
    let b1 = branch(hash_bytes(b"genesis"), 1, 2, 0);
    let canon = branch(b1[0].hash.unwrap(), 2, 4, 7);
    let mut rig = Rig::new(&b1, 4);
    let forwarded = rig
        .answer(3, 2, vec![canon[1].clone(), canon[2].clone()])
        .expect("fork evidence");
    assert_eq!(forwarded.len(), 2, "tip+1 and its continuation both reach the apply path");
}

#[test]
fn fork_deeper_than_the_first_backtrack_widens_until_the_fork_point_is_covered() {
    // Shared 1..=10; local branch 11..=40; canonical branch 11..=42.
    let common = branch(hash_bytes(b"genesis"), 1, 10, 0);
    let tip10 = common[9].hash.unwrap();
    let local_tail = branch(tip10, 11, 40, 0);
    let canon_tail = branch(tip10, 11, 42, 3);
    let local: Vec<Block> = common.iter().chain(local_tail.iter()).cloned().collect();
    let canon: Vec<Block> = common.iter().chain(canon_tail.iter()).cloned().collect();
    let by_height = |lo: u64, hi: u64| -> Vec<Block> {
        canon.iter().filter(|b| b.height >= lo && b.height <= hi).cloned().collect()
    };
    let mut rig = Rig::new(&local, 42);

    // Round 1: forward range reveals the mismatch.
    let (start, r) = rig.answer_planned(by_height(41, 42));
    assert_eq!(start, 41);
    assert_eq!(r.unwrap().len(), 2);

    // Round 2: first backtrack reaches 16 blocks down only.
    let (start, r) = rig.answer_planned(by_height(25, 41));
    assert_eq!(start, 25);
    assert!(r.is_ok(), "round2: {r:?}");

    // Round 3: still not connected -> bare mismatch widens instead of punishing.
    let (start, r) = rig.answer_planned(by_height(41, 41));
    assert_eq!(start, 41);
    assert!(r.is_ok(), "widening is bounded and unpenalised: {r:?}");
    assert_eq!(rig.invalid(), 0);

    // Round 4: the widened request covers the fork point (height 10).
    let (start, r) = rig.answer_planned(by_height(9, 41));
    assert!(start <= 10, "widened backtrack must reach the fork point, start={start}");
    let forwarded = r.unwrap();
    assert!(forwarded.iter().any(|b| b.height == 11 && b.hash == canon_tail[0].hash));
    assert!(forwarded.iter().any(|b| b.height == 10));
}

/// The one-tip+1-per-response guard must hold even in the reply to our own
/// backtrack request, where `answers_backtrack` would otherwise allow all.
#[test]
fn backtrack_reply_forwards_only_one_tip_plus_one_block() {
    let local = branch(hash_bytes(b"genesis"), 1, 5, 0);
    let mk = |n: &[u8]| make_block(6, hash_bytes(n), Utc::now() - chrono::Duration::seconds(10));
    let mut rig = Rig::new(&local, 6);
    let (_, first) = rig.answer_planned(vec![mk(b"first")]);
    assert_eq!(first.expect("free pass").len(), 1);
    let (_, second) = rig.answer_planned(vec![mk(b"a"), mk(b"b")]);
    assert!(matches!(second, Err(SyncError::WrongParentHash)), "{second:?}");
    assert_eq!(rig.invalid(), 1);
}

#[test]
fn continuation_above_a_granted_tip_plus_one_is_capped() {
    let local = branch(hash_bytes(b"genesis"), 1, 5, 0);
    let canon = branch(hash_bytes(b"other-parent"), 6, 40, 3);
    let mut rig = Rig::new(&local, 40);
    let r = rig.answer(6, 35, canon);
    match r {
        Ok(f) => assert!(f.len() <= 17, "forwarded {}", f.len()),
        Err(_) => {}
    }
}

/// Follower at height 2 on a losing tip; peers hold 1, 2', 3. The forward
/// reply carries 3 (fork evidence), so a backtrack R = [1..=3] is opened.
fn losing_tip_at_two() -> (Rig, Vec<Block>) {
    let b1 = branch(hash_bytes(b"genesis"), 1, 1, 0);
    let local_2 = branch(b1[0].hash.unwrap(), 2, 2, 0);
    let canon_tail = branch(b1[0].hash.unwrap(), 2, 3, 7);
    assert_ne!(local_2[0].hash, canon_tail[0].hash);
    let local: Vec<Block> = b1.iter().chain(local_2.iter()).cloned().collect();
    let canon: Vec<Block> = b1.iter().chain(canon_tail.iter()).cloned().collect();
    let mut rig = Rig::new(&local, 3);
    let (start, r) = rig.answer_planned(vec![canon[2].clone()]);
    assert_eq!(start, 3);
    assert_eq!(r.expect("fork evidence").len(), 1);
    (rig, canon)
}

/// An empty reply (no `retry_after_ms`) to the open backtrack must not strand
/// it: the ancestors still have to be requested, not just tip+1 again.
#[test]
fn empty_backtrack_reply_re_requests_the_missing_ancestors() {
    let (mut rig, canon) = losing_tip_at_two();
    let (start, r) = rig.answer_planned(Vec::new());
    assert!(start <= 2, "backtrack covers height 2, start={start}");
    assert!(r.expect("empty reply is not a fault").is_empty());

    let (start, r) = rig.answer_planned(canon.clone());
    assert!(start <= 2, "the backtrack must be re-requested after an empty reply, start={start}");
    let forwarded = r.expect("re-requested backtrack reply must not error");
    assert!(
        forwarded.iter().any(|b| b.hash == canon[1].hash),
        "competing height-2 ancestor must reach the fork buffer"
    );
    assert!(forwarded.iter().any(|b| b.hash == canon[2].hash), "tip+1 forwarded with it");
    assert_eq!(rig.invalid(), 0, "serving peers must not be penalised");
}

/// The server truncates to a byte budget keeping the LOWEST blocks, which cuts
/// exactly the near-tip ancestors and tip+1. The remainder must be requested.
#[test]
fn truncated_backtrack_reply_re_requests_the_missing_ancestors() {
    // Shared 1..=10; local 11..=20; canonical 11..=21.
    let common = branch(hash_bytes(b"genesis"), 1, 10, 0);
    let tip10 = common[9].hash.unwrap();
    let local_tail = branch(tip10, 11, 20, 0);
    let canon_tail = branch(tip10, 11, 21, 3);
    let local: Vec<Block> = common.iter().chain(local_tail.iter()).cloned().collect();
    let canon: Vec<Block> = common.iter().chain(canon_tail.iter()).cloned().collect();
    let by_height = |lo: u64, hi: u64| -> Vec<Block> {
        canon.iter().filter(|b| b.height >= lo && b.height <= hi).cloned().collect()
    };
    let mut rig = Rig::new(&local, 21);

    let (start, r) = rig.answer_planned(by_height(21, 21));
    assert_eq!(start, 21);
    assert_eq!(r.unwrap().len(), 1);

    // Backtrack R = [5..=21]; the reply is truncated to its lowest blocks.
    let (start, r) = rig.answer_planned(by_height(5, 12));
    assert_eq!(start, 5);
    assert!(r.is_ok(), "{r:?}");

    let (start, r) = rig.answer_planned(by_height(13, 21));
    assert!(
        (5..=13).contains(&start),
        "the not-yet-received ancestors (13..=20) must be requested, start={start}"
    );
    let forwarded = r.expect("re-requested backtrack reply must not error");
    for h in 13..=21u64 {
        assert!(
            forwarded.iter().any(|b| b.height == h && b.hash == canon[h as usize - 1].hash),
            "canonical block {h} must be forwarded"
        );
    }
    assert_eq!(rig.invalid(), 0);
}

/// A responder that keeps answering the backtrack with nothing cannot spin it
/// forever; once the retries are spent the next mismatch widens, unpenalised.
#[test]
fn empty_backtrack_replies_are_bounded_then_the_backtrack_widens() {
    let (mut rig, canon) = losing_tip_at_two();
    let mut empty_backtracks = 0;
    loop {
        let (start, r) = rig.answer_planned(Vec::new());
        assert!(r.is_ok(), "{r:?}");
        if start > 2 {
            break;
        }
        empty_backtracks += 1;
        assert!(empty_backtracks <= 8, "empty backtrack replies must be bounded");
    }
    assert!(empty_backtracks >= 2, "an empty reply must be retried, got {empty_backtracks}");
    // That last planned request was the forward [3..=3]; it got an empty reply,
    // which is not fork evidence. Now a tip+1 mismatch must widen, not punish.
    let (start, r) = rig.answer_planned(vec![canon[2].clone()]);
    assert_eq!(start, 3);
    assert!(r.is_ok(), "{r:?}");
    assert_eq!(rig.invalid(), 0);
    let (start, r) = rig.answer_planned(canon.clone());
    assert!(start <= 2, "widened backtrack covers height 2, start={start}");
    assert!(r.unwrap().iter().any(|b| b.hash == canon[1].hash));
    assert_eq!(rig.invalid(), 0);
}

/// A faulty reply to the backtrack (two tip+1 blocks) is penalised, but the
/// backtrack itself must still be re-requested, not just tip+1.
#[test]
fn faulty_backtrack_reply_does_not_strand_the_backtrack() {
    let (mut rig, canon) = losing_tip_at_two();
    let mk = |n: &[u8]| make_block(3, hash_bytes(n), Utc::now() - chrono::Duration::seconds(10));
    let (start, r) = rig.answer_planned(vec![mk(b"a"), mk(b"b")]);
    assert!(start <= 2);
    assert!(matches!(r, Err(SyncError::WrongParentHash)), "{r:?}");
    assert_eq!(rig.invalid(), 1);
    let (start, r) = rig.answer_planned(canon.clone());
    assert!(start <= 2, "the backtrack must be re-requested, start={start}");
    assert!(r.expect("reply").iter().any(|b| b.hash == canon[1].hash));
    assert_eq!(rig.invalid(), 1);
}


/// One preferred peer answers every backtrack request with an empty reply
/// (and serves tip+1). The backtrack still reaches the other peer, which
/// serves the ancestors, and that peer is not penalised.
#[test]
fn preferred_empty_responder_does_not_lock_out_the_serving_peer() {
    use wrfcoin_p2p::scoring::TransportClass;
    let b1 = branch(hash_bytes(b"genesis"), 1, 1, 0);
    let local_2 = branch(b1[0].hash.unwrap(), 2, 2, 0);
    let canon_tail = branch(b1[0].hash.unwrap(), 2, 3, 7);
    let mut local = Blockchain::default();
    local.add_block(b1[0].clone()).unwrap();
    local.add_block(local_2[0].clone()).unwrap();
    let canon: Vec<Block> = b1.iter().chain(canon_tail.iter()).cloned().collect();

    let empty_peer = PeerId::random();
    let serving = PeerId::random();
    let mut mgr = SyncManager::new(2);
    mgr.record_peer_status(empty_peer, 3);
    mgr.record_peer_status(serving, 3);
    mgr.set_peer_transport(empty_peer, TransportClass::Wan);
    let mut behaviour = dummy_behaviour();

    let mut ancestor_forwarded = false;
    let mut serving_backtracks = 0;
    let mut empty_replies = 0;
    for _ in 0..200 {
        let now = Instant::now();
        let planned = mgr.plan_requests(now);
        if planned.is_empty() { break; }
        for p in planned {
            let id = behaviour.send_request(&p.peer_id, p.request.clone());
            mgr.register_request(id, p.peer_id, &p.request, now);
            let lo = p.request.start_height;
            let hi = lo + u64::from(p.request.count) - 1;
            let blocks: Vec<Block> = if p.peer_id == empty_peer && lo <= 2 {
                empty_replies += 1;
                Vec::new()
            } else {
                if p.peer_id == serving && lo <= 2 { serving_backtracks += 1; }
                canon.iter().filter(|b| b.height >= lo && b.height <= hi).cloned().collect()
            };
            if let Ok(f) = mgr.handle_response(id, BlockResponse { blocks, retry_after_ms: None }, now, &mut local) {
                if f.iter().any(|b| b.hash == canon[1].hash) { ancestor_forwarded = true; }
            }
        }
    }
    let hm = mgr.peer_metrics(&serving).unwrap();
    let am = mgr.peer_metrics(&empty_peer).unwrap();
    eprintln!("empty_replies={empty_replies} serving_backtracks={serving_backtracks} serving_invalid={} serving_banned={} empty_peer_invalid={}", hm.invalid_blocks, hm.banned, am.invalid_blocks);
    assert!(ancestor_forwarded, "competing ancestor must eventually be fetched from the serving peer");
    assert_eq!(hm.invalid_blocks, 0, "the serving peer must not be penalised");
}

/// The peer assigned the backtrack defers it with a long retry_after_ms. The
/// competing ancestor is still fetched, with no penalty.
#[test]
fn deferred_backtrack_still_fetches_the_ancestor() {
    use wrfcoin_p2p::scoring::TransportClass;
    let b1 = branch(hash_bytes(b"genesis"), 1, 1, 0);
    let local_2 = branch(b1[0].hash.unwrap(), 2, 2, 0);
    let canon_tail = branch(b1[0].hash.unwrap(), 2, 3, 7);
    let mut local = Blockchain::default();
    local.add_block(b1[0].clone()).unwrap();
    local.add_block(local_2[0].clone()).unwrap();
    let canon: Vec<Block> = b1.iter().chain(canon_tail.iter()).cloned().collect();
    let empty_peer = PeerId::random();
    let serving = PeerId::random();
    let mut mgr = SyncManager::new(2);
    mgr.record_peer_status(empty_peer, 3);
    mgr.record_peer_status(serving, 3);
    mgr.set_peer_transport(empty_peer, TransportClass::Wan);
    let mut behaviour = dummy_behaviour();
    let mut ancestor_forwarded = false;
    for _ in 0..40 {
        let now = Instant::now();
        let planned = mgr.plan_requests(now);
        if planned.is_empty() { break; }
        for p in planned {
            let id = behaviour.send_request(&p.peer_id, p.request.clone());
            mgr.register_request(id, p.peer_id, &p.request, now);
            let lo = p.request.start_height;
            let hi = lo + u64::from(p.request.count) - 1;
            let resp = if p.peer_id == empty_peer && lo <= 2 {
                BlockResponse { blocks: Vec::new(), retry_after_ms: Some(3_600_000) }
            } else {
                BlockResponse { blocks: canon.iter().filter(|b| b.height >= lo && b.height <= hi).cloned().collect(), retry_after_ms: None }
            };
            if let Ok(f) = mgr.handle_response(id, resp, now, &mut local) {
                if f.iter().any(|b| b.hash == canon[1].hash) { ancestor_forwarded = true; }
            }
        }
    }
    let hm = mgr.peer_metrics(&serving).unwrap();
    eprintln!("deferral: serving_invalid={} banned={} ancestor_forwarded={ancestor_forwarded}", hm.invalid_blocks, hm.banned);
    assert!(ancestor_forwarded);
    assert_eq!(hm.invalid_blocks, 0);
}

/// Single peer, fork that never connects: the ladder ends at depth 100, then
/// tip+1 mismatches are held (no penalty, no spin) and the backtrack reopens
/// at the shallowest depth once the cooldown has passed.
#[test]
fn widest_backtrack_holds_then_reopens_after_the_cooldown() {
    let (mut rig, canon) = losing_tip_at_two();
    let serve = |start: u64, count: u32| -> Vec<Block> {
        let hi = start + u64::from(count) - 1;
        canon.iter().filter(|b| b.height >= start && b.height <= hi).cloned().collect()
    };
    let t0 = Instant::now();
    let mut rounds = 0;
    loop {
        let planned = rig.mgr.plan_requests(t0);
        if planned.is_empty() {
            break;
        }
        rounds += 1;
        assert!(rounds <= 20, "the ladder must end");
        let p = &planned[0];
        let id = rig.behaviour.send_request(&p.peer_id, p.request.clone());
        rig.mgr.register_request(id, p.peer_id, &p.request, t0);
        let blocks = serve(p.request.start_height, p.request.count);
        let r = rig.mgr.handle_response(id, BlockResponse { blocks, retry_after_ms: None }, t0, &mut rig.local);
        assert!(r.is_ok(), "{r:?}");
    }
    assert_eq!(rig.invalid(), 0, "a held mismatch is not penalised");

    let later = t0 + std::time::Duration::from_secs(121);
    let planned = rig.mgr.plan_requests(later);
    assert_eq!(planned[0].request.start_height, 3, "tip+1 is retried after the cooldown");
    let id = rig.behaviour.send_request(&planned[0].peer_id, planned[0].request.clone());
    rig.mgr.register_request(id, planned[0].peer_id, &planned[0].request, later);
    let r = rig.mgr.handle_response(id, BlockResponse { blocks: serve(3, 1), retry_after_ms: None }, later, &mut rig.local);
    assert_eq!(r.expect("reopen forwards tip+1").len(), 1);
    let planned = rig.mgr.plan_requests(later);
    assert!(planned[0].request.start_height <= 2, "backtrack reopened");
    assert_eq!(rig.invalid(), 0);
}

// ---- Several peers answering backtracks with empty replies ----
/// Returns (serving_backtracks, empty_replies, ancestor_forwarded, serving_invalid, reopen_cycles)
fn run_with_empty_responders(n_empty: usize, cycles: usize) -> (u32, u32, bool, u32, usize) {
    let b1 = branch(hash_bytes(b"genesis"), 1, 1, 0);
    let local_2 = branch(b1[0].hash.unwrap(), 2, 2, 0);
    let canon_tail = branch(b1[0].hash.unwrap(), 2, 3, 7);
    let mut local = Blockchain::default();
    local.add_block(b1[0].clone()).unwrap();
    local.add_block(local_2[0].clone()).unwrap();
    let canon: Vec<Block> = b1.iter().chain(canon_tail.iter()).cloned().collect();

    let serving = PeerId::random();
    let empty_peers: Vec<PeerId> = (0..n_empty).map(|_| PeerId::random()).collect();
    let mut mgr = SyncManager::new(2);
    mgr.record_peer_status(serving, 3);
    for a in &empty_peers {
        mgr.record_peer_status(*a, 3);
        mgr.set_peer_transport(*a, wrfcoin_p2p::scoring::TransportClass::Wan);
    }
    let mut behaviour = RequestResponseBehaviour::with_codec(
        BlockSyncCodec,
        std::iter::once((StreamProtocol::new(BLOCK_SYNC_PROTOCOL), ProtocolSupport::Full)),
        Config::default(),
    );
    let (mut hb, mut ae, mut fwd, mut reopen) = (0u32, 0u32, false, 0usize);
    let mut now = Instant::now();
    let mut idle = 0;
    'outer: for _ in 0..5000 {
        let planned = mgr.plan_requests(now);
        if planned.is_empty() {
            idle += 1;
            if idle > cycles { break; }
            reopen += 1;
            now += Duration::from_secs(121);
            continue;
        }
        for p in planned {
            let id = behaviour.send_request(&p.peer_id, p.request.clone());
            mgr.register_request(id, p.peer_id, &p.request, now);
            let lo = p.request.start_height;
            let hi = lo + u64::from(p.request.count) - 1;
            let blocks: Vec<Block> = if p.peer_id != serving && lo <= 2 {
                ae += 1;
                Vec::new()
            } else {
                if p.peer_id == serving && lo <= 2 { hb += 1; }
                canon.iter().filter(|b| b.height >= lo && b.height <= hi).cloned().collect()
            };
            if let Ok(f) = mgr.handle_response(id, BlockResponse { blocks, retry_after_ms: None }, now, &mut local) {
                if f.iter().any(|b| b.hash == canon[1].hash) { fwd = true; break 'outer; }
            }
        }
    }
    let hi = mgr.peer_metrics(&serving).unwrap().invalid_blocks;
    (hb, ae, fwd, hi as u32, reopen)
}

#[test]
fn eight_empty_responders_do_not_stall() {
    let r = run_with_empty_responders(8, 10);
    eprintln!("n=8 serving_backtracks={} empty_replies={} forwarded={} serving_invalid={} reopen_cycles={}", r.0, r.1, r.2, r.3, r.4);
    assert!(r.2);
}

#[test]
fn nine_empty_responders_do_not_stall() {
    let r = run_with_empty_responders(9, 10);
    eprintln!("n=9 serving_backtracks={} empty_replies={} forwarded={} serving_invalid={} reopen_cycles={}", r.0, r.1, r.2, r.3, r.4);
    assert!(r.2, "the ancestor must eventually be fetched with 9 empty responders");
}

/// An authoritative rejection while a backtrack is open rewinds the shadow to
/// the SAME (height, tip) and clears every queue, but leaves `fork_backtrack`
/// open/unanswered with its range no longer queued. Trigger: one hash-valid
/// block that extends our own tip (context-free shadow accepts it) and that
/// authoritative validation rejects (LocalRuleRejection: no penalty to sender).
#[test]
fn authoritative_rejection_does_not_strand_open_backtrack() {
    use wrfcoin_p2p::scoring::TransportClass;
    use wrfcoin_p2p::sync::AuthoritativeSyncRejectionAttribution;
    let b1 = branch(hash_bytes(b"genesis"), 1, 1, 0);
    let local_2 = branch(b1[0].hash.unwrap(), 2, 2, 0);
    let canon_tail = branch(b1[0].hash.unwrap(), 2, 3, 7);
    let mut local = Blockchain::default();
    local.add_block(b1[0].clone()).unwrap();
    local.add_block(local_2[0].clone()).unwrap();
    let pristine = local.clone();
    let canon: Vec<Block> = b1.iter().chain(canon_tail.iter()).cloned().collect();
    // Junk extension of OUR tip at height 3 (passes the context-free shadow).
    let junk_ext = branch(local_2[0].hash.unwrap(), 3, 3, 99);

    let serving = PeerId::random();
    let empty_peer = PeerId::random();
    let mut mgr = SyncManager::new(2);
    mgr.record_peer_status(serving, 3);
    mgr.record_peer_status(empty_peer, 3);
    mgr.set_peer_transport(empty_peer, TransportClass::Wan);
    let mut behaviour = RequestResponseBehaviour::with_codec(
        BlockSyncCodec,
        std::iter::once((StreamProtocol::new(BLOCK_SYNC_PROTOCOL), ProtocolSupport::Full)),
        Config::default(),
    );
    let serve = |lo: u64, hi: u64| -> Vec<Block> {
        canon.iter().filter(|b| b.height >= lo && b.height <= hi).cloned().collect()
    };
    let mut now = Instant::now();
    let mut injected = false;
    let mut fwd = false;
    let mut idle = 0;
    'outer: for _ in 0..2000 {
        let planned = mgr.plan_requests(now);
        if planned.is_empty() {
            idle += 1;
            if idle > 10 { break; }
            now += Duration::from_secs(121);
            continue;
        }
        for p in planned {
            let id = behaviour.send_request(&p.peer_id, p.request.clone());
            mgr.register_request(id, p.peer_id, &p.request, now);
            let lo = p.request.start_height;
            let hi = lo + u64::from(p.request.count) - 1;
            if !injected && lo <= 2 {
                // The first backtrack request: whoever it went to, the reply is
                // a junk extension of our tip, which the shadow accepts.
                injected = true;
                let _ = mgr.handle_response(id, BlockResponse { blocks: junk_ext.clone(), retry_after_ms: None }, now, &mut local);
                assert_eq!(local.height, 3, "shadow accepted the junk extension");
                // launch_api: authoritative apply rejects it (rule rejection).
                mgr.note_authoritative_rejection(&empty_peer, 2, AuthoritativeSyncRejectionAttribution::LocalRuleRejection);
                mgr.note_rule_rejection_at(&empty_peer, 3, now);
                local = pristine.clone();
                continue;
            }
            if let Ok(f) = mgr.handle_response(id, BlockResponse { blocks: serve(lo, hi), retry_after_ms: None }, now, &mut local) {
                if f.iter().any(|b| b.hash == canon[1].hash) { fwd = true; break 'outer; }
            }
        }
    }
    let hm = mgr.peer_metrics(&serving).unwrap();
    eprintln!("strand: injected={injected} ancestor_forwarded={fwd} serving_invalid={} serving_banned={}", hm.invalid_blocks, hm.banned);
    assert!(injected);
    assert!(fwd, "the competing ancestor must still be fetched after the rewind");
    assert_eq!(hm.invalid_blocks, 0, "the serving peer must not be penalised");
}

/// An oversized reply to the backtrack is rejected and penalised, but the
/// backtrack range must still be scheduled afterwards.
#[test]
fn oversized_backtrack_reply_does_not_strand_the_backtrack() {
    let (mut rig, canon) = losing_tip_at_two();
    let mut oversized = canon.clone();
    oversized.extend(canon.clone());
    oversized.extend(canon.clone());
    oversized.extend(canon.clone());
    let (start, r) = rig.answer_planned(oversized);
    assert!(start <= 2);
    assert!(matches!(r, Err(SyncError::InvalidBatchSize)), "{r:?}");
    let (start, r) = rig.answer_planned(canon.clone());
    assert!(start <= 2, "the backtrack must still be requested, start={start}");
    assert!(r.expect("reply").iter().any(|b| b.hash == canon[1].hash));
}

/// Many more empty responders than one depth's free budget: the serving peer
/// is still reached (peer memory persists across widening and reopening),
/// with no penalty.
#[test]
fn forty_empty_responders_still_reach_the_serving_peer() {
    let r = run_with_empty_responders(40, 10);
    eprintln!("n=40 serving_backtracks={} empty_replies={} forwarded={} serving_invalid={} reopen_cycles={}", r.0, r.1, r.2, r.3, r.4);
    assert!(r.2);
    assert_eq!(r.3, 0);
}
