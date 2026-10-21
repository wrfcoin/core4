// #2537 — the production path that makes a contribution reward-eligible.
//
// The defect this closes: `State::register_weather_station` was reachable ONLY
// from test code. No transaction type, no state transition, and no HTTP route
// wrote into `station_registry`. So on a live chain the registry was
// permanently empty, every `RegisterCommit` fell through to
// `ContributionProvenanceClass::Unclassified`, `is_reward_eligible()` was
// false, and the earn loop could not pay anyone. The chain minted a correct
// coinbase for a producer and zero for every contributor, forever.
//
// What this file proves, end to end and on the production path only:
//
//   1. A governance-authorized `StationRegistration` TRANSACTION, mined in a
//      block, registers a station and makes an otherwise-identical
//      contribution reward-eligible — non-zero `rewards_minted_count`,
//      non-zero `real_paid`, non-zero contributor `earned_balance_total`.
//   2. THE ANTI-VACUITY CONTROL: the identical loop with the registration tx
//      REMOVED pays exactly zero. That is what stops this file from being a
//      test that passes by looking nowhere — the registration tx is the only
//      difference between paid and unpaid.
//   3. The gate is fail-closed on every authorization edge: an unallowlisted
//      governance key, a missing governance signature (a station trying to
//      self-assert `Real`), a missing owner possession proof, and a payload
//      tampered after signing all register NOTHING.
//
// What this file does NOT look at, stated explicitly per the house standard:
//   - the applier-side (P2P/WAL) extractor, which lives in the `native-chain`
//     bin and is covered by the parity test in
//     `native-chain/src/bin/launch_api/transition_extractor.rs`;
//   - the HTTP submission surface;
//   - late-quorum settlement (#2464, owned separately).
//
// Run from $HOME/wrfcoin/core4:
//   cargo test -p wrfcoin-shared-protocol --test station_registration_production_path_2537

use std::sync::Arc;

use rand::rngs::StdRng;
use rand::{SeedableRng, random};
use tokio::sync::{Mutex, MutexGuard, RwLock};
use wrfcoin_reputation_engine::SCORE_SCALE;

use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::state::StateTransition;
use wrfcoin_shared_protocol::blockchain::transaction::{
    CommitPayload, OracleAttestationPayload, RevealPayload, StationRegistrationPayload,
    Transaction, TransactionInput, TransactionOutput, TransactionType,
};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::ProofOfPublishing;
use wrfcoin_shared_protocol::crypto::hash::hash_string;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::mempool::pool::Mempool;
use wrfcoin_shared_protocol::station_registry::{ContributionProvenanceClass, DataQualityTier};

const GOVERNANCE_ENV: &str = "WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS";

/// `oracle_governance_pubkeys()` reads process env, so tests that install an
/// allowlist must not overlap. Integration tests share one binary, and cargo
/// runs test binaries sequentially, so a file-local lock is sufficient.
///
/// Async-aware on purpose: the earn-loop helper holds this guard across `.await`
/// points while mining blocks, which a `std::sync::Mutex` must never do.
static ENV_LOCK: Mutex<()> = Mutex::const_new(());

async fn lock_env() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().await
}

// ---------------------------------------------------------------------------
// Helpers (same shapes as earn_loop_oracle_gate_e2e.rs)
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

/// Build a fully-authorized station-registration payload: owner possession
/// proof plus governance authorisation, both over the explicit chain domain.
fn signed_registration_payload(
    station_id: &str,
    owner_address: &str,
    provenance_class: ContributionProvenanceClass,
    owner_key: &PrivateKey,
    governance_key: &PrivateKey,
) -> StationRegistrationPayload {
    let mut payload = StationRegistrationPayload::new(
        station_id,
        owner_address,
        46.7867,
        -92.1005,
        "test-real-station",
        DataQualityTier::Gold,
        provenance_class,
    );
    // Owner first: the owner key is folded into the governance preimage, so the
    // possession proof must exist before governance signs it.
    payload.sign_ownership(owner_key);
    payload.sign(governance_key);
    payload
}

fn registration_tx(payload: &StationRegistrationPayload, tag: &str) -> Transaction {
    let mut tx = build_tx(
        TransactionType::StationRegistration,
        &format!("station-reg-{tag}"),
    );
    tx.data = Some(serde_json::to_string(payload).expect("serialize registration payload"));
    tx.set_id();
    tx
}

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
    // Non-zero fee so the compute-fee gate (#821/#1487) does not drop the
    // reveal during assembly. Orthogonal to registration.
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

struct Harness {
    blockchain: Arc<RwLock<Blockchain>>,
    contributor: Address,
    contributor_key: PrivateKey,
    miner_hex: String,
    oracle_a: (PrivateKey, String),
    oracle_b: (PrivateKey, String),
}

/// A chain with an oracle quorum, a funded contributor account, and tier
/// policy — but **deliberately no station**. Every test in this file must get
/// its station on-chain through a transaction or not at all.
fn setup_chain_without_station(seed: u64, station_id: &str) -> Harness {
    let mut rng = StdRng::seed_from_u64(seed);
    let contributor_key = PrivateKey::generate(&mut rng);
    let contributor_pk = PublicKey::from_private_key(&contributor_key);
    let contributor = Address::from_public_key(&contributor_pk);

    let miner_key = PrivateKey::generate(&mut rng);
    let miner_pk = PublicKey::from_private_key(&miner_key);
    let miner_address = Address::from_public_key(&miner_pk);
    let miner_hex = format!("0x{}", hex::encode(miner_address.0));

    let mut chain = Blockchain::default();
    chain.state.create_account(contributor, Balance::new(0));

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
        .set_reputation_score(&hex::encode(contributor.0), rep_score);
    chain
        .state
        .oracle_registry_mut()
        .set_reputation_score(&oracle_a.1, rep_score);
    chain
        .state
        .oracle_registry_mut()
        .set_reputation_score(&oracle_b.1, rep_score);

    Harness {
        blockchain: Arc::new(RwLock::new(chain)),
        contributor,
        contributor_key,
        miner_hex,
        oracle_a,
        oracle_b,
    }
}

/// Drive the full earn loop. `register` decides whether the station-registration
/// transaction is included — the single variable the anti-vacuity control flips.
///
/// Returns `(rewards_minted_count, rewards_minted_total, real_paid, earned_balance_total)`.
async fn run_earn_loop(seed: u64, tag: &str, register: bool) -> (u64, u64, u128, u128) {
    let _guard = lock_env().await;

    let station_id = format!("prod-station-{tag}-{}", random::<u16>());
    let harness = setup_chain_without_station(seed, &station_id);

    let mut rng = StdRng::seed_from_u64(seed + 999);
    let governance_key = PrivateKey::generate(&mut rng);
    let governance_hex = PublicKey::from_private_key(&governance_key).to_hex();
    std::env::set_var(GOVERNANCE_ENV, &governance_hex);

    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let mut pop = ProofOfPublishing::new(1, 60, 0, 16);
    pop.declare_no_genesis_oracles();

    // ---- Block 1: the STATION REGISTRATION transaction --------------------
    if register {
        let payload = signed_registration_payload(
            &station_id,
            &harness.contributor.to_string(),
            ContributionProvenanceClass::Real,
            &harness.contributor_key,
            &governance_key,
        );
        {
            let mut mp = mempool.write().await;
            mp.add_transaction(registration_tx(&payload, tag))
                .expect("enqueue station registration");
        }
        pop.create_next_block(
            harness.blockchain.clone(),
            mempool.clone(),
            &harness.miner_hex,
        )
        .await
        .expect("mine station registration block");

        let chain = harness.blockchain.read().await;
        let station = chain
            .state
            .station_registry
            .get(&station_id)
            .expect("the registration TRANSACTION must have written the station registry");
        assert_eq!(
            station.provenance_class,
            ContributionProvenanceClass::Real,
            "governance authorized Real; the applied record must carry it"
        );
        assert_eq!(
            station.reputation_score, 0,
            "a registration must never seed reputation (#2537)"
        );
    }

    let ts = 1_700_800_000i64;
    let (commit_payload, reveal_payload, commit_id) =
        build_weather_commit_and_reveal(&station_id, ts, tag);

    // ---- COMMIT ------------------------------------------------------------
    {
        let mut mp = mempool.write().await;
        mp.add_transaction(commit_tx_from(&commit_payload, tag))
            .expect("enqueue commit");
    }
    pop.create_next_block(
        harness.blockchain.clone(),
        mempool.clone(),
        &harness.miner_hex,
    )
    .await
    .expect("mine commit block");

    // ---- REVEAL + oracle quorum -------------------------------------------
    {
        let mut mp = mempool.write().await;
        let signing_keys = { harness.blockchain.read().await.state.signing_keys.clone() };
        mp.add_transaction(reveal_tx_from(&reveal_payload, harness.contributor, tag))
            .expect("enqueue reveal");
        mp.add_transaction_with_signing_keys(
            signed_attestation_tx(
                &commit_id,
                &harness.oracle_a.0,
                &harness.oracle_a.1,
                ts + 1,
                &format!("{tag}-a"),
            ),
            Some(&signing_keys),
        )
        .expect("enqueue attestation a");
        mp.add_transaction_with_signing_keys(
            signed_attestation_tx(
                &commit_id,
                &harness.oracle_b.0,
                &harness.oracle_b.1,
                ts + 2,
                &format!("{tag}-b"),
            ),
            Some(&signing_keys),
        )
        .expect("enqueue attestation b");
    }
    let reveal_block = pop
        .create_next_block(
            harness.blockchain.clone(),
            mempool.clone(),
            &harness.miner_hex,
        )
        .await
        .expect("mine reveal block with quorum");

    std::env::remove_var(GOVERNANCE_ENV);

    let minted_count = metadata_u64(&reveal_block, "rewards_minted_count");
    let minted_total = metadata_u64(&reveal_block, "rewards_minted_total");

    let chain = harness.blockchain.read().await;
    let aggregate = chain.state.contribution_reward_aggregate(&harness.contributor);
    let earned = chain
        .state
        .get_account(&harness.contributor)
        .expect("contributor account exists")
        .earned_balance_total;

    (minted_count, minted_total, aggregate.real_paid.0, earned.0)
}

// ---------------------------------------------------------------------------
// 1. THE HEADLINE: a mined registration transaction makes a real contribution
//    reward-eligible.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn production_registration_makes_a_contribution_reward_eligible() {
    let (minted_count, minted_total, real_paid, earned) =
        run_earn_loop(84_100 + random::<u16>() as u64, "paid", true).await;

    assert_eq!(
        minted_count, 1,
        "a registered Real station with oracle quorum must mint exactly one contribution reward"
    );
    assert!(
        minted_total > 0,
        "minted total must be non-zero, got {minted_total}"
    );
    assert!(
        real_paid > 0,
        "real_paid must be non-zero — this is the counter that stayed at 0 on the live chain \
         through height 1003 in the cut rehearsal; got {real_paid}"
    );
    assert!(
        earned > 0,
        "the contributor's earned_balance_total must be non-zero, got {earned}"
    );
}

// ---------------------------------------------------------------------------
// 2. THE ANTI-VACUITY CONTROL.
//
//    Identical seed, identical commit, identical reveal, identical oracle
//    quorum — the ONLY difference is that the registration transaction is not
//    mined. If this test ever starts paying, the test above has stopped
//    proving that the registration path is what makes the reward possible.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn without_the_registration_transaction_the_same_loop_pays_nothing() {
    let (minted_count, minted_total, real_paid, earned) =
        run_earn_loop(84_100 + random::<u16>() as u64, "unpaid", false).await;

    assert_eq!(
        minted_count, 0,
        "with no registration the commit is Unclassified and must mint nothing"
    );
    assert_eq!(minted_total, 0, "no registration must mean no minted total");
    assert_eq!(real_paid, 0, "no registration must mean zero real_paid");
    assert_eq!(
        earned, 0,
        "no registration must mean the contributor earns nothing"
    );
}

// ---------------------------------------------------------------------------
// 3. Fail-closed authorization edges. Each asserts the transition is NOT
//    emitted, which is what keeps an unauthorized station out of state_root.
// ---------------------------------------------------------------------------

/// Shared shape: build a payload, mutate it, and assert the tx does not
/// authorize. `verify_station_registration_authorization` is the single
/// predicate both the producer and the applier gate on, so proving it false
/// proves neither path emits a transition.
fn authorizes(payload: &StationRegistrationPayload) -> bool {
    registration_tx(payload, "edge").verify_station_registration_authorization()
}

#[tokio::test]
async fn a_fully_signed_registration_authorizes() {
    // Positive control for the four negatives below: without this, every
    // negative could be passing because the fixture is simply broken.
    let _guard = lock_env().await;
    let mut rng = StdRng::seed_from_u64(4_242);
    let owner = PrivateKey::generate(&mut rng);
    let governance = PrivateKey::generate(&mut rng);
    std::env::set_var(
        GOVERNANCE_ENV,
        PublicKey::from_private_key(&governance).to_hex(),
    );

    let payload = signed_registration_payload(
        "edge-station",
        "0xowner",
        ContributionProvenanceClass::Real,
        &owner,
        &governance,
    );
    assert!(authorizes(&payload), "the positive control must authorize");
    std::env::remove_var(GOVERNANCE_ENV);
}

#[tokio::test]
async fn an_unallowlisted_governance_key_never_authorizes() {
    let _guard = lock_env().await;
    let mut rng = StdRng::seed_from_u64(4_243);
    let owner = PrivateKey::generate(&mut rng);
    let governance = PrivateKey::generate(&mut rng);
    let stranger = PrivateKey::generate(&mut rng);
    // Allowlist someone else entirely.
    std::env::set_var(
        GOVERNANCE_ENV,
        PublicKey::from_private_key(&stranger).to_hex(),
    );

    let payload = signed_registration_payload(
        "edge-station",
        "0xowner",
        ContributionProvenanceClass::Real,
        &owner,
        &governance,
    );
    assert!(
        !authorizes(&payload),
        "a valid signature from an unallowlisted key must not authorize"
    );
    std::env::remove_var(GOVERNANCE_ENV);
}

#[tokio::test]
async fn a_station_cannot_self_assert_real_without_governance() {
    let _guard = lock_env().await;
    let mut rng = StdRng::seed_from_u64(4_244);
    let owner = PrivateKey::generate(&mut rng);
    let governance = PrivateKey::generate(&mut rng);
    std::env::set_var(
        GOVERNANCE_ENV,
        PublicKey::from_private_key(&governance).to_hex(),
    );

    // Owner possession only — exactly what a station operator can produce alone.
    let mut payload = StationRegistrationPayload::new(
        "edge-station",
        "0xowner",
        46.7867,
        -92.1005,
        "self-asserted",
        DataQualityTier::Gold,
        ContributionProvenanceClass::Real,
    );
    payload.sign_ownership(&owner);

    assert!(
        !authorizes(&payload),
        "possession alone must never confer the reward-authorising Real class (#2537)"
    );
    std::env::remove_var(GOVERNANCE_ENV);
}

#[tokio::test]
async fn a_registration_without_owner_possession_never_authorizes() {
    let _guard = lock_env().await;
    let mut rng = StdRng::seed_from_u64(4_245);
    let governance = PrivateKey::generate(&mut rng);
    std::env::set_var(
        GOVERNANCE_ENV,
        PublicKey::from_private_key(&governance).to_hex(),
    );

    let mut payload = StationRegistrationPayload::new(
        "edge-station",
        "0xowner",
        46.7867,
        -92.1005,
        "no-owner-proof",
        DataQualityTier::Gold,
        ContributionProvenanceClass::Real,
    );
    payload.sign(&governance);

    assert!(
        !authorizes(&payload),
        "governance alone must not enroll a station whose owner proved no key"
    );
    std::env::remove_var(GOVERNANCE_ENV);
}

#[tokio::test]
async fn flipping_provenance_to_real_after_signing_breaks_authorization() {
    let _guard = lock_env().await;
    let mut rng = StdRng::seed_from_u64(4_246);
    let owner = PrivateKey::generate(&mut rng);
    let governance = PrivateKey::generate(&mut rng);
    std::env::set_var(
        GOVERNANCE_ENV,
        PublicKey::from_private_key(&governance).to_hex(),
    );

    // Governance authorizes Synthetic...
    let mut payload = signed_registration_payload(
        "edge-station",
        "0xowner",
        ContributionProvenanceClass::Synthetic,
        &owner,
        &governance,
    );
    assert!(authorizes(&payload), "the Synthetic registration is valid");

    // ...and the submitter tries to upgrade it to the class that earns.
    payload.provenance_class = ContributionProvenanceClass::Real;
    assert!(
        !authorizes(&payload),
        "provenance_class is inside the governance preimage; flipping it must break the signature"
    );
    std::env::remove_var(GOVERNANCE_ENV);
}

#[tokio::test]
async fn re_pairing_a_harvested_governance_signature_with_another_owner_fails() {
    let _guard = lock_env().await;
    let mut rng = StdRng::seed_from_u64(4_247);
    let owner = PrivateKey::generate(&mut rng);
    let attacker = PrivateKey::generate(&mut rng);
    let governance = PrivateKey::generate(&mut rng);
    std::env::set_var(
        GOVERNANCE_ENV,
        PublicKey::from_private_key(&governance).to_hex(),
    );

    let mut payload = signed_registration_payload(
        "edge-station",
        "0xowner",
        ContributionProvenanceClass::Real,
        &owner,
        &governance,
    );
    // Attacker substitutes their own valid possession proof, keeping the
    // harvested governance signature.
    payload.sign_ownership(&attacker);

    assert!(
        !authorizes(&payload),
        "owner_pubkey is bound into the governance preimage, so the swap must fail"
    );
    std::env::remove_var(GOVERNANCE_ENV);
}

// ---------------------------------------------------------------------------
// 4. Structural validation — non-finite coordinates never reach the signed
//    surface or the state-root encoding.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn non_finite_coordinates_are_rejected_at_parse() {
    let _guard = lock_env().await;
    let mut rng = StdRng::seed_from_u64(4_248);
    let owner = PrivateKey::generate(&mut rng);
    let governance = PrivateKey::generate(&mut rng);
    std::env::set_var(
        GOVERNANCE_ENV,
        PublicKey::from_private_key(&governance).to_hex(),
    );

    for (lat, lon, label) in [
        (f64::NAN, -92.0, "NaN latitude"),
        (46.0, f64::INFINITY, "infinite longitude"),
        (91.0, -92.0, "out-of-range latitude"),
        (46.0, 181.0, "out-of-range longitude"),
    ] {
        let mut payload = StationRegistrationPayload::new(
            "edge-station",
            "0xowner",
            lat,
            lon,
            "bad-coords",
            DataQualityTier::Gold,
            ContributionProvenanceClass::Real,
        );
        payload.sign_ownership(&owner);
        payload.sign(&governance);
        assert!(
            !authorizes(&payload),
            "{label} must be rejected before it can be signed into consensus"
        );
    }
    std::env::remove_var(GOVERNANCE_ENV);
}

// ---------------------------------------------------------------------------
// #2742: the ownership signature must PROVE ownership of owner_address.
//
// RED control, measured before the fix: `a_fully_signed_registration_
// authorizes` above passes with owner_address = "0xowner" — a value that is
// not derived from the owner key and is not even hex. The chain accepted a
// possession proof for an address the prover had no relationship to, and
// #2743 routes payment by that field.
// ---------------------------------------------------------------------------

/// The owner key's canonical on-chain address, in the ceremony CLI's `0x` form.
fn bound_owner_address(owner: &PrivateKey) -> String {
    format!(
        "0x{}",
        PublicKey::from_private_key(owner)
            .to_address()
            .trim_start_matches("wrf")
    )
}

#[tokio::test]
async fn at_the_binding_activation_height_an_unbound_owner_address_stops_authorizing() {
    let _guard = lock_env().await;
    let mut rng = StdRng::seed_from_u64(27_420);
    let owner = PrivateKey::generate(&mut rng);
    let governance = PrivateKey::generate(&mut rng);
    std::env::set_var(
        GOVERNANCE_ENV,
        PublicKey::from_private_key(&governance).to_hex(),
    );

    // Fully signed, provenance Real — valid in every respect EXCEPT that
    // owner_address has nothing to do with the owner key. This is the exact
    // shape of the registrations already on chain (the CLI took the address
    // as a free-form flag).
    let unbound = signed_registration_payload(
        "unbound-station",
        "0x5eaf842a8341cd38812a63de6b5dcc5103f071c0",
        ContributionProvenanceClass::Real,
        &owner,
        &governance,
    );
    let tx = registration_tx(&unbound, "unbound");
    let h = wrfcoin_shared_protocol::consensus::proof_of_publishing::STATION_OWNERSHIP_BINDING_ACTIVATION_HEIGHT;

    assert!(
        tx.verify_station_registration_authorization_at_height(h - 1),
        "below activation the legacy rule holds — the block-674 registrations \
         must keep replaying byte-identically"
    );
    assert!(
        !tx.verify_station_registration_authorization_at_height(h),
        "at activation an unbound owner_address must stop authorizing"
    );
    assert!(
        !tx.verify_station_registration_authorization_at_height(h + 1_000),
        "and it must stay refused after activation"
    );
    std::env::remove_var(GOVERNANCE_ENV);
}

#[tokio::test]
async fn a_bound_owner_address_authorizes_on_both_sides_of_activation() {
    let _guard = lock_env().await;
    let mut rng = StdRng::seed_from_u64(27_421);
    let owner = PrivateKey::generate(&mut rng);
    let governance = PrivateKey::generate(&mut rng);
    std::env::set_var(
        GOVERNANCE_ENV,
        PublicKey::from_private_key(&governance).to_hex(),
    );

    let bound = signed_registration_payload(
        "bound-station",
        &bound_owner_address(&owner),
        ContributionProvenanceClass::Real,
        &owner,
        &governance,
    );
    let tx = registration_tx(&bound, "bound");
    let h = wrfcoin_shared_protocol::consensus::proof_of_publishing::STATION_OWNERSHIP_BINDING_ACTIVATION_HEIGHT;

    assert!(tx.verify_station_registration_authorization_at_height(h - 1));
    assert!(
        tx.verify_station_registration_authorization_at_height(h),
        "a properly derived owner_address must keep authorizing at activation"
    );

    // The wrf-prefixed presentation of the same 20 bytes binds identically —
    // the rule is about the bytes, not the prefix.
    let wrf_form = PublicKey::from_private_key(&owner).to_address();
    let bound_wrf = signed_registration_payload(
        "bound-station-wrf",
        &wrf_form,
        ContributionProvenanceClass::Real,
        &owner,
        &governance,
    );
    assert!(
        registration_tx(&bound_wrf, "bound-wrf")
            .verify_station_registration_authorization_at_height(h)
    );
    std::env::remove_var(GOVERNANCE_ENV);
}

#[tokio::test]
async fn the_binding_predicate_fails_closed_on_malformed_inputs() {
    let _guard = lock_env().await;
    let mut rng = StdRng::seed_from_u64(27_422);
    let owner = PrivateKey::generate(&mut rng);

    let mut payload = StationRegistrationPayload::new(
        "predicate-station",
        &bound_owner_address(&owner),
        46.7867,
        -92.1005,
        "predicate",
        DataQualityTier::Gold,
        ContributionProvenanceClass::Real,
    );
    // No owner_pubkey at all → false, never a free pass.
    payload.owner_pubkey = None;
    assert!(!payload.owner_address_is_bound_to_owner_pubkey());

    // Garbage pubkey hex → false.
    payload.owner_pubkey = Some("zz-not-hex".to_string());
    assert!(!payload.owner_address_is_bound_to_owner_pubkey());

    // Correct pubkey but the CLI keystore's SHA-256 derivation → false: the
    // wrong hash is exactly the mismatch the canonicity decision guards.
    payload.owner_pubkey = Some(PublicKey::from_private_key(&owner).to_hex());
    payload.owner_address = {
        use sha2::{Digest, Sha256};
        let pk = PublicKey::from_private_key(&owner);
        let digest = Sha256::digest(pk.as_bytes());
        format!("0x{}", hex::encode(&digest[0..20]))
    };
    assert!(
        !payload.owner_address_is_bound_to_owner_pubkey(),
        "the CLI keystore's SHA-256 derivation must NOT satisfy the chain's \
         SHA3-256 binding — that mismatch is the #2742 canonicity decision"
    );

    // And the canonical derivation, mixed-case hex → true (bytes, not case).
    payload.owner_address = bound_owner_address(&owner).to_uppercase().replace("0X", "0x");
    assert!(payload.owner_address_is_bound_to_owner_pubkey());
}

// ---------------------------------------------------------------------------
// #2744 — the self-verification domain split, pinned.
// ---------------------------------------------------------------------------

/// The offline builder signs from explicit `--chain-id`/`--network-id` values;
/// before #2744, self-verification resolved the identity from the process
/// environment instead — two sources of truth for one value, so correct flags
/// with an unset env failed the builder's own self-check and the error blamed
/// the governance key.
///
/// GREEN half: the explicit-domain verifier accepts a transaction signed with
/// the same explicit domain, with NO chain identity in the environment.
/// RED control: the env-reading verifier — the pre-#2744 self-check — refuses
/// that same transaction under that same environment (it resolves the default
/// mainnet domain), which is precisely the ceremony failure the issue records.
#[tokio::test]
async fn explicit_domain_self_verification_does_not_depend_on_the_environment_2744() {
    let _guard = lock_env().await;
    let mut rng = StdRng::seed_from_u64(27_440);
    let owner = PrivateKey::generate(&mut rng);
    let governance = PrivateKey::generate(&mut rng);
    std::env::set_var(
        GOVERNANCE_ENV,
        PublicKey::from_private_key(&governance).to_hex(),
    );
    // The trap's precondition: no chain identity in the environment. The
    // env-reading paths then resolve the default (mainnet) domain.
    std::env::remove_var("WRFCOIN_CHAIN_ID");
    std::env::remove_var("WRFCOIN_NETWORK_ID");

    let mut payload = StationRegistrationPayload::new(
        "station-2744",
        "0xowner2744",
        46.7867,
        -92.1005,
        "domain-split-regression",
        DataQualityTier::Gold,
        ContributionProvenanceClass::Real,
    );
    payload.sign_ownership_with_domain("777", "testnet", &owner);
    let tx = Transaction::new_station_registration_with_domain(
        "777",
        "testnet",
        payload,
        &governance,
    );

    assert!(
        tx.verify_station_registration_authorization_with_domain("777", "testnet"),
        "explicit-domain verification must accept its own signing domain \
         regardless of the process environment (#2744)"
    );
    assert!(
        !tx.verify_station_registration_authorization(),
        "RED control: the env-reading verifier resolves the default domain here \
         and must refuse — this is the pre-#2744 self-check that failed the \
         ceremony build. If this starts passing, the env default changed and \
         the control is no longer exercising the split."
    );

    std::env::remove_var(GOVERNANCE_ENV);
}
