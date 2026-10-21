//! #3332: `replay_verify` replays under the EFFECTIVE feature schedule — the
//! base schedule plus the #3277 schedule extensions applied in the replayed
//! canonical state — exactly as the node does after S3.
//!
//! The feature-gated behaviour is one the replay's own extraction reads:
//! `AccountAuthorizationDispatch` (#2168). A `Stake` from an account that
//! declared a `SmartAccount` rule extracts to `LockToStake` before the
//! dispatch activation and is refused at and after it. A schedule extension
//! schedules that activation; the producer (the node) applies the chain under
//! it, so a replay that never refreshes the overlay re-extracts the Stake,
//! computes a different root and DIVERGES at the Stake's height.
//!
//! Two carriers, one per refresh site:
//!
//! - **Anchor-carried**: the extension is already in the genesis state the
//!   replay starts from and activates dispatch at block 1, so only the
//!   refresh ON THE ANCHOR (before the first block) puts it in force.
//! - **Block-carried**: the extension is applied by block `CARRY`, so only a
//!   refresh AFTER each applied block puts it in force for later blocks. No
//!   transaction extracts to `ApplyFeatureScheduleExtension` until #3277 S4,
//!   so the replay's observer seam applies the transition at `CARRY` — the
//!   same transition the producer applies there, standing in for S4's
//!   extraction.
//!
//! Own process: the base schedule, the authority and the extension overlay
//! are process-wide; every test installs identical values.

// The extractor's own #[cfg(test)] module references
// `crate::helpers::test_env`; same shim as replay_harness_2750.rs.
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

use std::path::Path;

use rand::SeedableRng;
use rand::rngs::StdRng;

use replay_core::{ReplayVerdict, replay_persisted_chain, replay_persisted_chain_observed};
use transition_extractor::extract_transitions;
use wrfcoin_native_chain::storage::Storage;
use wrfcoin_shared_protocol::accounts::authorization::{
    AccountAuthorization, AccountAuthorizationDeclaration, AuthorizationRule,
};
use wrfcoin_shared_protocol::blockchain::block::{Block, STATE_ROOT_METADATA_KEY};
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::transaction::{
    StakeAuthPayload, Transaction, TransactionType,
};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ConsensusFeature, GENESIS_ACTIVATION_SCHEDULE_KEY, SigningProtocolVersion,
    install_activation_table, install_feature_schedule, refresh_feature_schedule_extensions,
    resolve_activation_schedule, resolve_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::hash::Hash;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::governance::schedule_extension::{
    AppliedExtension, FeatureScheduleExtension, MIN_RUNUP_BLOCKS, SCHEDULE_EXTENSION_FEATURE,
    ScheduleEntry, SignedFeatureScheduleExtension,
    install_schedule_extension_authority_for_tests, sign_feature_schedule_extension_with_key,
};

const GENESIS: &str = "abababababababababababababababababababababababababababababababab";
/// The base schedule enables the extension transaction from height 2.
const H_ENABLE: u64 = 2;
const STAKE_AMOUNT: u64 = 100;

static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn governance() -> PrivateKey {
    PrivateKey::generate(&mut StdRng::seed_from_u64(3277))
}

fn staker() -> PrivateKey {
    PrivateKey::generate(&mut StdRng::seed_from_u64(3332))
}

fn staker_address() -> Address {
    Address::from_public_key(&PublicKey::from_private_key(&staker()))
}

fn schedule_raw() -> String {
    format!("0:v1,{H_ENABLE}:{SCHEDULE_EXTENSION_FEATURE}")
}

/// The boot gate's schedule half plus the extension authority — what a node
/// (and this process) runs before applying anything. First install wins, and
/// every test installs identical values.
fn install_gate() {
    let raw = schedule_raw();
    let pairs = vec![(GENESIS_ACTIVATION_SCHEDULE_KEY, raw.as_str())];
    let table = resolve_activation_schedule(pairs.clone(), SigningProtocolVersion::V1)
        .expect("activation table");
    let features = resolve_feature_schedule(pairs, &table).expect("feature schedule");
    SigningProtocolVersion::install_active(SigningProtocolVersion::V1);
    install_activation_table(table);
    install_feature_schedule(features);
    install_schedule_extension_authority_for_tests("777", "testnet", GENESIS, vec![
            PublicKey::from_private_key(&governance())
                .to_hex()
                .to_ascii_lowercase(),
        ]);
}

/// A governance-signed extension scheduling dispatch at `activation`.
fn dispatch_extension(activation: u64) -> SignedFeatureScheduleExtension {
    let extension = FeatureScheduleExtension {
        chain_id: "777".into(),
        network_id: "testnet".into(),
        genesis_hash: GENESIS.into(),
        sequence: 0,
        entries: vec![ScheduleEntry::new(
            ConsensusFeature::AccountAuthorizationDispatch.name(),
            activation,
        )],
        version_steps: Vec::new(),
        min_protocol_version: 3,
        release_commit: "3332".into(),
    };
    let signatures = vec![sign_feature_schedule_extension_with_key(
        &extension,
        &governance(),
    )
            .expect("governance signs a non-held extension")];
    SignedFeatureScheduleExtension {
        extension,
        signatures,
    }
}

/// Genesis: the staker is funded and has declared a `SmartAccount` rule, so
/// its native-key `Stake` is accepted only while dispatch is inactive.
fn genesis_state(anchor_extension: Option<u64>) -> State {
    let mut state = State::new();
    let owner = staker_address();
    state.create_account(owner, Balance::new(10_000));
    state.authorization_declarations.insert(
        owner,
        AccountAuthorizationDeclaration {
            next_nonce: 0,
            current: Some(AccountAuthorization::declaring(
                AuthorizationRule::SmartAccount { program: [7; 32] },
            )),
            generation: 0,
        },
    );
    if let Some(activation) = anchor_extension {
        // Built the way the S3 halt tests build a canonical state that holds
        // an applied extension.
        state.feature_schedule_extensions.push(AppliedExtension {
            sequence: 0,
            carried_at_height: 1,
            entries: vec![ScheduleEntry::new(
                ConsensusFeature::AccountAuthorizationDispatch.name(),
                activation,
            )],
            version_steps: Vec::new(),
            min_protocol_version: 3,
            release_commit: "3332".into(),
            signers: Vec::new(),
        });
    }
    state.recalculate_state_root();
    state
}

/// The staker's native-key `Stake` (same construction as the extractor's
/// #2168 dispatch tests).
fn stake_tx() -> Transaction {
    let key = staker();
    let mut payload = StakeAuthPayload {
        account: staker_address().to_hex(),
        amount: STAKE_AMOUNT,
        auth_pubkey: None,
        auth_signature: None,
    };
    payload.sign_authorization(&key, false, 0);
    let mut tx = Transaction::new(vec![], vec![]);
    tx.tx_type = TransactionType::Stake;
    tx.sender_address = staker_address();
    tx.nonce = 0;
    tx.data = Some(serde_json::to_string(&payload).expect("payload json"));
    tx.set_id();
    tx
}

fn locks_stake(transitions: &[StateTransition]) -> bool {
    transitions
        .iter()
        .any(|t| matches!(t, StateTransition::LockToStake { .. }))
}

struct Fixture {
    /// Height of the block carrying the Stake: the dispatch activation.
    stake_at: u64,
    /// Block that applies the extension (None = anchor-carried).
    carry: Option<u64>,
    tip: u64,
}

fn carried_transition(fixture: &Fixture, height: u64) -> Vec<StateTransition> {
    match fixture.carry {
        Some(carry) if carry == height => vec![StateTransition::ApplyFeatureScheduleExtension {
            extension: dispatch_extension(fixture.stake_at),
            block_height: height,
        }],
        _ => Vec::new(),
    }
}

/// Produce and persist the chain as the node does: extract, apply, finalize,
/// seal the committed root, then refresh the effective schedule from the new
/// canonical state (the S3 hook after every applied block). Returns the
/// producer's final root.
async fn persist_chain(path: &Path, fixture: &Fixture, genesis: State) -> String {
    let mut storage = Storage::open(path, u64::MAX).expect("open");
    storage
        .persist_genesis_state(&genesis)
        .await
        .expect("anchor");
    let mut state = genesis;
    state.refresh_effective_feature_schedule();
    let mut prev = Hash::default();
    for height in 1..=fixture.tip {
        let txs = if height == fixture.stake_at {
            vec![stake_tx()]
        } else {
            Vec::new()
        };
        let mut block = Block::new(1, prev.clone(), txs, 1, height);
        if height == 1 {
            block
                .metadata
                .insert(GENESIS_ACTIVATION_SCHEDULE_KEY.to_string(), schedule_raw());
        }
        let extracted = extract_transitions(&state, &block, height, block.timestamp.timestamp());
        if height == fixture.stake_at {
            assert!(
                !locks_stake(&extracted),
                "fixture: the producer refuses the Stake at the dispatch activation"
            );
        }
        let mut persisted = extracted.clone();
        state
            .apply_transitions_batch_at_height(height, extracted.into_iter())
            .unwrap_or_else(|e| panic!("producer applies height {height}: {e}"));
        let carried = carried_transition(fixture, height);
        persisted.extend(carried.iter().cloned());
        state
            .apply_transitions_batch_at_height(height, carried.into_iter())
            .unwrap_or_else(|e| panic!("producer applies the extension at {height}: {e}"));
        state.apply_block_finalization(height);
        block.metadata.insert(
            STATE_ROOT_METADATA_KEY.to_string(),
            state.state_root.to_string(),
        );
        let hash = block.calculate_hash();
        block.set_hash(hash.clone());
        prev = hash;
        storage
            .persist_block(block, persisted, None)
            .await
            .expect("persist block");
        state.refresh_effective_feature_schedule();
    }
    assert_eq!(
        state.feature_schedule_extensions.len(),
        1,
        "fixture: the canonical chain holds the applied extension"
    );
    state.state_root.to_string()
}

/// The fixture is discriminating: under the base schedule the replay's own
/// extraction ACCEPTS the Stake at its height, so a replay that skipped the
/// refresh would commit a different root there.
fn assert_base_schedule_accepts_the_stake(fixture: &Fixture) {
    refresh_feature_schedule_extensions(Vec::new());
    let block = Block::new(1, Hash::default(), vec![stake_tx()], 1, fixture.stake_at);
    let transitions = extract_transitions(&genesis_state(None), &block, fixture.stake_at, 0);
    assert!(
        locks_stake(&transitions),
        "discrimination guard: under the base schedule the Stake at {} must extract",
        fixture.stake_at
    );
}

fn expect_verified(verdict: ReplayVerdict, fixture: &Fixture) {
    match verdict {
        ReplayVerdict::Verified {
            blocks_replayed,
            roots_asserted,
            last_height,
            ..
        } => {
            assert_eq!(blocks_replayed, fixture.tip);
            assert_eq!(
                roots_asserted, fixture.tip,
                "every block's root is asserted"
            );
            assert_eq!(last_height, fixture.tip);
        },
        other => panic!(
            "replay_verify must replay a chain with an applied schedule extension under the \
             effective schedule; got {other:?}"
        ),
    }
}

#[tokio::test]
async fn an_extension_in_the_genesis_anchor_is_in_force_for_the_replay() {
    let _g = LOCK.lock().await;
    install_gate();
    // The Stake rides in block 1: no block has been applied before it, so
    // only the refresh ON THE ANCHOR can put the extension in force — a
    // per-block refresh after block 1 would come too late.
    let fixture = Fixture {
        stake_at: 1,
        carry: None,
        tip: 2,
    };
    assert_base_schedule_accepts_the_stake(&fixture);

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("wrfcoin.redb");
    persist_chain(&path, &fixture, genesis_state(Some(fixture.stake_at))).await;

    // A fresh process: the gate is installed, the overlay is empty.
    refresh_feature_schedule_extensions(Vec::new());
    let storage = Storage::open(&path, u64::MAX).expect("reopen");
    expect_verified(replay_persisted_chain(&storage, None), &fixture);
}

#[tokio::test]
async fn an_extension_applied_by_a_block_is_in_force_for_later_replayed_blocks() {
    let _g = LOCK.lock().await;
    install_gate();
    let carry = 3;
    let fixture = Fixture {
        stake_at: carry + MIN_RUNUP_BLOCKS,
        carry: Some(carry),
        tip: carry + MIN_RUNUP_BLOCKS + 1,
    };
    assert_base_schedule_accepts_the_stake(&fixture);

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("wrfcoin.redb");
    persist_chain(&path, &fixture, genesis_state(None)).await;

    refresh_feature_schedule_extensions(Vec::new());
    let storage = Storage::open(&path, u64::MAX).expect("reopen");
    // Stand-in for S4's extraction: apply the extension transition the
    // producer applied at CARRY (after the extracted transitions, before
    // finalization — the producer's order).
    let verdict = replay_persisted_chain_observed(&storage, None, |height, state| {
        let carried = carried_transition(&fixture, height);
        if !carried.is_empty() {
            state
                .apply_transitions_batch_at_height(height, carried.into_iter())
                .expect("the replay applies the carried extension");
        }
    });
    expect_verified(verdict, &fixture);
}
