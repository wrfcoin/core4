//! `attester_pay_v2`: producer / follower / WAL-replay / checkpoint parity
//! through the real block producer, across the activation height `H`.
//!
//! One child process (the feature schedule is process-wide), on a chain whose
//! node registry holds one bond. Four oracles, quorum 2, so three reject votes
//! settle a decision as rejected.
//!
//! - **H - 2** (inactive): a real station's reading gets three reject votes.
//!   The producer holds the reveal AND the votes back, as on main. A reading
//!   from an unclassified station with two approvals is mined and its
//!   approvers are paid to their own addresses.
//! - **H - 1** (inactive): nothing changes; the held group stays held.
//! - **H**: the same three reject votes are now mined (the reveal is still
//!   held back, as block validity requires), the rejected decision is owed and
//!   settled in that block together with an approved one, and oracle 1, whose
//!   registration in the same block names the bond, is paid to the bond's
//!   payout account.
//! - **H + 1**: a fourth reject vote is mined and paid nothing.
//!
//! At every height the producer's block is verified and validated as a
//! follower would, applied through `extract_transitions`, and the follower's
//! `state_root` must equal the producer's. A tampered settlement record is
//! refused. The WAL replays to the same root, and a checkpoint holding the
//! binding and an unpaid reject debt restores to the same root and pays the
//! same settlement.

#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/transition_extractor.rs"]
mod transition_extractor;

mod helpers {
    pub(crate) mod test_env {
        // Mount the canonical test-only lock and fixture pins (#3521).
        include!("../src/bin/launch_api/test_env.rs");
    }
}

#[allow(dead_code)]
#[path = "../../shared-protocol/tests/common/node_bond_fixture.rs"]
mod fixture;

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use rustc_hash::FxHashSet;
use tokio::sync::RwLock;
use transition_extractor::extract_transitions;
use wrfcoin_consensus_engines::{ConsensusEngine, PopConsensusAdapter};
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::config::settings::Config;
use wrfcoin_native_chain::consensus::proof_of_publishing::{Clock, ProofOfPublishing};
use wrfcoin_native_chain::mempool::pool::Mempool;
use wrfcoin_native_chain::storage::Storage;
use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::transaction::{
    CommitPayload, OracleAttestationPayload, RevealPayload, Transaction, TransactionInput,
    TransactionOutput, TransactionType,
};
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::consensus::node_share_contracts::NodeBondAction;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::{
    ATTESTER_SETTLEMENTS_METADATA_KEY, EmissionRole, RICH_AMBIENT_REWARD_ACTIVATION_HEIGHT,
};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ChainIdentity, ConsensusFeature, FeatureSchedule, feature_active, install_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::hash::hash_string;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::oracle::attestation::AttesterSettlementRecord;
use wrfcoin_shared_protocol::station_registry::{
    ContributionProvenanceClass, DataQualityTier, StationLocation, WeatherStation,
};

/// The state is seeded at this height: the height from which the producer
/// holds a real reveal back until it has approve quorum.
const BASE: u64 = RICH_AMBIENT_REWARD_ACTIVATION_HEIGHT;
/// Activation height of `attester_pay_v2` in this child.
const H: u64 = BASE + 3;
const CHILD_ENV: &str = "WRFCOIN_ATTESTER_PAY_V2_PARITY_CHILD";
const TEST_NAME: &str = "attester_pay_v2_producer_follower_wal_checkpoint";
const GOVERNANCE_ENV: &str = "WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS";
const GOVERNANCE_SEED: u8 = 0x77;
const REAL_STATION: &str = "st-apv2-real";
const FREE_STATION: &str = "st-apv2-free";
const START: i64 = 1_800_000_000;
const STEP: i64 = 10;

struct ManualClock(AtomicI64);
impl Clock for ManualClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp(self.0.load(Ordering::SeqCst), 0).expect("representable")
    }
}

fn key(seed: u8) -> PrivateKey {
    PrivateKey::from_bytes(&[seed; 32]).expect("32-byte ed25519 seed")
}
fn address_of(k: &PrivateKey) -> Address {
    Address::from_public_key(&PublicKey::from_private_key(k))
}
/// Oracle `n` (1-based), clear of every key the bond fixture uses.
fn oracle_key(n: u8) -> PrivateKey {
    key(0x30 + n)
}
fn oracle(n: u8) -> String {
    PublicKey::from_private_key(&oracle_key(n)).to_hex()
}
fn own_address(n: u8) -> Address {
    address_of(&oracle_key(n))
}

fn shell(tx_type: TransactionType, sender: Address, now: i64, tag: &str) -> Transaction {
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_string(&format!("attester-pay-v2-parity-{tag}")),
            vout: 0,
            signature: None,
            public_key: None,
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            amount: 0,
            address: sender.to_string(),
            script: "OP_TRUE".to_string(),
            nft_data: None,
        }],
    );
    tx.tx_type = tx_type;
    tx.sender_address = sender;
    tx.timestamp = chrono::DateTime::from_timestamp(now, 0).expect("representable");
    tx
}

fn with_data<T: serde::Serialize>(mut tx: Transaction, payload: &T, fee: u64) -> Transaction {
    tx.data = Some(serde_json::to_string(payload).expect("payload serializes"));
    tx.fee = fee;
    tx.set_id();
    tx
}

struct Reading {
    commit: Transaction,
    reveal: Transaction,
    commit_id: String,
}

/// A station reading (commit and reveal) signed by the station owner.
fn reading(owner: &PrivateKey, station: &str, now: i64, tag: &str) -> Reading {
    let data = serde_json::json!({
        "quality": 1.0, "volume": 1.0, "uniqueness": 1.0,
        "temperature_c": 20.0, "humidity_percent": 60.0, "pressure_hpa": 1014.0,
        "wind_speed_ms": 2.8, "precipitation_mm": 0.0, "dewpoint_c": 12.0
    });
    let nonce = format!("nonce-{tag}");
    let commit_id = Transaction::compute_commit_id(&data, &nonce, station, now, "contribution");
    let mut commit = CommitPayload {
        commit_id: commit_id.clone(),
        station_id: station.to_string(),
        timestamp: now,
        contribution_type: "contribution".to_string(),
        auth_pubkey: None,
        auth_signature: None,
    };
    commit.sign_authorization(owner);
    let reveal = RevealPayload {
        commit_id: commit_id.clone(),
        station_id: station.to_string(),
        timestamp: now,
        contribution_type: "contribution".to_string(),
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
    let sender = address_of(owner);
    Reading {
        commit: with_data(shell(TransactionType::ContributionCommit, sender, now, &format!("commit-{tag}")), &commit, 0),
        // Non-zero fee: a zero-fee reveal needs a compute receipt.
        reveal: with_data(shell(TransactionType::ContributionReveal, sender, now, &format!("reveal-{tag}")), &reveal, 1),
        commit_id,
    }
}

fn vote(commit_id: &str, n: u8, approved: bool, now: i64) -> Transaction {
    let tag = format!("vote-{commit_id}-{n}");
    let mut payload = OracleAttestationPayload {
        commit_id: commit_id.to_string(),
        oracle_id: oracle(n),
        approved,
        signature: None,
        confidence: Some(1.0),
        timestamp: now,
        nonce: Some(format!("att-{tag}")),
    };
    payload.signature = Some(oracle_key(n).sign(payload.signing_message().as_bytes()).to_string());
    with_data(shell(TransactionType::OracleAttestation, own_address(n), now, &tag), &payload, 0)
}

fn station(id: &str, owner: &PrivateKey, provenance_class: ContributionProvenanceClass) -> WeatherStation {
    WeatherStation {
        station_id: id.to_string(),
        owner_address: address_of(owner).to_string(),
        location: StationLocation { latitude: 46.7867, longitude: -92.1005, elevation_meters: Some(215.0) },
        hardware_model: "parity-station".to_string(),
        calibration_date: None,
        is_active: true,
        reputation_score: 50,
        data_quality_tier: DataQualityTier::Gold,
        provenance_class,
        ring_public_key: None,
    }
}

fn balance(state: &State, address: &Address) -> u128 {
    state.get_account(address).map_or(0, |a| a.balance.into_inner())
}

/// Diagnostics for a root mismatch: the snapshot entry keys that differ.
fn differing_entries(a: &State, b: &State) -> Vec<String> {
    use wrfcoin_shared_protocol::blockchain::state_snapshot::export_state_entries;
    let (a, b) = (export_state_entries(a).unwrap(), export_state_entries(b).unwrap());
    let (a, b): (std::collections::BTreeMap<_, _>, std::collections::BTreeMap<_, _>) =
        (a.into_iter().collect(), b.into_iter().collect());
    a.keys()
        .chain(b.keys())
        .filter(|key| a.get(*key) != b.get(*key))
        .map(|key| String::from_utf8_lossy(key).chars().take(90).collect())
        .collect::<std::collections::BTreeSet<String>>()
        .into_iter()
        .collect()
}

fn carries(block: &Block, tx: &Transaction) -> bool {
    block.transactions.iter().any(|t| t.id == tx.id)
}

fn settlements(block: &Block) -> Vec<AttesterSettlementRecord> {
    block
        .metadata
        .get(ATTESTER_SETTLEMENTS_METADATA_KEY)
        .map(|raw| serde_json::from_str(raw).expect("settlement metadata parses"))
        .unwrap_or_default()
}

fn amount(records: &[AttesterSettlementRecord], decision: &str, n: u8) -> Option<u128> {
    records
        .iter()
        .find(|r| r.decision_id == decision)?
        .allocations
        .iter()
        .find(|a| a.attester_id == oracle(n))
        .map(|a| a.amount)
}

#[test]
fn attester_pay_v2_producer_follower_wal_checkpoint() {
    if std::env::var_os(CHILD_ENV).is_none() {
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST_NAME, "--nocapture"])
            .env(CHILD_ENV, "1")
            .env("WRFCOIN_CHAIN_ID", "777")
            .env("WRFCOIN_NETWORK_ID", "testnet")
            .env(GOVERNANCE_ENV, PublicKey::from_private_key(&key(GOVERNANCE_SEED)).to_hex())
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success() && stdout.contains("1 passed"),
            "child failed:\n{stdout}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        return;
    }
    // Before the bond fixture can install its own schedule: the first install wins.
    install_feature_schedule(
        FeatureSchedule::from_entries(vec![
            ("node_share_settlement".into(), fixture::A),
            ("account_authorization_dispatch".into(), fixture::A),
            ("attester_pay_v2".into(), H),
        ])
        .unwrap(),
    );
    assert!(!feature_active(ConsensusFeature::AttesterPayV2, H - 1));
    assert!(feature_active(ConsensusFeature::AttesterPayV2, H));
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
        .block_on(run());
}

async fn run() {
    let owner = key(0x01);
    let oracle_set: FxHashSet<String> = (1..=4).map(oracle).collect();
    let bond_account = fixture::address(10);

    // The seeded pre-state at BASE: one bond, four oracles (quorum 2), a real
    // station and an unclassified one.
    let mut seeded = fixture::state();
    fixture::apply(&mut seeded, fixture::auth(10, 1, 0, NodeBondAction::Deposit, 1), fixture::A).expect("bond deposit");
    seeded
        .apply_transition(StateTransition::SetOracleConfig {
            oracle_set: oracle_set.iter().cloned().collect(),
            oracle_quorum: 2,
        })
        .unwrap();
    seeded.register_weather_station(station(REAL_STATION, &owner, ContributionProvenanceClass::Real)).unwrap();
    seeded.register_weather_station(station(FREE_STATION, &owner, ContributionProvenanceClass::Unclassified)).unwrap();
    fixture::advance(&mut seeded, BASE);
    seeded.recalculate_state_root();
    assert!(seeded.attester_payout_bonds.is_empty());

    let mut follower = seeded.clone();
    let chain = Arc::new(RwLock::new(Blockchain::default()));
    {
        let mut c = chain.write().await;
        c.height = BASE;
        c.state = seeded.clone();
    }
    let db = std::env::temp_dir().join(format!("attester-pay-v2-parity-{}.redb", std::process::id()));
    let _ = std::fs::remove_file(&db);
    // No checkpoint inside the run, so the WAL keeps every block for the replay.
    let mut storage = Storage::open(&db, 1_000_000).unwrap();
    storage.force_checkpoint(&seeded, BASE).await.unwrap();

    let clock = Arc::new(ManualClock(AtomicI64::new(START)));
    let mut producer_pop = ProofOfPublishing::new(1, 60, 0, 32);
    producer_pop.declare_no_genesis_oracles();
    producer_pop.set_clock(clock.clone());
    let mut verifier_pop = ProofOfPublishing::new(1, 60, 0, 32);
    verifier_pop.declare_no_genesis_oracles();
    verifier_pop.set_clock(clock.clone());
    let mut validator_pop = ProofOfPublishing::new(1, 60, 0, 32);
    validator_pop.declare_no_genesis_oracles();
    validator_pop.set_clock(clock.clone());
    let validator = PopConsensusAdapter::new(validator_pop);
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let miner = Address([0x41; 20]).to_string();

    let mut height = BASE;
    let mut blocks: Vec<(State, Block)> = Vec::new();
    // One block: admit `txs`, produce, verify and validate as a follower,
    // apply as a follower, persist, compare roots.
    macro_rules! block {
        ($txs:expr) => {{
            height += 1;
            let now = START + STEP * (height - BASE) as i64;
            clock.0.store(now, Ordering::SeqCst);
            let txs: Vec<(&str, Transaction)> = $txs;
            for (tag, tx) in txs {
                let signing_keys = chain.read().await.state.signing_keys.clone();
                mempool
                    .write()
                    .await
                    .add_transaction_with_signing_keys_at_height(tx, Some(&signing_keys), Some(height - 1))
                    .unwrap_or_else(|e| panic!("{tag} refused at admission for height {height}: {e}"));
            }
            let before = follower.clone();
            let block = producer_pop
                .create_next_block(chain.clone(), mempool.clone(), &miner)
                .await
                .unwrap_or_else(|e| panic!("producer failed at {height}: {e}"));
            assert_eq!(block.height, height);
            assert!(
                verifier_pop.verify_block(&block, &oracle_set, 2, &[]).await.expect("verdict"),
                "a follower rejects the producer's block at {height}"
            );
            {
                let mut pre_block = Blockchain::default();
                pre_block.height = height - 1;
                pre_block.tip = block.prev_block_hash;
                pre_block.state = before.clone();
                assert!(
                    validator
                        .validate_block(&block, &pre_block)
                        .await
                        .unwrap_or_else(|e| panic!("validate_block failed at {height}: {e}")),
                    "validate_block refuses the producer's block at {height}"
                );
            }
            let producer = chain.read().await.state.clone();
            let transitions = extract_transitions(&before, &block, block.height, block.timestamp.timestamp());
            follower
                .apply_transitions_batch_at_height(block.height, transitions.clone())
                .unwrap_or_else(|e| panic!("follower apply failed at {height}: {e}"));
            // The node finalizes the block after its transitions, as the producer did.
            follower.apply_block_finalization(block.height);
            storage.persist_block(block.clone(), transitions, None).await.unwrap();
            assert_eq!(
                follower.state_root,
                producer.state_root,
                "follower root at {height}; differing state entries: {:?}",
                differing_entries(&follower, &producer)
            );
            assert_eq!(follower.attester_payout_bonds, producer.attester_payout_bonds, "bindings at {height}");
            assert_eq!(follower.attester_settlement_due, producer.attester_settlement_due, "debts at {height}");
            assert_eq!(follower.attester_settlements, producer.attester_settlements, "settlements at {height}");
            for n in 1..=4u8 {
                assert_eq!(balance(&follower, &own_address(n)), balance(&producer, &own_address(n)), "oracle {n} at {height}");
            }
            assert_eq!(balance(&follower, &bond_account), balance(&producer, &bond_account), "bond payout at {height}");
            blocks.push((before, block.clone()));
            (block, producer)
        }};
    }

    // ---- H - 2: inactive -------------------------------------------------
    let now = START + STEP;
    let rejected = reading(&owner, REAL_STATION, now, "rejected");
    let approved = reading(&owner, FREE_STATION, now, "approved-below");
    let reject_votes: Vec<Transaction> = (1..=3).map(|n| vote(&rejected.commit_id, n, false, now)).collect();
    let approve_votes = [vote(&approved.commit_id, 1, true, now), vote(&approved.commit_id, 2, true, now)];
    let (block, producer) = block!(vec![
        ("rejected commit", rejected.commit.clone()),
        ("rejected reveal", rejected.reveal.clone()),
        ("reject vote 1", reject_votes[0].clone()),
        ("reject vote 2", reject_votes[1].clone()),
        ("reject vote 3", reject_votes[2].clone()),
        ("approved commit", approved.commit.clone()),
        ("approved reveal", approved.reveal.clone()),
        ("approve vote 1", approve_votes[0].clone()),
        ("approve vote 2", approve_votes[1].clone()),
    ]);
    assert_eq!(height, H - 2);
    assert!(carries(&block, &rejected.commit) && carries(&block, &approved.commit));
    assert!(carries(&block, &approved.reveal) && approve_votes.iter().all(|v| carries(&block, v)));
    assert!(!carries(&block, &rejected.reveal), "the real reveal without approve quorum is held back");
    assert!(reject_votes.iter().all(|v| !carries(&block, v)), "below the activation its votes are held back with it");
    let below = settlements(&block);
    assert_eq!(below.len(), 1, "only the approved decision settles");
    assert_eq!(below[0].decision_id, approved.commit_id);
    for n in [1u8, 2] {
        assert_eq!(balance(&producer, &own_address(n)), amount(&below, &approved.commit_id, n).unwrap(), "own address below the activation");
    }
    assert!(producer.attester_payout_bonds.is_empty() && producer.attester_settlement_due.is_empty());
    assert!(!producer.attestations.contains_key(&rejected.commit_id));

    // ---- H - 1: inactive, the held group stays held ----------------------
    let (block, producer) = block!(vec![]);
    assert_eq!(height, H - 1);
    assert!(reject_votes.iter().all(|v| !carries(&block, v)) && !carries(&block, &rejected.reveal));
    assert!(settlements(&block).is_empty());
    assert!(!producer.attestations.contains_key(&rejected.commit_id));
    let own_before: Vec<u128> = (1..=4).map(|n| balance(&producer, &own_address(n))).collect();
    let bond_before = balance(&producer, &bond_account);

    // ---- H: active -------------------------------------------------------
    let now = START + STEP * 3;
    let identity = ChainIdentity::active();
    let registration = Transaction::new_oracle_registration_with_payout_bond_and_domain(
        &identity.chain_id,
        &identity.network_id,
        (1..=4).map(oracle).collect(),
        2,
        &oracle(1),
        &PublicKey::from_private_key(&key(0x91)).to_hex(),
        1,
        // Later than every vote here, so oracle 1's votes stay verifiable.
        START + 1_000_000,
        Some(&hex::encode([1u8; 32])),
        &key(GOVERNANCE_SEED),
    );
    let mut registration = registration;
    registration.timestamp = chrono::DateTime::from_timestamp(now, 0).unwrap();
    registration.set_id();
    assert!(registration.verify_oracle_registration_authorization_at(H));
    let second = reading(&owner, FREE_STATION, now, "approved-at");
    let second_votes = [vote(&second.commit_id, 2, true, now), vote(&second.commit_id, 4, true, now)];
    let (block, producer) = block!(vec![
        ("registration", registration.clone()),
        ("second commit", second.commit.clone()),
        ("second reveal", second.reveal.clone()),
        ("second vote 2", second_votes[0].clone()),
        ("second vote 4", second_votes[1].clone()),
    ]);
    assert_eq!(height, H);
    assert!(carries(&block, &registration), "the registration naming the bond is mined");
    assert!(reject_votes.iter().all(|v| carries(&block, v)), "from the activation the decided reject votes are mined");
    assert!(!carries(&block, &rejected.reveal), "the rejected reveal is still held back");
    assert_eq!(producer.attester_payout_bonds.get(&oracle(1)), Some(&[1u8; 32]));
    let at = settlements(&block);
    let mut decisions: Vec<&str> = at.iter().map(|r| r.decision_id.as_str()).collect();
    decisions.sort();
    let mut expected = vec![rejected.commit_id.as_str(), second.commit_id.as_str()];
    expected.sort();
    assert_eq!(decisions, expected, "the rejected and the approved decision settle together");
    let slice: u128 = at.iter().map(AttesterSettlementRecord::total_amount).sum();
    {
        let mut opened = blocks.last().unwrap().0.clone();
        opened.open_emission_block(H);
        assert_eq!(slice, opened.emission_ledger.remaining(EmissionRole::Attester), "exactly the attester slice");
    }
    // The rejected decision pays its three reject voters; oracle 1 is bound.
    let to_one = amount(&at, &rejected.commit_id, 1).expect("oracle 1 is paid for its reject vote");
    assert!(to_one > 0);
    assert_eq!(balance(&producer, &bond_account) - bond_before, to_one, "the bound attester is paid to the bond payout account");
    assert_eq!(balance(&producer, &own_address(1)), own_before[0], "and not to its own address");
    let to_two = amount(&at, &rejected.commit_id, 2).unwrap() + amount(&at, &second.commit_id, 2).unwrap();
    assert_eq!(balance(&producer, &own_address(2)) - own_before[1], to_two);
    assert_eq!(balance(&producer, &own_address(3)) - own_before[2], amount(&at, &rejected.commit_id, 3).unwrap());
    assert_eq!(balance(&producer, &own_address(4)) - own_before[3], amount(&at, &second.commit_id, 4).unwrap());
    assert!(producer.attester_settlement_due.is_empty());
    // A rejected decision pays the contributor nothing.
    assert!(!producer.contribution_reward_records.contains_key(&rejected.commit_id));

    // A tampered settlement record is refused by the follower's apply.
    {
        let (before, honest) = blocks.last().unwrap().clone();
        for edit in ["payee", "amount"] {
            let mut records = settlements(&honest);
            let record = records.iter_mut().find(|r| r.decision_id == rejected.commit_id).unwrap();
            if edit == "payee" {
                let allocations = record
                    .allocations
                    .iter()
                    .map(|a| {
                        let mut a = a.clone();
                        if a.attester_id == oracle(3) {
                            a.attester_id = oracle(4);
                        }
                        a
                    })
                    .collect();
                *record = AttesterSettlementRecord::new(record.decision_id.clone(), H, allocations);
            } else {
                record.allocations[0].amount += 1;
            }
            let mut tampered = honest.clone();
            tampered.metadata.insert(ATTESTER_SETTLEMENTS_METADATA_KEY.to_string(), serde_json::to_string(&records).unwrap());
            let transitions = extract_transitions(&before, &tampered, H, tampered.timestamp.timestamp());
            let mut peer = before.clone();
            let refused = peer.apply_transitions_batch_at_height(H, transitions);
            assert!(refused.is_err(), "a tampered {edit} must fail the follower's apply");
        }
    }

    // ---- H + 1: a straggler ---------------------------------------------
    let now = START + STEP * 4;
    let straggler = vote(&rejected.commit_id, 4, false, now);
    let four_before = balance(&producer, &own_address(4));
    let (block, producer) = block!(vec![("straggler", straggler.clone())]);
    assert!(carries(&block, &straggler));
    assert!(settlements(&block).is_empty(), "settled is final");
    assert_eq!(balance(&producer, &own_address(4)), four_before, "the straggler is unpaid");
    assert!(producer.attester_settlement_due.is_empty());

    // ---- WAL replay -------------------------------------------------------
    let rows = storage.load_wal_transitions_range(BASE + 1, height).unwrap();
    let mut wal = seeded.clone();
    for h in BASE + 1..=height {
        wal.apply_transitions_batch_at_height(h, rows.iter().filter(|(row, _)| *row == h).map(|(_, t)| t.clone()))
            .unwrap_or_else(|e| panic!("WAL replay failed at {h}: {e}"));
        wal.apply_block_finalization(h);
    }
    assert_eq!(wal.state_root, producer.state_root, "WAL replay root");
    assert_eq!(wal.attester_payout_bonds, producer.attester_payout_bonds);

    // ---- Checkpoint restart with a binding and an UNPAID reject debt ------
    let mut live = producer.clone();
    let next = height + 1;
    for n in 1..=3u8 {
        let payload: OracleAttestationPayload =
            serde_json::from_str(vote("unpaid-decision", n, false, now).data.as_deref().unwrap()).unwrap();
        live.apply_transition(StateTransition::RecordOracleAttestation { payload, current_block: next }).unwrap();
    }
    assert_eq!(live.attester_settlement_due["unpaid-decision"].eligible.len(), 3);
    assert!(!live.attester_payout_bonds.is_empty());
    // Stored under the tip height: the node registry in the state is at it.
    storage.force_checkpoint(&live, height).await.unwrap();
    drop(storage);
    let reopened = Storage::open(&db, 1_000_000).unwrap();
    let (at_height, mut restored) = reopened.load_checkpoint_at_or_before(height).unwrap().expect("checkpoint");
    assert_eq!(at_height, height);
    assert_eq!(restored.state_root, live.state_root, "restored root");
    assert_eq!(restored.attester_payout_bonds, live.attester_payout_bonds);
    assert_eq!(restored.attester_settlement_due, live.attester_settlement_due);
    restored.recalculate_state_root();
    assert_eq!(restored.state_root, live.state_root, "recomputed root after restart");
    // Both pay the same settlement and reach the same root.
    let mut roots = Vec::new();
    for state in [&mut live, &mut restored] {
        state.open_emission_block(next + 1);
        let due: Vec<String> = state.attester_settlement_due.keys().cloned().collect();
        let slice = state.emission_ledger.remaining(EmissionRole::Attester);
        let records = state.derive_attester_settlements(&due, next + 1, slice).unwrap();
        assert_eq!(records[0].allocations.len(), 3);
        state.apply_transitions_batch(vec![StateTransition::ApplyAttesterSettlements { records }]).unwrap();
        roots.push((state.state_root, balance(state, &bond_account)));
    }
    assert_eq!(roots[0], roots[1], "the restored node settles identically");

    drop(reopened);
    std::fs::remove_file(db).unwrap();
}
