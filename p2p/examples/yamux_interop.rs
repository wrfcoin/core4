//! Loopback-only two-process P2P compatibility probe.
//!
//! Build this identical source against two dependency graphs (for example the
//! libp2p version on `main` and an upgrade candidate), then run one listener
//! and one dialler. The pair must:
//!
//! 1. connect (TCP + Noise + yamux) and pass the genesis handshake,
//! 2. exchange an exact 32 KiB block over gossipsub in both directions, and
//! 3. complete a request-response block sync: the listener serves a
//!    deterministic `SYNC_BLOCKS`-block chain and the dialler, starting from
//!    an empty chain, must receive every block byte-for-byte.
//!
//! Each process exits zero only when its half passed; a timeout is a failure.
//! No chain is run and no live peer is contacted: dial addresses must be
//! loopback TCP.
//!
//! ```text
//! cargo build --locked -p wrfcoin-p2p --example yamux_interop   # in each tree
//! old/yamux_interop listen            # prints LISTEN /ip4/127.0.0.1/tcp/PORT
//! new/yamux_interop dial /ip4/127.0.0.1/tcp/PORT
//! ```
//!
//! Run it both ways round (old listens / new dials, then new listens / old
//! dials), plus a same-version control.
use std::{collections::BTreeMap, io::Write, time::Duration};

use chrono::{TimeZone, Utc};
use wrfcoin_p2p::{
    InitialChainState, P2pCommand, P2pConfig, P2pEvent, P2pNode, sync::BlockResponse,
};
use wrfcoin_shared_protocol::{
    blockchain::block::Block,
    crypto::{
        chain_binding::{ActivationTable, SigningProtocolVersion, install_activation_table},
        hash::hash_bytes,
    },
};

/// Height of the chain the listener serves over block sync.
const SYNC_BLOCKS: u64 = 20;
/// Gossip payload: below the default 64 KiB inline budget.
const GOSSIP_BYTES: usize = 32_768;

/// A deterministic chain, identical in both processes.
fn fixture_chain() -> Vec<Block> {
    let base = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let mut prev = hash_bytes(b"yamux-interop-genesis");
    (1..=SYNC_BLOCKS)
        .map(|height| {
            let mut block = Block::new(1, prev, Vec::new(), 1, height);
            block.timestamp = base + chrono::Duration::seconds(height as i64);
            let hash = block.calculate_hash();
            block.set_hash(hash);
            prev = hash;
            block
        })
        .collect()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Match the synthetic chain declared by the two-node tests. Production
    // startup installs its own canonical schedule before starting P2P.
    SigningProtocolVersion::install_active(SigningProtocolVersion::V1);
    install_activation_table(ActivationTable::default());

    let args: Vec<_> = std::env::args().collect();
    let dial = args.get(1).is_some_and(|s| s == "dial");
    if !dial && args.get(1).map(String::as_str) != Some("listen") {
        return Err("usage: yamux_interop listen | dial /ip4/127.0.0.1/tcp/PORT".into());
    }
    let mut cfg = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse()?],
        quic_listen_addresses: vec![],
        bootstrap_peers: vec![],
        network_id: "interop-fixture".into(),
        chain_id: "interop-fixture".into(),
        genesis_hash: "ab".repeat(32),
        ..P2pConfig::default()
    };
    if dial {
        let addr = args.get(2).ok_or("missing loopback address")?;
        let port = addr
            .strip_prefix("/ip4/127.0.0.1/tcp/")
            .ok_or("loopback TCP only")?;
        let _: u16 = port.parse()?;
        cfg.bootstrap_peers.push(addr.parse()?);
    }

    let chain = fixture_chain();
    let tip = chain.last().and_then(|b| b.hash).ok_or("fixture tip")?;
    let tip_hash: [u8; 32] = tip.into();
    // The listener serves the chain; the dialler starts empty and syncs it.
    let initial = if dial {
        InitialChainState::default()
    } else {
        InitialChainState {
            height: SYNC_BLOCKS,
            tip_hash,
            state_root: [0u8; 32],
        }
    };
    let mut h = P2pNode::start_with_initial_chain_state(cfg, initial).await?;

    let sent = vec![if dial { 0x71 } else { 0x29 }; GOSSIP_BYTES];
    let expected = vec![if dial { 0x29 } else { 0x71 }; GOSSIP_BYTES];

    let result = tokio::time::timeout(Duration::from_secs(90), async {
        let mut published = false;
        let mut connected = false;
        let mut subscribed = false;
        let mut gossip_ok = false;
        // Listener: heights served. Dialler: heights received.
        let mut served = std::collections::BTreeSet::new();
        let mut synced: BTreeMap<u64, Vec<u8>> = BTreeMap::new();
        let mut sync_ok = false;
        let mut status_tick = tokio::time::interval(Duration::from_secs(2));
        loop {
            let event = tokio::select! {
                event = h.event_rx.recv() => event.ok_or("event channel closed")?,
                _ = status_tick.tick() => {
                    // Gossip can drop a status sent before the mesh forms;
                    // repeat until the dialler has the whole chain.
                    if !dial && connected && subscribed && !sync_ok {
                        h.command_tx.send(P2pCommand::BroadcastChainStatus {
                            height: SYNC_BLOCKS,
                            tip_hash,
                            state_root: [0u8; 32],
                        }).await?;
                    }
                    continue;
                }
            };
            match event {
                P2pEvent::ListeningAddress(addr) if !dial => {
                    println!("LISTEN {addr}");
                    std::io::stdout().flush()?;
                },
                P2pEvent::PeerConnected(_) => connected = true,
                P2pEvent::SubscriptionStateChanged {
                    topic,
                    subscribed: true,
                    ..
                } if topic.contains("blocks") => subscribed = true,
                P2pEvent::NewBlock(bytes) => {
                    if bytes != expected {
                        return Err::<(), Box<dyn std::error::Error>>(
                            "gossip block bytes differ".into(),
                        );
                    }
                    if !gossip_ok {
                        println!("PASS gossip exact_block_bytes={}", bytes.len());
                    }
                    gossip_ok = true;
                },
                P2pEvent::InboundBlockSyncRequest {
                    request_id,
                    request,
                    ..
                } if !dial => {
                    let start = request.start_height.max(1);
                    let end = request
                        .start_height
                        .saturating_add(u64::from(request.count))
                        .min(SYNC_BLOCKS + 1);
                    let blocks: Vec<Block> = (start..end)
                        .map(|height| chain[(height - 1) as usize].clone())
                        .collect();
                    served.extend(blocks.iter().map(|b| b.height));
                    h.command_tx
                        .send(P2pCommand::RespondToBlockSyncRequest {
                            request_id,
                            response: BlockResponse {
                                blocks,
                                retry_after_ms: None,
                            },
                        })
                        .await?;
                    if served.len() as u64 == SYNC_BLOCKS && !sync_ok {
                        println!("PASS sync served_blocks={SYNC_BLOCKS}");
                        sync_ok = true;
                    }
                },
                P2pEvent::SyncedBlock {
                    height,
                    block_bytes,
                    ..
                } if dial => {
                    synced.insert(height, block_bytes);
                },
                P2pEvent::SyncCompleted { height } if dial && height == SYNC_BLOCKS => {
                    for block in &chain {
                        let want = bincode::serialize(block)?;
                        if synced.get(&block.height) != Some(&want) {
                            return Err(format!(
                                "synced block {} differs or is missing",
                                block.height
                            )
                            .into());
                        }
                    }
                    println!("PASS sync exact_blocks={SYNC_BLOCKS}");
                    sync_ok = true;
                },
                _ => {},
            }
            if connected && subscribed && !published {
                h.command_tx
                    .send(P2pCommand::BroadcastBlock(sent.clone()))
                    .await?;
                published = true;
            }
            if gossip_ok && sync_ok {
                // Let the peer's in-flight traffic drain before shutdown.
                tokio::time::sleep(Duration::from_secs(2)).await;
                return Ok(());
            }
        }
    })
    .await;
    let _ = h.command_tx.send(P2pCommand::Shutdown).await;
    match result {
        Ok(inner) => inner,
        Err(_) => Err("timed out: not a passing compatibility result".into()),
    }
}
