//! #2143 — the per-block verification budget as a SCHEDULED consensus rule.
//!
//! The unit tests beside the module prove the pricing arithmetic. This target
//! drives the real producer and applier paths at activation: an over-budget
//! mempool is packed into successive in-budget blocks, while an incoming block
//! that exceeds the same budget is refused before expensive verification. The
//! same paths also pin that both behaviors remain dormant below activation.
//!
//! It also pins the #3093 interaction. `verify_unit_pricing` is a real
//! consensus rule, so its name must be in `KNOWN_FEATURES`: a binary that does
//! not implement it must stop cleanly at the activation rather than accept
//! blocks it cannot price.

use std::sync::Arc;

use rustc_hash::FxHashSet;
use tokio::sync::RwLock;
use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::transaction::{
    Transaction, TransactionInput, TransactionOutput, TransactionType,
};
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::ProofOfPublishing;
use wrfcoin_shared_protocol::consensus::verify_cost::{
    MAX_RING_SIZE, VerifyCost, VerifyPricing, block_verify_work,
};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ConsensusFeature, FeatureSchedule, feature_active, install_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::protocol_support::{
    KNOWN_FEATURES, upgrade_required_in_schedule,
};
use wrfcoin_shared_protocol::mempool::pool::Mempool;

/// The schedule this target installs. `ACTIVE_FEATURES` is a `OnceLock`, so a
/// test binary installs exactly one schedule — every test here reads it.
const ACTIVATION: u64 = 14_000;
const PRODUCER_TXS: usize = 40;

/// Pricing-only producer fixture that remains admissible under node policy.
/// Privacy claims are now refused at admission (Refs #3408); incoming-block
/// controls below still use the original claims to pin consensus behavior.
fn heavy_regular_transaction(tag: usize) -> Transaction {
    let mut tx = maximal_reward_claim(tag);
    let input = tx.inputs[0].clone();
    tx.inputs = (0..1_400).map(|vout| TransactionInput { vout, ..input.clone() }).collect();
    tx.tx_type = TransactionType::Regular;
    tx.set_id();
    tx
}

fn producer_pool() -> Mempool {
    // Deliberately raise only the fixture byte ceiling to isolate the verify
    // budget from transaction-size policy; this does not bypass admission.
    let mut config = Config::default();
    config.max_transaction_size = 1_000_000;
    Mempool::new(&config)
}

fn install_once() {
    let schedule = FeatureSchedule::from_entries(vec![(
        ConsensusFeature::VerifyUnitPricing.name().to_string(),
        ACTIVATION,
    )])
    .expect("a single-feature schedule is valid");
    install_feature_schedule(schedule);
}

/// A transaction that forces a maximal amount of verification work: an
/// unparseable reward-claim payload, which the fail-expensive rule charges as
/// a full 1024-member ring.
fn maximal_reward_claim(tag: usize) -> Transaction {
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: wrfcoin_shared_protocol::crypto::hash::hash_bytes(
                format!("2143-hostile-{tag}").as_bytes(),
            ),
            vout: 0,
            signature: Some("00".into()),
            public_key: Some("00".into()),
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            amount: 1,
            address: format!("wrf-2143-hostile-{tag}"),
            script: "OP_TRUE".to_string(),
            nft_data: None,
        }],
    );
    tx.tx_type = TransactionType::RewardClaim;
    tx.set_id();
    tx
}

/// A well-formed reward claim carrying a real `members`-sized ring — what
/// honest privacy traffic looks like.
fn well_formed_reward_claim(members: usize) -> Transaction {
    let ring: Vec<serde_json::Value> = (0..members)
        .map(|i| serde_json::json!({ "public_key": format!("{i:064x}") }))
        .collect();
    let ring_signature = hex::encode(
        serde_json::to_vec(&serde_json::json!({
            "key_image": "00", "c": "00", "s": [], "ring": ring
        }))
        .expect("serialize"),
    );
    let payload = serde_json::json!({
        "epoch": 1,
        "ring_signature": ring_signature,
        "key_image": "00",
        "amount_commitment": "00",
        "range_proof": "00",
        "destination": "0x00",
    });
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: wrfcoin_shared_protocol::crypto::hash::hash_bytes(b"2143-wf"),
            vout: 0,
            signature: Some("00".into()),
            public_key: Some("00".into()),
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            amount: 1,
            address: format!("wrf-2143-ring-{members}"),
            script: "OP_TRUE".to_string(),
            nft_data: None,
        }],
    );
    tx.tx_type = TransactionType::RewardClaim;
    tx.data = Some(payload.to_string());
    tx.set_id();
    tx
}

async fn produce_one_below_activation() -> Block {
    install_once();
    let mut chain = Blockchain::default();
    chain.height = ACTIVATION - 2;
    let chain = Arc::new(RwLock::new(chain));
    let mempool = Arc::new(RwLock::new(producer_pool()));
    {
        let mut pool = mempool.write().await;
        for tx in (0..PRODUCER_TXS).map(heavy_regular_transaction) {
            pool.add_transaction(tx)
                .expect("fixture with a real output must be admitted");
        }
    }
    let mut pop = ProofOfPublishing::new(1, 60, 0, 64);
    pop.declare_no_genesis_oracles();
    pop.create_next_block(chain, mempool, "wrf-2143-dormant-miner")
        .await
        .expect("below activation must seal")
}

/// M3 regression: before activation the real producer must ignore the verify
/// budget and preserve the legacy packing behavior, even when the selected set
/// is over the future budget.
#[tokio::test]
async fn producer_below_activation_packs_the_over_budget_set() {
    let block = produce_one_below_activation().await;
    assert_eq!(block.height, ACTIVATION - 1);
    assert!(!feature_active(
        ConsensusFeature::VerifyUnitPricing,
        block.height
    ));
    let units = VerifyPricing::CALIBRATED.units(&block_verify_work(&block.transactions));
    assert!(
        units.0 > VerifyPricing::CALIBRATED.block_verify_budget,
        "below-activation fixture must be over the future budget: {units}"
    );
    assert_eq!(
        block.transactions.len() - 1,
        PRODUCER_TXS,
        "M3: below activation the producer must not apply the verify budget"
    );
}

/// M4 regression: the real applier must accept the same over-budget block
/// below activation. The block is expanded independently so this assertion
/// does not depend on the producer dormancy assertion above.
#[tokio::test]
async fn applier_below_activation_accepts_an_over_budget_block() {
    let produced = produce_one_below_activation().await;
    let mut block = produced.clone();
    block.transactions.truncate(1);
    block.transactions.extend((0..20).map(maximal_reward_claim));
    let units = VerifyPricing::CALIBRATED.units(&block_verify_work(&block.transactions));
    assert!(
        units.0 > VerifyPricing::CALIBRATED.block_verify_budget,
        "below-activation applier fixture must be over the future budget: {units}"
    );

    let mut verifier = ProofOfPublishing::new(1, 60, 0, 64);
    verifier.declare_no_genesis_oracles();
    let accepted = verifier
        .verify_block(&block, &FxHashSet::default(), 0, &[])
        .await
        .expect("below-activation verification must return a consensus verdict");
    assert!(
        accepted,
        "M4: below activation the applier must not refuse an over-budget block"
    );
}

/// B1 regression: drive the real producer with a mempool whose total verify
/// work is well over budget. It must seal an in-budget block and leave the
/// deferred transactions available so the next call also advances the chain.
///
/// M2 (delete the producer's budget-aware selection) makes the first
/// `create_next_block` return `Err` at the retained whole-block invariant, so
/// the `expect` below is the distinct mutation assertion for the producer gate.
#[tokio::test]
async fn production_skips_over_budget_transactions_without_wedging() {
    install_once();

    let hostile: Vec<Transaction> = (0..PRODUCER_TXS).map(heavy_regular_transaction).collect();
    let units = VerifyPricing::CALIBRATED.units(&block_verify_work(&hostile));
    assert!(
        units.0 > VerifyPricing::CALIBRATED.block_verify_budget,
        "fixture must actually exceed the budget: {units}"
    );

    assert!(feature_active(
        ConsensusFeature::VerifyUnitPricing,
        ACTIVATION
    ));
    let mut chain = Blockchain::default();
    chain.height = ACTIVATION - 1;
    let chain = Arc::new(RwLock::new(chain));
    let mempool = Arc::new(RwLock::new(producer_pool()));
    {
        let mut pool = mempool.write().await;
        for tx in hostile {
            pool.add_transaction(tx)
                .expect("fixture with a real output must be admitted");
        }
    }

    let mut pop = ProofOfPublishing::new(1, 60, 0, 64);
    pop.declare_no_genesis_oracles();

    let before = mempool.read().await.size();
    let first = pop
        .create_next_block(chain.clone(), mempool.clone(), "wrf-2143-miner")
        .await
        .expect("M2: budget-aware producer must seal the first in-budget block");
    let first_selected = first.transactions.len().saturating_sub(1);
    let first_units = VerifyPricing::CALIBRATED.units(&block_verify_work(&first.transactions));
    let after_first = mempool.read().await.size();
    assert!(first.hash.is_some(), "the first block must be sealed");
    assert_eq!(chain.read().await.height, ACTIVATION);
    assert!(first_selected > 0, "the budget must not starve production");
    assert!(
        first_selected < before,
        "the over-budget remainder must be deferred"
    );
    assert!(first_units.0 <= VerifyPricing::CALIBRATED.block_verify_budget);
    assert_eq!(after_first, before - first_selected);

    let second = pop
        .create_next_block(chain.clone(), mempool.clone(), "wrf-2143-miner")
        .await
        .expect("the deferred remainder must not wedge the next production attempt");
    let second_selected = second.transactions.len().saturating_sub(1);
    let second_units = VerifyPricing::CALIBRATED.units(&block_verify_work(&second.transactions));
    let after_second = mempool.read().await.size();
    assert!(
        second.hash.is_some(),
        "the second block must also be sealed"
    );
    assert_eq!(chain.read().await.height, ACTIVATION + 1);
    assert!(second_selected > 0, "the second attempt must make progress");
    assert!(second_units.0 <= VerifyPricing::CALIBRATED.block_verify_budget);
    assert_eq!(after_second, after_first - second_selected);
}

/// B2 regression: the real applier accepts the producer's in-budget block and
/// refuses the same valid block shape when enough otherwise-admissible claims
/// are added to cross the verify budget.
///
/// M1 (delete the early `verify_block` budget gate) makes the final refusal
/// assertion fail because malformed claims are fail-expensive for pricing but
/// carry no parseable MLSAG for the later verifier to reject.
#[tokio::test]
async fn verify_block_accepts_within_budget_and_refuses_over_budget() {
    install_once();

    let claims: Vec<Transaction> = (0..20).map(maximal_reward_claim).collect();
    let mut chain = Blockchain::default();
    chain.height = ACTIVATION - 1;
    let chain = Arc::new(RwLock::new(chain));
    let mempool = Arc::new(RwLock::new(producer_pool()));
    {
        let mut pool = mempool.write().await;
        for tx in (0..PRODUCER_TXS).map(heavy_regular_transaction) {
            pool.add_transaction(tx)
                .expect("fixture with a real output must be admitted");
        }
    }

    let mut producer = ProofOfPublishing::new(1, 60, 0, 64);
    producer.declare_no_genesis_oracles();
    let within_budget = producer
        .create_next_block(chain, mempool, "wrf-2143-miner")
        .await
        .expect("producer must build the in-budget verification fixture");

    let oracle_set = FxHashSet::default();
    let mut verifier = ProofOfPublishing::new(1, 60, 0, 64);
    verifier.declare_no_genesis_oracles();
    assert!(
        verifier
            .verify_block(&within_budget, &oracle_set, 0, &[])
            .await
            .expect("within-budget verification must not error"),
        "the real applier must accept the producer's within-budget block"
    );

    let mut over_budget = within_budget.clone();
    over_budget.transactions.truncate(1);
    over_budget.transactions.extend(claims);
    let over_units = VerifyPricing::CALIBRATED.units(&block_verify_work(&over_budget.transactions));
    assert!(
        over_units.0 > VerifyPricing::CALIBRATED.block_verify_budget,
        "applier fixture must actually cross the budget: {over_units}"
    );
    assert!(
        !verifier
            .verify_block(&over_budget, &oracle_set, 0, &[])
            .await
            .expect("over-budget verification must return a consensus verdict"),
        "M1: the real applier must refuse an over-budget block"
    );
}

/// #3093 interaction: a real consensus rule must be a known feature, so a
/// binary without it halts cleanly at the activation instead of accepting
/// blocks it prices differently.
#[test]
fn the_feature_name_is_registered_so_an_old_binary_halts_rather_than_forks() {
    let name = ConsensusFeature::VerifyUnitPricing.name();
    assert_eq!(name, "verify_unit_pricing");
    assert!(
        KNOWN_FEATURES.contains(&name),
        "a scheduled consensus rule this binary implements must be in KNOWN_FEATURES, \
         or #3093 would halt this binary at its own feature's activation"
    );
    // This binary implements it, so a genesis scheduling it demands no upgrade.
    assert_eq!(
        upgrade_required_in_schedule(&format!("0:v1,{ACTIVATION}:{name}")),
        None
    );
    // Every enum name must round-trip, so the schedule string and the enum
    // cannot drift.
    for feature in ConsensusFeature::ALL {
        assert_eq!(ConsensusFeature::parse(feature.name()), Some(*feature));
        assert!(
            KNOWN_FEATURES.contains(&feature.name()),
            "{}",
            feature.name()
        );
    }
    assert_eq!(KNOWN_FEATURES.len(), ConsensusFeature::ALL.len());
}

/// The vector, stated as a test: before this story a maximal ring and a minimal
/// one cost a sender the same.
#[test]
fn a_maximal_ring_is_priced_far_above_a_minimal_one() {
    let pricing = VerifyPricing::CALIBRATED;
    // Both sides WELL-FORMED, so the only thing that varies is the ring — the
    // dimension the story is about. An earlier version compared a malformed
    // payload against a hand-edited work struct, which varied the range-proof
    // dimension at the same time and measured the wrong thing.
    let maximal = pricing.units(&well_formed_reward_claim(MAX_RING_SIZE as usize).verify_work());
    let minimal = pricing.units(&well_formed_reward_claim(1).verify_work());
    assert!(
        maximal.0 > minimal.0 * 4,
        "a {MAX_RING_SIZE}-member ring must cost far more than a 1-member one: \
         {maximal} vs {minimal}"
    );
    // The ring term must DOMINATE at a maximal ring, or the constant pricing
    // the attacker-controlled dimension has stopped mattering.
    let work = well_formed_reward_claim(MAX_RING_SIZE as usize).verify_work();
    assert!(
        (work.ring_members as u64) * pricing.per_ring_sig_member
            > (work.range_proofs as u64) * pricing.per_range_proof,
        "the ring term must dominate at a maximal ring, not be swamped by proofs"
    );
}
