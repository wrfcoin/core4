//! #3361 F2: the faucet sink binding across its activation `H_F`, through
//! every way a node comes to hold the chain — restart (mid-run-up and after
//! `H_F`), IBD from genesis, `replay_verify`, snapshot join (before and after
//! `H_F`) — plus the clean halt of a binary that lacks the feature.
//!
//! The chain has the LIVE 777 shape: block 1 records chain id `777` and no
//! faucet address, and enables the extension transaction at `H_ENABLE`.
//! Block `CARRY` carries E0f — a governance-signed `FeatureScheduleExtension`
//! TRANSACTION scheduling `faucet_sink_binding` at
//! `H_F = CARRY + MIN_RUNUP_BLOCKS` — and every block carries a coinbase, so
//! every block extracts `ApplyFaucetReward`. The chain is produced and
//! persisted the way the node does it (extract, apply, finalize, commit the
//! root, refresh the effective schedule), then consumed by each path.
//!
//! What every path must reproduce: the producer's `state_root` at EVERY
//! height, and the faucet wallet unpaid through `H_F − 1` and paid exactly its
//! slice from `H_F`. The discriminating fact (asserted by
//! [`the_binding_is_what_moves_the_root_at_h_f`]): a node that holds the chain
//! but not the extension in its effective schedule computes the SAME roots
//! through `H_F − 1` and a DIFFERENT root at `H_F`, so each path's parity at
//! `H_F` proves it put the extension in force.
//!
//! The extension is signed over its preimage with the raw governance key —
//! not a library signer, which refuses a faucet binding without the faucet
//! key's proof of possession (#3361 F3). A node judges only the governance
//! signature; a block carrying the extension is valid whatever its admission
//! policy (T8).
//!
//! Own process: the base schedule, the extension authority, the extension
//! overlay and the upgrade deadline are process-wide; every test installs
//! identical values under one lock.
//!
//! Run: `cargo test -p wrfcoin-native-chain --test faucet_sink_binding_lifecycle_3361`

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

use rand::rngs::StdRng;
use rand::SeedableRng;

use replay_core::{replay_persisted_chain, ReplayVerdict};
use transition_extractor::extract_transitions;
use wrfcoin_native_chain::storage::{gate_then_load_state, Storage};
use wrfcoin_native_chain::upgrade::{
    production_refused_at, refresh_schedule_from_canonical_state, reset_for_tests,
    tighten_upgrade_deadline, upgrade_deadline, UpgradeDeadline,
};
use wrfcoin_shared_protocol::blockchain::block::{Block, STATE_ROOT_METADATA_KEY};
use wrfcoin_shared_protocol::blockchain::state::State;
use wrfcoin_shared_protocol::blockchain::state_snapshot::export_state_entries;
use wrfcoin_shared_protocol::blockchain::transaction::Transaction;
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::{
    coinbase_allowance, EmissionPartition, EmissionRole,
};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    effective_feature_schedule, feature_active, feature_schedule_extension_entries,
    install_activation_table, install_feature_schedule, refresh_feature_schedule_extensions,
    resolve_activation_schedule, resolve_feature_schedule, ConsensusFeature,
    SigningProtocolVersion, GENESIS_ACTIVATION_SCHEDULE_KEY, GENESIS_CHAIN_ID_KEY,
    LEGACY_FAUCET_BINDINGS,
};
use wrfcoin_shared_protocol::crypto::hash::Hash;
use wrfcoin_shared_protocol::crypto::protocol_support::{
    boot_refused, upgrade_required_in_feature_schedule,
    upgrade_required_in_feature_schedule_with, KNOWN_FEATURES,
};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::governance::schedule_extension::{
    install_schedule_extension_authority_for_tests, FeatureScheduleExtension, GovernanceSignature,
    ScheduleEntry, SignedFeatureScheduleExtension, MIN_RUNUP_BLOCKS,
    SCHEDULE_EXTENSION_FEATURE,
};

/// The live 777 instance's block-1 hash (the committed faucet row's).
const LIVE_777_BLOCK1: &str = "6ad436386cfc6c746146f6ef2f9d32ddb7a93f18efd3635606abd27c9ca17db8"; // pragma: allowlist secret -- public genesis hash
const LIVE_FAUCET: &str = "0xf72059c3b4a6d3c431920c3cf3e40dbf68a6b5d3"; // pragma: allowlist secret -- public faucet wallet ADDRESS, ruled on core4#3361
const FEATURE: &str = "faucet_sink_binding";
/// The base schedule enables the extension transaction from this height (V1).
const H_ENABLE: u64 = 2;
/// The block carrying E0f.
const CARRY: u64 = 3;
/// The binding's activation: the earliest V7 allows for `CARRY`.
const H_F: u64 = CARRY + MIN_RUNUP_BLOCKS;
const TIP: u64 = H_F + 2;
/// A height inside the run-up: E0f carried, `H_F` not reached.
const MID: u64 = 1_000;
const MINER: Address = Address([0x41; 20]);

static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn governance() -> PrivateKey {
    PrivateKey::generate(&mut StdRng::seed_from_u64(3361))
}

fn schedule_raw() -> String {
    format!("0:v1,{H_ENABLE}:{SCHEDULE_EXTENSION_FEATURE}")
}

/// The boot gate's schedule half and the extension authority — what a node
/// installs before applying anything. First install wins; every test in this
/// process installs identical values. Also resets the per-process state a
/// FRESH node starts with: an empty overlay and no deadline.
fn fresh_node() {
    let raw = schedule_raw();
    let pairs = vec![(GENESIS_ACTIVATION_SCHEDULE_KEY, raw.as_str())];
    let table = resolve_activation_schedule(pairs.clone(), SigningProtocolVersion::V1)
        .expect("activation table");
    let features = resolve_feature_schedule(pairs, &table).expect("feature schedule");
    SigningProtocolVersion::install_active(SigningProtocolVersion::V1);
    install_activation_table(table);
    install_feature_schedule(features);
    install_schedule_extension_authority_for_tests("777", "testnet", LIVE_777_BLOCK1, vec![PublicKey::from_private_key(&governance())
            .to_hex()
            .to_ascii_lowercase()]);
    refresh_feature_schedule_extensions(Vec::new());
    reset_for_tests();
}

/// E0f, signed over its preimage with the raw governance key.
fn e0f() -> SignedFeatureScheduleExtension {
    let extension = FeatureScheduleExtension {
        chain_id: "777".into(),
        network_id: "testnet".into(),
        genesis_hash: LIVE_777_BLOCK1.into(),
        sequence: 0,
        entries: vec![ScheduleEntry::new(FEATURE, H_F)],
        version_steps: Vec::new(),
        min_protocol_version: 3,
        release_commit: "f2-test-release".into(),
    };
    let key = governance();
    let signature = GovernanceSignature {
        signer_pubkey: PublicKey::from_private_key(&key)
            .to_hex()
            .to_ascii_lowercase(),
        signature: key.sign(&extension.preimage()).to_hex(),
    };
    SignedFeatureScheduleExtension {
        extension,
        signatures: vec![signature],
    }
}

fn faucet_balance(state: &State) -> u128 {
    let live = Address::from_hex(LIVE_FAUCET).unwrap();
    state
        .get_account(&live)
        .map(|a| a.balance.into_inner())
        .unwrap_or(0)
}

fn slice(height: u64) -> u128 {
    EmissionPartition::at_height(height, Some("777")).slice(EmissionRole::Faucet)
}

/// The faucet balance the binding must have produced by `height`.
fn expected_faucet(height: u64) -> u128 {
    (H_F..=height).map(slice).sum()
}

/// The chain as produced: every block (hash-linked, root committed) and the
/// producer's state after each height (`states[h]`; `states[0]` = genesis).
struct Produced {
    blocks: Vec<Block>,
    states: Vec<State>,
}

impl Produced {
    fn root(&self, height: u64) -> String {
        self.states[height as usize].state_root.to_string()
    }
}

/// The block at `height` before its root is committed: a coinbase (so the
/// extractor emits `ApplyFaucetReward`), E0f at `CARRY`, and block 1's
/// chain id and base schedule.
fn raw_block(height: u64, prev: &Hash) -> Block {
    let mut txs = vec![Transaction::new_coinbase(
        &MINER.to_string(),
        coinbase_allowance(height, Some("777")) as u64,
        height,
    )];
    if height == CARRY {
        txs.push(Transaction::new_feature_schedule_extension(&e0f()));
    }
    let mut block = Block::new(1, prev.clone(), txs, 1, height);
    if height == 1 {
        block
            .metadata
            .insert(GENESIS_CHAIN_ID_KEY.to_string(), "777".into());
        block
            .metadata
            .insert(GENESIS_ACTIVATION_SCHEDULE_KEY.to_string(), schedule_raw());
    }
    block
}

/// One block through the node's apply path from `state`: extract, apply,
/// finalize; then (when `refresh`) the post-block schedule refresh every
/// live apply hook runs. Returns the extracted transitions.
fn apply_block(
    state: &mut State,
    block: &Block,
    refresh: bool,
) -> Vec<wrfcoin_shared_protocol::blockchain::state::StateTransition> {
    let transitions = extract_transitions(state, block, block.height, block.timestamp.timestamp());
    state
        .apply_transitions_batch_at_height(block.height, transitions.clone().into_iter())
        .unwrap_or_else(|e| panic!("apply height {}: {e}", block.height));
    state.apply_block_finalization(block.height);
    if refresh {
        state.refresh_effective_feature_schedule();
    }
    transitions
}

/// Produce the chain to `TIP` and, when `path` is given, persist it as the
/// node does (block + WAL rows; a checkpoint every `checkpoint_every` blocks,
/// 0 = none).
async fn produce(path: Option<&Path>, checkpoint_every: u64) -> Produced {
    fresh_node();
    let interval = if checkpoint_every == 0 { u64::MAX } else { checkpoint_every };
    let mut storage = path.map(|p| Storage::open(p, interval).expect("open"));
    let mut state = State::new();
    if let Some(storage) = storage.as_mut() {
        storage.persist_genesis_state(&state).await.expect("anchor");
    }
    let mut states = vec![state.clone()];
    let mut blocks = Vec::new();
    let mut prev = Hash::default();
    for height in 1..=TIP {
        let mut block = raw_block(height, &prev);
        let transitions = apply_block(&mut state, &block, false);
        block
            .metadata
            .insert(STATE_ROOT_METADATA_KEY.to_string(), state.state_root.to_string());
        let hash = block.calculate_hash();
        block.set_hash(hash.clone());
        prev = hash;
        if let Some(storage) = storage.as_mut() {
            let checkpoint = if checkpoint_every != 0 && height % checkpoint_every == 0 {
                storage.checkpoint_state_bytes(height, &state).expect("encode")
            } else {
                None
            };
            storage
                .persist_block(block.clone(), transitions, checkpoint)
                .await
                .expect("persist block");
        }
        state.refresh_effective_feature_schedule();
        states.push(state.clone());
        blocks.push(block);
    }

    // The fixture is what it claims to be.
    let at = |h: u64| &states[h as usize];
    assert_eq!(
        at(CARRY).feature_schedule_extension_entries(),
        vec![(FEATURE.to_string(), H_F)],
        "E0f is extracted from its transaction and applied at CARRY"
    );
    assert_eq!(
        at(1).genesis_emission_config.as_ref().map(|c| (c.chain_id.clone(), c.faucet_address)),
        Some(("777".to_string(), None)),
        "the live 777 shape: no genesis faucet"
    );
    assert_eq!(faucet_balance(at(H_F - 1)), 0, "unpaid through H_F - 1");
    for height in H_F..=TIP {
        assert!(slice(height) > 0);
        assert_eq!(faucet_balance(at(height)), expected_faucet(height), "paid its slice at {height}");
    }
    Produced { blocks, states }
}

/// Continue from `state` (the node's state at `from`) through `TIP`,
/// asserting the producer's root at every height. `refresh` is the per-block
/// schedule refresh.
fn follow(produced: &Produced, mut state: State, from: u64, refresh: bool) -> Result<State, String> {
    for block in &produced.blocks[from as usize..] {
        apply_block(&mut state, block, refresh);
        let want = produced.root(block.height);
        if state.state_root.to_string() != want {
            return Err(format!(
                "root diverges at {} (faucet {} vs {})",
                block.height,
                faucet_balance(&state),
                faucet_balance(&produced.states[block.height as usize])
            ));
        }
    }
    Ok(state)
}

/// The discriminating fact every other test leans on: the same blocks, with
/// the extension kept OUT of the effective schedule, give identical roots
/// through `H_F − 1` and diverge exactly at `H_F`.
#[tokio::test]
async fn the_binding_is_what_moves_the_root_at_h_f() {
    let _g = LOCK.lock().await;
    let produced = produce(None, 0).await;
    fresh_node();
    let err = follow(&produced, State::new(), 0, false).expect_err("must diverge");
    assert!(err.starts_with(&format!("root diverges at {H_F} ")), "{err}");
}

/// IBD: a fresh node applies every block from genesis through the P2P apply
/// path, refreshing after each block as the live hook does, and matches the
/// producer at every height across `H_F`.
#[tokio::test]
async fn ibd_from_genesis_across_h_f_matches_the_producer() {
    let _g = LOCK.lock().await;
    let produced = produce(None, 0).await;
    fresh_node();
    let synced = follow(&produced, State::new(), 0, true).expect("IBD reproduces every root");
    assert_eq!(faucet_balance(&synced), expected_faucet(TIP));
    assert!(feature_active(ConsensusFeature::FaucetSinkBinding, H_F));
    assert!(!feature_active(ConsensusFeature::FaucetSinkBinding, H_F - 1));
}

/// Restart mid-run-up: the node stops at `MID` (E0f carried, `H_F` ahead)
/// and boots again — through the gate-then-load path, from a checkpoint
/// that already holds E0f plus a WAL tail, and from the WAL alone — then
/// follows the chain through `H_F`. RED control: a load that never refreshes
/// the overlay boots fine but diverges at `H_F`.
#[tokio::test]
async fn restart_mid_run_up_then_follow_through_h_f() {
    let _g = LOCK.lock().await;
    let produced = produce(None, 0).await;
    for checkpoint_every in [0, 400] {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wrfcoin.redb");
        persist_prefix(&produced, &path, MID, checkpoint_every).await;

        fresh_node();
        let storage = Storage::open(&path, interval(checkpoint_every)).expect("reopen");
        let ((), loaded) = gate_then_load_state(&storage, |_| Ok::<(), String>(()))
            .unwrap_or_else(|e| panic!("boot at MID: {e:?}"));
        let chain = loaded.expect("a persisted chain");
        assert_eq!(chain.height, MID);
        assert_eq!(chain.state.state_root.to_string(), produced.root(MID));
        // The boot refresh (launch_api after load) installs the schedule the
        // restored state holds; this binary knows the feature: no deadline.
        refresh_installs_no_binding_deadline(&chain.state, "boot load");
        assert_eq!(feature_schedule_extension_entries(), vec![(FEATURE.to_string(), H_F)]);
        let tip = follow(&produced, chain.state, MID, true)
            .unwrap_or_else(|e| panic!("checkpoint every {checkpoint_every}: {e}"));
        assert_eq!(faucet_balance(&tip), expected_faucet(TIP));

        // RED control: plain load, no refresh anywhere.
        fresh_node();
        let unobserved = storage.load_state().expect("load").expect("chain");
        assert_eq!(unobserved.state.state_root.to_string(), produced.root(MID));
        let err = follow(&produced, unobserved.state, MID, false).expect_err("must diverge");
        assert!(err.starts_with(&format!("root diverges at {H_F} ")), "{err}");
    }
}

/// Restart after `H_F`: the node stops at `TIP` and boots again (WAL alone,
/// and a checkpoint taken after `H_F` plus a WAL tail); the restored state
/// is the producer's, faucet paid from `H_F`, and the boot installs no
/// deadline.
#[tokio::test]
async fn restart_after_h_f_restores_the_paid_faucet() {
    let _g = LOCK.lock().await;
    let produced = produce(None, 0).await;
    for checkpoint_every in [0, H_F + 1] {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wrfcoin.redb");
        persist_prefix(&produced, &path, TIP, checkpoint_every).await;

        fresh_node();
        let storage = Storage::open(&path, interval(checkpoint_every)).expect("reopen");
        let ((), loaded) = gate_then_load_state(&storage, |_| Ok::<(), String>(()))
            .unwrap_or_else(|e| panic!("boot at TIP: {e:?}"));
        let chain = loaded.expect("a persisted chain");
        assert_eq!(chain.height, TIP);
        assert_eq!(chain.state.state_root.to_string(), produced.root(TIP), "{checkpoint_every}");
        assert_eq!(faucet_balance(&chain.state), expected_faucet(TIP));
        refresh_installs_no_binding_deadline(&chain.state, "boot load");
        assert_eq!(upgrade_deadline(), None, "this binary implements the feature");
    }
}

/// `replay_verify` from genesis across `H_F`: it extracts E0f from its
/// transaction, refreshes after each block, and verifies every committed
/// root — including `H_F`, where a replay without the binding diverges.
#[tokio::test]
async fn replay_verify_across_h_f_verifies_every_root() {
    let _g = LOCK.lock().await;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("wrfcoin.redb");
    produce(Some(&path), 0).await;

    fresh_node();
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
        other => panic!("replay_verify must verify across H_F; got {other:?}"),
    }
}

/// Snapshot join, before `H_F` (inside the run-up, E0f carried) and after
/// it: the importer installs the snapshot (the carried
/// `feature_schedule_extensions` and faucet account included), refreshes its
/// schedule from the installed state as ingest does, and follows the chain
/// to `TIP`. RED control: a run-up join that skips the refresh diverges at
/// `H_F`.
#[tokio::test]
async fn snapshot_join_before_and_after_h_f() {
    let _g = LOCK.lock().await;
    let produced = produce(None, 0).await;
    for at in [MID, H_F + 1] {
        let source = &produced.states[at as usize];
        let entries = export_state_entries(source).expect("export");
        fresh_node();
        let mut joined = State::new();
        joined.apply_snapshot_entries(&entries).expect("install");
        assert_eq!(joined.state_root, source.state_root, "snapshot at {at}");
        assert_eq!(joined.feature_schedule_extensions, source.feature_schedule_extensions);
        assert_eq!(faucet_balance(&joined), faucet_balance(source));
        // `install_snapshot_state`'s refresh.
        refresh_installs_no_binding_deadline(&joined, "snapshot import");
        let tip = follow(&produced, joined, at, true)
            .unwrap_or_else(|e| panic!("joined at {at}: {e}"));
        assert_eq!(faucet_balance(&tip), expected_faucet(TIP));
    }

    // RED control.
    let entries = export_state_entries(&produced.states[MID as usize]).expect("export");
    fresh_node();
    let mut joined = State::new();
    joined.apply_snapshot_entries(&entries).expect("install");
    let err = follow(&produced, joined, MID, false).expect_err("must diverge");
    assert!(err.starts_with(&format!("root diverges at {H_F} ")), "{err}");
}

/// A binary WITHOUT the `FaucetSinkBinding` variant (every B3 built before
/// F1, simulated by this binary's known list minus the name) accepts the E0f
/// block, then — from the applied extension — installs the #3093 clean-halt
/// deadline at `H_F`: it produces and applies through `H_F − 1`, refuses
/// `H_F`, and refuses to boot once its tip is at or past `H_F`.
///
/// RED control (design §6): with the variant present — this binary — the
/// same canonical state installs no deadline and nothing is refused.
#[tokio::test]
async fn a_binary_without_the_variant_halts_at_h_f_minus_1() {
    let _g = LOCK.lock().await;
    let produced = produce(None, 0).await;
    let canonical = &produced.states[CARRY as usize];

    // RED control first: this binary knows the feature, so the applied
    // extension installs no deadline for it and nothing is refused.
    fresh_node();
    assert!(KNOWN_FEATURES.contains(&FEATURE));
    refresh_installs_no_binding_deadline(canonical, "block apply");
    canonical.refresh_effective_feature_schedule();
    assert_eq!(
        upgrade_required_in_feature_schedule_with(&effective_feature_schedule(), &known_with_gate()),
        None
    );
    assert!(upgrade_required_in_feature_schedule(&effective_feature_schedule())
        .is_none_or(|r| r.feature != FEATURE));
    assert!(production_refused_at(H_F).is_none());

    // The pre-F1 binary: same state, same effective schedule, a known list
    // without the name.
    let without: Vec<&str> = known_with_gate().into_iter().filter(|f| *f != FEATURE).collect();
    fresh_node();
    canonical.refresh_effective_feature_schedule();
    let required = upgrade_required_in_feature_schedule_with(&effective_feature_schedule(), &without)
        .expect("the old binary cannot follow the scheduled binding");
    assert_eq!((required.feature.as_str(), required.height), (FEATURE, H_F));
    assert!(tighten_upgrade_deadline(UpgradeDeadline::from_required(&required), "block apply"));
    assert!(production_refused_at(H_F - 1).is_none(), "produces and applies through H_F - 1");
    assert!(production_refused_at(H_F).is_some(), "halts before H_F");
    assert!(!boot_refused(&required, H_F - 1));
    assert!(boot_refused(&required, H_F), "refuses to boot at tip >= H_F");
    assert!(boot_refused(&required, TIP));
    reset_for_tests();

    // The deadline comes from the applied extension alone: the committed
    // row's feature is the one scheduled.
    assert_eq!(LEGACY_FAUCET_BINDINGS[0].feature.name(), FEATURE);
}

/// The boot / snapshot-import refresh (`refresh_schedule_from_canonical_state`)
/// on a binary that implements the binding: it must install no deadline
/// for it. Until #3277 S5 makes the gate's own name `schedule_extension` a
/// known feature, the refresh installs a stand-in deadline at `H_ENABLE` for
/// THAT name (S4 trap (a)); it is tolerated here, never the binding's, and
/// cleared so it cannot leak into the next step.
fn refresh_installs_no_binding_deadline(state: &State, surface: &str) {
    if let Some(deadline) = refresh_schedule_from_canonical_state(state, surface) {
        assert_eq!(
            (deadline.feature.as_str(), deadline.height),
            (SCHEDULE_EXTENSION_FEATURE, H_ENABLE),
            "{surface}: the only deadline allowed is the pre-S5 gate stand-in"
        );
    }
    reset_for_tests();
}

/// The known list of THIS binary with the gate's own name, which #3277 S5
/// adds to `KNOWN_FEATURES` (S4 trap (a)); idempotent once S5 is in.
fn known_with_gate() -> Vec<&'static str> {
    let mut known: Vec<&'static str> = KNOWN_FEATURES.to_vec();
    if !known.contains(&SCHEDULE_EXTENSION_FEATURE) {
        known.push(SCHEDULE_EXTENSION_FEATURE);
    }
    known
}

fn interval(checkpoint_every: u64) -> u64 {
    if checkpoint_every == 0 {
        u64::MAX
    } else {
        checkpoint_every
    }
}

/// Persist `produced` through `last` as the node would have (block + WAL
/// rows; checkpoints every `checkpoint_every`).
async fn persist_prefix(produced: &Produced, path: &Path, last: u64, checkpoint_every: u64) {
    let mut storage = Storage::open(path, interval(checkpoint_every)).expect("open");
    storage
        .persist_genesis_state(&produced.states[0])
        .await
        .expect("anchor");
    fresh_node();
    let mut state = State::new();
    for block in &produced.blocks[..last as usize] {
        let transitions = apply_block(&mut state, block, true);
        assert_eq!(state.state_root.to_string(), produced.root(block.height));
        let checkpoint = if checkpoint_every != 0 && block.height % checkpoint_every == 0 {
            storage
                .checkpoint_state_bytes(block.height, &state)
                .expect("encode")
        } else {
            None
        };
        storage
            .persist_block(block.clone(), transitions, checkpoint)
            .await
            .expect("persist");
    }
}
