// End-to-end earn-loop integration test with an explicit oracle reward gate
// (testnet launch confidence — value loop sign-off, human gate #724).
//
// The WRFCoin core value loop is:
//   weather contribution -> COMMIT (on-chain) -> REVEAL (on-chain)
//                        -> reward (GATED on oracle attestation)
//
// Commit + reveal are proven working (#767/#769). The launch-critical property
// that no single existing test asserts end-to-end is the *oracle reward gate*:
// a valid reveal must land on-chain and update state, but NO contribution
// reward may be minted without an oracle quorum attestation. That gate is the
// on-chain enforcement of the #724 "oracle operators required" boundary.
//
// This test drives the full loop against a LOCAL / in-process chain (never the
// live N5), using the same in-process `Blockchain` + `Mempool` +
// `ProofOfPublishing` harness as `weather_reward_e2e.rs` and
// `reward_integration.rs`.
//
// It exercises three things:
//   1. Multi-block commit -> reveal landing on-chain across SEPARATE blocks
//      (realistic node cadence: commit at height N, reveal at height N+1),
//      asserting `state.pending_commits` then `state.pending_reveals` update.
//   2. The oracle GATE: with NO attestation, the reveal still applies to state
//      but `rewards_minted_count == 0` and the contributor earns nothing.
//   3. Positive control: with a signed oracle quorum attestation, an identical
//      loop DOES mint a reward — proving attestation is the only difference
//      between reward and no-reward.
//
// Run from $HOME/wrfcoin/core4:
//   cargo test -p wrfcoin-shared-protocol --test earn_loop_oracle_gate_e2e -- --nocapture

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
use wrfcoin_shared_protocol::station_registry::{
    ContributionProvenanceClass, DataQualityTier, StationLocation, WeatherStation,
};

// ---------------------------------------------------------------------------
// Helpers (mirroring the proven pattern from weather_reward_e2e.rs)
// ---------------------------------------------------------------------------

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

/// Build a commit-reveal pair carrying real weather observation fields plus the
/// quality/volume/uniqueness metrics the scoring path expects. The commit id is
/// derived via the canonical commit-reveal hash contract (SHA3-256 over the
/// sorted-key JSON object + 5-field `||` preimage), exactly as the collector /
/// backend produce it.
fn build_weather_commit_and_reveal(
    station_id: &str,
    timestamp: i64,
    tag: &str,
) -> (CommitPayload, RevealPayload, String) {
    let payload = serde_json::json!({
        "quality": 1.0,
        "volume": 1.0,
        "uniqueness": 1.0,
        "temperature_c": 21.5,
        "humidity_percent": 60.0,
        "pressure_hpa": 1014.0,
        "wind_speed_ms": 2.8,
        "precipitation_mm": 0.0,
        "dewpoint_c": 13.5
    });
    let nonce = format!("nonce-{tag}");
    let commit_id =
        Transaction::compute_commit_id(&payload, &nonce, station_id, timestamp, "contribution");

    let seed = tag.bytes().fold(33u64, |acc, b| acc.saturating_add(b as u64));
    let mut rng = StdRng::seed_from_u64(seed);
    let private_key = PrivateKey::generate(&mut rng);
    let mut commit_payload = CommitPayload {
        commit_id: commit_id.clone(),
        station_id: station_id.to_string(),
        timestamp,
        contribution_type: "contribution".to_string(),
        auth_pubkey: None,
        auth_signature: None,
    };
    commit_payload.sign_authorization(&private_key);

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

    (commit_payload, reveal_payload, commit_id)
}

fn commit_tx_from(payload: &CommitPayload, tag: &str) -> Transaction {
    let mut tx = build_tx(
        TransactionType::ContributionCommit,
        &format!("commit-{tag}"),
    );
    tx.data = Some(serde_json::to_string(payload).expect("serialize commit payload"));
    tx.set_id();
    tx
}

fn reveal_tx_from(payload: &RevealPayload, sender: Address, tag: &str) -> Transaction {
    let mut tx = build_tx(
        TransactionType::ContributionReveal,
        &format!("reveal-{tag}"),
    );
    tx.data = Some(serde_json::to_string(payload).expect("serialize reveal payload"));
    tx.sender_address = sender;
    // Non-zero fee so the reveal is not treated as a feeless weather tx: the
    // compute-fee gate (#821/#1487) drops a zero-fee ContributionReveal that
    // carries no valid ComputeReceipt during block assembly (unless the
    // work-factor floor is 0, the testnet override — not the default test env).
    // Mirrors `reward_integration.rs` and the launch_api parity fixtures. The
    // fee is orthogonal to the oracle reward gate this test asserts.
    tx.fee = 1;
    tx.set_id();
    tx
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

fn metadata_u64(block: &wrfcoin_shared_protocol::blockchain::block::Block, key: &str) -> u64 {
    block
        .metadata
        .get(key)
        .and_then(|raw| raw.parse::<u64>().ok())
        .unwrap_or_else(|| panic!("block metadata must contain numeric '{key}'"))
}

/// Set up a fresh local chain with a registered contributor account, a trusted
/// weather station, an oracle quorum config, and seeded reputation. Returns the
/// shared chain handle, the contributor address, the miner hex, and the oracle
/// identities.
///
/// IMPORTANT: the miner is a DISTINCT address from the contributor. The block
/// producer always earns a (non-gated) coinbase reward; keeping the miner
/// separate means the contributor account earns *only* via the
/// oracle-gated contribution-reward path. That makes the gate assertion
/// (`earned_balance_total == 0` without attestation) exact rather than
/// entangled with mining/coinbase rewards.
fn setup_chain(
    seed: u64,
    station_id: &str,
) -> (
    Arc<RwLock<Blockchain>>,
    Address,
    String,
    (PrivateKey, String),
    (PrivateKey, String),
) {
    setup_chain_with_provenance(seed, station_id, ContributionProvenanceClass::Real)
}

fn setup_chain_with_provenance(
    seed: u64,
    station_id: &str,
    provenance_class: ContributionProvenanceClass,
) -> (
    Arc<RwLock<Blockchain>>,
    Address,
    String,
    (PrivateKey, String),
    (PrivateKey, String),
) {
    let mut rng = StdRng::seed_from_u64(seed);
    let contributor_sk = PrivateKey::generate(&mut rng);
    let contributor_pk = PublicKey::from_private_key(&contributor_sk);
    let contributor_address = Address::from_public_key(&contributor_pk);

    // Distinct miner identity so the coinbase reward never lands on the
    // contributor account (see doc comment above).
    let miner_sk = PrivateKey::generate(&mut rng);
    let miner_pk = PublicKey::from_private_key(&miner_sk);
    let miner_address = Address::from_public_key(&miner_pk);
    let miner_hex = format!("0x{}", hex::encode(miner_address.0));

    let mut chain = Blockchain::default();
    chain
        .state
        .create_account(contributor_address, Balance::new(0));
    chain
        .state
        .register_weather_station(WeatherStation {
            station_id: station_id.to_string(),
            owner_address: contributor_address.to_string(),
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
            provenance_class,
            ring_public_key: None,
        })
        .expect("register canonical real station");

    let oracle_a = oracle_identity(seed + 10);
    let oracle_b = oracle_identity(seed + 20);

    chain
        .apply_state_transition(StateTransition::SetOracleConfig {
            oracle_set: vec![oracle_a.1.clone(), oracle_b.1.clone()],
            oracle_quorum: 2,
        })
        .expect("set oracle quorum");

    chain
        .state
        .governance
        .params
        .contributor_tier_policy
        .trusted_provider_station_ids
        .insert(station_id.to_string());

    let rep_score = SCORE_SCALE * 85 / 100;
    chain
        .state
        .oracle_registry_mut()
        .set_reputation_score(&hex::encode(contributor_address.0), rep_score);
    chain
        .state
        .oracle_registry_mut()
        .set_reputation_score(&oracle_a.1, rep_score);
    chain
        .state
        .oracle_registry_mut()
        .set_reputation_score(&oracle_b.1, rep_score);

    (
        Arc::new(RwLock::new(chain)),
        contributor_address,
        miner_hex,
        oracle_a,
        oracle_b,
    )
}

#[tokio::test]
async fn synthetic_contribution_with_quorum_is_recorded_but_never_paid() {
    let station_id = format!("synthetic-station-{}", random::<u16>());
    let (blockchain, contributor, miner_hex, oracle_a, oracle_b) =
        setup_chain_with_provenance(
            72_700 + random::<u16>() as u64,
            &station_id,
            ContributionProvenanceClass::Synthetic,
        );
    let ts = 1_700_750_000i64;
    let (commit_payload, reveal_payload, commit_id) =
        build_weather_commit_and_reveal(&station_id, ts, "synthetic-quorum");
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let mut pop = ProofOfPublishing::new(1, 60, 0, 16);
    // #2523: a test chain with no oracle ceremony must SAY so — silence at
    // height 1 is refused, because it cannot be told from a misconfigured cut.
    pop.declare_no_genesis_oracles();

    {
        let mut mp = mempool.write().await;
        mp.add_transaction(commit_tx_from(&commit_payload, "synthetic-quorum"))
            .expect("enqueue synthetic commit");
    }
    pop.create_next_block(blockchain.clone(), mempool.clone(), &miner_hex)
        .await
        .expect("mine synthetic commit");

    {
        let mut mp = mempool.write().await;
        let signing_keys = { blockchain.read().await.state.signing_keys.clone() };
        mp.add_transaction(reveal_tx_from(
            &reveal_payload,
            contributor,
            "synthetic-quorum",
        ))
        .expect("enqueue synthetic reveal");
        mp.add_transaction_with_signing_keys(
            signed_attestation_tx(
                &commit_id,
                &oracle_a.0,
                &oracle_a.1,
                ts + 1,
                "synthetic-a",
            ),
            Some(&signing_keys),
        )
        .expect("enqueue synthetic attestation a");
        mp.add_transaction_with_signing_keys(
            signed_attestation_tx(
                &commit_id,
                &oracle_b.0,
                &oracle_b.1,
                ts + 2,
                "synthetic-b",
            ),
            Some(&signing_keys),
        )
        .expect("enqueue synthetic attestation b");
    }
    let reveal_block = pop
        .create_next_block(blockchain.clone(), mempool.clone(), &miner_hex)
        .await
        .expect("mine synthetic reveal with quorum");

    assert_eq!(
        metadata_u64(&reveal_block, "oracle_reward_quorum_commit_count"),
        1
    );
    assert_eq!(metadata_u64(&reveal_block, "rewards_minted_count"), 0);
    assert_eq!(metadata_u64(&reveal_block, "rewards_minted_total"), 0);

    let chain = blockchain.read().await;
    let decision = chain
        .state
        .contribution_reward_records
        .get(&commit_id)
        .expect("synthetic zero decision must be durable");
    assert_eq!(
        decision.provenance_class,
        ContributionProvenanceClass::Synthetic
    );
    assert_eq!(decision.amount, Balance::new(0));
    assert!(
        chain
            .state
            .finalized_contribution_rewards
            .contains(&commit_id)
    );
    assert_eq!(
        chain
            .state
            .get_account(&contributor)
            .expect("contributor account exists")
            .earned_balance_total,
        Balance::new(0)
    );
}

// ---------------------------------------------------------------------------
// 1 + 2. Multi-block commit -> reveal on-chain, then the ORACLE GATE:
//        a valid reveal lands on-chain but mints NO reward without attestation.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn earn_loop_commit_reveal_lands_on_chain_but_reward_gated_without_attestation() {
    let station_id = format!("earn-gate-station-{}", random::<u16>());
    let (blockchain, contributor, miner_hex, _oracle_a, _oracle_b) =
        setup_chain(72_400 + random::<u16>() as u64, &station_id);

    let ts = 1_700_700_000i64;
    let (commit_payload, reveal_payload, commit_id) =
        build_weather_commit_and_reveal(&station_id, ts, "earn-gate");

    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let mut pop = ProofOfPublishing::new(1, 60, 0, 16);
    // #2523: a test chain with no oracle ceremony must SAY so — silence at
    // height 1 is refused, because it cannot be told from a misconfigured cut.
    pop.declare_no_genesis_oracles();

    // ---- Block 1: COMMIT lands on-chain -----------------------------------
    {
        let mut mp = mempool.write().await;
        mp.add_transaction(commit_tx_from(&commit_payload, "earn-gate"))
            .expect("enqueue commit");
    }

    let block1 = pop
        .create_next_block(blockchain.clone(), mempool.clone(), &miner_hex)
        .await
        .expect("produce block 1 with commit");
    assert_eq!(block1.height, 1, "commit block must be height 1");
    assert!(
        block1
            .transactions
            .iter()
            .any(|tx| tx.tx_type == TransactionType::ContributionCommit),
        "block 1 must contain the ContributionCommit tx"
    );

    {
        let chain = blockchain.read().await;
        assert!(
            chain.state.pending_commits.contains_key(&commit_id),
            "commit must be registered on-chain in pending_commits after block 1"
        );
        assert!(
            !chain.state.pending_reveals.contains_key(&commit_id),
            "reveal must not exist yet before block 2"
        );
    }

    // ---- Block 2: REVEAL lands on-chain (NO attestation) ------------------
    {
        let mut mp = mempool.write().await;
        mp.add_transaction(reveal_tx_from(&reveal_payload, contributor, "earn-gate"))
            .expect("enqueue reveal");
    }

    let block2 = pop
        .create_next_block(blockchain.clone(), mempool.clone(), &miner_hex)
        .await
        .expect("produce block 2 with reveal");
    assert_eq!(block2.height, 2, "reveal block must be height 2");
    assert!(
        block2
            .transactions
            .iter()
            .any(|tx| tx.tx_type == TransactionType::ContributionReveal),
        "block 2 must contain the ContributionReveal tx"
    );
    assert!(
        !block2
            .transactions
            .iter()
            .any(|tx| tx.tx_type == TransactionType::OracleAttestation),
        "block 2 must NOT contain any attestation in the gate scenario"
    );

    // Reveal must have applied to state across the two-block boundary.
    {
        let chain = blockchain.read().await;
        assert!(
            chain.state.pending_reveals.contains_key(&commit_id),
            "reveal must be recorded on-chain in pending_reveals after block 2"
        );
        assert!(
            !chain.state.pending_commits.contains_key(&commit_id),
            "the pending commit must be consumed once revealed"
        );
    }

    // ---- THE GATE: a valid on-chain reveal with NO attestation mints NO reward.
    let revealed = metadata_u64(&block2, "oracle_reward_revealed_commit_count");
    let quorum = metadata_u64(&block2, "oracle_reward_quorum_commit_count");
    let missing = metadata_u64(&block2, "oracle_reward_missing_quorum_count");
    let minted_count = metadata_u64(&block2, "rewards_minted_count");
    let minted_total = metadata_u64(&block2, "rewards_minted_total");

    assert_eq!(
        revealed, 1,
        "exactly one commit must have been revealed in block 2"
    );
    assert_eq!(
        quorum, 0,
        "no commit may reach oracle quorum without any attestation"
    );
    assert_eq!(
        missing, 1,
        "the revealed commit must be counted as missing-quorum"
    );
    assert_eq!(
        minted_count, 0,
        "ORACLE GATE: no contribution reward may be minted without attestation"
    );
    assert_eq!(
        minted_total, 0,
        "ORACLE GATE: minted reward total must be zero without attestation"
    );

    // And the contributor must not have earned any contribution reward.
    {
        let chain = blockchain.read().await;
        let account = chain
            .state
            .get_account(&contributor)
            .expect("contributor account must exist");
        assert_eq!(
            account.earned_balance_total,
            Balance::new(0),
            "ORACLE GATE: contributor must earn zero contribution reward without attestation; \
             earned = {}",
            account.earned_balance_total
        );
        let decision = chain
            .state
            .contribution_reward_records
            .get(&commit_id)
            .expect("the unpaid decision must be durable");
        assert_eq!(
            decision.provenance_class,
            ContributionProvenanceClass::Real
        );
        assert!(decision.amount > Balance::new(0));
        assert!(
            !chain
                .state
                .finalized_contribution_rewards
                .contains(&commit_id),
            "a positive real decision remains pending until quorum"
        );
    }

    println!(
        "[earn-gate PASS] commit(h1)+reveal(h2) landed on-chain; reward correctly GATED: \
         revealed={revealed} quorum={quorum} missing={missing} minted_count={minted_count}"
    );
}

// ---------------------------------------------------------------------------
// 3. Positive control: the SAME loop WITH a signed oracle quorum attestation
//    DOES mint a reward — proving attestation is the only difference.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn earn_loop_with_oracle_attestation_mints_reward() {
    let station_id = format!("earn-reward-station-{}", random::<u16>());
    let (blockchain, contributor, miner_hex, oracle_a, oracle_b) =
        setup_chain(72_900 + random::<u16>() as u64, &station_id);

    let ts = 1_700_800_000i64;
    let (commit_payload, reveal_payload, commit_id) =
        build_weather_commit_and_reveal(&station_id, ts, "earn-reward");

    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let mut pop = ProofOfPublishing::new(1, 60, 0, 16);
    // #2523: a test chain with no oracle ceremony must SAY so — silence at
    // height 1 is refused, because it cannot be told from a misconfigured cut.
    pop.declare_no_genesis_oracles();

    // ---- Block 1: COMMIT --------------------------------------------------
    {
        let mut mp = mempool.write().await;
        mp.add_transaction(commit_tx_from(&commit_payload, "earn-reward"))
            .expect("enqueue commit");
    }
    let block1 = pop
        .create_next_block(blockchain.clone(), mempool.clone(), &miner_hex)
        .await
        .expect("produce block 1 with commit");
    assert_eq!(block1.height, 1);
    {
        let chain = blockchain.read().await;
        assert!(chain.state.pending_commits.contains_key(&commit_id));
    }

    // ---- Block 2: REVEAL + quorum of signed oracle attestations -----------
    {
        let mut mp = mempool.write().await;
        let signing_keys = { blockchain.read().await.state.signing_keys.clone() };
        mp.add_transaction(reveal_tx_from(&reveal_payload, contributor, "earn-reward"))
            .expect("enqueue reveal");
        mp.add_transaction_with_signing_keys(
            signed_attestation_tx(&commit_id, &oracle_a.0, &oracle_a.1, ts + 1, "earn-reward-a"),
            Some(&signing_keys),
        )
        .expect("enqueue attestation a");
        mp.add_transaction_with_signing_keys(
            signed_attestation_tx(&commit_id, &oracle_b.0, &oracle_b.1, ts + 2, "earn-reward-b"),
            Some(&signing_keys),
        )
        .expect("enqueue attestation b");
    }

    let block2 = pop
        .create_next_block(blockchain.clone(), mempool.clone(), &miner_hex)
        .await
        .expect("produce block 2 with reveal + attestations");
    assert_eq!(block2.height, 2);

    assert!(
        block2
            .transactions
            .iter()
            .any(|tx| tx.tx_type == TransactionType::ContributionReveal),
        "block 2 must contain the reveal"
    );
    assert!(
        block2
            .transactions
            .iter()
            .any(|tx| tx.tx_type == TransactionType::OracleAttestation),
        "block 2 must contain oracle attestations"
    );

    // Reveal applied on-chain.
    {
        let chain = blockchain.read().await;
        assert!(
            chain.state.pending_reveals.contains_key(&commit_id),
            "reveal must be recorded on-chain in pending_reveals"
        );
    }

    // With quorum attestation, the reward IS minted.
    let quorum = metadata_u64(&block2, "oracle_reward_quorum_commit_count");
    let minted_count = metadata_u64(&block2, "rewards_minted_count");
    let minted_total = metadata_u64(&block2, "rewards_minted_total");

    assert_eq!(
        quorum, 1,
        "the revealed commit must reach oracle quorum with two attestations"
    );
    assert!(
        minted_count >= 1,
        "at least one reward must be minted with quorum attestation; got {minted_count}"
    );
    assert!(
        minted_total > 0,
        "minted reward total must be positive with quorum attestation; got {minted_total}"
    );

    // Contributor must have earned a contribution reward (gate opened).
    {
        let chain = blockchain.read().await;
        let account = chain
            .state
            .get_account(&contributor)
            .expect("contributor account must exist");
        assert!(
            account.earned_balance_total > Balance::new(0),
            "contributor must earn a reward once the oracle gate is satisfied; earned = {}",
            account.earned_balance_total
        );
    }

    println!(
        "[earn-reward PASS] commit(h1)+reveal+attest(h2) landed on-chain; reward MINTED: \
         quorum={quorum} minted_count={minted_count} minted_total={minted_total}"
    );
}

// ---------------------------------------------------------------------------
// 4. LATE QUORUM (#2464) — deferred settlement.
//
//    A reveal at height N records its typed, provenance-bearing reward decision
//    without minting. When quorum reaches the canonical attestation state at
//    N+1, a sorted deferred scan emits the same transition and credits once.
//
//    This test constructs exactly that timing, asserts the detector fires, and
//    proves the contributor is paid through the production block path.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn late_quorum_finalizes_the_durable_reward_decision() {
    let station_id = format!("late-quorum-station-{}", random::<u16>());
    let (blockchain, contributor, miner_hex, oracle_a, oracle_b) =
        setup_chain(73_400 + random::<u16>() as u64, &station_id);

    let ts = 1_700_900_000i64;
    let (commit_payload, reveal_payload, commit_id) =
        build_weather_commit_and_reveal(&station_id, ts, "late-quorum");

    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let mut pop = ProofOfPublishing::new(1, 60, 0, 16);
    // #2523: a test chain with no oracle ceremony must SAY so — silence at
    // height 1 is refused, because it cannot be told from a misconfigured cut.
    pop.declare_no_genesis_oracles();

    // ---- Block 1: COMMIT --------------------------------------------------
    {
        let mut mp = mempool.write().await;
        mp.add_transaction(commit_tx_from(&commit_payload, "late-quorum"))
            .expect("enqueue commit");
    }
    let block1 = pop
        .create_next_block(blockchain.clone(), mempool.clone(), &miner_hex)
        .await
        .expect("produce block 1 with commit");
    assert_eq!(block1.height, 1);
    assert_eq!(
        metadata_u64(&block1, "oracle_reward_late_quorum_count"),
        0,
        "a commit-only block cannot contain a late-quorum arrival"
    );

    // ---- Block 2: REVEAL with NO attestation ------------------------------
    // This creates the durable reward plan, but cannot mint yet.
    {
        let mut mp = mempool.write().await;
        mp.add_transaction(reveal_tx_from(&reveal_payload, contributor, "late-quorum"))
            .expect("enqueue reveal");
    }
    let block2 = pop
        .create_next_block(blockchain.clone(), mempool.clone(), &miner_hex)
        .await
        .expect("produce block 2 with reveal");
    assert_eq!(block2.height, 2);
    assert_eq!(
        metadata_u64(&block2, "oracle_reward_missing_quorum_count"),
        1,
        "the reveal must be counted as missing-quorum at decision time"
    );
    assert_eq!(
        metadata_u64(&block2, "rewards_minted_count"),
        0,
        "no reward may be minted at the decision block without quorum"
    );
    {
        let chain = blockchain.read().await;
        let planned = chain
            .state
            .contribution_reward_records
            .get(&commit_id)
            .expect("block 2 must durably retain the planned reward");
        assert_eq!(
            planned.provenance_class,
            ContributionProvenanceClass::Real
        );
        assert!(planned.amount > Balance::new(0));
        assert!(
            !chain
                .state
                .finalized_contribution_rewards
                .contains(&commit_id)
        );
    }
    // RED control (a): the reveal block is NOT a late arrival — the decision is
    // being made right here, so the detector must stay silent.
    assert_eq!(
        metadata_u64(&block2, "oracle_reward_late_quorum_count"),
        0,
        "the block that carries the reveal makes the decision; it is not late"
    );

    // ---- Block 3: quorum arrives and finalizes the pending plan ------------
    {
        let mut mp = mempool.write().await;
        let signing_keys = { blockchain.read().await.state.signing_keys.clone() };
        mp.add_transaction_with_signing_keys(
            signed_attestation_tx(&commit_id, &oracle_a.0, &oracle_a.1, ts + 1, "late-quorum-a"),
            Some(&signing_keys),
        )
        .expect("enqueue attestation a");
        mp.add_transaction_with_signing_keys(
            signed_attestation_tx(&commit_id, &oracle_b.0, &oracle_b.1, ts + 2, "late-quorum-b"),
            Some(&signing_keys),
        )
        .expect("enqueue attestation b");
    }
    let block3 = pop
        .create_next_block(blockchain.clone(), mempool.clone(), &miner_hex)
        .await
        .expect("produce block 3 with the late attestations");
    assert_eq!(block3.height, 3);
    assert!(
        block3
            .transactions
            .iter()
            .any(|tx| tx.tx_type == TransactionType::OracleAttestation),
        "block 3 must carry the late attestations"
    );
    assert!(
        !block3
            .transactions
            .iter()
            .any(|tx| tx.tx_type == TransactionType::ContributionReveal),
        "block 3 must NOT re-carry the reveal — that is what makes the quorum late"
    );

    // Quorum genuinely reached, and the reveal is genuinely on-chain from an
    // earlier height. Both preconditions of the gap hold.
    {
        let chain = blockchain.read().await;
        let attestations = chain
            .state
            .attestations
            .get(&commit_id)
            .expect("attestations must be recorded for the commit");
        assert!(
            attestations.has_quorum(chain.state.oracle_quorum.max(1)),
            "the commit must have reached oracle quorum at block 3"
        );
        let reveal = chain
            .state
            .pending_reveals
            .get(&commit_id)
            .expect("reveal must still be recorded from block 2");
        assert_eq!(
            reveal.revealed_at, 2,
            "the reveal must be recorded at the earlier height, not block 3"
        );
    }

    // ---- THE DETECTOR fires ------------------------------------------------
    let late = metadata_u64(&block3, "oracle_reward_late_quorum_count");
    assert_eq!(
        late, 1,
        "#2464 DETECTOR: the commit reached quorum after its reward decision \
         closed and must be counted; got {late}"
    );
    assert_eq!(
        block3
            .metadata
            .get("oracle_reward_late_quorum_commit_ids")
            .map(String::as_str),
        Some(commit_id.as_str()),
        "the late-quorum commit id must be named in block metadata"
    );

    // ---- THE SINK: quorum reached, exactly one reward is paid --------------
    assert_eq!(
        metadata_u64(&block3, "rewards_minted_count"),
        1,
        "#2464: the late-quorum block must settle exactly one contribution"
    );
    {
        let chain = blockchain.read().await;
        let account = chain
            .state
            .get_account(&contributor)
            .expect("contributor account must exist");
        assert!(
            account.earned_balance_total > Balance::new(0),
            "#2464: contributor must earn once late quorum finalizes; earned = {}",
            account.earned_balance_total
        );
        assert!(
            chain
                .state
                .finalized_contribution_rewards
                .contains(&commit_id),
            "the canonical exactly-once marker must be set"
        );
    }

    println!(
        "[late-quorum PASS] reveal(h2, no quorum) + quorum(h3) => \
         late_quorum_count={late}, minted_count=1 — the #2464 gap is settled."
    );
}

// ---------------------------------------------------------------------------
// 5. RED CONTROL for the #2464 detector.
//
//    Same three-block shape, but the quorum arrives ON TIME (in the reveal
//    block). The reward is paid, and the detector must read ZERO in every
//    block — including the later block, which proves the detector counts a
//    genuine late ARRIVAL rather than re-reporting any already-settled commit
//    that happens to still hold quorum in state.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn on_time_quorum_leaves_the_late_quorum_detector_at_zero() {
    let station_id = format!("on-time-quorum-station-{}", random::<u16>());
    let (blockchain, contributor, miner_hex, oracle_a, oracle_b) =
        setup_chain(73_900 + random::<u16>() as u64, &station_id);

    let ts = 1_701_000_000i64;
    let (commit_payload, reveal_payload, commit_id) =
        build_weather_commit_and_reveal(&station_id, ts, "on-time-quorum");

    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let mut pop = ProofOfPublishing::new(1, 60, 0, 16);
    // #2523: a test chain with no oracle ceremony must SAY so — silence at
    // height 1 is refused, because it cannot be told from a misconfigured cut.
    pop.declare_no_genesis_oracles();

    // ---- Block 1: COMMIT --------------------------------------------------
    {
        let mut mp = mempool.write().await;
        mp.add_transaction(commit_tx_from(&commit_payload, "on-time-quorum"))
            .expect("enqueue commit");
    }
    let block1 = pop
        .create_next_block(blockchain.clone(), mempool.clone(), &miner_hex)
        .await
        .expect("produce block 1 with commit");
    assert_eq!(block1.height, 1);

    // ---- Block 2: REVEAL + quorum in the SAME block (the healthy path) ----
    {
        let mut mp = mempool.write().await;
        let signing_keys = { blockchain.read().await.state.signing_keys.clone() };
        mp.add_transaction(reveal_tx_from(
            &reveal_payload,
            contributor,
            "on-time-quorum",
        ))
        .expect("enqueue reveal");
        mp.add_transaction_with_signing_keys(
            signed_attestation_tx(&commit_id, &oracle_a.0, &oracle_a.1, ts + 1, "on-time-a"),
            Some(&signing_keys),
        )
        .expect("enqueue attestation a");
        mp.add_transaction_with_signing_keys(
            signed_attestation_tx(&commit_id, &oracle_b.0, &oracle_b.1, ts + 2, "on-time-b"),
            Some(&signing_keys),
        )
        .expect("enqueue attestation b");
    }
    let block2 = pop
        .create_next_block(blockchain.clone(), mempool.clone(), &miner_hex)
        .await
        .expect("produce block 2 with reveal + attestations");
    assert_eq!(block2.height, 2);

    // The reward IS paid — this is the condition-absent case.
    assert_eq!(
        metadata_u64(&block2, "oracle_reward_quorum_commit_count"),
        1,
        "the commit must reach quorum in its own reveal block"
    );
    assert!(
        metadata_u64(&block2, "rewards_minted_count") >= 1,
        "the on-time path must mint a reward"
    );
    // RED CONTROL: quorum was not late, so the detector must read zero.
    assert_eq!(
        metadata_u64(&block2, "oracle_reward_late_quorum_count"),
        0,
        "RED CONTROL: quorum arrived with the reveal — nothing is late"
    );

    // ---- Block 3: a later block, no new attestations ----------------------
    // The commit still holds quorum in state and its reveal is now at an
    // earlier height — the two conditions a naive detector would trip on.
    let block3 = pop
        .create_next_block(blockchain.clone(), mempool.clone(), &miner_hex)
        .await
        .expect("produce block 3");
    assert_eq!(block3.height, 3);
    assert!(
        !block3
            .transactions
            .iter()
            .any(|tx| tx.tx_type == TransactionType::OracleAttestation),
        "block 3 must carry no attestation — there is no arrival to report"
    );
    assert_eq!(
        metadata_u64(&block3, "oracle_reward_late_quorum_count"),
        0,
        "RED CONTROL: an already-settled commit that still holds quorum must \
         NOT be re-counted in every later block"
    );
    assert_eq!(
        block3
            .metadata
            .get("oracle_reward_late_quorum_commit_ids")
            .map(String::as_str),
        Some(""),
        "RED CONTROL: the commit-id list must be present and empty"
    );

    {
        let chain = blockchain.read().await;
        let account = chain
            .state
            .get_account(&contributor)
            .expect("contributor account must exist");
        assert!(
            account.earned_balance_total > Balance::new(0),
            "RED CONTROL: the on-time contributor must actually have been paid"
        );
    }

    println!(
        "[on-time-quorum RED CONTROL PASS] reveal+quorum(h2) => paid, \
         late_quorum_count=0 at h2 AND h3."
    );
}

// ---------------------------------------------------------------------------
// 6 + 7. NEAR-MISS controls for the #2464 detector.
//
//    These exist because a test that manufactures the condition it is checking
//    cannot fail. Nothing in this file writes `state.attestations` — quorum is
//    only ever earned through the real path (registered oracle set -> signed
//    attestation tx -> mempool -> block production -> state transition). The
//    two tests below prove that dependency is load-bearing rather than
//    incidental: each removes ONE ingredient of a genuine quorum and requires
//    the detector to fall silent.
//
//    Together with test 4 (fires) and test 5 (silent when on time), the metric
//    is pinned from both directions and at the boundary.
// ---------------------------------------------------------------------------

/// One attestation against a quorum of two: below threshold, so no quorum ever
/// forms and there is nothing late to report. If the detector fired here it
/// would be counting *attestation arrival* rather than *quorum arrival*.
#[tokio::test]
async fn late_quorum_detector_stays_silent_below_the_quorum_threshold() {
    let station_id = format!("sub-quorum-station-{}", random::<u16>());
    let (blockchain, contributor, miner_hex, oracle_a, _oracle_b) =
        setup_chain(74_400 + random::<u16>() as u64, &station_id);

    let ts = 1_701_100_000i64;
    let (commit_payload, reveal_payload, commit_id) =
        build_weather_commit_and_reveal(&station_id, ts, "sub-quorum");

    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let mut pop = ProofOfPublishing::new(1, 60, 0, 16);
    // #2523: a test chain with no oracle ceremony must SAY so — silence at
    // height 1 is refused, because it cannot be told from a misconfigured cut.
    pop.declare_no_genesis_oracles();

    {
        let mut mp = mempool.write().await;
        mp.add_transaction(commit_tx_from(&commit_payload, "sub-quorum"))
            .expect("enqueue commit");
    }
    pop.create_next_block(blockchain.clone(), mempool.clone(), &miner_hex)
        .await
        .expect("produce block 1 with commit");

    {
        let mut mp = mempool.write().await;
        mp.add_transaction(reveal_tx_from(&reveal_payload, contributor, "sub-quorum"))
            .expect("enqueue reveal");
    }
    let block2 = pop
        .create_next_block(blockchain.clone(), mempool.clone(), &miner_hex)
        .await
        .expect("produce block 2 with reveal");
    assert_eq!(metadata_u64(&block2, "rewards_minted_count"), 0);

    // Exactly ONE late attestation — one short of the required two.
    {
        let mut mp = mempool.write().await;
        let signing_keys = { blockchain.read().await.state.signing_keys.clone() };
        mp.add_transaction_with_signing_keys(
            signed_attestation_tx(&commit_id, &oracle_a.0, &oracle_a.1, ts + 1, "sub-quorum-a"),
            Some(&signing_keys),
        )
        .expect("enqueue the single attestation");
    }
    let block3 = pop
        .create_next_block(blockchain.clone(), mempool.clone(), &miner_hex)
        .await
        .expect("produce block 3 with one late attestation");

    // The attestation genuinely landed and was genuinely recorded...
    {
        let chain = blockchain.read().await;
        let required = chain.state.oracle_quorum.max(1);
        assert_eq!(required, 2, "this control is only meaningful at quorum > 1");
        let attestations = chain
            .state
            .attestations
            .get(&commit_id)
            .expect("the single attestation must still be recorded in state");
        assert_eq!(attestations.approvals(), 1, "exactly one approval expected");
        assert!(
            !attestations.has_quorum(required),
            "one approval must NOT satisfy a quorum of two"
        );
    }

    // ...but quorum was never reached, so nothing arrived late.
    assert_eq!(
        metadata_u64(&block3, "oracle_reward_late_quorum_count"),
        0,
        "NEAR-MISS: below-threshold attestations are not a late quorum"
    );

    println!("[sub-quorum PASS] 1 of 2 approvals at h3 => late_quorum_count=0");
}

/// A later valid attestation for a commit that ALREADY has quorum must not be
/// counted again (#2464, found in review of this PR).
///
/// The original predicate was level-triggered: "an approved attestation is in
/// this block AND the commit now has quorum". Once quorum exists that stays
/// true forever, so every subsequent re-attestation or replacement re-satisfied
/// it. The scrape gauge sums per-block metadata across its window, so the
/// metric inflated — and this metric is the evidence for the 5,578-observations
/// gap, where an overstated count would misdirect the payout work.
///
/// The fix is an edge trigger: only the block that carries a commit ACROSS the
/// threshold may report it. This test drives a real signed replacement
/// attestation through mempool -> create_next_block, not a synthetic state
/// write, so it exercises the production path.
#[tokio::test]
async fn post_quorum_reattestation_is_not_counted_again() {
    let station_id = format!("requorum-station-{}", random::<u16>());
    let (blockchain, contributor, miner_hex, oracle_a, oracle_b) =
        setup_chain(75_400 + random::<u16>() as u64, &station_id);

    let ts = 1_701_300_000i64;
    let (commit_payload, reveal_payload, commit_id) =
        build_weather_commit_and_reveal(&station_id, ts, "requorum");

    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let mut pop = ProofOfPublishing::new(1, 60, 0, 16);
    // #2523: a test chain with no oracle ceremony must SAY so — silence at
    // height 1 is refused, because it cannot be told from a misconfigured cut.
    pop.declare_no_genesis_oracles();

    // ---- Block 1: COMMIT --------------------------------------------------
    {
        let mut mp = mempool.write().await;
        mp.add_transaction(commit_tx_from(&commit_payload, "requorum"))
            .expect("enqueue commit");
    }
    pop.create_next_block(blockchain.clone(), mempool.clone(), &miner_hex)
        .await
        .expect("produce block 1 with commit");

    // ---- Block 2: REVEAL + quorum (on time, so it is PAID) ----------------
    {
        let mut mp = mempool.write().await;
        let signing_keys = { blockchain.read().await.state.signing_keys.clone() };
        mp.add_transaction(reveal_tx_from(&reveal_payload, contributor, "requorum"))
            .expect("enqueue reveal");
        mp.add_transaction_with_signing_keys(
            signed_attestation_tx(&commit_id, &oracle_a.0, &oracle_a.1, ts + 1, "requorum-a"),
            Some(&signing_keys),
        )
        .expect("enqueue attestation a");
        mp.add_transaction_with_signing_keys(
            signed_attestation_tx(&commit_id, &oracle_b.0, &oracle_b.1, ts + 2, "requorum-b"),
            Some(&signing_keys),
        )
        .expect("enqueue attestation b");
    }
    let block2 = pop
        .create_next_block(blockchain.clone(), mempool.clone(), &miner_hex)
        .await
        .expect("produce block 2 with reveal + quorum");
    assert_eq!(
        metadata_u64(&block2, "oracle_reward_quorum_commit_count"),
        1,
        "the commit must reach quorum in its reveal block"
    );
    assert_eq!(
        metadata_u64(&block2, "oracle_reward_late_quorum_count"),
        0,
        "on-time quorum is not late"
    );

    // ---- Block 3: a LATER valid attestation for the same commit -----------
    // Quorum is already true and the reveal is now at an earlier height — the
    // two conditions the old level-triggered predicate keyed on.
    {
        let mut mp = mempool.write().await;
        let signing_keys = { blockchain.read().await.state.signing_keys.clone() };
        mp.add_transaction_with_signing_keys(
            signed_attestation_tx(&commit_id, &oracle_a.0, &oracle_a.1, ts + 3, "requorum-a2"),
            Some(&signing_keys),
        )
        .expect("enqueue the post-quorum replacement attestation");
    }
    let block3 = pop
        .create_next_block(blockchain.clone(), mempool.clone(), &miner_hex)
        .await
        .expect("produce block 3 with the post-quorum attestation");

    // The attestation really is in the block and the commit really still has
    // quorum — so this is the genuine scenario, not a vacuous pass.
    assert!(
        block3
            .transactions
            .iter()
            .any(|tx| tx.tx_type == TransactionType::OracleAttestation),
        "block 3 must carry the post-quorum attestation"
    );
    {
        let chain = blockchain.read().await;
        let required = chain.state.oracle_quorum.max(1);
        assert!(
            chain
                .state
                .attestations
                .get(&commit_id)
                .map(|s| s.has_quorum(required))
                .unwrap_or(false),
            "the commit must still hold quorum at block 3"
        );
    }

    assert_eq!(
        metadata_u64(&block3, "oracle_reward_late_quorum_count"),
        0,
        "EDGE TRIGGER: a commit already past quorum must not be reported again — \
         the gauge sums these across its window, so a repeat inflates the metric"
    );
    assert_eq!(
        block3
            .metadata
            .get("oracle_reward_late_quorum_commit_ids")
            .map(String::as_str),
        Some(""),
        "and no commit id may be named"
    );

    println!("[requorum PASS] post-quorum re-attestation at h3 => late_quorum_count=0");
}

/// Quorum-many attestations from oracles that are NOT in the registered set.
///
/// NAMED FOR WHAT IT ACTUALLY PROVES. The first draft of this test was called
/// `late_quorum_detector_ignores_unregistered_oracles` and claimed to cover the
/// detector. It does not, and an over-firing mutation proved it: with the
/// detector's `has_quorum` check deleted, the sub-quorum test below failed but
/// THIS one still passed. The reason is visible once you look — the rogue
/// attestations never reach a block at all:
///
/// ```text
/// enqueue rogue-a => Err(oracle attestation signature verification failed)
/// block3 attestation tx count = 0
/// ```
///
/// The gate is upstream, at the mempool boundary, and the detector is never
/// consulted. Rather than delete the test or leave a misleading name on it,
/// it now asserts the real mechanism: rogue attestations are refused on entry,
/// so no unregistered quorum can exist for the detector to consider. Detector
/// coverage of the quorum threshold lives in the sub-quorum test above, which
/// does bite.
#[tokio::test]
async fn unregistered_oracle_attestations_are_refused_at_the_mempool_boundary() {
    let seed = 74_900 + random::<u16>() as u64;
    let station_id = format!("unregistered-station-{}", random::<u16>());
    let (blockchain, contributor, miner_hex, _oracle_a, _oracle_b) = setup_chain(seed, &station_id);

    let ts = 1_701_200_000i64;
    let (commit_payload, reveal_payload, commit_id) =
        build_weather_commit_and_reveal(&station_id, ts, "unregistered");

    // Two identities that were never passed to SetOracleConfig.
    let rogue_a = oracle_identity(seed + 7_000);
    let rogue_b = oracle_identity(seed + 8_000);
    {
        let chain = blockchain.read().await;
        for rogue in [&rogue_a, &rogue_b] {
            assert!(
                !chain.state.oracle_set.contains(&rogue.1),
                "the rogue identity must not be in the registered oracle set"
            );
        }
    }

    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let mut pop = ProofOfPublishing::new(1, 60, 0, 16);
    // #2523: a test chain with no oracle ceremony must SAY so — silence at
    // height 1 is refused, because it cannot be told from a misconfigured cut.
    pop.declare_no_genesis_oracles();

    {
        let mut mp = mempool.write().await;
        mp.add_transaction(commit_tx_from(&commit_payload, "unregistered"))
            .expect("enqueue commit");
    }
    pop.create_next_block(blockchain.clone(), mempool.clone(), &miner_hex)
        .await
        .expect("produce block 1 with commit");

    {
        let mut mp = mempool.write().await;
        mp.add_transaction(reveal_tx_from(&reveal_payload, contributor, "unregistered"))
            .expect("enqueue reveal");
    }
    pop.create_next_block(blockchain.clone(), mempool.clone(), &miner_hex)
        .await
        .expect("produce block 2 with reveal");

    // Quorum-many attestations, correctly signed — by the wrong keys. Each
    // enqueue must be REFUSED; asserting on the Err is the whole point, because
    // an ignored return value is what let the first draft look like detector
    // coverage.
    {
        let mut mp = mempool.write().await;
        let signing_keys = { blockchain.read().await.state.signing_keys.clone() };
        for (rogue, tag, offset) in [(&rogue_a, "rogue-a", 1i64), (&rogue_b, "rogue-b", 2)] {
            let err = mp
                .add_transaction_with_signing_keys(
                    signed_attestation_tx(&commit_id, &rogue.0, &rogue.1, ts + offset, tag),
                    Some(&signing_keys),
                )
                .expect_err("an unregistered oracle's attestation must be refused on entry");
            assert!(
                err.to_string().contains("signature verification failed"),
                "refusal must come from signature verification against the registered \
                 set, not some unrelated mempool guard; got: {err}"
            );
        }
    }
    let block3 = pop
        .create_next_block(blockchain.clone(), mempool.clone(), &miner_hex)
        .await
        .expect("produce block 3");

    // Nothing rogue reached the chain: no attestation tx, no state entry.
    assert_eq!(
        block3
            .transactions
            .iter()
            .filter(|tx| tx.tx_type == TransactionType::OracleAttestation)
            .count(),
        0,
        "a refused attestation must not appear in the block"
    );
    {
        let chain = blockchain.read().await;
        assert!(
            !chain.state.attestations.contains_key(&commit_id),
            "a refused attestation must not be recorded in state"
        );
    }

    // The detector consequently reads zero — because nothing arrived, NOT
    // because the detector filtered anything. Stated so the next reader does
    // not mistake this for detector coverage.
    assert_eq!(
        metadata_u64(&block3, "oracle_reward_late_quorum_count"),
        0,
        "no arrival, so nothing to report as late"
    );
    assert_eq!(
        metadata_u64(&block3, "rewards_minted_count"),
        0,
        "and certainly nothing may be minted"
    );

    println!(
        "[unregistered PASS] 2 rogue attestations REFUSED at the mempool \
         (signature verification) => 0 attestation txs on-chain, late_quorum_count=0"
    );
}
