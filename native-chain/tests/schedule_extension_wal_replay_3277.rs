//! #3277 S3: a restart replays each WAL block under the EFFECTIVE feature
//! schedule the producer and the P2P applier used for it — the base schedule
//! from block 1 (#3301 gate) plus the schedule extensions applied before that
//! block.
//!
//! Scenario: block 1 enables the extension transaction at height 2. An
//! extension carried at height 3 schedules `paymaster` at `3 + MIN_RUNUP`,
//! and a paymaster `Stake` (a feature-gated transition) is applied one block
//! after that activation.
//!
//! - **WAL-carried** (no checkpoint): the extension itself is a WAL row, so
//!   only a refresh AFTER each replayed block makes the Stake replay under it.
//! - **Checkpoint-carried**: the extension is already in the checkpoint the
//!   replay anchors on, and no WAL row precedes the Stake, so only a refresh
//!   ON THE ANCHOR makes it replay under it.
//!
//! Plain `Storage::load_state` observes nothing, so it replays the Stake with
//! `paymaster` off and fails: the RED control in both scenarios.
//!
//! Own process: the base schedule, the authority and the extension overlay
//! are process-wide.

use std::path::{Path, PathBuf};

use wrfcoin_native_chain::storage::{gate_then_load_state, Storage};
use wrfcoin_shared_protocol::accounts::paymaster::PaymasterAction;
use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    feature_schedule_extension_entries, install_activation_table, install_feature_schedule,
    refresh_feature_schedule_extensions, resolve_activation_schedule, resolve_feature_schedule,
    ConsensusFeature, SigningProtocolVersion, GENESIS_ACTIVATION_SCHEDULE_KEY,
};
use wrfcoin_shared_protocol::crypto::hash::Hash;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::governance::schedule_extension::{
    install_schedule_extension_authority_for_tests, sign_feature_schedule_extension_with_key,
    FeatureScheduleExtension, ScheduleEntry, SignedFeatureScheduleExtension, MIN_RUNUP_BLOCKS, SCHEDULE_EXTENSION_FEATURE,
};
use rand::rngs::StdRng;
use rand::SeedableRng;

const GENESIS: &str = "abababababababababababababababababababababababababababababababab";
const H_ENABLE: u64 = 2;
const CARRY: u64 = 3;
const ACTIVATION: u64 = CARRY + MIN_RUNUP_BLOCKS;
const STAKE_AT: u64 = ACTIVATION + 1;
const TIP: u64 = STAKE_AT + 1;
const PAYMASTER: Address = Address([0x2b; 20]);

static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn governance() -> PrivateKey {
    PrivateKey::generate(&mut StdRng::seed_from_u64(3277))
}

fn schedule_raw() -> String {
    format!("0:v1,{H_ENABLE}:{SCHEDULE_EXTENSION_FEATURE}")
}

fn extension() -> SignedFeatureScheduleExtension {
    let extension = FeatureScheduleExtension {
        chain_id: "777".into(),
        network_id: "testnet".into(),
        genesis_hash: GENESIS.into(),
        sequence: 0,
        entries: vec![ScheduleEntry::new(ConsensusFeature::Paymaster.name(), ACTIVATION)],
        version_steps: Vec::new(),
        min_protocol_version: 3,
        release_commit: "ab1b1a493".into(),
    };
    let signatures = vec![sign_feature_schedule_extension_with_key(&extension, &governance())
            .expect("governance signs a non-held extension")];
    SignedFeatureScheduleExtension {
        extension,
        signatures,
    }
}

fn genesis_state() -> State {
    let mut state = State::new();
    state.create_account(PAYMASTER, Balance::new(10_000));
    state.recalculate_state_root();
    state
}

fn transitions_at(height: u64) -> Vec<StateTransition> {
    match height {
        CARRY => vec![StateTransition::ApplyFeatureScheduleExtension {
            extension: extension(),
            block_height: height,
        }],
        STAKE_AT => vec![StateTransition::ApplyPaymasterAction {
            action: PaymasterAction::Stake {
                paymaster: PAYMASTER,
                amount: 1_000,
                unstake_delay_blocks: 2,
            },
            block_height: height,
        }],
        _ => Vec::new(),
    }
}

/// The boot gate's schedule half (read DURABLE block 1, install what it
/// declares) plus the S5-to-be authority install. First install wins, and
/// both scenarios install identical values.
fn install_gate_from_block1(storage: &Storage) -> Result<(), String> {
    let block1 = storage
        .load_lowest_block()
        .map_err(|e| e.to_string())?
        .ok_or("no block 1")?;
    let pairs: Vec<(&str, &str)> = block1
        .metadata
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let table = resolve_activation_schedule(pairs.clone(), SigningProtocolVersion::V1)
        .map_err(|e| e.to_string())?;
    let features = resolve_feature_schedule(pairs, &table).map_err(|e| e.to_string())?;
    SigningProtocolVersion::install_active(SigningProtocolVersion::V1);
    install_activation_table(table);
    install_feature_schedule(features);
    install_schedule_extension_authority_for_tests("777", "testnet", GENESIS, vec![PublicKey::from_private_key(&governance()).to_hex().to_ascii_lowercase()]);
    Ok(())
}

fn next_block(height: u64, prev: &mut Hash) -> Block {
    let mut block = Block::new(1, prev.clone(), vec![], 1, height);
    if height == 1 {
        block
            .metadata
            .insert(GENESIS_ACTIVATION_SCHEDULE_KEY.to_string(), schedule_raw());
    }
    let hash = block.calculate_hash();
    block.set_hash(hash.clone());
    *prev = hash;
    block
}

/// Persist the chain exactly as the live producer does and return the
/// producer's post-state. Block 1 is persisted first and the gate installed
/// from it, so the producer applies under the chain's schedule — refreshing
/// the extension overlay after every block, as the live hooks do.
/// `checkpoint_every` = 0 takes no checkpoint.
async fn persist_chain(path: &Path, checkpoint_every: u64) -> State {
    let interval = if checkpoint_every == 0 { u64::MAX } else { checkpoint_every };
    let mut storage = Storage::open(path, interval).expect("open");
    storage.persist_genesis_state(&genesis_state()).await.expect("anchor");
    let mut state = genesis_state();
    let mut prev = Hash::default();
    for height in 1..=TIP {
        state
            .apply_transitions_batch_at_height(height, transitions_at(height))
            .unwrap_or_else(|e| panic!("the producer applies height {height}: {e}"));
        state.apply_block_finalization(height);
        state.refresh_effective_feature_schedule();
        let checkpoint = if checkpoint_every != 0 && height % checkpoint_every == 0 {
            storage.checkpoint_state_bytes(height, &state).expect("encode")
        } else {
            None
        };
        storage
            .persist_block(next_block(height, &mut prev), transitions_at(height), checkpoint)
            .await
            .expect("persist block + WAL rows");
        if height == 1 {
            install_gate_from_block1(&storage).expect("gate from block 1");
        }
    }
    state
}

async fn assert_restart_replays_under_the_effective_schedule(checkpoint_every: u64) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path: PathBuf = dir.path().join("wrfcoin.redb");
    let expected = persist_chain(&path, checkpoint_every).await;
    assert_eq!(
        expected.paymasters.get(&PAYMASTER).map(|p| p.deposit),
        Some(1_000),
        "fixture: the producer's Stake applied under the extension"
    );

    let interval = if checkpoint_every == 0 { u64::MAX } else { checkpoint_every };
    let storage = Storage::open(&path, interval).expect("reopen");

    // A fresh process: the gate is installed, the overlay is empty.
    refresh_feature_schedule_extensions(Vec::new());
    let unobserved = storage.load_state();
    assert!(
        unobserved.is_err(),
        "RED: a replay that never refreshes the overlay applies height {STAKE_AT}'s Stake with \
         paymaster off, got {:?}",
        unobserved.map(|c| c.map(|c| c.height))
    );

    refresh_feature_schedule_extensions(Vec::new());
    let ((), loaded) = gate_then_load_state(&storage, install_gate_from_block1)
        .unwrap_or_else(|e| panic!("gate-then-load must replay under the extension: {e:?}"));
    let chain = loaded.expect("a persisted chain");
    assert_eq!(chain.height, TIP);
    assert_eq!(chain.state.state_root, expected.state_root);
    assert_eq!(chain.state.paymasters.get(&PAYMASTER).map(|p| p.deposit), Some(1_000));
    assert_eq!(
        feature_schedule_extension_entries(),
        vec![(ConsensusFeature::Paymaster.name().to_string(), ACTIVATION)],
        "the overlay after boot reflects the restored canonical state"
    );
}

#[tokio::test]
async fn an_extension_carried_in_the_wal_tail_is_in_force_for_later_replayed_blocks() {
    let _g = LOCK.lock().await;
    assert_restart_replays_under_the_effective_schedule(0).await;
}

#[tokio::test]
async fn an_extension_in_the_checkpoint_anchor_is_in_force_for_the_replayed_tail() {
    let _g = LOCK.lock().await;
    // Checkpoints at 1000 and 2000: the latest (2000) already holds the
    // extension carried at 3, and the tail 2001..=TIP has no WAL row before
    // the Stake at STAKE_AT.
    assert!(STAKE_AT > 2_000 && STAKE_AT < 3_000);
    assert_restart_replays_under_the_effective_schedule(1_000).await;
}

/// The boot half: after the load, `launch_api` refreshes the effective
/// schedule from the restored canonical state AFTER installing the #3093
/// genesis deadline (a replace), so an applied extension can only TIGHTEN it,
/// and refuses to start when the tip is already at or past its activation.
#[test]
fn launch_api_refreshes_the_schedule_from_the_restored_state_after_the_genesis_deadline() {
    let main_rs = include_str!("../src/bin/launch_api/main.rs");
    let genesis_deadline = main_rs
        .find("None => install_upgrade_deadline(None),")
        .expect("the #3093 genesis deadline install");
    let refresh = main_rs
        .find("refresh_schedule_from_canonical_state(\n            &blockchain.state,\n            \"boot load\",")
        .expect("launch_api must refresh the schedule from the restored canonical state at boot");
    assert!(
        refresh > genesis_deadline,
        "the boot refresh must run after the genesis deadline is installed, or that install \
         would replace (relax) a deadline an applied extension tightened"
    );
    let refusal = &main_rs[refresh..];
    let refusal = &refusal[..refusal.find("std::process::exit(").expect("a boot refusal")];
    assert!(
        refusal.contains("if blockchain.height >= deadline.height"),
        "a tip at or past the extension's activation must refuse to start, as boot_refused does"
    );
}
