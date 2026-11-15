//! Silent-gossip stall (ws node, 2026-11-08).
//!
//! After a restart the ws node redialled its only peer, took one gossip block
//! and then heard nothing for 9.5 hours. The connection stayed up, no
//! `ChainStatus` or `NewBlock` arrived, the sync manager knew no peer height
//! above its own, so it never asked for a block. No errors, no redial.
//!
//! These tests model that peer: the follower drops its own blocks-topic
//! subscription (`TestDropChainGossipSubscription`) while the connection stays
//! up, and the source then moves ahead. Every `ChainStatus` and `NewBlock` the
//! source publishes is lost, exactly as on ws.

use std::time::Duration;

use chrono::{DateTime, Utc};
use libp2p::{PeerId, multiaddr::Protocol};
use tokio::time::{Instant, timeout};
use wrfcoin_shared_protocol::{
    blockchain::{block::Block, transaction::Transaction},
    crypto::hash::{Hash, hash_bytes},
};

use crate::{
    InitialChainState, P2pCommand, P2pConfig, P2pEvent, P2pHandle, P2pNode,
    sync::{BlockRequest, BlockResponse},
};

const SILENCE: Duration = Duration::from_secs(3);
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const RECOVERY_BOUND: Duration = Duration::from_secs(40);

fn make_block(height: u64, prev_hash: Hash, timestamp: DateTime<Utc>) -> Block {
    let mut block = Block::new(1, prev_hash, Vec::<Transaction>::new(), 1, height);
    block.timestamp = timestamp;
    block.set_hash(block.calculate_hash());
    block
}

fn build_chain(count: u64) -> Vec<Block> {
    let base = Utc::now() - chrono::Duration::seconds(count as i64 + 60);
    let mut prev_hash = hash_bytes(b"genesis");
    (1..=count)
        .map(|height| {
            let block =
                make_block(height, prev_hash, base + chrono::Duration::seconds(height as i64));
            prev_hash = block.hash.expect("test block hash");
            block
        })
        .collect()
}

fn state_at(blocks: &[Block], height: u64) -> InitialChainState {
    InitialChainState {
        height,
        tip_hash: blocks[height as usize - 1].hash.expect("tip hash").into(),
        state_root: [height as u8; 32],
    }
}

/// The source's block-sync server: answers from `blocks[..served_height]`,
/// optionally ignoring the first `ignore_probes` one-block requests.
struct Source {
    handle: P2pHandle,
    blocks: Vec<Block>,
    served_height: u64,
    ignore_probes: usize,
    probes_seen: usize,
}

impl Source {
    async fn serve(&mut self, request_id: libp2p::request_response::InboundRequestId, request: BlockRequest) {
        if request.count == 1 {
            self.probes_seen += 1;
            if self.ignore_probes > 0 {
                self.ignore_probes -= 1;
                return;
            }
        }
        let start = request.start_height.max(1);
        let end = start
            .saturating_add(u64::from(request.count))
            .saturating_sub(1)
            .min(self.served_height);
        let blocks = if start > end {
            Vec::new()
        } else {
            self.blocks[(start - 1) as usize..end as usize].to_vec()
        };
        self.handle
            .command_tx
            .send(P2pCommand::RespondToBlockSyncRequest {
                request_id,
                response: BlockResponse { blocks, retry_after_ms: None },
            })
            .await
            .expect("source command channel open");
    }
}

/// What the follower saw while the pump ran.
#[derive(Default)]
struct Seen {
    disconnects: usize,
    reconnects: usize,
    synced_to: u64,
    gossip_block: Option<u64>,
}

/// Drive both nodes until `done(seen)` or `within` elapses; returns whether
/// `done` was reached.
async fn pump(
    follower: &mut P2pHandle,
    source: &mut Source,
    source_peer: PeerId,
    seen: &mut Seen,
    within: Duration,
    mut done: impl FnMut(&Seen) -> bool,
) -> bool {
    let deadline = Instant::now() + within;
    while !done(seen) {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return false;
        }
        tokio::select! {
            event = follower.event_rx.recv() => match event {
                Some(P2pEvent::PeerDisconnected(peer)) if peer == source_peer => seen.disconnects += 1,
                Some(P2pEvent::PeerConnected(peer)) if peer == source_peer => seen.reconnects += 1,
                Some(P2pEvent::SyncedBlock { height, .. }) => seen.synced_to = seen.synced_to.max(height),
                Some(P2pEvent::NewBlock(bytes)) => {
                    if let Ok(block) = bincode::deserialize::<Block>(&bytes) {
                        seen.gossip_block = Some(block.height);
                    }
                },
                Some(_) => {},
                None => panic!("follower event channel closed"),
            },
            event = source.handle.event_rx.recv() => match event {
                Some(P2pEvent::InboundBlockSyncRequest { request_id, request, .. }) => {
                    source.serve(request_id, request).await;
                },
                Some(_) => {},
                None => panic!("source event channel closed"),
            },
            _ = tokio::time::sleep(left) => return done(seen),
        }
    }
    true
}

/// Start a source at height 10 and a follower bootstrapped to it, wait until
/// the follower holds the source's proven height-10 status, then make the
/// follower deaf to chain gossip.
async fn silent_pair(blocks: &[Block], ignore_probes: usize) -> (P2pHandle, Source, PeerId) {
    let port = super::helpers::allocate_port().await;
    let addr: libp2p::Multiaddr = format!("/ip4/127.0.0.1/tcp/{port}").parse().expect("addr");
    let source_cfg = P2pConfig {
        listen_addresses: vec![addr.clone()],
        bootstrap_peers: Vec::new(),
        ..super::helpers::peering_config()
    };
    let source_handle = P2pNode::start_with_initial_chain_state(source_cfg, state_at(blocks, 10))
        .await
        .expect("source starts");
    let source_peer = source_handle.local_peer_id;

    let follower_cfg = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().expect("addr")],
        bootstrap_peers: vec![addr.with(Protocol::P2p(source_peer))],
        bootstrap_redial_interval: Duration::from_secs(1),
        bootstrap_redial_max_backoff: Duration::from_secs(2),
        gossip_silence_timeout: SILENCE,
        gossip_probe_timeout: PROBE_TIMEOUT,
        ..super::helpers::peering_config()
    };
    let mut follower = P2pNode::start_with_initial_chain_state(follower_cfg, state_at(blocks, 10))
        .await
        .expect("follower starts");

    let mut source = Source {
        handle: source_handle,
        blocks: blocks.to_vec(),
        served_height: 10,
        ignore_probes,
        probes_seen: 0,
    };
    timeout(Duration::from_secs(15), async {
        loop {
            tokio::select! {
                event = follower.event_rx.recv() => match event {
                    Some(P2pEvent::PeerChainStatus { peer_id, height: 10, .. }) if peer_id == source_peer => break,
                    Some(_) => {},
                    None => panic!("follower event channel closed"),
                },
                event = source.handle.event_rx.recv() => if event.is_none() {
                    panic!("source event channel closed");
                },
            }
        }
    })
    .await
    .expect("follower must first hold the source's proven ChainStatus");

    follower
        .command_tx
        .send(P2pCommand::TestDropChainGossipSubscription)
        .await
        .expect("follower command channel open");
    (follower, source, source_peer)
}

/// Move the source to height 20 and announce it the normal way. The follower
/// is deaf, so none of this reaches it.
async fn source_moves_ahead(source: &mut Source) {
    source.served_height = 20;
    let tip = state_at(&source.blocks, 20);
    source
        .handle
        .command_tx
        .send(P2pCommand::UpdateLocalChainState {
            height: tip.height,
            tip_hash: tip.tip_hash,
            state_root: tip.state_root,
        })
        .await
        .expect("source command channel open");
    source
        .handle
        .command_tx
        .send(P2pCommand::BroadcastChainStatus {
            height: tip.height,
            tip_hash: tip.tip_hash,
            state_root: tip.state_root,
        })
        .await
        .expect("source command channel open");
}

async fn recovers_from_a_silent_peer(ignore_probes: usize) {
    let _guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).filter_level(log::LevelFilter::Warn).try_init();

    let blocks = build_chain(21);
    let (mut follower, mut source, source_peer) = silent_pair(&blocks, ignore_probes).await;
    source_moves_ahead(&mut source).await;

    let mut seen = Seen::default();
    let caught_up = pump(&mut follower, &mut source, source_peer, &mut seen, RECOVERY_BOUND, |s| {
        s.synced_to >= 20
    })
    .await;
    let stats = follower.stats();
    assert!(
        caught_up,
        "follower stayed at height {} behind a connected, silent peer at 20 \
         (source saw {} probes; follower disconnects={} reconnects={})",
        seen.synced_to.max(10),
        source.probes_seen,
        seen.disconnects,
        seen.reconnects,
    );
    assert!(source.probes_seen >= 1, "the follower must probe before it reconnects");
    assert!(stats.gossip_silence_probes >= 1);
    assert!(seen.disconnects >= 1 && seen.reconnects >= 1, "the silent connection must be rebuilt");
    assert!(stats.gossip_silence_reconnects >= 1);

    // Gossip works again after the redial: the follower re-announced its
    // blocks-topic subscription, so a fresh NewBlock reaches it.
    let block_21 = bincode::serialize(&blocks[20]).expect("serialize block 21");
    let mut heard = false;
    for _ in 0..15 {
        source
            .handle
            .command_tx
            .send(P2pCommand::BroadcastBlock(block_21.clone()))
            .await
            .expect("source command channel open");
        if pump(&mut follower, &mut source, source_peer, &mut seen, Duration::from_secs(1), |s| {
            s.gossip_block == Some(21)
        })
        .await
        {
            heard = true;
            break;
        }
    }
    assert!(heard, "after the redial, NewBlock gossip from the source must reach the follower again");

    let _ = follower.command_tx.send(P2pCommand::Shutdown).await;
    let _ = source.handle.command_tx.send(P2pCommand::Shutdown).await;
}

/// The ws incident: the silent peer is ahead and answers the probe.
#[tokio::test]
async fn silent_peer_that_is_ahead_is_probed_then_reconnected() {
    recovers_from_a_silent_peer(0).await;
}

/// The silent peer does not answer the probe either.
#[tokio::test]
async fn silent_peer_that_ignores_the_probe_is_reconnected() {
    recovers_from_a_silent_peer(1).await;
}

/// A quiet chain is not a broken one: a peer at our height answers the probe
/// with nothing newer, and the connection is kept.
#[tokio::test]
async fn level_peer_on_a_quiet_chain_is_probed_but_kept() {
    let _guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).filter_level(log::LevelFilter::Warn).try_init();

    let blocks = build_chain(21);
    let (mut follower, mut source, source_peer) = silent_pair(&blocks, 0).await;

    let mut seen = Seen::default();
    pump(&mut follower, &mut source, source_peer, &mut seen, SILENCE * 3 + Duration::from_secs(2), |_| false)
        .await;
    let stats = follower.stats();
    assert!(
        source.probes_seen >= 2,
        "a silent window must be probed each time it recurs (probes seen: {})",
        source.probes_seen
    );
    assert!(stats.gossip_silence_probes >= 2);
    assert_eq!(seen.disconnects, 0, "a level peer that answers must not be disconnected");
    assert_eq!(stats.gossip_silence_reconnects, 0);

    let _ = follower.command_tx.send(P2pCommand::Shutdown).await;
    let _ = source.handle.command_tx.send(P2pCommand::Shutdown).await;
}
