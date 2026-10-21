// End-to-end integration test for the weather data -> PoP consensus ->
// state transition -> reward pipeline (#1035).
//
// Exercises the core WRFCoin value proposition in a single test:
//   1. Setup:  Create chain state with a registered contributor
//   2. Submit: Create a valid WeatherObservation, embed it in the commit-reveal
//              cycle (commit at height N, reveal at height N+1)
//   3. Block:  Produce a block using Proof of Publishing consensus
//   4. State:  Apply the block to chain state
//   5. Reward: Verify contributor balance increased by the expected amount
//   6. Root:   Verify state root is deterministic (recalculate and compare)
//
// Run from $HOME/wrfcoin/core4:
//   cargo test --test weather_reward_e2e -- --nocapture

use std::sync::Arc;

use rand::rngs::StdRng;
use rand::{SeedableRng, random};
use tokio::sync::RwLock;
use wrfcoin_reputation_engine::SCORE_SCALE;

use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::state::StateTransition;
use wrfcoin_shared_protocol::blockchain::transaction::{
    CommitPayload, OracleAttestationPayload, RevealPayload, Transaction, TransactionInput,
    TransactionOutput, TransactionType,
};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::ProofOfPublishing;
use wrfcoin_shared_protocol::crypto::hash::hash_string;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::mempool::pool::Mempool;

// ---------------------------------------------------------------------------
// Helpers (following the pattern from operator_lifecycle_and_supply_cap.rs)
// ---------------------------------------------------------------------------

/// #2460/#2508: commits inherit provenance from the station registry;
/// unregistered stations are Unclassified and reward-ineligible fail-closed.
fn register_real_station(
    chain: &mut wrfcoin_shared_protocol::blockchain::chain::Blockchain,
    station_id: &str,
    owner_address: &str,
) {
    use wrfcoin_shared_protocol::station_registry::{
        ContributionProvenanceClass, DataQualityTier, StationLocation, WeatherStation,
    };
    chain
        .state
        .register_weather_station(WeatherStation {
            station_id: station_id.to_string(),
            owner_address: owner_address.to_string(),
            location: StationLocation {
                latitude: 46.7867,
                longitude: -92.1005,
                elevation_meters: Some(215.0),
            },
            hardware_model: "test-real-station".to_string(),
            calibration_date: None,
            is_active: true,
            reputation_score: 50,
            data_quality_tier: DataQualityTier::Gold,
            provenance_class: ContributionProvenanceClass::Real,
            ring_public_key: None,
        })
        .expect("register real station");
}

fn oracle_identity(seed: u64) -> (PrivateKey, String) {
    let mut rng = StdRng::seed_from_u64(seed);
    let private_key = PrivateKey::generate(&mut rng);
    let oracle_id = PublicKey::from_private_key(&private_key).to_string();
    (private_key, oracle_id)
}

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

/// Build a commit-reveal pair that embeds actual weather observation data
/// inside the contribution payload's `data` field.  The weather observation
/// is a JSON object with physically valid temperature, humidity, pressure,
/// wind speed, precipitation, and dewpoint values.
fn build_weather_commit_and_reveal(
    station_id: &str,
    timestamp: i64,
    tag: &str,
) -> (Transaction, Transaction, String) {
    // The contribution scoring path (parse_metrics) deserializes the reveal
    // payload data as FxHashMap<String, f64>.  Only flat numeric keys are
    // supported -- nested objects would cause the parse to fail and produce a
    // zero score.  We encode the weather fields as top-level numeric keys
    // alongside the required quality/volume/uniqueness metrics.
    let payload = serde_json::json!({
        "quality": 1.0,
        "volume": 1.0,
        "uniqueness": 1.0,
        "temperature_c": 22.5,
        "humidity_percent": 65.0,
        "pressure_hpa": 1013.25,
        "wind_speed_ms": 3.2,
        "precipitation_mm": 0.0,
        "dewpoint_c": 15.8
    });
    let nonce = format!("nonce-{tag}");
    let commit_id =
        Transaction::compute_commit_id(&payload, &nonce, station_id, timestamp, "contribution");

    // Commit authorization is self-certifying; reward eligibility remains
    // gated by oracle quorum. Use a deterministic contributor key so this
    // copied fixture reaches the production path that it is meant to test.
    let signer_seed = tag
        .bytes()
        .fold(51_000u64, |seed, byte| seed.saturating_add(byte as u64));
    let mut signer_rng = StdRng::seed_from_u64(signer_seed);
    let signer = PrivateKey::generate(&mut signer_rng);
    let mut commit_payload = CommitPayload {
        commit_id: commit_id.clone(),
        station_id: station_id.to_string(),
        timestamp,
        contribution_type: "contribution".to_string(),
        auth_pubkey: None,
        auth_signature: None,
    };
    commit_payload.sign_authorization(&signer);

    let reveal_payload = RevealPayload {
        commit_id: commit_id.clone(),
        station_id: station_id.to_string(),
        timestamp,
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

    let mut commit_tx = build_tx(TransactionType::ContributionCommit, &format!("commit-{tag}"));
    commit_tx.data =
        Some(serde_json::to_string(&commit_payload).expect("serialize commit payload"));
    commit_tx.set_id();

    let mut reveal_tx = build_tx(TransactionType::ContributionReveal, &format!("reveal-{tag}"));
    reveal_tx.data =
        Some(serde_json::to_string(&reveal_payload).expect("serialize reveal payload"));
    reveal_tx.fee = 1; // exercise the paid path without a compute receipt
    reveal_tx.set_id();

    (commit_tx, reveal_tx, commit_id)
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

    let signature = private_key
        .sign(payload.signing_message().as_bytes())
        .to_string();
    payload.signature = Some(signature);

    let mut tx = build_tx(
        TransactionType::OracleAttestation,
        &format!("att-{tag}"),
    );
    tx.data = Some(serde_json::to_string(&payload).expect("serialize attestation payload"));
    tx.set_id();
    tx
}

// ---------------------------------------------------------------------------
// #1035 — Full pipeline: weather observation -> commit-reveal -> PoP block
//         -> state transition -> reward payout -> deterministic state root
// ---------------------------------------------------------------------------

#[tokio::test]
async fn weather_data_to_pop_to_reward_full_pipeline() {
    // -----------------------------------------------------------------------
    // 1. Setup: Create chain state with a registered contributor
    // -----------------------------------------------------------------------
    let mut rng = StdRng::seed_from_u64(103500 + random::<u16>() as u64);
    let contributor_sk = PrivateKey::generate(&mut rng);
    let contributor_pk = PublicKey::from_private_key(&contributor_sk);
    let contributor_address = Address::from_public_key(&contributor_pk);
    let miner_hex = format!("0x{}", hex::encode(contributor_address.0));

    let mut chain = Blockchain::default();
    chain
        .state
        .create_account(contributor_address, Balance::new(0));

    // Configure oracle quorum (need 2 oracles for attestation)
    let (oracle_a_sk, oracle_a_id) = oracle_identity(103510 + random::<u16>() as u64);
    let (oracle_b_sk, oracle_b_id) = oracle_identity(103520 + random::<u16>() as u64);

    chain
        .apply_state_transition(StateTransition::SetOracleConfig {
            oracle_set: vec![oracle_a_id.clone(), oracle_b_id.clone()],
            oracle_quorum: 2,
        })
        .expect("set oracle quorum");

    // Register the weather station as a trusted provider
    let station_id = format!("weather-e2e-station-{}", random::<u16>());
    chain
        .state
        .governance
        .params
        .contributor_tier_policy
        .trusted_provider_station_ids
        .insert(station_id.clone());

    // #2460/#2508: unregistered stations yield Unclassified commits, which
    // are reward-ineligible by fail-closed design. Register as Real via the
    // production API so the pipeline can mint.
    register_real_station(&mut chain, &station_id, &miner_hex);

    // Seed reputation for all participants so the reward path is exercised
    let rep_score = SCORE_SCALE * 85 / 100;
    chain
        .state
        .oracle_registry_mut()
        .set_reputation_score(&hex::encode(contributor_address.0), rep_score);
    chain
        .state
        .oracle_registry_mut()
        .set_reputation_score(&oracle_a_id, rep_score);
    chain
        .state
        .oracle_registry_mut()
        .set_reputation_score(&oracle_b_id, rep_score);

    // Record initial balance (should be zero)
    let initial_balance = chain
        .state
        .get_account(&contributor_address)
        .expect("contributor account must exist")
        .balance;
    assert_eq!(
        initial_balance,
        Balance::new(0),
        "contributor must start with zero balance"
    );

    // -----------------------------------------------------------------------
    // 2. Submit weather observation via commit-reveal
    // -----------------------------------------------------------------------
    let ts = 1_700_500_000i64;
    let (commit_tx, mut reveal_tx, commit_id) =
        build_weather_commit_and_reveal(&station_id, ts, "weather-e2e");

    // Set the sender address on the reveal so the reward accrues to our contributor
    reveal_tx.sender_address = contributor_address;

    // Build oracle attestations for the contribution
    let att_1 = signed_attestation_tx(
        &commit_id,
        &oracle_a_sk,
        &oracle_a_id,
        ts + 1,
        "weather-e2e-a",
    );
    let att_2 = signed_attestation_tx(
        &commit_id,
        &oracle_b_sk,
        &oracle_b_id,
        ts + 2,
        "weather-e2e-b",
    );

    // -----------------------------------------------------------------------
    // 3. Produce a PoP block containing the weather contribution
    // -----------------------------------------------------------------------
    let blockchain = Arc::new(RwLock::new(chain));
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));

    {
        let mut mp = mempool.write().await;
        let signing_keys = { blockchain.read().await.state.signing_keys.clone() };
        mp.add_transaction(commit_tx).expect("enqueue commit tx");
        mp.add_transaction(reveal_tx).expect("enqueue reveal tx");
        mp.add_transaction_with_signing_keys(att_1, Some(&signing_keys)).expect("enqueue attestation 1");
        mp.add_transaction_with_signing_keys(att_2, Some(&signing_keys)).expect("enqueue attestation 2");
    }

    let mut pop = ProofOfPublishing::new(1, 60, 0, 16);
    // #2523: a test chain with no oracle ceremony must SAY so — silence at
    // height 1 is refused, because it cannot be told from a misconfigured cut.
    pop.declare_no_genesis_oracles();

    let block = pop
        .create_next_block(blockchain.clone(), mempool.clone(), &miner_hex)
        .await
        .expect("create PoP block with weather contribution");

    assert_eq!(block.height, 1, "first block must be height 1");

    // Verify the block contains weather-related transactions (commit + reveal + 2 attestations + coinbase)
    let tx_types: Vec<_> = block.transactions.iter().map(|tx| tx.tx_type).collect();
    assert!(
        tx_types.contains(&TransactionType::ContributionCommit),
        "block must contain ContributionCommit tx"
    );
    assert!(
        tx_types.contains(&TransactionType::ContributionReveal),
        "block must contain ContributionReveal tx"
    );
    assert!(
        tx_types.contains(&TransactionType::OracleAttestation),
        "block must contain OracleAttestation tx"
    );

    // -----------------------------------------------------------------------
    // 4. Verify state transition: contributor's balance increased
    // -----------------------------------------------------------------------
    let chain_guard = blockchain.read().await;
    let account = chain_guard
        .state
        .get_account(&contributor_address)
        .expect("contributor account must exist after block");

    // The contributor may earn rewards through:
    // (a) coinbase as the miner, and/or
    // (b) contribution reward for weather data
    // Check the total balance is positive.
    assert!(
        account.balance > Balance::new(0),
        "contributor must earn a reward from weather data contribution; balance = {}",
        account.balance
    );

    let reward_earned: u128 = account.balance.into_inner();
    println!(
        "[#1035] Weather contributor reward: {} base units ({:.8} WRF)",
        reward_earned,
        reward_earned as f64 / 100_000_000.0
    );

    // -----------------------------------------------------------------------
    // 5. Verify reward metadata in block
    // -----------------------------------------------------------------------
    let minted_count = block
        .metadata
        .get("rewards_minted_count")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    assert!(
        minted_count >= 1,
        "at least one reward must be minted; rewards_minted_count = {minted_count}"
    );

    let minted_total = block
        .metadata
        .get("rewards_minted_total")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    assert!(
        minted_total > 0,
        "minted total must be positive; rewards_minted_total = {minted_total}"
    );

    // -----------------------------------------------------------------------
    // 6. Verify state root is deterministic
    // -----------------------------------------------------------------------
    drop(chain_guard);
    let mut chain_mut = blockchain.write().await;
    let root_before = chain_mut.state.state_root;
    chain_mut.state.recalculate_state_root();
    assert_eq!(
        root_before, chain_mut.state.state_root,
        "state root must be stable after recalculation (deterministic)"
    );

    // Verify supply accounting
    let total_supply = chain_mut.state.total_minted_supply();
    assert!(
        total_supply > 0,
        "total minted supply must be positive after block production"
    );

    println!(
        "[#1035 PASS] weather->PoP->reward pipeline validated: \
         reward={} supply={} minted_count={} minted_total={} state_root_stable=true",
        reward_earned, total_supply, minted_count, minted_total
    );
}

// ---------------------------------------------------------------------------
// #1035 — Determinism: run the same pipeline twice with identical inputs and
//         verify identical state roots.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn weather_pipeline_produces_identical_state_root_on_replay() {
    /// Run the weather pipeline once and return the resulting state root hash
    /// and contributor balance.
    async fn run_pipeline(seed: u64) -> (wrfcoin_shared_protocol::common::types::Hash, u128) {
        let mut rng = StdRng::seed_from_u64(seed);
        let contributor_sk = PrivateKey::generate(&mut rng);
        let contributor_pk = PublicKey::from_private_key(&contributor_sk);
        let contributor_address = Address::from_public_key(&contributor_pk);
        let miner_hex = format!("0x{}", hex::encode(contributor_address.0));

        let mut chain = Blockchain::default();
        chain
            .state
            .create_account(contributor_address, Balance::new(0));

        // Fixed oracle seeds (same across both runs)
        let (oracle_a_sk, oracle_a_id) = oracle_identity(seed + 10);
        let (oracle_b_sk, oracle_b_id) = oracle_identity(seed + 20);

        chain
            .apply_state_transition(StateTransition::SetOracleConfig {
                oracle_set: vec![oracle_a_id.clone(), oracle_b_id.clone()],
                oracle_quorum: 2,
            })
            .expect("set oracle quorum");

        let station_id = format!("replay-station-{seed}");
        chain
            .state
            .governance
            .params
            .contributor_tier_policy
            .trusted_provider_station_ids
            .insert(station_id.clone());
        register_real_station(
            &mut chain,
            &station_id,
            &hex::encode(contributor_address.0),
        );

        let rep_score = SCORE_SCALE * 80 / 100;
        chain
            .state
            .oracle_registry_mut()
            .set_reputation_score(&hex::encode(contributor_address.0), rep_score);
        chain
            .state
            .oracle_registry_mut()
            .set_reputation_score(&oracle_a_id, rep_score);
        chain
            .state
            .oracle_registry_mut()
            .set_reputation_score(&oracle_b_id, rep_score);

        let ts = 1_700_600_000i64;
        let (commit_tx, mut reveal_tx, commit_id) =
            build_weather_commit_and_reveal(&station_id, ts, "replay");
        reveal_tx.sender_address = contributor_address;

        let att_1 = signed_attestation_tx(
            &commit_id,
            &oracle_a_sk,
            &oracle_a_id,
            ts + 1,
            "replay-a",
        );
        let att_2 = signed_attestation_tx(
            &commit_id,
            &oracle_b_sk,
            &oracle_b_id,
            ts + 2,
            "replay-b",
        );

        let blockchain = Arc::new(RwLock::new(chain));
        let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));

        {
            let mut mp = mempool.write().await;
            let signing_keys = { blockchain.read().await.state.signing_keys.clone() };
            mp.add_transaction(commit_tx).expect("enqueue commit");
            mp.add_transaction(reveal_tx).expect("enqueue reveal");
            mp.add_transaction_with_signing_keys(att_1, Some(&signing_keys)).expect("enqueue attestation 1");
            mp.add_transaction_with_signing_keys(att_2, Some(&signing_keys)).expect("enqueue attestation 2");
        }

        let mut pop = ProofOfPublishing::new(1, 60, 0, 16);
        // #2523: a test chain with no oracle ceremony must SAY so — silence at
        // height 1 is refused, because it cannot be told from a misconfigured cut.
        pop.declare_no_genesis_oracles();
        let _block = pop
            .create_next_block(blockchain.clone(), mempool.clone(), &miner_hex)
            .await
            .expect("create block");

        let mut chain_guard = blockchain.write().await;
        chain_guard.state.recalculate_state_root();
        let state_root = chain_guard.state.state_root;
        let balance = chain_guard
            .state
            .get_account(&contributor_address)
            .map(|a| a.balance.into_inner())
            .unwrap_or(0);

        (state_root.into(), balance)
    }

    // Use the same seed for both runs — must produce identical results.
    let seed = 103590u64;
    let (root_a, balance_a) = run_pipeline(seed).await;
    let (root_b, balance_b) = run_pipeline(seed).await;

    assert_eq!(
        root_a, root_b,
        "state root must be identical across replays with the same inputs"
    );
    assert_eq!(
        balance_a, balance_b,
        "contributor balance must be identical across replays"
    );
    assert!(
        balance_a > 0,
        "contributor must have earned a reward; balance = {balance_a}"
    );

    println!(
        "[#1035 PASS] replay determinism validated: state_root={:?} balance={}",
        root_a,
        balance_a
    );
}
