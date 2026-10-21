//! #3277 S4: `replay_verify` and a syncing node's extraction replay a chain
//! whose schedule extension rides in a block as a real
//! `FeatureScheduleExtension` TRANSACTION, across the activation it
//! schedules — no observer seam (the #3332 test stood in for this
//! extraction with one).
//!
//! The chain: block 1 enables the extension transaction at height 2. Block
//! `CARRY` carries a governance-signed extension scheduling
//! `AccountAuthorizationDispatch` at `CARRY + MIN_RUNUP_BLOCKS`; a staker who
//! declared a `SmartAccount` rule sends a native-key `Stake` exactly at that
//! activation, which the producer's extraction REFUSES because dispatch is
//! then in force. The chain is persisted as the node does (extract, apply,
//! finalize, commit the root, refresh the effective schedule).
//!
//! `replay_persisted_chain` — the production entry point, no observer —
//! must extract the extension from the transaction itself, refresh after
//! each block, refuse the Stake, and verify every committed root. The
//! fixture is discriminating: under the base schedule alone the Stake
//! extracts, so a replay that dropped the extension would diverge at the
//! activation height.
//!
//! Own process: the base schedule, the authority and the extension overlay
//! are process-wide.

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

use replay_core::{ReplayVerdict, replay_persisted_chain};
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
    FeatureScheduleExtension, MIN_RUNUP_BLOCKS, SCHEDULE_EXTENSION_FEATURE, ScheduleEntry,
    SignedFeatureScheduleExtension,
    install_schedule_extension_authority_for_tests, sign_feature_schedule_extension_with_key,
};

const GENESIS: &str = "abababababababababababababababababababababababababababababababab";
/// The base schedule enables the extension transaction from height 2.
const H_ENABLE: u64 = 2;
const STAKE_AMOUNT: u64 = 100;


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
        release_commit: "s4-test-release".into(),
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
fn genesis_state() -> State {
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

const CARRY: u64 = 3;
const STAKE_AT: u64 = CARRY + MIN_RUNUP_BLOCKS;
const TIP: u64 = STAKE_AT + 1;

fn extension_is_applied(transitions: &[StateTransition]) -> bool {
    transitions
        .iter()
        .any(|t| matches!(t, StateTransition::ApplyFeatureScheduleExtension { .. }))
}

/// Produce and persist the chain as the node does. The extension is ONLY a
/// transaction in block `CARRY`: nothing applies it except extraction.
async fn persist_chain(path: &Path) -> String {
    let mut storage = Storage::open(path, u64::MAX).expect("open");
    let genesis = genesis_state();
    storage
        .persist_genesis_state(&genesis)
        .await
        .expect("anchor");
    let mut state = genesis;
    state.refresh_effective_feature_schedule();
    let mut prev = Hash::default();
    for height in 1..=TIP {
        let txs = match height {
            CARRY => vec![Transaction::new_feature_schedule_extension(&dispatch_extension(STAKE_AT))],
            STAKE_AT => vec![stake_tx()],
            _ => Vec::new(),
        };
        let mut block = Block::new(1, prev.clone(), txs, 1, height);
        if height == 1 {
            block
                .metadata
                .insert(GENESIS_ACTIVATION_SCHEDULE_KEY.to_string(), schedule_raw());
        }
        let extracted = extract_transitions(&state, &block, height, block.timestamp.timestamp());
        assert_eq!(
            extension_is_applied(&extracted),
            height == CARRY,
            "the extension is extracted from its transaction, at {CARRY} only"
        );
        if height == STAKE_AT {
            assert!(
                !locks_stake(&extracted),
                "fixture: the producer refuses the Stake at the dispatch activation"
            );
        }
        state
            .apply_transitions_batch_at_height(height, extracted.clone().into_iter())
            .unwrap_or_else(|e| panic!("producer applies height {height}: {e}"));
        state.apply_block_finalization(height);
        block.metadata.insert(
            STATE_ROOT_METADATA_KEY.to_string(),
            state.state_root.to_string(),
        );
        let hash = block.calculate_hash();
        block.set_hash(hash.clone());
        prev = hash;
        storage
            .persist_block(block, extracted, None)
            .await
            .expect("persist block");
        state.refresh_effective_feature_schedule();
    }
    assert_eq!(
        state.feature_schedule_extension_entries(),
        vec![(
            ConsensusFeature::AccountAuthorizationDispatch.name().to_string(),
            STAKE_AT
        )],
        "fixture: the canonical chain holds the extension its transaction carried"
    );
    state.state_root.to_string()
}

#[tokio::test]
async fn replay_verify_extracts_a_transaction_carried_extension_across_its_activation() {
    install_gate();
    // Discrimination guard: under the base schedule alone the Stake extracts,
    // so a replay that never applied the extension would commit a different
    // root at STAKE_AT.
    refresh_feature_schedule_extensions(Vec::new());
    let probe = Block::new(1, Hash::default(), vec![stake_tx()], 1, STAKE_AT);
    assert!(
        locks_stake(&extract_transitions(&genesis_state(), &probe, STAKE_AT, 0)),
        "discrimination guard: the Stake extracts under the base schedule"
    );

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("wrfcoin.redb");
    let producer_root = persist_chain(&path).await;

    // A fresh process: the gate is installed, the overlay is empty.
    refresh_feature_schedule_extensions(Vec::new());
    let storage = Storage::open(&path, u64::MAX).expect("reopen");
    match replay_persisted_chain(&storage, None) {
        ReplayVerdict::Verified {
            blocks_replayed,
            roots_asserted,
            last_height,
            ..
        } => {
            assert_eq!(blocks_replayed, TIP);
            assert_eq!(roots_asserted, TIP, "every block's root is asserted");
            assert_eq!(last_height, TIP);
        },
        other => panic!(
            "replay_verify must extract the carried extension and verify across its \
             activation; got {other:?}"
        ),
    }
    assert!(!producer_root.is_empty());
}

/// #3277 S5: `replay_verify` arms the chain's protocol from its own block 1
/// (version, table, base schedule with the bootstrap) before replaying — the
/// production binary never had anything else install it. The chain is
/// persisted by the parent under the test gate; a CHILD process with no
/// schedule installed at all (only the test authority, since this chain's
/// block 1 is not the live one the pins know) replays it. Only a replay that
/// installs block 1's schedule itself opens the V1 gate and verifies; the
/// RED control (in the PR) drops that install and the carried extension
/// fails closed.
#[tokio::test]
async fn replay_verify_arms_the_protocol_from_block1_itself_3277() {
    const CHILD_DB: &str = "WRFCOIN_REPLAY_BLOCK1_3277_DB";
    if let Some(db) = std::env::var_os(CHILD_DB) {
        assert!(
            wrfcoin_shared_protocol::crypto::chain_binding::try_feature_schedule().is_none(),
            "the child starts with no schedule installed"
        );
        install_schedule_extension_authority_for_tests("777", "testnet", GENESIS, vec![
            PublicKey::from_private_key(&governance()).to_hex().to_ascii_lowercase(),
        ]);
        let storage = Storage::open(Path::new(&db), u64::MAX).expect("reopen");
        match replay_persisted_chain(&storage, None) {
            ReplayVerdict::Verified { blocks_replayed, roots_asserted, .. } => {
                assert_eq!(blocks_replayed, TIP);
                assert_eq!(roots_asserted, TIP);
            },
            other => panic!("replay_verify must arm block 1's schedule itself; got {other:?}"),
        }
        return;
    }
    install_gate();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("wrfcoin.redb");
    persist_chain(&path).await;
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "replay_verify_arms_the_protocol_from_block1_itself_3277", "--nocapture"])
        .env(CHILD_DB, &path)
        .status()
        .expect("re-exec child");
    assert!(status.success(), "the isolated replay child failed");
}
