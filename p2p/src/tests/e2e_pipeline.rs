use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow};
use rand::rngs::StdRng;
use rand::SeedableRng;
use rustc_hash::FxHashSet;
use tokio::sync::RwLock;
use tokio::time::timeout;

use crate::{P2pCommand, P2pConfig, P2pEvent, P2pHandle, P2pNode};
use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::state::StateTransition;
use wrfcoin_shared_protocol::blockchain::transaction::{
    CommitPayload, ComputeReceipt, ForecastAccuracyMetrics, ForecastArtifactManifest,
    ForecastVerificationReceipt, OracleAttestationPayload, RevealPayload, Transaction,
    TransactionInput, TransactionOutput, TransactionType, forecast_manifest_digest,
};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::{
    MAX_SUPPLY, ProofOfPublishing, compute_target_block_time,
};
use wrfcoin_shared_protocol::crypto::hash::{Hash, hash_string};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::mempool::pool::Mempool;
use wrfcoin_shared_protocol::station_registry::{
    ContributionProvenanceClass, DataQualityTier, StationLocation, WeatherStation,
};

const TEST_TIMEOUT: Duration = Duration::from_secs(30);

struct NodeContext {
    name: &'static str,
    handle: P2pHandle,
    chain: Arc<RwLock<Blockchain>>,
    mempool: Arc<RwLock<Mempool>>,
    pop: ProofOfPublishing,
}

impl NodeContext {
    fn new(name: &'static str, handle: P2pHandle) -> Self {
        Self {
            name,
            handle,
            chain: Arc::new(RwLock::new(Blockchain::default())),
            mempool: Arc::new(RwLock::new(Mempool::new(&Config::default()))),
            // #2523: this harness chain has no oracle ceremony and says so —
            // silence at height 1 is refused, being indistinguishable from a
            // misconfigured cut.
            pop: {
                let mut pop = ProofOfPublishing::new(1, 60, 0, 32);
                pop.declare_no_genesis_oracles();
                pop
            },
        }
    }
}

fn addr(byte: u8) -> Address {
    Address([byte; 20])
}

fn oracle_identity(seed: u64) -> (PrivateKey, String) {
    let mut rng = StdRng::seed_from_u64(seed);
    let private_key = PrivateKey::generate(&mut rng);
    let oracle_id = PublicKey::from_private_key(&private_key).to_string();
    (private_key, oracle_id)
}

fn build_tx(tx_type: TransactionType, tag: &str) -> Transaction {
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_string(&format!("input-{tag}")),
            vout: 0,
            signature: None,
            public_key: None,
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            amount: 1,
            address: format!("wrf-{tag}"),
            script: "OP_TRUE".to_string(),
            nft_data: None,
        }],
    );
    tx.tx_type = tx_type;
    tx.fee = 1_000;
    tx.set_id();
    tx
}

fn test_manifest(tag: &str) -> ForecastArtifactManifest {
    ForecastArtifactManifest {
        schema_version: 1,
        model_id: format!("wrf-gfs-{tag}"),
        input_hash: "a".repeat(64),
        output_hash: "b".repeat(64),
        produced_at: 1_700_000_000,
        region: "us-west".to_string(),
        horizon_hours: 24,
        input_variables: vec![
            "humidity".to_string(),
            "pressure".to_string(),
            "temperature".to_string(),
        ],
        output_variables: vec!["temperature".to_string(), "wind_speed".to_string()],
    }
}

fn denver_weather_payload() -> serde_json::Value {
    serde_json::json!({
        "temperature_c": 18.4,
        "humidity": 55.0,
        "pressure_hpa": 835.0,
        "wind_speed_ms": 4.2
    })
}

fn coastal_weather_payload() -> serde_json::Value {
    serde_json::json!({
        "temperature_c": 17.8,
        "humidity": 58.0,
        "pressure_hpa": 1012.0,
        "wind_speed_ms": 5.0
    })
}

fn la_weather_payload() -> serde_json::Value {
    serde_json::json!({
        "temperature_c": 20.1,
        "humidity": 52.0,
        "pressure_hpa": 1008.0,
        "wind_speed_ms": 3.8
    })
}

fn invalid_weather_payload() -> serde_json::Value {
    serde_json::json!({
        "temperature_c": 200.0,
        "humidity": 55.0,
        "pressure_hpa": 50.0,
        "wind_speed_ms": -4.0
    })
}

fn weather_station(id: &str, owner: &str, lat: f64, lon: f64, elevation_m: f64) -> WeatherStation {
    WeatherStation {
        station_id: id.to_string(),
        owner_address: owner.to_string(),
        location: StationLocation {
            latitude: lat,
            longitude: lon,
            elevation_meters: Some(elevation_m),
        },
        hardware_model: "WS-5000".to_string(),
        calibration_date: Some("2026-03-20".to_string()),
        is_active: true,
        reputation_score: 90,
        data_quality_tier: DataQualityTier::Gold,
        provenance_class: ContributionProvenanceClass::Real,
        ring_public_key: None,
    }
}

fn signed_compute_receipt(worker_sk: &PrivateKey, tag: &str, completed_at: i64) -> ComputeReceipt {
    let worker_id = PublicKey::from_private_key(worker_sk).to_string();
    let mut receipt = ComputeReceipt {
        task_id: format!("task-{tag}"),
        worker_id,
        work_hash: "cd".repeat(32),
        work_factor: 20_000,
        completed_at,
        nonce: format!("compute-{tag}"),
        signature: None,
    };
    let signature = worker_sk.sign(receipt.signing_message().as_bytes()).to_string();
    receipt.signature = Some(signature);
    receipt
}

fn signed_receipt(
    oracle_sk: &PrivateKey,
    oracle_id: &str,
    manifest: &ForecastArtifactManifest,
    commit_id: &str,
    skill_score: f64,
    verified_at: i64,
) -> ForecastVerificationReceipt {
    let mut receipt = ForecastVerificationReceipt {
        manifest_digest: forecast_manifest_digest(manifest),
        commit_id: commit_id.to_string(),
        oracle_id: oracle_id.to_string(),
        metrics: ForecastAccuracyMetrics {
            mae: 0.5,
            rmse: 0.7,
            bias: 0.1,
            skill_score,
        },
        verified_at,
        nonce: format!("receipt-{commit_id}"),
        zk_proof: None,
        signature: None,
    };
    let signature = oracle_sk.sign(receipt.signing_message().as_bytes()).to_string();
    receipt.signature = Some(signature);
    receipt
}

fn signed_attestation_tx(
    commit_id: &str,
    private_key: &PrivateKey,
    oracle_id: &str,
    timestamp: i64,
    tag: &str,
) -> Transaction {
    let mut payload = OracleAttestationPayload {
        commit_id: commit_id.to_string(),
        oracle_id: oracle_id.to_string(),
        approved: true,
        signature: None,
        confidence: Some(1.0),
        timestamp,
        nonce: Some(format!("nonce-{tag}")),
    };
    let signature = private_key.sign(payload.signing_message().as_bytes()).to_string();
    payload.signature = Some(signature);

    let mut tx = build_tx(TransactionType::OracleAttestation, &format!("att-{tag}"));
    tx.data = Some(serde_json::to_string(&payload).expect("serialize attestation payload"));
    tx.set_id();
    tx
}

fn build_forecast_commit_reveal(
    station_id: &str,
    timestamp: i64,
    data: serde_json::Value,
    manifest: Option<ForecastArtifactManifest>,
    receipt: Option<ForecastVerificationReceipt>,
    compute_receipt: Option<ComputeReceipt>,
    tag: &str,
) -> (Transaction, Transaction, String) {
    let nonce = format!("nonce-{tag}");
    let commit_id = Transaction::compute_commit_id(&data, &nonce, station_id, timestamp, "forecast");

    // Sign with a deterministic key so the #1848 fail-closed admission check
    // (`verify_commit_authorization`) passes in tests (#1863 fixture completion).
    let seed = tag.bytes().fold(42u64, |acc, b| acc.saturating_add(b as u64));
    let mut rng = StdRng::seed_from_u64(seed);
    let commit_private_key = PrivateKey::generate(&mut rng);
    let mut commit_payload = CommitPayload {
        commit_id: commit_id.clone(),
        station_id: station_id.to_string(),
        timestamp,
        contribution_type: "forecast".to_string(),
        auth_pubkey: None,
        auth_signature: None,
    };
    commit_payload.sign_authorization(&commit_private_key);

    let reveal_payload = RevealPayload {
        commit_id: commit_id.clone(),
        station_id: station_id.to_string(),
        timestamp,
        contribution_type: "forecast".to_string(),
        data,
        nonce,
        data_cid: None,
        data_hash: None,
        storage_provider: None,
        quality_tier: None,
        provenance: None,
        manifest,
        verification_receipt: receipt,
        compute_receipt: compute_receipt.clone(),
    };

    let mut commit_tx = build_tx(TransactionType::ForecastCommit, &format!("commit-{tag}"));
    commit_tx.data = Some(serde_json::to_string(&commit_payload).expect("serialize commit payload"));
    commit_tx.set_id();

    let mut reveal_tx = build_tx(TransactionType::ForecastReveal, &format!("reveal-{tag}"));
    reveal_tx.data = Some(serde_json::to_string(&reveal_payload).expect("serialize reveal payload"));
    reveal_tx.fee = if compute_receipt.is_some() { 0 } else { 1_000 };
    reveal_tx.set_id();

    (commit_tx, reveal_tx, commit_id)
}

fn seed_chain(
    chain: &mut Blockchain,
    oracle_ids: &[String],
    stations: &[WeatherStation],
    contributor: Address,
) -> Result<()> {
    chain.apply_state_transition(StateTransition::SetOracleConfig {
        oracle_set: oracle_ids.to_vec(),
        oracle_quorum: 2,
    })?;
    chain.state.create_account(contributor, Balance::new(0));
    for station in stations {
        chain.state.register_weather_station(station.clone())?;
    }
    chain.state.recalculate_state_root();
    Ok(())
}

async fn three_node_mesh() -> Result<(P2pHandle, P2pHandle, P2pHandle)> {
    // Declare all identities and addresses before startup, so each node can
    // bootstrap both other nodes instead of relying on discovery to turn a
    // hub into the two-peer mesh asserted by the pipeline below (Refs #2534).
    let keypairs = [0xa1, 0xa2, 0xa3].map(|seed| {
        libp2p::identity::Keypair::ed25519_from_bytes([seed; 32])
            .expect("valid synthetic fixture identity")
    });
    // Hold all three reservations together so the OS cannot hand the same
    // released ephemeral port back to another member of this fixture.
    let mut listeners = Vec::new();
    let mut listen_addresses = Vec::new();
    for _ in 0..3 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address: libp2p::Multiaddr =
            format!("/ip4/127.0.0.1/tcp/{}", listener.local_addr()?.port()).parse()?;
        listeners.push(listener);
        listen_addresses.push(address);
    }
    let peer_addresses: Vec<_> = listen_addresses
        .iter()
        .zip(&keypairs)
        .map(|(address, keypair)| {
            address.clone().with(libp2p::multiaddr::Protocol::P2p(
                keypair.public().to_peer_id(),
            ))
        })
        .collect();
    let mut handles = Vec::new();
    for (index, (keypair, listener)) in keypairs.into_iter().zip(listeners).enumerate() {
        let config = P2pConfig {
            listen_addresses: vec![listen_addresses[index].clone()],
            bootstrap_peers: peer_addresses
                .iter()
                .enumerate()
                .filter(|(peer, _)| *peer != index)
                .map(|(_, address)| address.clone())
                .collect(),
            max_peers_per_subnet: 0,
            ..super::helpers::peering_config()
        };
        drop(listener);
        handles.push(P2pNode::start_with_keypair(config, keypair).await?);
    }
    let [a, b, c]: [P2pHandle; 3] = handles
        .try_into()
        .unwrap_or_else(|_| unreachable!("exactly three fixture nodes"));
    Ok((a, b, c))
}

async fn wait_for_connected_peers(handle: &mut P2pHandle, expected: usize) -> Result<HashSet<libp2p::PeerId>> {
    let seen = timeout(TEST_TIMEOUT, async {
        let mut peers = HashSet::new();
        while peers.len() < expected {
            match handle.event_rx.recv().await {
                Some(P2pEvent::PeerConnected(pid)) => {
                    peers.insert(pid);
                },
                Some(_) => {},
                None => return Err(anyhow!("event channel closed")),
            }
        }
        Ok::<_, anyhow::Error>(peers)
    })
    .await
    .map_err(|_| anyhow!("timed out waiting for peers"))??;
    Ok(seen)
}

async fn enqueue(node: &NodeContext, txs: Vec<Transaction>) -> Result<()> {
    let signing_keys = {
        let chain = node.chain.read().await;
        chain.state.signing_keys.clone()
    };
    let mut mempool = node.mempool.write().await;
    for tx in txs {
        mempool.add_transaction_with_signing_keys(tx, Some(&signing_keys))?;
    }
    Ok(())
}

fn recent_timestamps(chain: &Blockchain) -> Vec<i64> {
    chain.blocks().values().rev().take(11).map(|block| block.timestamp.timestamp()).collect()
}

async fn wait_for_weather_data(handle: &mut P2pHandle, station_id: &str, data: &[u8]) -> Result<()> {
    timeout(TEST_TIMEOUT, async {
        loop {
            match handle.event_rx.recv().await {
                Some(P2pEvent::WeatherData { station_id: seen_id, data: seen_data, .. })
                    if seen_id == station_id && seen_data == data =>
                {
                    return Ok::<(), anyhow::Error>(());
                },
                Some(_) => {},
                None => return Err(anyhow!("event channel closed")),
            }
        }
    })
    .await
    .map_err(|_| anyhow!("timed out waiting for weather gossip"))??;
    Ok(())
}

async fn wait_for_block_bytes(handle: &mut P2pHandle, expected_height: u64) -> Result<Vec<u8>> {
    let bytes = timeout(TEST_TIMEOUT, async {
        loop {
            match handle.event_rx.recv().await {
                Some(P2pEvent::NewBlock(bytes)) => {
                    let block: Block = bincode::deserialize(&bytes)?;
                    if block.height == expected_height {
                        return Ok::<Vec<u8>, anyhow::Error>(bytes);
                    }
                },
                Some(_) => {},
                None => return Err(anyhow!("event channel closed")),
            }
        }
    })
    .await
    .map_err(|_| anyhow!("timed out waiting for block gossip"))??;
    Ok(bytes)
}

async fn verify_and_replay_received_block(node: &mut NodeContext, block_bytes: Vec<u8>) -> Result<Block> {
    let mut block: Block = bincode::deserialize(&block_bytes)?;
    let (oracle_set, oracle_quorum, signing_keys, recent) = {
        let chain = node.chain.read().await;
        (
            chain.state.oracle_set.clone(),
            chain.state.oracle_quorum,
            chain.state.signing_keys.clone(),
            recent_timestamps(&chain),
        )
    };
    node.pop.set_oracle_signing_keys(signing_keys);
    let accepted = node
        .pop
        .verify_block(&block, &oracle_set, oracle_quorum, &recent)
        .await?;
    if !accepted {
        return Err(anyhow!("{} rejected block at height {}", node.name, block.height));
    }

    let mut chain = node.chain.write().await;
    node.pop
        .replay_received_block_for_test_support(&mut chain, &mut block)?;
    Ok(block)
}

async fn broadcast_and_replay_block(
    producer: &mut NodeContext,
    follower_a: &mut NodeContext,
    follower_b: &mut NodeContext,
    block: &Block,
) -> Result<()> {
    let payload = bincode::serialize(block)?;
    producer
        .handle
        .command_tx
        .send(P2pCommand::BroadcastBlock(payload))
        .await
        .map_err(|_| anyhow!("failed to broadcast block from {}", producer.name))?;

    let bytes_a = wait_for_block_bytes(&mut follower_a.handle, block.height).await?;
    let bytes_b = wait_for_block_bytes(&mut follower_b.handle, block.height).await?;
    verify_and_replay_received_block(follower_a, bytes_a).await?;
    verify_and_replay_received_block(follower_b, bytes_b).await?;
    Ok(())
}

async fn assert_converged(
    node_a: &NodeContext,
    node_b: &NodeContext,
    node_c: &NodeContext,
    contributor: Address,
) -> Result<()> {
    let chain_a = node_a.chain.read().await;
    let chain_b = node_b.chain.read().await;
    let chain_c = node_c.chain.read().await;

    assert_eq!(chain_a.height, chain_b.height, "height mismatch between A and B");
    assert_eq!(chain_b.height, chain_c.height, "height mismatch between B and C");
    assert_eq!(chain_a.tip, chain_b.tip, "tip mismatch between A and B");
    assert_eq!(chain_b.tip, chain_c.tip, "tip mismatch between B and C");
    assert_eq!(chain_a.state.state_root, chain_b.state.state_root, "state root mismatch between A and B");
    assert_eq!(chain_b.state.state_root, chain_c.state.state_root, "state root mismatch between B and C");

    let bal_a = chain_a.state.get_account(&contributor).map(|account| account.balance);
    let bal_b = chain_b.state.get_account(&contributor).map(|account| account.balance);
    let bal_c = chain_c.state.get_account(&contributor).map(|account| account.balance);
    assert_eq!(bal_a, bal_b, "contributor balance mismatch between A and B");
    assert_eq!(bal_b, bal_c, "contributor balance mismatch between B and C");

    for chain in [&*chain_a, &*chain_b, &*chain_c] {
        assert!(
            chain.state.total_minted_supply() <= MAX_SUPPLY,
            "total supply exceeded MAX_SUPPLY"
        );
    }

    Ok(())
}

async fn shutdown(node: &NodeContext) {
    let _ = node.handle.command_tx.send(P2pCommand::Shutdown).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn end_to_end_multi_node_pipeline_converges() -> Result<()> {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    let (handle_a, handle_b, handle_c) = three_node_mesh().await?;
    let mut node_a = NodeContext::new("node-a", handle_a);
    let mut node_b = NodeContext::new("node-b", handle_b);
    let mut node_c = NodeContext::new("node-c", handle_c);

    let contributor = addr(0x42);
    let station_owner = hex::encode(contributor.0);
    let station_a = weather_station("station-alpha", &station_owner, 39.7392, -104.9903, 1600.0);
    let station_b = weather_station("station-bravo", &station_owner, 37.7749, -122.4194, 16.0);
    let station_c = weather_station("station-charlie", &station_owner, 34.0522, -118.2437, 71.0);
    let stations = vec![station_a.clone(), station_b.clone(), station_c.clone()];

    let (oracle_a_sk, oracle_a_id) = oracle_identity(5001);
    let (oracle_b_sk, oracle_b_id) = oracle_identity(5002);
    let (oracle_c_sk, oracle_c_id) = oracle_identity(5003);
    let oracle_ids = vec![oracle_a_id.clone(), oracle_b_id.clone(), oracle_c_id.clone()];

    for node in [&node_a, &node_b, &node_c] {
        let mut chain = node.chain.write().await;
        seed_chain(&mut chain, &oracle_ids, &stations, contributor)?;
    }

    let peers_a = wait_for_connected_peers(&mut node_a.handle, 2).await?;
    let peers_b = wait_for_connected_peers(&mut node_b.handle, 2).await?;
    let peers_c = wait_for_connected_peers(&mut node_c.handle, 2).await?;
    assert_eq!(peers_a.len(), 2);
    assert_eq!(peers_b.len(), 2);
    assert_eq!(peers_c.len(), 2);
    assert_eq!(
        peers_a,
        HashSet::from([node_b.handle.local_peer_id, node_c.handle.local_peer_id])
    );
    assert_eq!(
        peers_b,
        HashSet::from([node_a.handle.local_peer_id, node_c.handle.local_peer_id])
    );
    assert_eq!(
        peers_c,
        HashSet::from([node_a.handle.local_peer_id, node_b.handle.local_peer_id])
    );

    // Wait for gossipsub mesh formation via subscription confirmations.
    let (_, _, _) = tokio::join!(
        super::helpers::wait_for_gossipsub_subscriptions(&mut node_a.handle, 1, Duration::from_secs(30)),
        super::helpers::wait_for_gossipsub_subscriptions(&mut node_b.handle, 1, Duration::from_secs(30)),
        super::helpers::wait_for_gossipsub_subscriptions(&mut node_c.handle, 1, Duration::from_secs(30))
    );

    let weather_bytes = serde_json::to_vec(&denver_weather_payload())?;
    node_a
        .handle
        .command_tx
        .send(P2pCommand::BroadcastWeatherData {
            station_id: station_a.station_id.clone(),
            data: weather_bytes.clone(),
            station_signing_key: oracle_a_sk.clone(),
        })
        .await
        .map_err(|_| anyhow!("weather gossip send failed"))?;
    wait_for_weather_data(&mut node_c.handle, &station_a.station_id, &weather_bytes).await?;

    let worker_receipt = signed_compute_receipt(&oracle_c_sk, "alpha", 1_700_330_000);
    let manifest_a = test_manifest("alpha");
    let payload_a = denver_weather_payload();
    let timestamp_a = 1_700_330_000i64;
    let (commit_a, mut reveal_a, commit_id_a) = build_forecast_commit_reveal(
        &station_a.station_id,
        timestamp_a,
        payload_a.clone(),
        Some(manifest_a.clone()),
        None,
        Some(worker_receipt),
        "alpha",
    );
    reveal_a.sender_address = contributor;
    reveal_a.set_id();

    enqueue(&node_a, vec![commit_a.clone()]).await?;
    let block_1 = node_a
        .pop
        .create_next_block(node_a.chain.clone(), node_a.mempool.clone(), "wrf-miner-a")
        .await?;
    broadcast_and_replay_block(&mut node_a, &mut node_b, &mut node_c, &block_1).await?;
    assert_converged(&node_a, &node_b, &node_c, contributor).await?;

    for node in [&node_a, &node_b, &node_c] {
        let chain = node.chain.read().await;
        assert!(
            chain.state.pending_commits.contains_key(&commit_id_a),
            "{} missing commit record after block 1",
            node.name
        );
    }

    let receipt_a = signed_receipt(
        &oracle_a_sk,
        &oracle_a_id,
        &manifest_a,
        &commit_id_a,
        0.92,
        timestamp_a + 15,
    );
    let (_, mut reward_reveal_a, _) = build_forecast_commit_reveal(
        &station_a.station_id,
        timestamp_a,
        payload_a,
        Some(manifest_a),
        Some(receipt_a),
        Some(signed_compute_receipt(&oracle_c_sk, "alpha-replay", 1_700_330_015)),
        "alpha",
    );
    reward_reveal_a.sender_address = contributor;
    reward_reveal_a.set_id();
    let att_a = signed_attestation_tx(&commit_id_a, &oracle_a_sk, &oracle_a_id, timestamp_a + 20, "alpha-a");
    let att_b = signed_attestation_tx(&commit_id_a, &oracle_b_sk, &oracle_b_id, timestamp_a + 21, "alpha-b");

    let contributor_before = {
        let chain = node_b.chain.read().await;
        chain.state.get_account(&contributor).unwrap().earned_balance_total
    };

    enqueue(&node_b, vec![reward_reveal_a, att_a, att_b]).await?;
    let block_2 = node_b
        .pop
        .create_next_block(node_b.chain.clone(), node_b.mempool.clone(), "wrf-miner-b")
        .await?;
    broadcast_and_replay_block(&mut node_b, &mut node_a, &mut node_c, &block_2).await?;
    assert_converged(&node_a, &node_b, &node_c, contributor).await?;

    let contributor_after = {
        let chain = node_a.chain.read().await;
        chain.state.get_account(&contributor).unwrap().earned_balance_total
    };
    assert!(contributor_after > contributor_before, "contributor should earn forecast rewards");
    assert_eq!(
        block_2.metadata.get("forecast_rewards_minted_count").map(String::as_str),
        Some("1"),
        "reward block should mint one forecast reward"
    );

    let timestamp_b = 1_700_330_100i64;
    let manifest_b = test_manifest("bravo");
    let payload_b = coastal_weather_payload();
    let (commit_b, _reveal_b, commit_id_b) = build_forecast_commit_reveal(
        &station_b.station_id,
        timestamp_b,
        payload_b.clone(),
        Some(manifest_b.clone()),
        None,
        Some(signed_compute_receipt(&oracle_c_sk, "bravo-commit", timestamp_b)),
        "bravo",
    );
    let receipt_b = signed_receipt(&oracle_b_sk, &oracle_b_id, &manifest_b, &commit_id_b, 0.88, timestamp_b + 10);
    let (_, mut reveal_b, _) = build_forecast_commit_reveal(
        &station_b.station_id,
        timestamp_b,
        payload_b,
        Some(manifest_b),
        Some(receipt_b),
        Some(signed_compute_receipt(&oracle_c_sk, "bravo-reveal", timestamp_b + 5)),
        "bravo",
    );
    reveal_b.sender_address = contributor;
    reveal_b.set_id();
    let bravo_att_a = signed_attestation_tx(&commit_id_b, &oracle_a_sk, &oracle_a_id, timestamp_b + 11, "bravo-a");
    let bravo_att_b = signed_attestation_tx(&commit_id_b, &oracle_b_sk, &oracle_b_id, timestamp_b + 12, "bravo-b");
    enqueue(&node_c, vec![commit_b, reveal_b, bravo_att_a, bravo_att_b]).await?;
    let block_3 = node_c
        .pop
        .create_next_block(node_c.chain.clone(), node_c.mempool.clone(), "wrf-miner-c")
        .await?;
    broadcast_and_replay_block(&mut node_c, &mut node_a, &mut node_b, &block_3).await?;
    assert_converged(&node_a, &node_b, &node_c, contributor).await?;

    let timestamp_c = 1_700_330_130i64;
    let manifest_c = test_manifest("charlie");
    let payload_c = la_weather_payload();
    let (commit_c, _reveal_c, commit_id_c) = build_forecast_commit_reveal(
        &station_c.station_id,
        timestamp_c,
        payload_c.clone(),
        Some(manifest_c.clone()),
        None,
        Some(signed_compute_receipt(&oracle_c_sk, "charlie-commit", timestamp_c)),
        "charlie",
    );
    let receipt_c = signed_receipt(&oracle_c_sk, &oracle_c_id, &manifest_c, &commit_id_c, 0.89, timestamp_c + 10);
    let (_, mut reveal_c, _) = build_forecast_commit_reveal(
        &station_c.station_id,
        timestamp_c,
        payload_c,
        Some(manifest_c),
        Some(receipt_c),
        Some(signed_compute_receipt(&oracle_c_sk, "charlie-reveal", timestamp_c + 5)),
        "charlie",
    );
    reveal_c.sender_address = contributor;
    reveal_c.set_id();
    let charlie_att_a = signed_attestation_tx(&commit_id_c, &oracle_a_sk, &oracle_a_id, timestamp_c + 11, "charlie-a");
    let charlie_att_b = signed_attestation_tx(&commit_id_c, &oracle_b_sk, &oracle_b_id, timestamp_c + 12, "charlie-b");
    enqueue(&node_a, vec![commit_c, reveal_c, charlie_att_a, charlie_att_b]).await?;
    let block_4 = node_a
        .pop
        .create_next_block(node_a.chain.clone(), node_a.mempool.clone(), "wrf-miner-a-2")
        .await?;
    broadcast_and_replay_block(&mut node_a, &mut node_b, &mut node_c, &block_4).await?;
    assert_converged(&node_a, &node_b, &node_c, contributor).await?;

    let heights = {
        let chain = node_a.chain.read().await;
        (1..=chain.height).map(compute_target_block_time).collect::<Vec<_>>()
    };
    assert!(
        heights.windows(2).all(|pair| pair[1] <= pair[0]),
        "target block time should be non-increasing across heights"
    );

    shutdown(&node_a).await;
    shutdown(&node_b).await;
    shutdown(&node_c).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    Ok(())
}

fn single_node_harness() -> Result<(Arc<RwLock<Blockchain>>, Arc<RwLock<Mempool>>, ProofOfPublishing, WeatherStation, Address, PrivateKey, String, PrivateKey, String, PrivateKey, String)> {
    let contributor = addr(0x44);
    let station_owner = hex::encode(contributor.0);
    let station = weather_station("station-negative", &station_owner, 39.7392, -104.9903, 1600.0);
    let (oracle_a_sk, oracle_a_id) = oracle_identity(6001);
    let (oracle_b_sk, oracle_b_id) = oracle_identity(6002);
    let (oracle_c_sk, oracle_c_id) = oracle_identity(6003);
    let oracle_ids = vec![oracle_a_id.clone(), oracle_b_id.clone(), oracle_c_id.clone()];

    let mut chain = Blockchain::default();
    seed_chain(&mut chain, &oracle_ids, std::slice::from_ref(&station), contributor)?;

    Ok((
        Arc::new(RwLock::new(chain)),
        Arc::new(RwLock::new(Mempool::new(&Config::default()))),
        {
            // #2523: no oracle ceremony on this harness chain, stated explicitly.
            let mut pop = ProofOfPublishing::new(1, 60, 0, 32);
            pop.declare_no_genesis_oracles();
            pop
        },
        station,
        contributor,
        oracle_a_sk,
        oracle_a_id,
        oracle_b_sk,
        oracle_b_id,
        oracle_c_sk,
        oracle_c_id,
    ))
}

#[tokio::test]
async fn reject_invalid_oracle_attestation_signature() -> Result<()> {
    let (_chain, mempool, _pop, station, contributor, _oracle_a_sk, _oracle_a_id, _oracle_b_sk, _oracle_b_id, _oracle_c_sk, _oracle_c_id) =
        single_node_harness()?;
    let manifest = test_manifest("unauthorized");
    let timestamp = 1_700_440_000i64;
    let (outsider_sk, outsider_id) = oracle_identity(6999);
    let (commit, mut reveal, commit_id) = build_forecast_commit_reveal(
        &station.station_id,
        timestamp,
        denver_weather_payload(),
        Some(manifest),
        None,
        None,
        "unauthorized",
    );
    reveal.sender_address = contributor;
    reveal.set_id();
    let mut bad_attestation = signed_attestation_tx(
        &commit_id,
        &outsider_sk,
        &outsider_id,
        timestamp + 1,
        "invalid-oracle-signature",
    );
    let mut payload = bad_attestation
        .parse_oracle_attestation_payload()
        .expect("attestation payload");
    payload.signature = Some("00".repeat(64));
    bad_attestation.data = Some(serde_json::to_string(&payload)?);
    bad_attestation.set_id();

    {
        let mut guard = mempool.write().await;
        guard.add_transaction(commit)?;
        guard.add_transaction(reveal)?;
        let err = guard
            .add_transaction(bad_attestation)
            .expect_err("invalid oracle attestation signature must be rejected");
        let err_text = err.to_string().to_lowercase();
        assert!(
            err_text.contains("oracle") || err_text.contains("signature") || err_text.contains("attestation"),
            "unexpected oracle attestation rejection: {err_text}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn reject_unauthorized_oracle_submission() -> Result<()> {
    let (chain, _mempool, mut pop, station, contributor, _oracle_a_sk, _oracle_a_id, _oracle_b_sk, _oracle_b_id, _oracle_c_sk, _oracle_c_id) =
        single_node_harness()?;
    let (outsider_sk, outsider_id) = oracle_identity(6999);
    let manifest = test_manifest("unauthorized-score");
    let timestamp = 1_700_440_050i64;
    let payload = denver_weather_payload();
    let commit_id = Transaction::compute_commit_id(
        &payload,
        "nonce-unauthorized-score",
        &station.station_id,
        timestamp,
        "forecast",
    );
    let receipt = signed_receipt(
        &outsider_sk,
        &outsider_id,
        &manifest,
        &commit_id,
        0.97,
        timestamp + 5,
    );
    let (commit, mut reveal, _) = build_forecast_commit_reveal(
        &station.station_id,
        timestamp,
        payload,
        Some(manifest),
        Some(receipt),
        None,
        "unauthorized-score",
    );
    reveal.sender_address = contributor;
    reveal.set_id();

    let claimed_score = {
        let mut stale_oracle_pop = ProofOfPublishing::new(1, 60, 0, 32);
        let mut stale_oracles = FxHashSet::default();
        stale_oracles.insert(outsider_id);
        stale_oracle_pop.set_authorized_oracles(stale_oracles);
        stale_oracle_pop.calculate_block_publishing_score(&[commit.clone(), reveal.clone()], 1)
    };
    let coinbase = Transaction::new_coinbase("wrf-negative-miner", 50, 1);
    let mut block = Block::new(3, Hash::default(), vec![coinbase, commit, reveal], 1, 1);
    block
        .metadata
        .insert("publishing_score".to_string(), claimed_score.saturating_add(1).to_string());
    let hash = block.calculate_hash();
    block.set_hash(hash);

    let (oracle_set, oracle_quorum) = {
        let chain = chain.read().await;
        (chain.state.oracle_set.clone(), chain.state.oracle_quorum)
    };
    let accepted = pop
        .verify_block(&block, &oracle_set, oracle_quorum, &[])
        .await?;
    assert!(!accepted, "block claiming an unauthorized oracle-scored receipt must be rejected");
    Ok(())
}

#[tokio::test]
async fn reject_physics_violation_weather_data() -> Result<()> {
    let (chain, mempool, mut pop, station, contributor, _oracle_a_sk, _oracle_a_id, _oracle_b_sk, _oracle_b_id, _oracle_c_sk, _oracle_c_id) =
        single_node_harness()?;
    let manifest = test_manifest("physics");
    let timestamp = 1_700_440_100i64;
    let (commit, mut reveal, _commit_id) = build_forecast_commit_reveal(
        &station.station_id,
        timestamp,
        invalid_weather_payload(),
        Some(manifest),
        None,
        None,
        "physics",
    );
    reveal.sender_address = contributor;
    reveal.set_id();
    let reveal_id = reveal.id.expect("forecast reveal must have an id");

    {
        let mut guard = mempool.write().await;
        guard.add_transaction(commit)?;
    }
    let _commit_block = pop
        .create_next_block(chain.clone(), mempool.clone(), "wrf-negative-miner")
        .await?;
    {
        let mut guard = mempool.write().await;
        guard.add_transaction(reveal)?;
    }

    let block = pop
        .create_next_block(chain.clone(), mempool.clone(), "wrf-negative-miner")
        .await
        .expect("invalid weather physics should be pruned, not abort block production");
    let skipped = block
        .metadata
        .get("pop_skipped_mempool_transactions")
        .map(String::as_str)
        .unwrap_or("missing");
    assert!(
        !block
            .transactions
            .iter()
            .any(|tx| tx.id == Some(reveal_id)),
        "physics-violating reveal should be pruned from the final block"
    );
    assert_eq!(
        skipped,
        "1",
        "exactly one invalid mempool transaction should be skipped"
    );
    {
        let guard = mempool.read().await;
        assert!(
            guard.get_transaction(&reveal_id).is_none(),
            "physics-violating reveal should be evicted from the mempool"
        );
    }
    Ok(())
}

#[tokio::test]
async fn reject_reveal_with_wrong_secret() -> Result<()> {
    let (chain, mempool, mut pop, station, contributor, _oracle_a_sk, _oracle_a_id, _oracle_b_sk, _oracle_b_id, _oracle_c_sk, _oracle_c_id) =
        single_node_harness()?;

    let timestamp = 1_700_440_200i64;
    let manifest = test_manifest("wrong-secret");
    let payload = denver_weather_payload();
    let (commit, mut valid_reveal, _commit_id) = build_forecast_commit_reveal(
        &station.station_id,
        timestamp,
        payload.clone(),
        Some(manifest.clone()),
        None,
        None,
        "wrong-secret",
    );
    valid_reveal.sender_address = contributor;
    valid_reveal.set_id();

    {
        let mut guard = mempool.write().await;
        guard.add_transaction(commit)?;
    }
    let _commit_block = pop
        .create_next_block(chain.clone(), mempool.clone(), "wrf-negative-miner")
        .await?;

    let mut wrong_reveal_payload = valid_reveal
        .parse_reveal_payload()
        .expect("valid reveal payload");
    wrong_reveal_payload.nonce = "nonce-wrong-secret-tampered".to_string();
    let mut wrong_reveal = build_tx(TransactionType::ForecastReveal, "wrong-secret-tampered");
    wrong_reveal.data = Some(serde_json::to_string(&wrong_reveal_payload)?);
    wrong_reveal.sender_address = contributor;
    wrong_reveal.set_id();

    let err = {
        let mut guard = mempool.write().await;
        guard
            .add_transaction(wrong_reveal)
            .expect_err("tampered reveal must be rejected before block creation")
    };
    assert!(err.to_string().to_lowercase().contains("hash"));
    Ok(())
}

#[tokio::test]
async fn reject_double_reveal() -> Result<()> {
    let (chain, mempool, mut pop, station, contributor, _oracle_a_sk, _oracle_a_id, _oracle_b_sk, _oracle_b_id, _oracle_c_sk, _oracle_c_id) =
        single_node_harness()?;

    let timestamp = 1_700_440_260i64;
    let manifest = test_manifest("double-reveal");
    let payload = denver_weather_payload();
    let (commit, mut reveal, _commit_id) = build_forecast_commit_reveal(
        &station.station_id,
        timestamp,
        payload,
        Some(manifest),
        None,
        None,
        "double-reveal",
    );
    reveal.sender_address = contributor;
    reveal.set_id();
    let reveal_id = reveal.id.expect("forecast reveal must have an id");

    {
        let mut guard = mempool.write().await;
        guard.add_transaction(commit)?;
    }
    let _commit_block = pop
        .create_next_block(chain.clone(), mempool.clone(), "wrf-negative-miner")
        .await?;

    {
        let mut guard = mempool.write().await;
        guard.add_transaction(reveal.clone())?;
    }
    let _first_reveal_block = pop
        .create_next_block(chain.clone(), mempool.clone(), "wrf-negative-miner")
        .await?;

    {
        let mut guard = mempool.write().await;
        guard.add_transaction(reveal)?;
    }
    let block = pop
        .create_next_block(chain.clone(), mempool.clone(), "wrf-negative-miner")
        .await
        .expect("double reveal should be pruned, not abort block production");
    assert!(
        !block
            .transactions
            .iter()
            .any(|tx| tx.id == Some(reveal_id)),
        "double reveal should be pruned from the final block"
    );
    assert_eq!(
        block
            .metadata
            .get("pop_skipped_mempool_transactions")
            .map(String::as_str),
        Some("1"),
        "exactly one invalid mempool transaction should be skipped"
    );
    {
        let guard = mempool.read().await;
        assert!(
            guard.get_transaction(&reveal_id).is_none(),
            "double reveal should be evicted from the mempool"
        );
    }

    Ok(())
}
