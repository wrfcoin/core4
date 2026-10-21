//! Three-node regression for the live reconnect/timeout stall in #2816.

use std::time::Duration;

use chrono::{DateTime, Utc};
use libp2p::{PeerId, multiaddr::Protocol, request_response::InboundRequestId};
use tokio::time::timeout;
use wrfcoin_shared_protocol::{
    blockchain::{block::Block, chain::Blockchain, transaction::Transaction},
    crypto::hash::{Hash, hash_bytes},
};

use crate::{
    InitialChainState, P2pCommand, P2pConfig, P2pEvent, P2pHandle, P2pNode,
    sync::{BlockRequest, BlockResponse},
};

const TEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Producer,
    Seed,
}

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
            let block = make_block(
                height,
                prev_hash,
                base + chrono::Duration::seconds(height as i64),
            );
            prev_hash = block.hash.expect("test block hash");
            block
        })
        .collect()
}

fn initial_state(blocks: &[Block], height: usize) -> InitialChainState {
    InitialChainState {
        height: height as u64,
        tip_hash: blocks[height - 1].hash.expect("tip hash").into(),
        state_root: [height as u8; 32],
    }
}

fn response_for(request: &BlockRequest, blocks: &[Block]) -> BlockResponse {
    let start = request.start_height.saturating_sub(1) as usize;
    let end = start
        .saturating_add(request.count as usize)
        .min(blocks.len());
    BlockResponse {
        blocks: blocks[start..end].to_vec(),
        retry_after_ms: None,
    }
}

async fn next_request(
    producer: &mut P2pHandle,
    seed: &mut P2pHandle,
    expected_peer: Option<PeerId>,
    within: Duration,
) -> (Source, PeerId, InboundRequestId, BlockRequest) {
    timeout(within, async {
        loop {
            tokio::select! {
                event = producer.event_rx.recv() => match event {
                    Some(P2pEvent::InboundBlockSyncRequest { peer_id, request_id, request })
                        if expected_peer.is_none_or(|expected| peer_id == expected) => {
                        break (Source::Producer, peer_id, request_id, request);
                    },
                    Some(_) => {},
                    None => panic!("producer event channel closed"),
                },
                event = seed.event_rx.recv() => match event {
                    Some(P2pEvent::InboundBlockSyncRequest { peer_id, request_id, request })
                        if expected_peer.is_none_or(|expected| peer_id == expected) => {
                        break (Source::Seed, peer_id, request_id, request);
                    },
                    Some(_) => {},
                    None => panic!("seed event channel closed"),
                },
            }
        }
    })
    .await
    .expect("a block-range request must arrive within the regression bound")
}

fn handle_for_source<'a>(
    source: Source,
    producer: &'a P2pHandle,
    seed: &'a P2pHandle,
) -> &'a P2pHandle {
    match source {
        Source::Producer => producer,
        Source::Seed => seed,
    }
}

async fn respond(
    source: Source,
    request_id: InboundRequestId,
    request: &BlockRequest,
    blocks: &[Block],
    producer: &P2pHandle,
    seed: &P2pHandle,
) {
    handle_for_source(source, producer, seed)
        .command_tx
        .send(P2pCommand::RespondToBlockSyncRequest {
            request_id,
            response: response_for(request, blocks),
        })
        .await
        .expect("responder command channel must remain open");
}

async fn apply_until(
    follower: &mut P2pHandle,
    local: &mut Blockchain,
    target: u64,
    external_penalties: &mut u64,
) {
    timeout(TEST_TIMEOUT, async {
        while local.height < target {
            match follower.event_rx.recv().await {
                Some(P2pEvent::SyncedBlock { block_bytes, .. }) => {
                    let block: Block =
                        bincode::deserialize(&block_bytes).expect("valid synced block bytes");
                    if block.height == local.height + 1 {
                        local.add_block(block).expect("synced block extends exact test chain");
                    }
                },
                Some(P2pEvent::PeerPenalized { .. }) => {
                    *external_penalties = external_penalties.saturating_add(1);
                },
                Some(_) => {},
                None => panic!("follower event channel closed"),
            }
        }
    })
    .await
    .expect("follower must converge within the regression bound");
}

async fn assert_no_request_from(
    handle: &mut P2pHandle,
    expected_peer: PeerId,
    within: Duration,
) {
    let request = timeout(within, async {
        loop {
            match handle.event_rx.recv().await {
                Some(P2pEvent::InboundBlockSyncRequest { peer_id, request, .. })
                    if peer_id == expected_peer => break request,
                Some(_) => {},
                None => panic!("source event channel closed"),
            }
        }
    })
    .await;
    if let Ok(request) = request {
        panic!(
            "a reconnected peer still in retained cooldown received range start={} count={}",
            request.start_height, request.count
        );
    }
}

async fn request_from_while_peer_stays_connected(
    source: &mut P2pHandle,
    follower: &mut P2pHandle,
    follower_peer: PeerId,
    stable_source_peer: PeerId,
    failed_source_peer: PeerId,
    external_penalties: &mut u64,
    within: Duration,
) -> (InboundRequestId, BlockRequest) {
    timeout(within, async {
        loop {
            match follower.event_rx.recv().await {
                Some(P2pEvent::PeerDisconnected(peer)) if peer == failed_source_peer => break,
                Some(P2pEvent::PeerDisconnected(peer)) if peer == stable_source_peer => {
                    panic!("stale alternate disconnected before ahead source shutdown completed");
                },
                Some(P2pEvent::PeerConnected(peer)) if peer == stable_source_peer => {
                    panic!("stale alternate reconnected before ahead source shutdown completed");
                },
                Some(P2pEvent::PeerPenalized { .. }) => {
                    *external_penalties = external_penalties.saturating_add(1);
                },
                Some(_) => {},
                None => panic!("follower event channel closed while awaiting ahead source death"),
            }
        }
    })
    .await
    .expect("ahead source must disconnect within the regression bound");

    timeout(within, async {
        loop {
            tokio::select! {
                event = source.event_rx.recv() => match event {
                    Some(P2pEvent::InboundBlockSyncRequest { peer_id, request_id, request })
                        if peer_id == follower_peer => break (request_id, request),
                    Some(_) => {},
                    None => panic!("stale source event channel closed"),
                },
                event = follower.event_rx.recv() => match event {
                    Some(P2pEvent::PeerDisconnected(peer)) if peer == stable_source_peer => {
                        panic!("stale alternate disconnected before recovery request");
                    },
                    Some(P2pEvent::PeerConnected(peer)) if peer == stable_source_peer => {
                        panic!("stale alternate reconnected before recovery request");
                    },
                    Some(P2pEvent::PeerPenalized { .. }) => {
                        *external_penalties = external_penalties.saturating_add(1);
                    },
                    Some(_) => {},
                    None => panic!("follower event channel closed while awaiting stale alternate"),
                },
            }
        }
    })
    .await
    .expect("stale connected alternate must receive the range without reconnecting")
}

async fn apply_until_without_peer_reconnect(
    follower: &mut P2pHandle,
    local: &mut Blockchain,
    target: u64,
    stable_source_peer: PeerId,
    external_penalties: &mut u64,
) {
    timeout(TEST_TIMEOUT, async {
        while local.height < target {
            match follower.event_rx.recv().await {
                Some(P2pEvent::SyncedBlock { block_bytes, .. }) => {
                    let block: Block =
                        bincode::deserialize(&block_bytes).expect("valid synced block bytes");
                    if block.height == local.height + 1 {
                        local.add_block(block).expect("synced block extends exact test chain");
                    }
                },
                Some(P2pEvent::PeerDisconnected(peer)) if peer == stable_source_peer => {
                    panic!("stale alternate disconnected while its response was applied");
                },
                Some(P2pEvent::PeerConnected(peer)) if peer == stable_source_peer => {
                    panic!("stale alternate reconnected while its response was applied");
                },
                Some(P2pEvent::PeerPenalized { .. }) => {
                    *external_penalties = external_penalties.saturating_add(1);
                },
                Some(_) => {},
                None => panic!("follower event channel closed"),
            }
        }
    })
    .await
    .expect("follower must converge without an incidental reconnect");
}

/// Reproduces the 2026-08-27 producer-cutover stall shape: the only peer that
/// advertises the target accepts a range request and then dies, while a second
/// peer remains connected in the same connection epoch but advertises only the
/// follower's current height. The stale alternate can nevertheless serve the
/// missing blocks and must be probed without waiting for it to reconnect or
/// publish a fresher ChainStatus.
#[tokio::test]
async fn follower_recovers_from_dead_ahead_peer_via_stale_connected_alternate_2816() {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder()
        .is_test(true)
        .filter_level(log::LevelFilter::Warn)
        .try_init();

    let blocks = build_chain(20);
    let ahead_port = super::helpers::allocate_port().await;
    let stale_port = super::helpers::allocate_port().await;
    let ahead_addr: libp2p::Multiaddr = format!("/ip4/127.0.0.1/tcp/{ahead_port}")
        .parse()
        .expect("ahead source address");
    let stale_addr: libp2p::Multiaddr = format!("/ip4/127.0.0.1/tcp/{stale_port}")
        .parse()
        .expect("stale source address");

    let mut ahead = P2pNode::start_with_initial_chain_state(
        P2pConfig {
            listen_addresses: vec![ahead_addr.clone()],
            bootstrap_peers: Vec::new(),
            ..super::helpers::peering_config()
        },
        initial_state(&blocks, 20),
    )
    .await
    .expect("ahead source starts");
    let mut stale = P2pNode::start_with_initial_chain_state(
        P2pConfig {
            listen_addresses: vec![stale_addr.clone()],
            bootstrap_peers: Vec::new(),
            ..super::helpers::peering_config()
        },
        initial_state(&blocks, 10),
    )
    .await
    .expect("stale source starts");

    let ahead_peer = ahead.local_peer_id;
    let stale_peer = stale.local_peer_id;
    let mut follower_cfg = super::helpers::peering_config();
    follower_cfg.listen_addresses = vec!["/ip4/127.0.0.1/tcp/0"
        .parse()
        .expect("follower address")];
    follower_cfg.bootstrap_peers = vec![
        ahead_addr.with(Protocol::P2p(ahead_peer)),
        stale_addr.with(Protocol::P2p(stale_peer)),
    ];
    let mut follower = P2pNode::start_with_initial_chain_state(
        follower_cfg,
        initial_state(&blocks, 10),
    )
    .await
    .expect("follower starts");
    let follower_peer = follower.local_peer_id;

    let mut saw_ahead_status = false;
    let mut saw_stale_status = false;
    timeout(Duration::from_secs(8), async {
        while !saw_ahead_status || !saw_stale_status {
            match follower.event_rx.recv().await {
                Some(P2pEvent::PeerChainStatus { peer_id, height: 20, .. })
                    if peer_id == ahead_peer => saw_ahead_status = true,
                Some(P2pEvent::PeerChainStatus { peer_id, height: 10, .. })
                    if peer_id == stale_peer => saw_stale_status = true,
                Some(_) => {},
                None => panic!("follower event channel closed during target discovery"),
            }
        }
    })
    .await
    .expect("follower must observe ahead and stale ChainStatus values");

    let (first_source, first_peer, _blackholed_id, failed_request) =
        next_request(&mut ahead, &mut stale, Some(follower_peer), Duration::from_secs(3)).await;
    assert_eq!(first_source, Source::Producer);
    assert_eq!(first_peer, follower_peer);
    assert_eq!(failed_request.start_height, 11);
    assert_eq!(failed_request.count, 10);

    ahead
        .command_tx
        .send(P2pCommand::Shutdown)
        .await
        .expect("ahead source shutdown accepted");

    let mut external_penalties = 0u64;
    let (recovery_id, recovery_request) = request_from_while_peer_stays_connected(
        &mut stale,
        &mut follower,
        follower_peer,
        stale_peer,
        ahead_peer,
        &mut external_penalties,
        Duration::from_secs(8),
    )
    .await;
    assert_eq!(recovery_request, failed_request);
    stale
        .command_tx
        .send(P2pCommand::RespondToBlockSyncRequest {
            request_id: recovery_id,
            response: response_for(&recovery_request, &blocks),
        })
        .await
        .expect("stale alternate response accepted");

    let mut follower_chain = Blockchain::default();
    for block in blocks.iter().take(10).cloned() {
        follower_chain.add_block(block).expect("seed follower chain");
    }
    apply_until_without_peer_reconnect(
        &mut follower,
        &mut follower_chain,
        20,
        stale_peer,
        &mut external_penalties,
    )
    .await;

    timeout(Duration::from_secs(3), async {
        loop {
            let stats = follower.stats();
            if stats.sync_local_height == 20 && stats.sync_outstanding_ranges == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("sync gauges must drain after stale-alternate recovery");

    let stats = follower.stats();
    assert_eq!(follower_chain.tip, blocks[19].hash.expect("height-20 hash"));
    assert_eq!(stats.sync_local_height, 20);
    assert!(stats.sync_transport_failures >= 1);
    assert!(stats.sync_ranges_requeued >= 1);
    assert!(stats.sync_peer_rotations >= 1);
    assert_eq!(stats.sync_connection_refresh_attempts, 0);
    assert_eq!(external_penalties, 0, "transport failure must remain sync-local");

    let _ = follower.command_tx.send(P2pCommand::Shutdown).await;
    let _ = stale.command_tx.send(P2pCommand::Shutdown).await;
}

#[tokio::test]
async fn follower_recovers_via_rotation_and_retains_cooldown_across_reconnect_2816() {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder()
        .is_test(true)
        .filter_level(log::LevelFilter::Warn)
        .try_init();

    let blocks = build_chain(30);
    let producer_port = super::helpers::allocate_port().await;
    let seed_port = super::helpers::allocate_port().await;
    let producer_addr: libp2p::Multiaddr = format!("/ip4/127.0.0.1/tcp/{producer_port}")
        .parse()
        .expect("producer address");
    let seed_addr: libp2p::Multiaddr = format!("/ip4/127.0.0.1/tcp/{seed_port}") // secret-leak-ok: `seed_port` is a loopback TCP PORT NUMBER; the heuristic matches the word "seed" in its mnemonic-seed sense
        .parse()
        .expect("seed address");

    let producer_cfg = P2pConfig {
        listen_addresses: vec![producer_addr.clone()],
        bootstrap_peers: Vec::new(),
        ..super::helpers::peering_config()
    };
    let seed_cfg = P2pConfig {
        listen_addresses: vec![seed_addr.clone()],
        bootstrap_peers: Vec::new(),
        ..super::helpers::peering_config()
    };
    let mut producer = P2pNode::start_with_initial_chain_state(
        producer_cfg,
        initial_state(&blocks, 20),
    )
    .await
    .expect("producer starts");
    let mut seed =
        P2pNode::start_with_initial_chain_state(seed_cfg, initial_state(&blocks, 20))
            .await
            .expect("seed starts");

    let mut follower_cfg = super::helpers::peering_config();
    follower_cfg.listen_addresses = vec!["/ip4/127.0.0.1/tcp/0"
        .parse()
        .expect("follower address")];
    follower_cfg.bootstrap_peers = vec![
        producer_addr.with(Protocol::P2p(producer.local_peer_id)),
        seed_addr.with(Protocol::P2p(seed.local_peer_id)),
    ];
    follower_cfg.bootstrap_redial_interval = Duration::from_secs(1);
    follower_cfg.bootstrap_redial_max_backoff = Duration::from_secs(2);
    let mut follower = P2pNode::start_with_initial_chain_state(
        follower_cfg,
        initial_state(&blocks, 10),
    )
    .await
    .expect("follower starts");

    let mut follower_chain = Blockchain::default();
    for block in blocks.iter().take(10).cloned() {
        follower_chain.add_block(block).expect("seed follower chain");
    }
    let mut external_penalties = 0u64;

    // The first request is intentionally blackholed. No NewBlock or explicit
    // ChainStatus command is sent: eager post-proof status is the only target
    // discovery path available before the periodic ten-second tick.
    let (failed_source, follower_peer, _blackholed_id, failed_request) =
        next_request(&mut producer, &mut seed, None, Duration::from_secs(5)).await;
    assert_eq!(failed_request.start_height, 11);
    assert_eq!(failed_request.count, 10);

    // The real libp2p request timer must expire, then the same range must rotate
    // to the other connected peer without an external reputation event.
    let (recovery_source, retrying_peer, recovery_id, recovery_request) =
        next_request(
            &mut producer,
            &mut seed,
            Some(follower_peer),
            Duration::from_secs(13),
        )
        .await;
    assert_ne!(recovery_source, failed_source);
    assert_eq!(retrying_peer, follower_peer);
    assert_eq!(recovery_request, failed_request);
    respond(
        recovery_source,
        recovery_id,
        &recovery_request,
        &blocks,
        &producer,
        &seed,
    )
    .await;
    apply_until(
        &mut follower,
        &mut follower_chain,
        20,
        &mut external_penalties,
    )
    .await;
    assert_eq!(
        follower_chain.tip,
        blocks[19].hash.expect("height-20 hash"),
        "first recovery must reach the exact common block hash"
    );

    let failed_peer_id = handle_for_source(failed_source, &producer, &seed).local_peer_id;
    follower
        .command_tx
        .send(P2pCommand::DisconnectPeer {
            peer_id: failed_peer_id,
            reason: "#2816 retained-cooldown regression".into(),
        })
        .await
        .expect("disconnect command accepted");
    timeout(Duration::from_secs(8), async {
        let mut disconnected = false;
        loop {
            match follower.event_rx.recv().await {
                Some(P2pEvent::PeerDisconnected(peer)) if peer == failed_peer_id => {
                    disconnected = true;
                },
                Some(P2pEvent::PeerConnected(peer)) if disconnected && peer == failed_peer_id => {
                    break;
                },
                Some(P2pEvent::PeerPenalized { .. }) => {
                    external_penalties = external_penalties.saturating_add(1);
                },
                Some(_) => {},
                None => panic!("follower event channel closed during reconnect"),
            }
        }
    })
    .await
    .expect("failed source must reconnect before its ten-second cooldown expires");

    let height_30 = initial_state(&blocks, 30);
    let failed_handle = match failed_source {
        Source::Producer => &mut producer,
        Source::Seed => &mut seed,
    };
    failed_handle
        .command_tx
        .send(P2pCommand::UpdateLocalChainState {
            height: height_30.height,
            tip_hash: height_30.tip_hash,
            state_root: height_30.state_root,
        })
        .await
        .expect("failed source state update accepted");
    failed_handle
        .command_tx
        .send(P2pCommand::BroadcastChainStatus {
            height: height_30.height,
            tip_hash: height_30.tip_hash,
            state_root: height_30.state_root,
        })
        .await
        .expect("failed source status accepted");
    assert_no_request_from(failed_handle, follower_peer, Duration::from_secs(1)).await;

    let recovery_handle = match recovery_source {
        Source::Producer => &mut producer,
        Source::Seed => &mut seed,
    };
    recovery_handle
        .command_tx
        .send(P2pCommand::UpdateLocalChainState {
            height: height_30.height,
            tip_hash: height_30.tip_hash,
            state_root: height_30.state_root,
        })
        .await
        .expect("recovery source state update accepted");
    recovery_handle
        .command_tx
        .send(P2pCommand::BroadcastChainStatus {
            height: height_30.height,
            tip_hash: height_30.tip_hash,
            state_root: height_30.state_root,
        })
        .await
        .expect("recovery source status accepted");

    let (final_source, _, final_request_id, final_request) =
        next_request(
            &mut producer,
            &mut seed,
            Some(follower_peer),
            Duration::from_secs(3),
        )
        .await;
    assert_eq!(final_source, recovery_source);
    assert_eq!(final_request.start_height, 21);
    respond(
        final_source,
        final_request_id,
        &final_request,
        &blocks,
        &producer,
        &seed,
    )
    .await;
    apply_until(
        &mut follower,
        &mut follower_chain,
        30,
        &mut external_penalties,
    )
    .await;

    timeout(Duration::from_secs(3), async {
        loop {
            let stats = follower.stats();
            if stats.sync_local_height == 30
                && stats.sync_target_height == 30
                && stats.sync_outstanding_ranges == 0
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("sync gauges must drain after exact convergence");

    let follower_stats = follower.stats();
    assert_eq!(follower_chain.tip, blocks[29].hash.expect("height-30 hash"));
    assert_eq!(follower_stats.sync_transport_failures, 1);
    assert!(follower_stats.sync_requests_dispatched >= 3);
    assert!(follower_stats.sync_responses_received >= 2);
    assert!(follower_stats.sync_ranges_requeued >= 1);
    assert!(follower_stats.sync_peer_rotations >= 1);
    assert!(follower_stats.sync_reconnect_status_broadcasts >= 3);
    assert_eq!(external_penalties, 0, "transport failures must not escape locally");

    let source_stats = producer.stats().sync_inbound_requests + seed.stats().sync_inbound_requests;
    let response_stats = producer.stats().sync_responses_sent + seed.stats().sync_responses_sent;
    assert!(source_stats >= 3, "both failed and successful dispatches must be observable");
    assert!(response_stats >= 2, "both successful response paths must be observable");

    let _ = follower.command_tx.send(P2pCommand::Shutdown).await;
    let _ = producer.command_tx.send(P2pCommand::Shutdown).await;
    let _ = seed.command_tx.send(P2pCommand::Shutdown).await;
}
