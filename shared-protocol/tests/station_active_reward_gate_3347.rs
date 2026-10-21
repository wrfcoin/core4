//! #3347 — rewards honour a station's `is_active` flag, from activation.
//!
//! The survey behind #3347 INFERRED from a grep that `proof_of_publishing.rs`
//! reads `is_active` only in test fixtures. This file turns that inference
//! into evidence, on the production path (the #2537 earn loop: a
//! governance-signed `StationRegistration` TRANSACTION, then a commit, then a
//! reveal with 2-of-2 oracle quorum, each mined by the real
//! `ProofOfPublishing::create_next_block`):
//!
//! 1. CHARACTERISATION (no schedule = the live v1 chain): a Real station
//!    registered with `is_active = false` IS PAID — non-zero
//!    `rewards_minted_count`, `real_paid` and `earned_balance_total`. The only
//!    production reads of `is_active` on the reward path are the data-desert
//!    multiplier (`State::station_coordinates` → `None` → neutral 100 bps) and
//!    the ring-signature reward-claim membership check. This is expected to
//!    keep passing below activation ("pre-activation unchanged").
//! 2. `ConsensusFeature::StationActiveRewardGate` is known, unscheduled, never
//!    halts an unscheduled genesis, and a pre-#3347 binary reports the #3093
//!    upgrade when a schedule names it.
//! 3. With the feature scheduled at H = 3 (the reveal block's height; test
//!    only): the ACTIVE station is still paid (anti-vacuity control — the gate
//!    does not simply stop all rewards), and the INACTIVE station now mints
//!    nothing.
//!
//! 3b. Owner ruling Q3 (2026-09-27): a reward PLANNED while active (reveal
//!    without quorum) is HELD while governance has the station inactive (the
//!    quorum arrives, nothing settles) and RELEASED once it is active again.
//!    RED control: with the #3347 clause removed from the #2464 deferred
//!    settlement scan, step 3b fails at "must be HELD".
//!
//! RED control (recorded in the PR): with the gate in
//! `ProofOfPublishing::station_reward_refused_inactive` forced to `false`,
//! step 3 fails at the "inactive station is not paid" assertion.
//!
//! One test: the feature schedule is installed process-wide (first-install-
//! wins `OnceLock`), so this file owns its process, the same discipline as
//! `station_owner_slash_identity_activation_3214`.
//!
//! Run from core4:
//!   cargo test -p wrfcoin-shared-protocol --test
//! station_active_reward_gate_3347

use std::sync::Arc;

use rand::SeedableRng;
use rand::rngs::StdRng;
use tokio::sync::{Mutex, MutexGuard, RwLock};
use wrfcoin_reputation_engine::SCORE_SCALE;
use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::state::StateTransition;
use wrfcoin_shared_protocol::blockchain::transaction::{
    CommitPayload, OracleAttestationPayload, RevealPayload, STATION_OWNER_ACTION_DOMAIN,
    StationOwnerActionKind, StationRegistrationPayload, Transaction, TransactionInput,
    TransactionOutput, TransactionType, station_owner_action_message_with_domain,
};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::ProofOfPublishing;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ConsensusFeature, GENESIS_ACTIVATION_SCHEDULE_KEY, SigningProtocolVersion, feature_active,
    install_activation_table, install_feature_schedule, resolve_activation_schedule,
    resolve_feature_schedule, try_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::hash::hash_string;
use wrfcoin_shared_protocol::crypto::protocol_support::{
    KNOWN_FEATURES, upgrade_required_at, upgrade_required_in_schedule,
    upgrade_required_in_schedule_with,
};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::mempool::pool::Mempool;
use wrfcoin_shared_protocol::station_registry::{ContributionProvenanceClass, DataQualityTier};

const GOVERNANCE_ENV: &str = "WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS";
/// The earn loop mines registration (h1), commit (h2), reveal (h3).
const ACTIVATION_HEIGHT: u64 = 3;

static ENV_LOCK: Mutex<()> = Mutex::const_new(());

async fn lock_env() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().await
}

// Helpers below are the #2537 production-path harness
// (`station_registration_production_path_2537.rs`), with the registration's
// governance-signed `is_active` made a parameter.

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
    is_active: bool,
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
    // `is_active` is governance-signed; set it before either signature.
    payload.is_active = is_active;
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

    let seed = tag
        .bytes()
        .fold(33u64, |acc, b| acc.saturating_add(b as u64));
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

/// Drive the full earn loop. `register` decides whether the
/// station-registration transaction is included — the single variable the
/// anti-vacuity control flips.
///
/// Returns `(rewards_minted_count, rewards_minted_total, real_paid,
/// earned_balance_total)`.
async fn run_earn_loop(seed: u64, tag: &str, is_active: bool) -> (u64, u64, u128, u128) {
    let _guard = lock_env().await;

    let station_id = format!("ws5000-3347-{tag}");
    let harness = setup_chain_without_station(seed, &station_id);

    let mut rng = StdRng::seed_from_u64(seed + 999);
    let governance_key = PrivateKey::generate(&mut rng);
    let governance_hex = PublicKey::from_private_key(&governance_key).to_hex();
    std::env::set_var(GOVERNANCE_ENV, &governance_hex);

    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let mut pop = ProofOfPublishing::new(1, 60, 0, 16);
    pop.declare_no_genesis_oracles();

    // ---- Block 1: the STATION REGISTRATION transaction --------------------
    {
        let payload = signed_registration_payload(
            &station_id,
            &harness.contributor.to_string(),
            ContributionProvenanceClass::Real,
            is_active,
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
            station.is_active, is_active,
            "the applied record must carry the governance-signed is_active"
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
    let aggregate = chain
        .state
        .contribution_reward_aggregate(&harness.contributor);
    let earned = chain
        .state
        .get_account(&harness.contributor)
        .expect("contributor account exists")
        .earned_balance_total;

    (minted_count, minted_total, aggregate.real_paid.0, earned.0)
}

/// Enqueue `txs` and mine one block on the production path.
async fn mine_block(
    pop: &mut ProofOfPublishing,
    harness: &Harness,
    mempool: &Arc<RwLock<Mempool>>,
    txs: Vec<Transaction>,
) {
    {
        let signing_keys = { harness.blockchain.read().await.state.signing_keys.clone() };
        let mut mp = mempool.write().await;
        for tx in txs {
            mp.add_transaction_with_signing_keys(tx, Some(&signing_keys))
                .expect("enqueue");
        }
    }
    pop.create_next_block(
        harness.blockchain.clone(),
        mempool.clone(),
        &harness.miner_hex,
    )
    .await
    .expect("mine block");
}

/// What the #3347 Q3 hold/release loop observed.
#[derive(Debug)]
struct HoldRelease {
    /// The reveal (no quorum yet) planned a non-zero, unfinalized reward.
    planned_amount: u128,
    planned_finalized: bool,
    /// After governance set `is_active = false` and quorum arrived.
    finalized_while_inactive: bool,
    earned_while_inactive: u128,
    /// After governance set `is_active = true` again.
    finalized_after_reactivation: bool,
    earned_after_reactivation: u128,
}

/// #3347 Q3 (owner ruling 2026-09-27): a reward PLANNED while the station was
/// active but not yet settled is HELD while the station is inactive and
/// RELEASED when it is active again. Production path, gate active:
/// register active → commit → reveal WITHOUT quorum (planned, unsettled) →
/// governance re-registers `is_active = false` → both attestations (quorum
/// reached while inactive: must NOT settle) → governance re-registers
/// `is_active = true` (the deferred #2464 scan must now settle it).
///
/// Governance `is_active` is today's only inactivity source; the owner-ledger
/// freeze/deactivation will feed the same predicate, so the same hold/release
/// applies to them.
async fn run_hold_release_loop(seed: u64, tag: &str) -> HoldRelease {
    let _guard = lock_env().await;

    let station_id = format!("ws5000-3347-{tag}");
    let harness = setup_chain_without_station(seed, &station_id);
    let mut rng = StdRng::seed_from_u64(seed + 999);
    let governance_key = PrivateKey::generate(&mut rng);
    std::env::set_var(
        GOVERNANCE_ENV,
        PublicKey::from_private_key(&governance_key).to_hex(),
    );
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let mut pop = ProofOfPublishing::new(1, 60, 0, 16);
    pop.declare_no_genesis_oracles();

    let owner = harness.contributor.to_string();
    let register = |is_active: bool| {
        signed_registration_payload(
            &station_id,
            &owner,
            ContributionProvenanceClass::Real,
            is_active,
            &harness.contributor_key,
            &governance_key,
        )
    };
    // h1 register (active), h2 commit, h3 reveal with NO attestations.
    mine_block(
        &mut pop,
        &harness,
        &mempool,
        vec![registration_tx(&register(true), &format!("{tag}-on"))],
    )
    .await;
    let ts = 1_700_900_000i64;
    let (commit_payload, reveal_payload, commit_id) =
        build_weather_commit_and_reveal(&station_id, ts, tag);
    mine_block(
        &mut pop,
        &harness,
        &mempool,
        vec![commit_tx_from(&commit_payload, tag)],
    )
    .await;
    mine_block(
        &mut pop,
        &harness,
        &mempool,
        vec![reveal_tx_from(&reveal_payload, harness.contributor, tag)],
    )
    .await;
    let (planned_amount, planned_finalized) = {
        let chain = harness.blockchain.read().await;
        let record = chain
            .state
            .contribution_reward_records
            .get(&commit_id)
            .expect("the reveal must PLAN a reward record even without quorum");
        (
            record.amount.0,
            chain
                .state
                .finalized_contribution_rewards
                .contains(&commit_id),
        )
    };

    // h4 governance deactivates; h5 the quorum arrives while inactive.
    mine_block(
        &mut pop,
        &harness,
        &mempool,
        vec![registration_tx(&register(false), &format!("{tag}-off"))],
    )
    .await;
    mine_block(
        &mut pop,
        &harness,
        &mempool,
        vec![
            signed_attestation_tx(
                &commit_id,
                &harness.oracle_a.0,
                &harness.oracle_a.1,
                ts + 1,
                &format!("{tag}-a"),
            ),
            signed_attestation_tx(
                &commit_id,
                &harness.oracle_b.0,
                &harness.oracle_b.1,
                ts + 2,
                &format!("{tag}-b"),
            ),
        ],
    )
    .await;
    let earned_of = |chain: &Blockchain| {
        chain
            .state
            .get_account(&harness.contributor)
            .map(|a| a.earned_balance_total.0)
            .unwrap_or(0)
    };
    let (finalized_while_inactive, earned_while_inactive) = {
        let chain = harness.blockchain.read().await;
        (
            chain
                .state
                .finalized_contribution_rewards
                .contains(&commit_id),
            earned_of(&chain),
        )
    };

    // h6 governance reactivates: the held reward is released.
    mine_block(
        &mut pop,
        &harness,
        &mempool,
        vec![registration_tx(&register(true), &format!("{tag}-on-again"))],
    )
    .await;
    let (finalized_after_reactivation, earned_after_reactivation) = {
        let chain = harness.blockchain.read().await;
        (
            chain
                .state
                .finalized_contribution_rewards
                .contains(&commit_id),
            earned_of(&chain),
        )
    };
    std::env::remove_var(GOVERNANCE_ENV);

    HoldRelease {
        planned_amount,
        planned_finalized,
        finalized_while_inactive,
        earned_while_inactive,
        finalized_after_reactivation,
        earned_after_reactivation,
    }
}

#[tokio::test]
async fn inactive_station_is_paid_today_and_refused_from_activation() {
    // ── 1. Characterisation: today an INACTIVE Real station is paid. ───────
    assert!(
        try_feature_schedule().is_none(),
        "this file owns its process; no schedule may pre-exist"
    );
    let (count, total, real_paid, earned) = run_earn_loop(34_701, "inactive-today", false).await;
    assert_eq!(
        count, 1,
        "CHARACTERISATION: with no schedule an is_active=false Real station mints a reward"
    );
    assert!(
        total > 0 && real_paid > 0 && earned > 0,
        "and it is a real, non-zero payment"
    );

    // ── 2. The feature: known, unscheduled, halts only an old binary. ───────
    let name = ConsensusFeature::StationActiveRewardGate.name();
    assert_eq!(name, "station_active_reward_gate");
    assert_eq!(
        ConsensusFeature::parse(name),
        Some(ConsensusFeature::StationActiveRewardGate)
    );
    assert!(KNOWN_FEATURES.contains(&name));
    let live_genesis: [(&str, &str); 0] = [];
    assert_eq!(
        upgrade_required_at(live_genesis),
        None,
        "an unscheduled feature never halts a node"
    );
    let raw = format!("0:v1,{ACTIVATION_HEIGHT}:{name}");
    assert_eq!(upgrade_required_in_schedule(&raw), None);
    let pre_3347_known: Vec<&str> = KNOWN_FEATURES
        .iter()
        .copied()
        .filter(|k| *k != name)
        .collect();
    let required = upgrade_required_in_schedule_with(&raw, &pre_3347_known)
        .expect("a pre-#3347 binary must report the upgrade (#3093 clean halt)");
    assert_eq!(
        (required.feature.as_str(), required.height),
        (name, ACTIVATION_HEIGHT)
    );

    // ── 3. Schedule the gate at H = the reveal block's height (test-only). ─
    let metadata = [(GENESIS_ACTIVATION_SCHEDULE_KEY, raw.as_str())];
    let table = resolve_activation_schedule(metadata, SigningProtocolVersion::V1).unwrap();
    let features = resolve_feature_schedule(metadata, &table).unwrap();
    SigningProtocolVersion::install_active(SigningProtocolVersion::V1);
    install_activation_table(table);
    install_feature_schedule(features);
    assert!(!feature_active(
        ConsensusFeature::StationActiveRewardGate,
        ACTIVATION_HEIGHT - 1
    ));
    assert!(feature_active(
        ConsensusFeature::StationActiveRewardGate,
        ACTIVATION_HEIGHT
    ));

    // Anti-vacuity: an ACTIVE station is still paid at H.
    let (count, _, real_paid, earned) = run_earn_loop(34_702, "active-at-h", true).await;
    assert_eq!(
        count, 1,
        "an active Real station must still be paid from activation"
    );
    assert!(real_paid > 0 && earned > 0);

    // The fix: an INACTIVE station is not paid at H.
    let (count, total, real_paid, earned) = run_earn_loop(34_703, "inactive-at-h", false).await;
    assert_eq!(
        (count, total, real_paid, earned),
        (0, 0, 0, 0),
        "from activation an inactive station's reveal must not be paid"
    );

    // ── 3b. Q3 (owner ruling 2026-09-27): planned-but-unsettled rewards are
    //        HELD while inactive and RELEASED on reactivation. ──────────────
    let hr = run_hold_release_loop(34_704, "hold-release").await;
    assert!(
        hr.planned_amount > 0 && !hr.planned_finalized,
        "precondition: the no-quorum reveal planned an unsettled reward: {hr:?}"
    );
    assert!(
        !hr.finalized_while_inactive && hr.earned_while_inactive == 0,
        "Q3: a planned reward must be HELD (not settled) while the station is inactive: {hr:?}"
    );
    assert!(
        hr.finalized_after_reactivation && hr.earned_after_reactivation > 0,
        "Q3: the held reward must be RELEASED when the station is active again: {hr:?}"
    );

    // ── 4. The DRAFT owner-action preimage: pinned, every field bound. ─────
    let build = |chain: &str, net: &str, sid: &str, owner: &str, kind, nonce, exp| {
        station_owner_action_message_with_domain(chain, net, sid, owner, kind, nonce, exp)
    };
    let pinned = build(
        "777",
        "testnet",
        "ws5000",
        "wrf00",
        StationOwnerActionKind::ReportStolen,
        3,
        900,
    );
    let mut expected = STATION_OWNER_ACTION_DOMAIN.to_vec();
    for field in ["777", "testnet", "ws5000", "wrf00"] {
        expected.extend_from_slice(&(field.len() as u32).to_le_bytes());
        expected.extend_from_slice(field.as_bytes());
    }
    expected.push(1);
    expected.extend_from_slice(&3u64.to_le_bytes());
    expected.extend_from_slice(&900u64.to_le_bytes());
    assert_eq!(
        pinned, expected,
        "preimage must match the design doc's field-by-field encoding"
    );
    assert_eq!(
        &pinned[..STATION_OWNER_ACTION_DOMAIN.len()],
        b"wrfcoin_station_owner_action_v1"
    );
    let variants = [
        build(
            "778",
            "testnet",
            "ws5000",
            "wrf00",
            StationOwnerActionKind::ReportStolen,
            3,
            900,
        ),
        build(
            "777",
            "mainnet",
            "ws5000",
            "wrf00",
            StationOwnerActionKind::ReportStolen,
            3,
            900,
        ),
        build(
            "777",
            "testnet",
            "ws5001",
            "wrf00",
            StationOwnerActionKind::ReportStolen,
            3,
            900,
        ),
        build(
            "777",
            "testnet",
            "ws5000",
            "wrf01",
            StationOwnerActionKind::ReportStolen,
            3,
            900,
        ),
        build(
            "777",
            "testnet",
            "ws5000",
            "wrf00",
            StationOwnerActionKind::Unfreeze,
            3,
            900,
        ),
        build(
            "777",
            "testnet",
            "ws5000",
            "wrf00",
            StationOwnerActionKind::Deactivate,
            3,
            900,
        ),
        build(
            "777",
            "testnet",
            "ws5000",
            "wrf00",
            StationOwnerActionKind::Reactivate,
            3,
            900,
        ),
        build(
            "777",
            "testnet",
            "ws5000",
            "wrf00",
            StationOwnerActionKind::ReportStolen,
            4,
            900,
        ),
        build(
            "777",
            "testnet",
            "ws5000",
            "wrf00",
            StationOwnerActionKind::ReportStolen,
            3,
            901,
        ),
    ];
    for (i, v) in variants.iter().enumerate() {
        assert_ne!(v, &pinned, "variant {i} must change the preimage");
    }
    let discriminants = [
        StationOwnerActionKind::ReportStolen,
        StationOwnerActionKind::Unfreeze,
        StationOwnerActionKind::Deactivate,
        StationOwnerActionKind::Reactivate,
    ]
    .map(StationOwnerActionKind::discriminant);
    assert_eq!(
        discriminants,
        [1, 2, 3, 4],
        "action discriminants are frozen"
    );
}
