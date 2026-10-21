// TDD-6.2 conversion: runnable reward integration test.
//
// Run from $HOME/wrfcoin/core4:
//   cargo test --test reward_integration -- --nocapture

use std::sync::Arc;

use rand::SeedableRng;
use rand::rngs::StdRng;
use tokio::sync::RwLock;
use wrfcoin_reputation_engine::SCORE_SCALE;

use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::state::{ContributionRewardRecord, StateTransition};
use wrfcoin_shared_protocol::blockchain::transaction::{
    CommitPayload, OracleAttestationPayload, RevealPayload, Transaction, TransactionInput,
    TransactionOutput, TransactionType,
};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::{
    ProofOfPublishing,
};
use wrfcoin_shared_protocol::crypto::hash::hash_string;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::mempool::pool::Mempool;
use wrfcoin_shared_protocol::security::key_registry::SigningRole;

fn addr(byte: u8) -> Address {
    Address([byte; 20])
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

fn build_commit_and_reveal(
    station_id: &str,
    timestamp: i64,
    quality: f64,
    tag: &str,
) -> (Transaction, Transaction, String) {
    let payload = serde_json::json!({"quality": quality, "volume": 1.0, "uniqueness": 1.0});
    let nonce = format!("nonce-{tag}");
    let commit_id =
        Transaction::compute_commit_id(&payload, &nonce, station_id, timestamp, "contribution");

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

    let mut commit_tx = build_tx(
        TransactionType::ContributionCommit,
        &format!("commit-{tag}"),
    );
    commit_tx.data =
        Some(serde_json::to_string(&commit_payload).expect("serialize commit payload"));
    commit_tx.set_id();

    let mut reveal_tx = build_tx(
        TransactionType::ContributionReveal,
        &format!("reveal-{tag}"),
    );
    reveal_tx.data =
        Some(serde_json::to_string(&reveal_payload).expect("serialize reveal payload"));
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

    let mut tx = build_tx(TransactionType::OracleAttestation, &format!("att-{tag}"));
    tx.data = Some(serde_json::to_string(&payload).expect("serialize attestation payload"));
    tx.set_id();
    tx
}

async fn run_reward_case(
    pop: &mut ProofOfPublishing,
    rep_score: u64,
    contributor: Address,
    station_id: &str,
    oracle_a_sk: &PrivateKey,
    oracle_a_id: &str,
    oracle_b_sk: &PrivateKey,
    oracle_b_id: &str,
    tag: &str,
) -> RewardCase {
    let mut chain = Blockchain::default();
    // Keep trust class identical so reward delta comes from reputation score.
    chain
        .state
        .governance
        .params
        .contributor_tier_policy
        .trusted_provider_station_ids
        .insert(station_id.to_string());

    // #2460/#2508: commits from stations absent from station_registry are
    // Unclassified, which is reward-ineligible by fail-closed design. Register
    // through the production API so the commit inherits Real provenance.
    {
        use wrfcoin_shared_protocol::station_registry::{
            ContributionProvenanceClass, DataQualityTier, StationLocation, WeatherStation,
        };
        chain
            .state
            .register_weather_station(WeatherStation {
                station_id: station_id.to_string(),
                owner_address: hex::encode(contributor.0),
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

    chain
        .apply_state_transition(StateTransition::SetOracleConfig {
            oracle_set: vec![oracle_a_id.to_string(), oracle_b_id.to_string()],
            oracle_quorum: 2,
        })
        .expect("set oracle quorum");
    for oracle_id in [oracle_a_id, oracle_b_id] {
        chain
            .apply_state_transition(StateTransition::RegisterSigningKeyEpoch {
                identity: oracle_id.to_string(),
                role: SigningRole::Oracle,
                epoch: 1,
                public_key: oracle_id.to_string(),
                activated_at: 1_700_000_000,
            })
            .expect("register oracle signing key");
    }

    // Seed reputation under both contributor and oracle IDs so the test remains
    // valid if reward keying is contributor-based or oracle-based.
    chain
        .state
        .oracle_registry_mut()
        .set_reputation_score(&hex::encode(contributor.0), rep_score);
    chain
        .state
        .oracle_registry_mut()
        .set_reputation_score(oracle_a_id, rep_score);
    chain
        .state
        .oracle_registry_mut()
        .set_reputation_score(oracle_b_id, rep_score);

    chain.state.create_account(contributor, Balance::new(0));

    let blockchain = Arc::new(RwLock::new(chain));
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));

    let ts = 1_700_330_000i64;
    let (commit_tx, mut reveal_tx, commit_id) = build_commit_and_reveal(station_id, ts, 1.0, tag);
    reveal_tx.sender_address = contributor;
    reveal_tx.fee = 1;

    let att_1 = signed_attestation_tx(
        &commit_id,
        oracle_a_sk,
        oracle_a_id,
        ts + 1,
        &format!("{tag}-a"),
    );
    let att_2 = signed_attestation_tx(
        &commit_id,
        oracle_b_sk,
        oracle_b_id,
        ts + 2,
        &format!("{tag}-b"),
    );

    {
        let mut mp = mempool.write().await;
        let signing_keys = { blockchain.read().await.state.signing_keys.clone() };
        mp.add_transaction(commit_tx).expect("enqueue commit");
        mp.add_transaction(reveal_tx).expect("enqueue reveal");
        mp.add_transaction_with_signing_keys(att_1, Some(&signing_keys))
            .expect("enqueue attestation 1");
        mp.add_transaction_with_signing_keys(att_2, Some(&signing_keys))
            .expect("enqueue attestation 2");
    }

    let block = pop
        .create_next_block(
            blockchain.clone(),
            mempool.clone(),
            "0x1111111111111111111111111111111111111111",
        )
        .await
        .expect("create PoP block");

    let minted_count = block
        .metadata
        .get("rewards_minted_count")
        .and_then(|raw| raw.parse::<u64>().ok())
        .unwrap_or_default();
    assert_eq!(
        minted_count, 1,
        "expected one minted contribution reward in block metadata"
    );

    // The DECIDED reward is what the producer serialized for this commit;
    // the CREDIT is that amount clamped to the per-user window cap (#2509,
    // #2969 a2). Reputation scaling is a sizing property, so it is asserted on
    // the decision; the clamp is asserted on the credit.
    let entries: Vec<ContributionRewardRecord> = block
        .metadata
        .get("reward_entries")
        .map(|raw| serde_json::from_str(raw).expect("reward_entries must be valid JSON"))
        .expect("reward_entries metadata must be present");
    let decided = entries
        .iter()
        .filter(|record| record.commit_id() == Some(commit_id.as_str()))
        .map(|record| record.amount.into_inner())
        .sum::<u128>();

    let chain_guard = blockchain.read().await;
    let account = chain_guard
        .state
        .get_account(&contributor)
        .expect("contributor account must exist");
    RewardCase {
        decided,
        cap: chain_guard.state.emission_for_tests().per_user_emission_cap(block.height),
        earned: account.earned_balance_total,
        balance: account.balance,
    }
}

/// One contributor's reward outcome: the producer's decision and what the
/// state actually credited.
struct RewardCase {
    decided: u128,
    cap: u128,
    earned: Balance,
    balance: Balance,
}

#[tokio::test]
async fn tdd_6_2_reward_integration_high_reputation_out_earns_low_reputation() {
    let mut pop = ProofOfPublishing::new(1, 60, 0, 16);
    // #2523: a test chain with no oracle ceremony must SAY so — silence at
    // height 1 is refused, because it cannot be told from a misconfigured cut.
    pop.declare_no_genesis_oracles();

    let (oracle_a_sk, oracle_a_id) = oracle_identity(51_100);
    let (oracle_b_sk, oracle_b_id) = oracle_identity(51_200);

    let high_score = SCORE_SCALE * 80 / 100;
    let low_score = SCORE_SCALE * 20 / 100;

    let high_case = run_reward_case(
        &mut pop,
        high_score,
        addr(71),
        "reward-station-high-phase3c",
        &oracle_a_sk,
        &oracle_a_id,
        &oracle_b_sk,
        &oracle_b_id,
        "high",
    )
    .await;

    let low_case = run_reward_case(
        &mut pop,
        low_score,
        addr(72),
        "reward-station-low-phase3c",
        &oracle_a_sk,
        &oracle_a_id,
        &oracle_b_sk,
        &oracle_b_id,
        "low",
    )
    .await;

    let high = high_case.decided;
    let low = low_case.decided;
    assert!(
        high > low,
        "expected high reputation to earn more reward: high={} low={}",
        high,
        low
    );

    let required_delta = (low / 2).max(1);
    assert!(
        high >= low.saturating_add(required_delta),
        "expected meaningful reward delta (>=50%+1): high={} low={} required_delta={}",
        high,
        low,
        required_delta
    );

    for (label, case) in [("high", &high_case), ("low", &low_case)] {
        assert_eq!(
            case.earned,
            Balance::new(case.decided.min(case.cap)),
            "the credit is the decided reward clamped to the per-user window cap \
             (#2969 a2) ({label}): decided={} cap={}",
            case.decided,
            case.cap
        );
        assert_eq!(
            case.earned, case.balance,
            "with zero initial funds, earned reward should match final balance ({label})"
        );
    }

    println!(
        "[TDD-6.2 PASS] reputation-weighted reward scaling validated: high={} low={} \
         (credited {} / {}, cap {})",
        high, low, high_case.earned, low_case.earned, high_case.cap
    );
}
