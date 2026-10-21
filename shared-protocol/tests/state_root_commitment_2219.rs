//! #2219 — the block hash commits to the post-state root, end-to-end at V2.
//!
//! `SigningProtocolVersion::active()` is a process-wide `OnceLock`, so V2
//! behavior cannot be exercised from the in-crate unit-test binary without
//! contaminating every other test in that process. This file is its own
//! process: it installs V2 once and drives the PRODUCTION producer and
//! verifier paths, exactly as a V2 fleet (the #2034 fresh-genesis cut) will.
//!
//! The version-explicit contract of the checked sink itself
//! (`apply_block_finalization_checked_at`) is unit-tested in `state.rs`; this
//! file proves the `active()`-wired production paths — emit in
//! `create_next_block`, presence gate in `verify_block`, equality in the
//! checked sink — actually fire at V2. Each test goes red if its guard is
//! reverted.
//!
//! Run:
//!   cargo test -p wrfcoin-shared-protocol --test state_root_commitment_2219

use std::sync::Arc;

use rustc_hash::FxHashSet;
use tokio::sync::RwLock;

use wrfcoin_shared_protocol::blockchain::block::{Block, STATE_ROOT_METADATA_KEY};
use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::state::StateError;
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::ProofOfPublishing;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ActivationTable, SigningProtocolVersion, install_activation_table, protocol_version_at,
};
use wrfcoin_shared_protocol::mempool::pool::Mempool;

const PRODUCER: &str = "0x00000000000000000000000000000000000019a5";

/// Install a real height transition for this whole test process.
///
/// The process default remains V1. That is deliberate: every assertion below
/// is RED if a production call site consults `SigningProtocolVersion::active()`
/// instead of resolving the block's own height from the genesis-pinned table.
/// `install_activation_table` is first-install-wins and idempotent, so every
/// test calling this races safely to the same schedule.
fn at_v2() {
    let installed = install_activation_table(
        ActivationTable::from_entries(vec![
            (0, SigningProtocolVersion::V1),
            (1, SigningProtocolVersion::V2),
        ])
        .expect("valid integer-only activation schedule"),
    );
    assert_eq!(
        installed.version_at(0),
        SigningProtocolVersion::V1,
        "the schedule must leave pre-transition block zero at V1"
    );
    assert_eq!(protocol_version_at(1), SigningProtocolVersion::V2);
    assert_eq!(
        SigningProtocolVersion::active(),
        SigningProtocolVersion::V1,
        "RED control premise: the process-global default must disagree with the height schedule"
    );
}

/// Produce one block through the production producer path, returning the block
/// and the producer's post-production chain (whose state holds the root the
/// commitment must match).
async fn produce() -> (Block, Arc<RwLock<Blockchain>>, wrfcoin_shared_protocol::crypto::hash::Hash) {
    let mut pop = ProofOfPublishing::new(1, 60, 0, 10);
    // #2523: a test chain with no oracle ceremony must SAY so — silence at
    // height 1 is refused, because it cannot be told from a misconfigured cut.
    pop.declare_no_genesis_oracles();
    let chain = Blockchain::default();
    let tip = chain.tip;
    let blockchain = Arc::new(RwLock::new(chain));
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));

    let block = pop
        .create_next_block(blockchain.clone(), mempool, PRODUCER)
        .await
        .expect("production producer path must build a block");
    (block, blockchain, tip)
}

async fn verify(block: &Block, tip: &wrfcoin_shared_protocol::crypto::hash::Hash) -> bool {
    let mut pop = ProofOfPublishing::new(1, 60, 0, 10);
    // #2523: a test chain with no oracle ceremony must SAY so — silence at
    // height 1 is refused, because it cannot be told from a misconfigured cut.
    pop.declare_no_genesis_oracles();
    let oracle_set: FxHashSet<String> = FxHashSet::default();
    pop.verify_block_chained(block, tip, &oracle_set, 0, &[])
        .await
        .expect("production verification must not error")
}

/// The producer emits the commitment, it equals the root the producer's own
/// finalized state holds, and the production verifier accepts the block.
/// Red if the `create_next_block` emit is reverted.
#[tokio::test]
async fn v2_producer_commits_the_post_state_root_and_the_verifier_accepts() {
    at_v2();
    let (block, blockchain, tip) = produce().await;

    assert!(
        block.has_well_formed_state_root_commitment(),
        "V2 producer must stamp a well-formed post-state-root commitment"
    );
    let committed = block
        .metadata
        .get(STATE_ROOT_METADATA_KEY)
        .expect("checked well-formed above")
        .clone();
    let producer_root = blockchain.read().await.state.state_root.to_string();
    assert_eq!(
        committed, producer_root,
        "the commitment must be the POST-finalization root the producer holds"
    );

    assert!(verify(&block, &tip).await, "verifier must accept the block");
}

/// The presence gate: stripping the commitment (and honestly re-sealing the
/// hash, so this is not caught as simple tampering) must be rejected by
/// `verify_block` before any state work. Red if the presence gate is reverted.
#[tokio::test]
async fn v2_verifier_rejects_a_block_missing_the_commitment() {
    at_v2();
    let (mut block, _blockchain, tip) = produce().await;

    block.metadata.remove(STATE_ROOT_METADATA_KEY);
    let resealed = block.calculate_hash();
    block.set_hash(resealed);

    assert!(
        !verify(&block, &tip).await,
        "a V2 block without a post-state-root commitment must be rejected"
    );
}

/// The equality check: a hash-consistent block whose committed root is wrong
/// must be rejected by the shared checked finalization sink — the applier-side
/// half that `verify_block` (pre-apply, stateless) cannot perform. Red if the
/// sink's equality guard is reverted.
#[tokio::test]
async fn v2_applier_rejects_a_resealed_wrong_commitment() {
    at_v2();
    let (mut block, blockchain, _tip) = produce().await;

    block
        .metadata
        .insert(STATE_ROOT_METADATA_KEY.to_string(), "ab".repeat(32));
    let resealed = block.calculate_hash();
    block.set_hash(resealed);

    let mut applier_state = blockchain.read().await.state.clone();
    let err = applier_state
        .apply_block_finalization_checked(&block)
        .expect_err("a wrong committed root must be rejected at finalization");
    assert!(
        matches!(err, StateError::StateRootMismatch { .. }),
        "expected StateRootMismatch, got: {err}"
    );
}
