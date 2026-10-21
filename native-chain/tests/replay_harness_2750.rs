//! #2750 — the historical-block replay harness, proven in both directions.
//!
//! Builds a fixture chain through the REAL stateful producer
//! (`ProofOfPublishing::create_next_block`, the same function the mining
//! route drives), carrying the shapes that matter to the live chain: a
//! governance-signed `Real` station registration (block 674's shape), a
//! commit, a reveal + attestation-quorum block with paid reward records
//! (716/851's shape), and filler blocks crossing checkpoint boundaries. Each
//! block is sealed with its state-root commitment exactly the way
//! `p2p_events`' committed fixtures seal theirs, then persisted.
//!
//! The replay half then re-executes those blocks through the follower path
//! (`replay_core`, mounted below — the same file the `replay_verify` binary
//! compiles) and asserts every committed root. The RED control perturbs the
//! replaying state at a chosen height and demands the harness FAIL naming
//! exactly that height — a harness never shown to fail is not evidence.

// The extractor's own #[cfg(test)] module references `crate::helpers::test_env`
// (the launch_api process-env lock). Mounting the real helpers.rs would drag
// in the whole AppState web, so this shim provides the one item that module
// needs, with the same poisoning-recovery semantics.
mod helpers {
    pub(crate) mod test_env {
        // Mount the canonical test-only lock and fixture pins (#3521).
        include!("../src/bin/launch_api/test_env.rs");
    }
}

#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/transition_extractor.rs"]
mod transition_extractor;

#[allow(dead_code, unused_imports)]
#[path = "../src/bin/replay_verify/replay_core.rs"]
mod replay_core;

use std::sync::Arc;

use rand::{SeedableRng, rngs::StdRng};
use tokio::sync::RwLock;

use replay_core::{ReplayVerdict, replay_persisted_chain, replay_persisted_chain_observed};
use transition_extractor::extract_transitions;
use wrfcoin_native_chain::storage::Storage;
use wrfcoin_shared_protocol::blockchain::block::STATE_ROOT_METADATA_KEY;
use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::state::StateTransition;
use wrfcoin_shared_protocol::blockchain::transaction::{
    CommitPayload, OracleAttestationPayload, RevealPayload, StationRegistrationPayload,
    Transaction, TransactionInput, TransactionOutput, TransactionType,
};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::ProofOfPublishing;
use wrfcoin_shared_protocol::crypto::hash::hash_string;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::mempool::pool::Mempool;
use wrfcoin_shared_protocol::station_registry::{ContributionProvenanceClass, DataQualityTier};

const GOVERNANCE_ENV: &str = "WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS";
/// Small on purpose: the fixture's 8 blocks cross two checkpoint boundaries
/// (heights 4 and 8), exercising the checkpoint write+prune path, and the
/// final block lands ON a boundary so the retained latest checkpoint is an
/// assertable anchor (storage keeps only the newest non-genesis checkpoint).
const CHECKPOINT_INTERVAL: u64 = 4;
const FIXTURE_TS: i64 = 1_700_800_000;

fn tmp_db(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("wrfcoin-2750-{tag}-{}.redb", fastrand::u64(..)))
}

// --- transaction builders, copied from the 2537 production-path suite -------

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
    tx.set_id();
    tx
}

fn oracle_identity(seed: u64) -> (PrivateKey, String) {
    let mut rng = StdRng::seed_from_u64(seed);
    let private_key = PrivateKey::generate(&mut rng);
    let oracle_id = PublicKey::from_private_key(&private_key).to_string();
    (private_key, oracle_id)
}

fn registration_tx(
    station_id: &str,
    owner_address: &str,
    owner_key: &PrivateKey,
    governance_key: &PrivateKey,
) -> Transaction {
    let mut payload = StationRegistrationPayload::new(
        station_id,
        owner_address,
        46.7867,
        -92.1005,
        "replay-fixture-station",
        DataQualityTier::Gold,
        ContributionProvenanceClass::Real,
    );
    payload.sign_ownership(owner_key);
    payload.sign(governance_key);
    let mut tx = build_tx(TransactionType::StationRegistration, "station-reg-2750");
    tx.data = Some(serde_json::to_string(&payload).expect("serialize registration"));
    tx.set_id();
    tx
}

fn weather_commit_and_reveal(station_id: &str) -> (CommitPayload, RevealPayload, String) {
    let payload = serde_json::json!({
        "quality": 1.0,
        "volume": 1.0,
        "uniqueness": 1.0,
        "temperature_c": 21.5,
        "humidity_percent": 60.0,
        "pressure_hpa": 1014.0,
        "wind_speed_ms": 2.8,
        "precipitation_mm": 0.0,
        "dewpoint_c": 13.5
    });
    let nonce = "nonce-2750".to_string();
    let commit_id = Transaction::compute_commit_id(
        &payload,
        &nonce,
        station_id,
        FIXTURE_TS,
        "contribution",
    );
    let mut rng = StdRng::seed_from_u64(27_500);
    let auth_key = PrivateKey::generate(&mut rng);
    let mut commit_payload = CommitPayload {
        commit_id: commit_id.clone(),
        station_id: station_id.to_string(),
        timestamp: FIXTURE_TS,
        contribution_type: "contribution".to_string(),
        auth_pubkey: None,
        auth_signature: None,
    };
    commit_payload.sign_authorization(&auth_key);

    let reveal_payload = RevealPayload {
        commit_id: commit_id.clone(),
        station_id: station_id.to_string(),
        timestamp: FIXTURE_TS,
        contribution_type: "contribution".to_string(),
        data: payload,
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
    (commit_payload, reveal_payload, commit_id)
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
    payload.signature =
        Some(private_key.sign(payload.signing_message().as_bytes()).to_string());
    let mut tx = build_tx(TransactionType::OracleAttestation, &format!("att-{tag}"));
    tx.data = Some(serde_json::to_string(&payload).expect("serialize attestation"));
    tx.set_id();
    tx
}

// --- the fixture producer ---------------------------------------------------

/// What the fixture builder reports about the chain it built, so tests can
/// assert against real numbers instead of magic constants.
struct FixtureSummary {
    db_path: std::path::PathBuf,
    blocks: u64,
    registration_height: u64,
    reward_height: u64,
}

/// Build a fixture chain through the real stateful producer and persist it —
/// genesis state, then one block each for registration / commit /
/// reveal+quorum, then filler blocks to cross two checkpoint boundaries.
async fn build_fixture(tag: &str) -> FixtureSummary {
    // Env is process-global: hold the lock for the whole build (signing AND
    // block application read the governance allowlist).
    let _env = helpers::test_env::lock();
    let mut rng = StdRng::seed_from_u64(2_055_215);
    let governance_key = PrivateKey::generate(&mut rng);
    std::env::set_var(
        GOVERNANCE_ENV,
        PublicKey::from_private_key(&governance_key).to_hex(),
    );
    let owner_key = PrivateKey::generate(&mut rng);
    let contributor = Address::from_public_key(&PublicKey::from_private_key(&owner_key));
    let miner_hex = format!("0x{}", hex::encode([0x4d_u8; 20]));

    let (oracle_a_key, oracle_a) = oracle_identity(27_510);
    let (oracle_b_key, oracle_b) = oracle_identity(27_520);

    // Genesis content: oracle config + contributor account, exactly the class
    // of state the live chain carries at its cut.
    let mut chain = Blockchain::default();
    chain.state.create_account(contributor, Balance::new(0));
    chain
        .apply_state_transition(StateTransition::SetOracleConfig {
            oracle_set: vec![oracle_a.clone(), oracle_b.clone()],
            oracle_quorum: 2,
        })
        .expect("set oracle config");

    let db_path = tmp_db(tag);
    let mut storage = Storage::open(&db_path, CHECKPOINT_INTERVAL).expect("open fixture db");
    storage
        .persist_genesis_state(&chain.state)
        .await
        .expect("persist genesis state");

    let station_id = "PWS-REPLAY-FIXTURE-674";
    let (commit_payload, reveal_payload, commit_id) = weather_commit_and_reveal(station_id);

    let blockchain = Arc::new(RwLock::new(chain));
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let mut pop = ProofOfPublishing::new(1, 60, 0, 16);
    pop.declare_no_genesis_oracles();

    let mut blocks = 0u64;
    let mut registration_height = 0u64;
    let mut reward_height = 0u64;

    for plan in 1..=8u64 {
        // Enqueue this block's transactions.
        {
            let mut mp = mempool.write().await;
            match plan {
                1 => {
                    mp.add_transaction(registration_tx(
                        station_id,
                        &contributor.to_string(),
                        &owner_key,
                        &governance_key,
                    ))
                    .expect("enqueue registration");
                },
                2 => {
                    let mut tx = build_tx(TransactionType::ContributionCommit, "commit-2750");
                    tx.data = Some(
                        serde_json::to_string(&commit_payload).expect("serialize commit"),
                    );
                    tx.set_id();
                    mp.add_transaction(tx).expect("enqueue commit");
                },
                3 => {
                    let signing_keys =
                        { blockchain.read().await.state.signing_keys.clone() };
                    let mut reveal_tx =
                        build_tx(TransactionType::ContributionReveal, "reveal-2750");
                    reveal_tx.data = Some(
                        serde_json::to_string(&reveal_payload).expect("serialize reveal"),
                    );
                    reveal_tx.sender_address = contributor;
                    reveal_tx.fee = 1;
                    reveal_tx.set_id();
                    mp.add_transaction(reveal_tx).expect("enqueue reveal");
                    mp.add_transaction_with_signing_keys(
                        signed_attestation_tx(
                            &commit_id,
                            &oracle_a_key,
                            &oracle_a,
                            FIXTURE_TS + 1,
                            "a",
                        ),
                        Some(&signing_keys),
                    )
                    .expect("enqueue attestation a");
                    mp.add_transaction_with_signing_keys(
                        signed_attestation_tx(
                            &commit_id,
                            &oracle_b_key,
                            &oracle_b,
                            FIXTURE_TS + 2,
                            "b",
                        ),
                        Some(&signing_keys),
                    )
                    .expect("enqueue attestation b");
                },
                _ => {}, // filler blocks: coinbase only
            }
        }

        // Producer state BEFORE this block — the extractor's contract.
        let pre_state = { blockchain.read().await.state.clone() };

        let mut block = pop
            .create_next_block(blockchain.clone(), mempool.clone(), &miner_hex)
            .await
            .expect("fixture block should be produced by the real stateful producer");

        // Seal the state-root commitment exactly the way the p2p committed
        // fixtures do: root into metadata, re-hash, rebuild the chain so its
        // tip is the sealed hash.
        {
            let produced = blockchain.read().await;
            block.metadata.insert(
                STATE_ROOT_METADATA_KEY.to_string(),
                produced.state.state_root.to_string(),
            );
            block.set_hash(block.calculate_hash());
            let mut window = produced.blocks().clone();
            window.insert(block.height, block.clone());
            let rebuilt = Blockchain::from_persisted(
                produced.height,
                block.hash.expect("sealed block has a hash"),
                produced.state.clone(),
                window,
                produced.total_block_count(),
                produced.finalized_height(),
            );
            drop(produced);
            *blockchain.write().await = rebuilt;
        }

        let transitions = extract_transitions(
            &pre_state,
            &block,
            block.height,
            block.timestamp.timestamp(),
        );
        let post_state = { blockchain.read().await.state.clone() };
        let checkpoint = storage
            .checkpoint_state_bytes(block.height, &post_state)
            .expect("checkpoint bytes");
        let height = block.height;
        storage
            .persist_block(block, transitions, checkpoint)
            .await
            .expect("persist fixture block");

        blocks += 1;
        match plan {
            1 => registration_height = height,
            3 => reward_height = height,
            _ => {},
        }
    }

    // The fixture must genuinely carry the live-chain shapes, or the harness
    // green run proves less than it claims.
    {
        let chain = blockchain.read().await;
        let station = chain
            .state
            .station_registry
            .get(station_id)
            .expect("fixture chain must carry a registered station (the 674 shape)");
        assert_eq!(station.provenance_class, ContributionProvenanceClass::Real);
        let paid = chain.state.contribution_reward_aggregate(&contributor).real_paid.0;
        assert!(paid > 0, "fixture chain must carry a PAID reward record (the 716 shape)");
    }

    FixtureSummary {
        db_path,
        blocks,
        registration_height,
        reward_height,
    }
}

// --- the harness, proven in both directions ---------------------------------

/// GREEN: the candidate binary replays the fixture chain and reproduces every
/// committed state root, including across two checkpoint boundaries.
#[tokio::test]
async fn a_committed_fixture_chain_replays_to_verified() {
    let fixture = build_fixture("green").await;
    // Signing verification during replay reads the same env the build used.
    let _env = helpers::test_env::lock();
    let mut rng = StdRng::seed_from_u64(2_055_215);
    let governance_key = PrivateKey::generate(&mut rng);
    std::env::set_var(
        GOVERNANCE_ENV,
        PublicKey::from_private_key(&governance_key).to_hex(),
    );

    let storage = Storage::open(&fixture.db_path, CHECKPOINT_INTERVAL).expect("reopen fixture");
    let verdict = replay_persisted_chain(&storage, None);

    match verdict {
        ReplayVerdict::Verified {
            blocks_replayed,
            roots_asserted,
            checkpoints_asserted,
            first_height,
            last_height,
        } => {
            assert_eq!(blocks_replayed, fixture.blocks);
            assert_eq!(
                roots_asserted, fixture.blocks,
                "every fixture block carries a commitment; every one must be asserted"
            );
            // The storage retains only the LATEST non-genesis checkpoint
            // (persist_block_sync prunes older ones, keeping the height-0
            // anchor), so exactly one checkpoint anchor is assertable — and
            // the fixture's final block sits on a boundary precisely so that
            // anchor exists and is compared.
            assert_eq!(
                checkpoints_asserted, 1,
                "the latest checkpoint anchor must actually be compared"
            );
            assert_eq!(first_height, 1);
            assert_eq!(last_height, fixture.blocks);
        },
        other => panic!("expected Verified, got {other:?}"),
    }
    let _ = std::fs::remove_file(&fixture.db_path);
}

/// RED control: perturb the replaying state at a chosen height and the
/// harness must FAIL and name exactly that height. This is the harness's
/// permanently-wired proof that it can detect divergence at all — the height
/// chosen is a filler block AFTER the reward block, so the divergence is
/// introduced mid-chain, not at the end.
#[tokio::test]
async fn red_control_a_perturbed_replay_diverges_at_the_named_height() {
    let fixture = build_fixture("red").await;
    let _env = helpers::test_env::lock();
    let mut rng = StdRng::seed_from_u64(2_055_215);
    let governance_key = PrivateKey::generate(&mut rng);
    std::env::set_var(
        GOVERNANCE_ENV,
        PublicKey::from_private_key(&governance_key).to_hex(),
    );

    let perturb_at = fixture.reward_height + 2;
    assert!(perturb_at < fixture.blocks, "perturbation must land mid-chain");
    let storage = Storage::open(&fixture.db_path, CHECKPOINT_INTERVAL).expect("reopen fixture");
    let verdict = replay_persisted_chain_observed(&storage, None, |height, state| {
        if height == perturb_at {
            // The smallest consensus-visible perturbation: one balance unit.
            let victim = Address([0x77; 20]);
            state.create_account(victim, Balance::new(1));
        }
    });

    match verdict {
        ReplayVerdict::Diverged {
            height,
            committed,
            applied,
            blocks_replayed,
        } => {
            assert_eq!(
                height, perturb_at,
                "the harness must name the FIRST height where divergence began"
            );
            assert_eq!(
                blocks_replayed,
                perturb_at - 1,
                "every block before the perturbation replayed clean"
            );
            assert_ne!(committed, applied, "the two roots must actually differ");
        },
        other => panic!("a perturbed replay must diverge, got {other:?}"),
    }
    let _ = std::fs::remove_file(&fixture.db_path);
}

/// A missing fixture asserts nothing and must say so — never pass.
#[tokio::test]
async fn an_empty_database_is_unverified_not_a_pass() {
    let db_path = tmp_db("empty");
    let storage = Storage::open(&db_path, CHECKPOINT_INTERVAL).expect("open empty db");
    match replay_persisted_chain(&storage, None) {
        ReplayVerdict::Unverified { reason } => {
            assert!(
                reason.contains("genesis"),
                "the reason must name what was missing, got: {reason}"
            );
        },
        other => panic!("an empty database must be UNVERIFIED, got {other:?}"),
    }
    let _ = std::fs::remove_file(&db_path);
}

/// Genesis state but zero blocks: still UNVERIFIED — a replay of nothing is
/// not evidence of anything.
#[tokio::test]
async fn genesis_without_blocks_is_unverified_not_a_pass() {
    let db_path = tmp_db("noblocks");
    let storage = Storage::open(&db_path, CHECKPOINT_INTERVAL).expect("open db");
    let chain = Blockchain::default();
    storage
        .persist_genesis_state(&chain.state)
        .await
        .expect("persist genesis");
    match replay_persisted_chain(&storage, None) {
        ReplayVerdict::Unverified { reason } => {
            assert!(
                reason.contains("zero blocks"),
                "the reason must name the absence, got: {reason}"
            );
        },
        other => panic!("zero persisted blocks must be UNVERIFIED, got {other:?}"),
    }
    let _ = std::fs::remove_file(&db_path);
}

/// Blocks that carry NO state-root commitments replay without asserting
/// anything — and a zero-assertion run must be UNVERIFIED, never Verified.
/// This is the vacuity guard: a version-skipped or commitment-stripped chain
/// cannot silently read as proof.
#[tokio::test]
async fn a_chain_with_no_commitments_is_unverified_not_a_pass() {
    let db_path = tmp_db("nocommit");
    let mut storage = Storage::open(&db_path, CHECKPOINT_INTERVAL).expect("open db");
    let mut chain = Blockchain::default();
    storage
        .persist_genesis_state(&chain.state)
        .await
        .expect("persist genesis");

    // One empty block, sealed WITHOUT a state-root commitment.
    let mut block =
        wrfcoin_shared_protocol::blockchain::block::Block::new(1, chain.tip, vec![], 1, 1);
    block.set_hash(block.calculate_hash());
    chain.state.apply_block_finalization(1);
    chain.add_block(block.clone()).expect("add block");
    let checkpoint = storage
        .checkpoint_state_bytes(1, &chain.state)
        .expect("checkpoint bytes");
    storage
        .persist_block(block, vec![], checkpoint)
        .await
        .expect("persist block");

    match replay_persisted_chain(&storage, None) {
        ReplayVerdict::Unverified { reason } => {
            assert!(
                reason.contains("NONE carried a state-root commitment"),
                "the reason must name the vacuity, got: {reason}"
            );
        },
        other => panic!("a zero-assertion replay must be UNVERIFIED, got {other:?}"),
    }
    let _ = std::fs::remove_file(&db_path);
}

// #3317: execute the shipped replay CLI against genuine persisted v1 blocks.
async fn checkpoint_only_cli_fixture(tampered: bool, anchored: bool) -> std::path::PathBuf {
    checkpoint_cli_fixture_with_schedule(tampered, anchored, None, false).await
}

async fn checkpoint_cli_fixture_with_schedule(
    tampered: bool,
    anchored: bool,
    signing_schedule: Option<&str>,
    first_root: bool,
) -> std::path::PathBuf {
    let db_path = tmp_db("checkpoint-cli-3317");
    let mut storage = Storage::open(&db_path, if anchored { 2 } else { 4 }).expect("open");
    let mut chain = Blockchain::default();
    storage
        .persist_genesis_state(&chain.state)
        .await
        .expect("genesis");
    for height in 1..=2 {
        let mut block =
            wrfcoin_shared_protocol::blockchain::block::Block::new(1, chain.tip, vec![], 1, height);
        block.timestamp = chrono::DateTime::from_timestamp(FIXTURE_TS + height as i64, 0).unwrap();
        if height == 1 {
            if let Some(schedule) = signing_schedule {
                block.metadata.insert(
                    wrfcoin_shared_protocol::crypto::chain_binding::GENESIS_ACTIVATION_SCHEDULE_KEY
                        .into(),
                    schedule.into(),
                );
                if schedule.starts_with("0:v2") {
                    block.metadata.insert(
                        wrfcoin_shared_protocol::crypto::chain_binding::GENESIS_PROTOCOL_VERSION_KEY.into(),
                        "v2".into(),
                    );
                }
            }
        }
        chain.state.apply_block_finalization(height);
        if first_root && height == 1 {
            block.metadata.insert(
                STATE_ROOT_METADATA_KEY.into(),
                chain.state.state_root.to_string(),
            );
        } else {
            assert!(!block.metadata.contains_key(STATE_ROOT_METADATA_KEY));
        }
        let signing_version = if signing_schedule
            .is_some_and(|raw| raw.starts_with("0:v2") || (raw.contains("2:v2") && height >= 2))
        {
            wrfcoin_shared_protocol::crypto::chain_binding::SigningProtocolVersion::V2
        } else {
            wrfcoin_shared_protocol::crypto::chain_binding::SigningProtocolVersion::V1
        };
        // Header version stays1: signing semantics come from the durable schedule.
        block.set_hash(block.calculate_hash_for(signing_version, None));
        chain.add_block(block.clone()).expect("add fixture block");
        let mut checkpoint_state = chain.state.clone();
        if tampered && height == 2 {
            checkpoint_state.create_account(Address([0x91; 20]), Balance::new(1));
            checkpoint_state.recalculate_state_root();
        }
        let checkpoint = storage
            .checkpoint_state_bytes(height, &checkpoint_state)
            .expect("checkpoint");
        storage
            .persist_block(block, vec![], checkpoint)
            .await
            .expect("persist");
    }
    db_path
}

fn replay_cli_3317(db_path: &std::path::Path) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_replay_verify"))
        .arg(db_path)
        .env_remove("WRFCOIN_PROTOCOL_FEATURE_SCHEDULE_JSON")
        .output()
        .expect("execute replay_verify")
}

#[tokio::test]
async fn v1_checkpoint_cli_reports_distinct_evidence_3317() {
    let db_path = checkpoint_only_cli_fixture(false, true).await;
    let output = replay_cli_3317(&db_path);
    let stdout = String::from_utf8(output.stdout).expect("stdout UTF8");
    let stderr = String::from_utf8(output.stderr).expect("stderr UTF8");
    assert_eq!(output.status.code(), Some(3), "{stdout}\n{stderr}");
    assert!(stdout.contains("VERIFIED_BY_CHECKPOINT:"), "{stdout}");
    assert!(stdout.contains("checkpoint_height=2"), "{stdout}");
    assert!(stdout.contains("checkpoint_root="), "{stdout}");
    assert!(stdout.contains("replayed_root="), "{stdout}");
    assert!(stdout.contains(" MATCH"), "{stdout}");
    assert!(stdout.contains("checkpoints_asserted=1"), "{stdout}");
    assert!(stdout.contains("final_replayed_root="), "{stdout}");
    assert!(stdout.contains("0 per-block"), "{stdout}");
    let storage = Storage::open(&db_path, 2).expect("read persisted checkpoint");
    let (height, checkpoint) = storage.load_checkpoint_at_or_before(2).unwrap().unwrap();
    assert_eq!(height, 2);
    let expected_root = checkpoint.state_root.to_string();
    assert!(
        stdout.contains(&format!("checkpoint_root={expected_root}")),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!("replayed_root={expected_root}")),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!("final_replayed_root={expected_root}")),
        "{stdout}"
    );
    drop(storage);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn v1_tampered_checkpoint_cli_names_mismatch_3317() {
    let db_path = checkpoint_only_cli_fixture(true, true).await;
    let output = replay_cli_3317(&db_path);
    let stdout = String::from_utf8(output.stdout).expect("stdout UTF8");
    let stderr = String::from_utf8(output.stderr).expect("stderr UTF8");
    assert_eq!(output.status.code(), Some(1), "{stdout}\n{stderr}");
    assert!(stdout.contains("checkpoint_height=2"), "{stdout}");
    assert!(stdout.contains(" MISMATCH"), "{stdout}");
    assert!(stderr.contains("DIVERGED at height 2"), "{stderr}");
    assert!(stdout.contains("checkpoints_asserted=1"), "{stdout}");
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn v1_no_anchor_cli_remains_unverified_3317() {
    let db_path = checkpoint_only_cli_fixture(false, false).await;
    let output = replay_cli_3317(&db_path);
    let stdout = String::from_utf8(output.stdout).expect("stdout UTF8");
    let stderr = String::from_utf8(output.stderr).expect("stderr UTF8");
    assert_eq!(output.status.code(), Some(2), "{stdout}\n{stderr}");
    assert!(stderr.contains("UNVERIFIED:"), "{stderr}");
    assert!(stdout.contains("checkpoints_asserted=0"), "{stdout}");
    assert!(!stdout.contains("VERIFIED_BY_CHECKPOINT:"), "{stdout}");
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn mandatory_root_history_is_not_checkpoint_evidence_3317() {
    for (schedule, first_root, expected_height) in [
        ("0:v2", false, 1),
        ("0:v1,2:v2", false, 2),
        ("0:v2", true, 2),
    ] {
        let db_path =
            checkpoint_cli_fixture_with_schedule(false, true, Some(schedule), first_root).await;
        let storage = Storage::open(&db_path, 2).expect("checkpoint control");
        assert_eq!(
            storage.load_checkpoint_at_or_before(2).unwrap().unwrap().0,
            2
        );
        drop(storage);
        let output = replay_cli_3317(&db_path);
        let stdout = String::from_utf8(output.stdout).unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert_eq!(
            output.status.code(),
            Some(1),
            "schedule={schedule} first_root={first_root}: {stdout}\n{stderr}"
        );
        assert!(
            stderr.contains(&format!("DIVERGED at height {expected_height}")),
            "{stderr}"
        );
        assert!(stderr.contains("mandatory"), "{stderr}");
        assert!(!stdout.contains("VERIFIED_BY_CHECKPOINT:"), "{stdout}");
        let _ = std::fs::remove_file(db_path);
    }
}
