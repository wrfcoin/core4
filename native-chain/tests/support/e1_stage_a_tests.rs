// Included only by the cfg(test) P2P module; this is not a production node path.
use super::*;
use crate::p2p_events::try_reorg_inner;
use std::sync::atomic::{AtomicI64, Ordering};
use wrfcoin_shared_protocol::blockchain::state::StateTransition;
use wrfcoin_shared_protocol::blockchain::transaction::{
    CommitPayload, OracleAttestationPayload, RevealPayload,
};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::consensus::proof_of_publishing::{Clock, ProofOfPublishing};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ConsensusFeature, SigningProtocolVersion, feature_active,
};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::governance::schedule_extension::MIN_RUNUP_BLOCKS;
use wrfcoin_shared_protocol::station_registry::{
    ContributionProvenanceClass, DataQualityTier, StationLocation, WeatherStation,
};

const H: u64 = 3 + MIN_RUNUP_BLOCKS;
const START: i64 = 1_700_000_000;
const GOOD: Address = Address([0x77; 20]);
const BAD: Address = Address([0x55; 20]);

struct PinnedClock(AtomicI64);
impl Clock for PinnedClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp(self.0.load(Ordering::Relaxed), 0).unwrap()
    }
}

fn miner(
    clock: Arc<PinnedClock>,
) -> Arc<TypedConsensusManager<wrfcoin_consensus_engines::PopConsensusAdapter>> {
    let config = ConsensusFactory::default_config(ConsensusType::ProofOfPublishing);
    let mut pop = ProofOfPublishing::new(1, 60, 0, 100);
    pop.set_clock(clock);
    let adapter = wrfcoin_consensus_engines::PopConsensusAdapter::new(pop);
    adapter.declare_no_genesis_oracles_blocking();
    Arc::new(TypedConsensusManager::new(adapter, config))
}

fn key(seed: u8) -> PrivateKey {
    PrivateKey::from_bytes(&[seed; 32]).unwrap()
}

fn genesis() -> Blockchain {
    let mut chain = Blockchain::default();
    let ids = [71, 72].map(|seed| PublicKey::from_private_key(&key(seed)).to_string());
    chain
        .apply_state_transition(StateTransition::SetOracleConfig {
            oracle_set: ids.to_vec(),
            oracle_quorum: 2,
        })
        .unwrap();
    for (sid, owner) in [("e1-good", GOOD), ("e1-bad", BAD)] {
        chain.state.create_account(owner, Balance::new(100));
        chain
            .state
            .register_weather_station(WeatherStation {
                station_id: sid.into(),
                owner_address: owner.to_string(),
                location: StationLocation {
                    latitude: 0.0,
                    longitude: 0.0,
                    elevation_meters: None,
                },
                hardware_model: "synthetic".into(),
                calibration_date: None,
                is_active: true,
                reputation_score: 50,
                data_quality_tier: DataQualityTier::Gold,
                provenance_class: ContributionProvenanceClass::Real,
                ring_public_key: None,
            })
            .unwrap();
    }
    chain.state.recalculate_state_root();
    chain
}

fn weather(height: u64, bad: bool) -> Vec<Transaction> {
    let (sid, sender) = if bad {
        ("e1-bad", BAD)
    } else {
        ("e1-good", GOOD)
    };
    let data = if bad {
        serde_json::json!({"temperature":200.0,"humidity":50.0})
    } else {
        serde_json::json!({"quality":1.0,"volume":1.0,"uniqueness":1.0})
    };
    let timestamp = START + height as i64;
    let nonce = format!("e1-{sid}-{height}");
    let cid = Transaction::compute_commit_id(&data, &nonce, sid, timestamp, "contribution");
    let mut payload = CommitPayload {
        commit_id: cid.clone(),
        station_id: sid.into(),
        timestamp,
        contribution_type: "contribution".into(),
        auth_pubkey: None,
        auth_signature: None,
    };
    payload.sign_authorization(&key(73));
    let reveal = RevealPayload {
        commit_id: cid.clone(),
        station_id: sid.into(),
        timestamp,
        contribution_type: "contribution".into(),
        data,
        nonce,
        data_cid: None,
        data_hash: None,
        storage_provider: None,
        provenance: None,
        manifest: None,
        verification_receipt: None,
        compute_receipt: None,
        quality_tier: None,
    };
    let make = |kind, value: serde_json::Value| {
        let mut tx = Transaction::new(
            vec![
                wrfcoin_shared_protocol::blockchain::transaction::TransactionInput {
                    txid: wrfcoin_shared_protocol::crypto::hash::hash_string(&format!(
                        "e1-{cid}-{kind:?}-{value}"
                    )),
                    vout: 0,
                    signature: None,
                    public_key: None,
                    coinbase_data: None,
                },
            ],
            vec![
                wrfcoin_shared_protocol::blockchain::transaction::TransactionOutput {
                    amount: 1,
                    address: sender.to_string(),
                    script: "OP_TRUE".into(),
                    nft_data: None,
                },
            ],
        );
        tx.tx_type = kind;
        tx.timestamp = chrono::DateTime::from_timestamp(timestamp, 0).unwrap();
        tx.sender_address = sender;
        tx.fee = 1;
        tx.data = Some(value.to_string());
        tx.set_id();
        tx
    };
    let mut txs = vec![
        make(
            TransactionType::ContributionCommit,
            serde_json::to_value(payload).unwrap(),
        ),
        make(
            TransactionType::ContributionReveal,
            serde_json::to_value(reveal).unwrap(),
        ),
    ];
    for seed in [71, 72] {
        let sk = key(seed);
        let mut att = OracleAttestationPayload {
            commit_id: cid.clone(),
            oracle_id: PublicKey::from_private_key(&sk).to_string(),
            approved: true,
            signature: None,
            confidence: Some(1.0),
            timestamp,
            nonce: Some(format!("e1-att-{sid}-{height}-{seed}")),
        };
        att.signature = Some(sk.sign(att.signing_message().as_bytes()).to_string());
        txs.push(make(
            TransactionType::OracleAttestation,
            serde_json::to_value(att).unwrap(),
        ));
    }
    txs
}

async fn enqueue(
    pool: &Arc<RwLock<Mempool>>,
    chain: &Arc<RwLock<Blockchain>>,
    txs: Vec<Transaction>,
) {
    // secret-copy-ok: this registry contains public verification keys only.
    let registry = chain.read().await.state.signing_keys.clone();
    for tx in txs {
        pool.write()
            .await
            .add_transaction_with_signing_keys(tx, Some(&registry))
            .unwrap();
    }
}

async fn feed(block: Block, chain: &Arc<RwLock<Blockchain>>, storage: &Arc<RwLock<Storage>>) {
    let peer_clock = Arc::new(PinnedClock(AtomicI64::new(block.timestamp.timestamp())));
    let wire = bincode::deserialize(&bincode::serialize(&block).unwrap()).unwrap();
    apply_p2p_block(
        wire,
        P2pBlockSource::HistoricalSync,
        chain,
        storage,
        &miner(peer_clock),
        None,
        &mut ForkCandidateBuffer::new(),
        &mut OrphanBlockBuffer::new(),
    )
    .await;
}

#[tokio::test]
async fn e1_stage_a_three_node_activation_wal_and_reorg_drill() {
    const CHILD: &str = "WRFCOIN_E1_STAGE_A_DRILL_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let result=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","p2p_events::tests::e1_stage_a::e1_stage_a_three_node_activation_wal_and_reorg_drill","--nocapture"])
            .env(CHILD,"1").status().unwrap();
        assert!(result.success(), "isolated StageA drill failed");
        return;
    }
    let governance = install_schedule_extension_gate_3277(1);
    let require_penalty = std::env::var_os("WRFCOIN_E1_STAGE_A_REQUIRE_PENALTY").is_some();
    let clock = Arc::new(PinnedClock(AtomicI64::new(START)));
    let consensus = miner(clock.clone());
    let producer = Arc::new(RwLock::new(genesis()));
    let pool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let dirs = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
    let peers = [
        Arc::new(RwLock::new(genesis())),
        Arc::new(RwLock::new(genesis())),
    ];
    let mut stores = Vec::new();
    for dir in &dirs {
        let store = Storage::open(&dir.path().join("chain.redb"), u64::MAX).unwrap();
        store.persist_genesis_state(&genesis().state).await.unwrap();
        stores.push(Arc::new(RwLock::new(store)));
    }
    let mut common = None;
    let mut snapshots = Vec::new();
    for height in 1..=H + 1 {
        clock.0.store(START + height as i64, Ordering::Relaxed);
        if height <= 3 {
            let feature = match height {
                1 => ConsensusFeature::RewardGrossOutlierGuard,
                2 => ConsensusFeature::WeatherSkewBlockTimeAllowance,
                _ => ConsensusFeature::StationOwnerSlashIdentity,
            };
            let mut artifact = signed_schedule_extension_3277(
                &governance,
                &[(feature.name(), height + MIN_RUNUP_BLOCKS)],
            );
            artifact.extension.sequence = height - 1;
            artifact.signatures=vec![wrfcoin_shared_protocol::governance::schedule_extension::sign_feature_schedule_extension_with_key(&artifact.extension,&governance).unwrap()];
            let mut tx = Transaction::new_feature_schedule_extension(&artifact);
            tx.timestamp = chrono::DateTime::from_timestamp(START + height as i64, 0).unwrap();
            tx.set_id();
            enqueue(&pool, &producer, vec![tx]).await;
        }
        let probe = height >= H - 1;
        let bad_txs = weather(height, true);
        if probe {
            // Discrimination guard: these exact wire payloads produce the
            // physical-quality trigger under the real shared extractor. A
            // missing quorum or malformed transaction must not explain away
            // the producer's later absence of a durable penalty.
            let parent = producer.read().await;
            let candidate = Block::new(1, parent.tip, bad_txs.clone(), 1, height);
            let transitions = crate::transition_extractor::extract_transitions(
                &parent.state,
                &candidate,
                height,
                START + height as i64,
            );
            let mut shadow = parent.state.clone();
            let result = shadow.apply_transitions_batch_at_height(height, transitions.into_iter());
            let expected = if height < H {
                "e1-bad".to_string()
            } else {
                BAD.to_string()
            };
            assert!(
                matches!(result, Err(wrfcoin_shared_protocol::blockchain::state::StateError::SlashingTriggered(ref trigger))
                if trigger.oracle_id() == expected),
                "same candidate must trigger physical-quality liability at {height}: {result:?}"
            );
            drop(parent);
            enqueue(&pool, &producer, weather(height, false)).await;
            enqueue(&pool, &producer, bad_txs.clone()).await;
        }
        let before_good = producer
            .read()
            .await
            .state
            .get_account(&GOOD)
            .unwrap()
            .balance;
        let block = consensus
            .create_pop_block_with_state(producer.clone(), pool.clone(), &test_address_string(0x63))
            .await
            .unwrap();
        producer
            .read()
            .await
            .state
            .refresh_effective_feature_schedule();
        assert_eq!(block.height, height);
        if height <= 3 {
            assert_eq!(
                producer
                    .read()
                    .await
                    .state
                    .feature_schedule_extensions
                    .len(),
                height as usize
            );
            assert!(
                block
                    .transactions
                    .iter()
                    .any(|t| t.tx_type == TransactionType::FeatureScheduleExtension)
            );
        }
        if probe {
            assert_eq!(
                feature_active(ConsensusFeature::StationOwnerSlashIdentity, height),
                height >= H
            );
            assert!(
                !block
                    .transactions
                    .iter()
                    .any(|t| t.tx_type == TransactionType::ContributionReveal
                        && t.sender_address == BAD),
                "physical violation must be pruned"
            );
            assert!(
                producer
                    .read()
                    .await
                    .state
                    .get_account(&GOOD)
                    .unwrap()
                    .balance
                    > before_good,
                "ordinary quorum-backed earning must continue"
            );
            if !require_penalty {
                assert_eq!(
                    producer
                        .read()
                        .await
                        .state
                        .get_account(&BAD)
                        .unwrap()
                        .balance,
                    Balance::new(100),
                    "current producer prunes the trigger with its rejected reveal"
                );
            }
            // A poisoned block is refused atomically; then the genuine block
            // advances the same peer. The candidate is never persisted.
            let mut poison = block.clone();
            poison.transactions.push(bad_txs[1].clone());
            poison.set_hash(poison.calculate_hash());
            let before = peers[0].read().await.state.state_root;
            feed(poison, &peers[0], &stores[0]).await;
            assert_eq!(peers[0].read().await.height, height - 1);
            assert_eq!(peers[0].read().await.state.state_root, before);
            assert!(
                stores[0]
                    .read()
                    .await
                    .load_blocks_range(height, height)
                    .unwrap()
                    .is_empty()
            );
        }
        for index in 0..2 {
            feed(block.clone(), &peers[index], &stores[index]).await;
            assert_eq!(
                peers[index].read().await.height,
                height,
                "peer{index} must progress"
            );
            assert_eq!(
                peers[index].read().await.state.state_root,
                producer.read().await.state.state_root,
                "peer{index} root at{height}"
            );
            if height == H - 2 {
                stores[index]
                    .read()
                    .await
                    .force_checkpoint(&peers[index].read().await.state, height)
                    .await
                    .unwrap();
            }
        }
        if height == H - 2 {
            common = Some(producer.read().await.clone());
        }
        if probe {
            let rows = stores[0]
                .read()
                .await
                .load_wal_transitions_range(height, height)
                .unwrap();
            assert!(!rows.iter().any(|(_,t)|matches!(t,StateTransition::ApplyReveal{payload,..} if payload.station_id=="e1-bad")),"discarded reveal must not become a durable trigger");
            wrfcoin_shared_protocol::crypto::chain_binding::refresh_feature_schedule_extensions(
                Vec::new(),
            );
            let wal = stores[0]
                .read()
                .await
                .load_state_observing_replay(|s| s.refresh_effective_feature_schedule())
                .unwrap()
                .unwrap();
            assert_eq!(wal.height, height);
            assert_eq!(
                wal.state.state_root,
                producer.read().await.state.state_root,
                "WAL checkpoint+tail at{height}"
            );
            if !require_penalty {
                assert_eq!(
                    wal.state.get_account(&BAD).unwrap().balance,
                    Balance::new(100)
                );
            }
            snapshots.push((height, wal.state.state_root));
        }
    }
    assert_eq!(snapshots.len(), 3);
    // A genuinely different, longer fork crosses H from the durable H-2
    // checkpoint, through the real in-memory AND storage reorg call sites.
    let fork = Arc::new(RwLock::new(common.unwrap()));
    let empty = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let mut fork_blocks = Vec::new();
    for height in H - 1..=H + 2 {
        clock.0.store(START + height as i64, Ordering::Relaxed);
        let block = consensus
            .create_pop_block_with_state(fork.clone(), empty.clone(), &test_address_string(0x64))
            .await
            .unwrap();
        fork.read().await.state.refresh_effective_feature_schedule();
        fork_blocks.push(block);
    }
    try_reorg_inner(
        SigningProtocolVersion::V1,
        fork_blocks,
        &peers[1],
        &stores[1],
        &miner(clock.clone()),
        None,
        std::convert::identity,
    )
    .await;
    assert_eq!(
        peers[1].read().await.height,
        H + 2,
        "longer fork must be installed"
    );
    assert_eq!(
        peers[1].read().await.state.state_root,
        fork.read().await.state.state_root
    );
    assert_eq!(
        peers[1]
            .read()
            .await
            .state
            .get_account(&GOOD)
            .unwrap()
            .balance,
        Balance::new(100),
        "orphaned contribution rewards must roll back"
    );
    let expected = [
        producer.read().await.state.state_root,
        fork.read().await.state.state_root,
    ];
    drop(stores);
    for (index, dir) in dirs.iter().enumerate() {
        wrfcoin_shared_protocol::crypto::chain_binding::refresh_feature_schedule_extensions(
            Vec::new(),
        );
        let reopened = Storage::open(&dir.path().join("chain.redb"), u64::MAX).unwrap();
        let wal = reopened
            .load_state_observing_replay(|s| s.refresh_effective_feature_schedule())
            .unwrap()
            .unwrap();
        assert_eq!(
            wal.state.state_root, expected[index],
            "actual reopen after activation/reorg"
        );
        assert_eq!(wal.state.feature_schedule_extensions.len(), 3);
        assert!(feature_active(
            ConsensusFeature::StationOwnerSlashIdentity,
            H
        ));
    }
    if require_penalty {
        assert_eq!(
            producer
                .read()
                .await
                .state
                .get_account(&BAD)
                .unwrap()
                .balance,
            Balance::new(70),
            "NO-GO: no durable physical-violation penalty survives producer pruning"
        );
        assert!(
            producer
                .read()
                .await
                .state
                .oracle_registry()
                .is_in_cooldown(&BAD.to_string(), H + 1),
            "NO-GO: the accountable owner has no slashing cooldown"
        );
    }
    eprintln!(
        "E1 StageA: three nodes, signed sequence2, H-1/H/H+1 earning/atomicity/WAL/reorg PASS; economic GO requirement NO-GO"
    );
}
