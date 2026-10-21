// TDD-6.1 conversion: runnable slashing integration test.
//
// Run from $HOME/wrfcoin/core4:
//   cargo test --test slashing_integration -- --nocapture

use std::sync::Arc;

use rand::SeedableRng;
use rand::rngs::StdRng;
use tokio::sync::RwLock;

use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::state::{
    AuditEventKind, QualityPolicyEventType, StateTransition,
};
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
use wrfcoin_shared_protocol::security::key_registry::SigningRole;
use wrfcoin_shared_protocol::weather::validation::DataQualityBand;

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
        .fold(36_000u64, |seed, byte| seed.saturating_add(byte as u64));
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

#[tokio::test]
async fn tdd_6_1_slashing_integration_low_quality_streak() {
    let mut pop = ProofOfPublishing::new(1, 60, 0, 16);
    // #2523: a test chain with no oracle ceremony must SAY so — silence at
    // height 1 is refused, because it cannot be told from a misconfigured cut.
    pop.declare_no_genesis_oracles();

    let mut chain = Blockchain::default();
    chain
        .state
        .governance
        .params
        .data_quality_policy
        .low_quality_slash_streak = 2;
    chain
        .state
        .governance
        .params
        .data_quality_policy
        .low_quality_slash_amount = 5;
    // Keep Low-band reveals in blocks so slashing can trigger in the mined path.
    chain
        .state
        .governance
        .params
        .data_quality_policy
        .quality_enforcement_activation_height = u64::MAX;

    let (oracle_a_sk, oracle_a_id) = oracle_identity(36_100);
    let (oracle_b_sk, oracle_b_id) = oracle_identity(36_200);

    chain
        .apply_state_transition(StateTransition::SetOracleConfig {
            oracle_set: vec![oracle_a_id.clone(), oracle_b_id.clone()],
            oracle_quorum: 2,
        })
        .expect("set oracle quorum");
    for oracle_id in [&oracle_a_id, &oracle_b_id] {
        chain
            .apply_state_transition(StateTransition::RegisterSigningKeyEpoch {
                identity: oracle_id.clone(),
                role: SigningRole::Oracle,
                epoch: 1,
                public_key: oracle_id.clone(),
                activated_at: 1_700_000_000,
            })
            .expect("register oracle signing key");
    }

    let contributor = addr(42);
    chain.state.create_account(contributor, Balance::new(100));

    let station_id = "slashing-station-phase3c".to_string();
    chain
        .state
        .governance
        .params
        .contributor_tier_policy
        .trusted_provider_station_ids
        .insert(station_id.clone());

    // #2460/#2508: unregistered stations yield Unclassified commits, which are
    // reward-ineligible by fail-closed design — and the slashing path under
    // test only fires on reward-eligible contributions. Register as Real via
    // the production API.
    {
        use wrfcoin_shared_protocol::station_registry::{
            ContributionProvenanceClass, DataQualityTier, StationLocation, WeatherStation,
        };
        chain
            .state
            .register_weather_station(WeatherStation {
                station_id: station_id.clone(),
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

    let blockchain = Arc::new(RwLock::new(chain));
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));

    for round in 0..2u64 {
        let ts = 1_700_220_000i64 + (round as i64 * 10);
        let (commit_tx, mut reveal_tx, commit_id) =
            build_commit_and_reveal(&station_id, ts, 0.20, &format!("r{round}"));
        reveal_tx.sender_address = contributor;
        reveal_tx.fee = 1;

        let att_1 = signed_attestation_tx(
            &commit_id,
            &oracle_a_sk,
            &oracle_a_id,
            ts + 1,
            &format!("r{round}-a"),
        );
        let att_2 = signed_attestation_tx(
            &commit_id,
            &oracle_b_sk,
            &oracle_b_id,
            ts + 2,
            &format!("r{round}-b"),
        );

        {
            let mut mp = mempool.write().await;
            let signing_keys = { blockchain.read().await.state.signing_keys.clone() };
            for attestation_tx in [&att_1, &att_2] {
                let payload = attestation_tx
                    .parse_oracle_attestation_payload()
                    .expect("attestation payload");
                assert!(signing_keys.has_identity_role(&payload.oracle_id, SigningRole::Oracle));
                assert!(signing_keys.verify_signature_for_identity_role(
                    &payload.oracle_id,
                    SigningRole::Oracle,
                    payload.timestamp,
                    payload.signing_message().as_bytes(),
                    payload.signature.as_deref().expect("attestation signature"),
                ));
            }
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

        assert_eq!(block.height, round + 1);
        assert_eq!(
            block
                .transactions
                .iter()
                .filter(|tx| tx.tx_type == TransactionType::ContributionReveal)
                .count(),
            1,
            "the low-quality reveal must be retained with registered oracle quorum"
        );
        assert_eq!(
            block
                .metadata
                .get("oracle_reward_quorum_commit_count")
                .map(String::as_str),
            Some("1"),
            "the mined reveal must reach the reward-processing quorum"
        );

        let chain_guard = blockchain.read().await;
        let balance = chain_guard
            .state
            .get_account(&contributor)
            .expect("contributor account must exist")
            .balance;

        if round == 0 {
            assert_eq!(balance, 100, "first low-quality event should not slash yet");
        } else {
            assert_eq!(
                balance, 95,
                "second consecutive low-quality event should slash by 5"
            );
            assert_eq!(
                block
                    .metadata
                    .get("quality_slashing_applied_count")
                    .map(String::as_str),
                Some("1"),
                "block metadata should record one applied slash"
            );
        }
    }

    let chain_guard = blockchain.read().await;
    let events = chain_guard.state.quality_policy_events();
    let slashing_event = events
        .iter()
        .find(|event| event.event_type == QualityPolicyEventType::SlashingApplied)
        .expect("slashing event must be recorded after streak threshold is reached");

    assert_eq!(slashing_event.quality_band, DataQualityBand::Low);
    assert_eq!(slashing_event.slashed_amount, Some(Balance::new(5)));
    assert!(
        chain_guard
            .state
            .audit_events()
            .iter()
            .any(|event| event.kind == AuditEventKind::QualitySlashingApplied),
        "unified audit log must include the slashing event"
    );

    println!(
        "[TDD-6.1 PASS] slashing applied after low-quality streak; final balance={} events={}",
        chain_guard
            .state
            .get_account(&contributor)
            .expect("contributor account")
            .balance,
        events.len()
    );
}
