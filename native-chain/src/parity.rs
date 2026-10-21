use crate::blockchain::token_emission::TokenEmissionManager;
use crate::blockchain::transaction::{
    CommitPayload, Transaction, TransactionInput, TransactionOutput, TransactionType,
};
use crate::config::settings::Config;
use crate::consensus::proof_of_publishing::{
    ProofOfPublishing, PublishingRole, adjusted_block_reward,
};
use crate::crypto::hash::hash_string;
use crate::crypto::signature::PrivateKey;
use crate::mempool::pool::Mempool;
use rand::{SeedableRng, rngs::StdRng};
use std::str::FromStr;

// Legacy modules use std HashMap – allow it for parity comparisons only.
#[allow(clippy::disallowed_types)]
use std::collections::HashMap as StdHashMap;

// Legacy source files imported via #[path] contain dead code and unused imports
// relative to the native-chain crate context. Suppress these warnings since we
// only need a subset of their API for parity testing.
#[allow(unused_imports, dead_code)]
#[path = "legacy/pool.rs"]
mod legacy_pool;
#[allow(unused_imports, dead_code)]
#[path = "legacy/proof_of_publishing.rs"]
mod legacy_pop;
#[allow(unused_imports, dead_code)]
#[path = "legacy/settings.rs"]
mod legacy_settings;
#[allow(unused_imports, dead_code)]
#[path = "legacy/token_emission.rs"]
mod legacy_token_emission;

fn build_commit_tx(tag: &str, commit_id: &str) -> Transaction {
    let seed = tag.bytes().fold(55u64, |acc, b| acc.saturating_add(b as u64));
    let mut rng = StdRng::seed_from_u64(seed);
    let private_key = PrivateKey::generate(&mut rng);
    let mut payload = CommitPayload {
        commit_id: commit_id.to_string(),
        station_id: format!("station-{tag}"),
        timestamp: 1_700_000_000,
        contribution_type: "contribution".to_string(),
        auth_pubkey: None,
        auth_signature: None,
    };
    payload.sign_authorization(&private_key);
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
    tx.tx_type = TransactionType::ContributionCommit;
    tx.data = Some(serde_json::to_string(&payload).unwrap());
    tx.set_id();
    tx
}

#[test]
fn config_default_parity_with_legacy() {
    let shared = Config::default();
    let legacy = legacy_settings::Config::default();

    assert_eq!(shared.chain_id, legacy.chain_id);
    assert_eq!(shared.network, legacy.network);
    assert_eq!(shared.database_path, legacy.database_path);
    assert_eq!(shared.p2p_port, legacy.p2p_port);
    assert_eq!(shared.api_host, legacy.api_host);
    assert_eq!(shared.api_port, legacy.api_port);
    assert_eq!(shared.mining_difficulty, legacy.mining_difficulty);
    assert_eq!(
        shared.block_time_target_seconds,
        legacy.block_time_target_seconds
    );
    assert_eq!(shared.initial_block_reward, legacy.initial_block_reward);
    assert_eq!(shared.max_block_size, legacy.max_block_size);
    assert_eq!(shared.max_transaction_size, legacy.max_transaction_size);
    assert_eq!(
        shared.max_transactions_per_block,
        legacy.max_transactions_per_block
    );
    assert_eq!(shared.cross_chain_validators, legacy.cross_chain_validators);
    assert_eq!(
        shared.cross_chain_min_signatures,
        legacy.cross_chain_min_signatures
    );
    assert_eq!(shared.enable_cross_chain, legacy.enable_cross_chain);
}

#[test]
fn mempool_launch_replay_parity_with_legacy() {
    let config = Config::default();
    let mut shared_pool = Mempool::new(&config);
    let mut legacy_pool = legacy_pool::Mempool::new(&config);

    let tx1 = build_commit_tx("parity-1", "commit-parity");
    shared_pool.add_transaction(tx1.clone()).unwrap();
    legacy_pool.add_transaction(tx1.clone()).unwrap();

    assert_eq!(shared_pool.size(), legacy_pool.size());

    let tx2 = build_commit_tx("parity-2", "commit-parity");
    let shared_err = shared_pool
        .add_transaction(tx2.clone())
        .expect_err("shared pool should reject replay");
    let legacy_err = legacy_pool
        .add_transaction(tx2)
        .expect_err("legacy pool should reject replay");

    assert!(
        shared_err.to_string().contains("replay"),
        "shared unexpected error: {shared_err}"
    );
    assert!(
        legacy_err.to_string().contains("replay"),
        "legacy unexpected error: {legacy_err}"
    );
}

#[test]
fn token_emission_parity_with_legacy() {
    let mut shared = TokenEmissionManager::default();
    let mut legacy = legacy_token_emission::TokenEmissionManager::new(250, 0.98, 1_000);

    shared.set_emission_throttle(0.87);
    legacy.set_emission_throttle(0.87);
    shared.set_accuracy_multiplier(1.25);
    legacy.set_accuracy_multiplier(1.25);
    shared.set_reputation_multiplier(0.9);
    legacy.set_reputation_multiplier(0.9);
    // 200 WRF in base units, matching MAX_BLOCK_EMISSION (#2485). Was 999_999 —
    // 0.00999 WRF — which is below a single genesis block reward, so both
    // implementations saturated at the cap and the comparison below asserted
    // nothing about their arithmetic.
    shared.set_max_emission_per_block(20_000_000_000);
    legacy.set_max_emission_per_block(20_000_000_000);

    let shared_dist = shared.distribute_role_rewards(10_000);
    let legacy_dist = legacy.distribute_role_rewards(10_000);
    // Compare as sorted vecs since FxHashMap != HashMap
    let mut shared_vec: Vec<_> = shared_dist
        .into_iter()
        .map(|(k, v)| (k.to_str().to_string(), v))
        .collect();
    let mut legacy_vec: Vec<_> = legacy_dist
        .into_iter()
        .map(|(k, v)| (k.to_str().to_string(), v))
        .collect();
    shared_vec.sort();
    legacy_vec.sort();
    assert_eq!(shared_vec, legacy_vec);

    assert_eq!(
        shared.apply_adjustments(500, Some("region-a"), Some(0.8), Some(0.6)),
        legacy.apply_adjustments(500, Some("region-a"), Some(0.8), Some(0.6)),
    );

    // The shared path applies `emission_throttle`; the legacy path does not.
    //
    // This was invisible until #2485. `adjusted_block_reward` returned 2 at
    // h=25_000 and 0 at both later heights, so the old `abs_diff(...) <= 1`
    // tolerance — nominally "the #1020 f64-to-BPS rounding change" — was really
    // just comparing 2 against 2 and 0 against 0. A systematic 13% difference
    // fits inside ±1 when both operands are 2.
    //
    // At base-unit magnitudes the ratio is exactly the throttle, at every height
    // and for both reward kinds, to within one unit of truncation:
    //
    //   h=25_000   contribution shared=106_031_249  legacy=121_875_000
    //   h=210_000  contribution shared= 10_603_124  legacy= 12_187_500
    //   h=420_000  contribution shared=  5_301_561  legacy=  6_093_750
    //   (forecast likewise: 130_499_999 vs 150_000_000, etc.)
    //
    // So the relationship is asserted rather than the equality. This is strictly
    // stronger than the old check: it pins WHY the two differ, and fails if
    // either side starts or stops applying the throttle. Which behaviour is
    // correct is a question about retired code — `legacy_token_emission` is
    // reachable only from this harness — and is tracked on #2485 rather than
    // decided here.
    const THROTTLE_BPS: u64 = 8_700; // set_emission_throttle(0.87), in bps
    let throttled = |legacy_value: u64| legacy_value * THROTTLE_BPS / 10_000;

    for height in [25_000_u64, 210_000, 420_000] {
        let consensus_reward = adjusted_block_reward(height);
        assert!(
            consensus_reward > 0,
            "height {height} must have a non-zero reward for this parity check to \
             assert anything (#2485)"
        );

        let shared_contribution =
            shared.calculate_contribution_reward(consensus_reward, 6.5, 0.9, None);
        let legacy_contribution =
            legacy.calculate_contribution_reward(height, 6.5, 0.9, Some(consensus_reward));
        assert!(
            shared_contribution.abs_diff(throttled(legacy_contribution)) <= 1,
            "contribution reward is not legacy x throttle at height {height}: \
             shared={shared_contribution}, legacy={legacy_contribution}, \
             expected≈{}",
            throttled(legacy_contribution),
        );

        let shared_forecast =
            shared.calculate_forecast_skill_reward(consensus_reward, 0.8, 0.9, None);
        let legacy_forecast =
            legacy.calculate_forecast_skill_reward(height, 0.8, 0.9, Some(consensus_reward));
        assert!(
            shared_forecast.abs_diff(throttled(legacy_forecast)) <= 1,
            "forecast reward is not legacy x throttle at height {height}: \
             shared={shared_forecast}, legacy={legacy_forecast}, expected≈{}",
            throttled(legacy_forecast),
        );
    }
}

#[test]
fn consensus_role_and_reward_policy_parity_with_legacy() {
    for role in ["collector", "analyzer", "keeper", "offsetter"] {
        let shared_role = PublishingRole::from_str(role)
            .expect("shared role should parse")
            .to_str()
            .to_string();
        let legacy_role = legacy_pop::PublishingRole::from_str(role)
            .expect("legacy role should parse")
            .to_str()
            .to_string();
        assert_eq!(shared_role, legacy_role);
    }

    let mut shared = ProofOfPublishing::new(1, 10, 2_000_000, 128);
    let mut legacy = legacy_pop::ProofOfPublishing::new(1, 10, 2.0, 128);

    let mut invalid_shared = std::collections::BTreeMap::new();
    invalid_shared.insert(PublishingRole::Analyzer, 0.6);
    invalid_shared.insert(PublishingRole::Collector, 0.1);
    let mut invalid_legacy = StdHashMap::new();
    invalid_legacy.insert(legacy_pop::PublishingRole::Analyzer, 0.6);
    invalid_legacy.insert(legacy_pop::PublishingRole::Collector, 0.1);
    assert!(shared.set_role_reward_percentages(invalid_shared).is_err());
    assert!(legacy.set_role_reward_percentages(invalid_legacy).is_err());

    let mut valid_shared = std::collections::BTreeMap::new();
    valid_shared.insert(PublishingRole::Analyzer, 0.35);
    valid_shared.insert(PublishingRole::Collector, 0.30);
    valid_shared.insert(PublishingRole::Keeper, 0.25);
    valid_shared.insert(PublishingRole::Offsetter, 0.10);
    let mut valid_legacy = StdHashMap::new();
    valid_legacy.insert(legacy_pop::PublishingRole::Analyzer, 0.35);
    valid_legacy.insert(legacy_pop::PublishingRole::Collector, 0.30);
    valid_legacy.insert(legacy_pop::PublishingRole::Keeper, 0.25);
    valid_legacy.insert(legacy_pop::PublishingRole::Offsetter, 0.10);
    assert!(shared.set_role_reward_percentages(valid_shared).is_ok());
    assert!(legacy.set_role_reward_percentages(valid_legacy).is_ok());
}
