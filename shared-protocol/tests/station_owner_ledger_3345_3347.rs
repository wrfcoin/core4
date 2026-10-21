//! #3345 / #3347 — the per-station OWNER LEDGER, one test per owner ruling
//! (2026-09-27, infra#1095). RED-first: every assertion below was written
//! before the state field existed (it did not compile), and each guard has a
//! recorded mutation in the PR body that turns a named assertion RED.
//!
//! Rulings exercised (numbering = the PR's per-ruling table):
//!
//! - R0  unscheduled ⇒ nothing: no extraction at any height, every appended
//!       transition refused, the ledger empty and ABSENT from the root
//!       pre-image; below H the same.
//! - R1  a transfer needs the NEW owner's countersignature.
//! - R2  a transfer keeps provenance and reputation (only the owner moves).
//! - R3  replay safety: the per-station nonce and the pinned current owner.
//! - R4  a frozen station cannot be transferred.
//! - R5  governance keeps data levers only: a registration can neither
//!       reassign (R1 of #3345) nor lift a freeze; only an owner-signed
//!       unfreeze or the recovery lifts it.
//! - R6  lost key ⇒ time-locked recovery: governance files, the new owner
//!       COUNTERSIGNS (…_recovery_accept_v1), ~14 days = 34,000 blocks.
//! - R7  the current owner can cancel before filed + 34,000, not at/after.
//! - R8  execution is a PERMISSIONLESS transaction valid only from
//!       filed + 34,000 — no begin-block hook, no per-block scan; it moves
//!       the owner (provenance kept) and clears freeze and deactivation.
//! - R9  a freeze suspends PENALTIES: ZK-receipt slashing, #3214
//!       data-quality slashing and the collusion reputation penalty.
//! - R10 a freeze (or owner deactivation) suspends REWARDS on the production
//!       path, and a planned-but-unsettled reward is HELD while frozen and
//!       RELEASED on unfreeze.
//!
//! One test: the feature schedule is process-wide (first-install-wins
//! `OnceLock`), so this file owns its process.
//!
//! Run from core4:
//!   cargo test -p wrfcoin-shared-protocol --test station_owner_ledger_3345_3347

use std::sync::Arc;

use rand::SeedableRng;
use rand::rngs::StdRng;
use tokio::sync::{Mutex, MutexGuard, RwLock};
use wrfcoin_reputation_engine::SCORE_SCALE;
use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::state::{State, StateError, StateTransition};
use wrfcoin_shared_protocol::blockchain::station_owner::{
    STATION_OWNER_RECOVERY_NOTICE_BLOCKS, STATION_OWNER_TRANSFER_MAX_VALIDITY_BLOCKS,
    StationOwnerActionPayload, StationOwnerLedgerRefusal, StationOwnerRecoveryCancelPayload,
    StationOwnerRecoveryExecutePayload, StationOwnerRecoveryFilingPayload,
    StationOwnerTransferPayload,
};
use wrfcoin_shared_protocol::blockchain::transaction::{
    CommitPayload, OracleAttestationPayload, RevealPayload, StationOwnerActionKind,
    StationRegistrationPayload, Transaction, TransactionInput, TransactionOutput, TransactionType,
};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::ProofOfPublishing;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ChainIdentity, ConsensusFeature, GENESIS_ACTIVATION_SCHEDULE_KEY, SigningProtocolVersion,
    feature_active, install_activation_table, install_feature_schedule,
    resolve_activation_schedule, resolve_feature_schedule, try_feature_schedule,
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
/// Both owner features activate at H. The production-path loops mine the
/// registration at h1 (legacy variant) and the first owner action at h2.
const H: u64 = 2;
const LEDGER_ROOT_TAG: &[u8] = b"wrfcoin_station_owner_ledger_v1";

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

/// Enqueue `txs` and mine one block on the production path.
async fn mine_block(
    pop: &mut ProofOfPublishing,
    harness: &Harness,
    mempool: &Arc<RwLock<Mempool>>,
    txs: Vec<Transaction>,
) -> wrfcoin_shared_protocol::blockchain::block::Block {
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
    .expect("mine block")
}

// ─── Owner-ledger fixtures ─────────────────────────────────────────────────

fn key(seed: u8) -> PrivateKey {
    PrivateKey::from_bytes(&[seed; 32]).expect("32-byte ed25519 seed")
}

fn governance_key() -> PrivateKey {
    key(0x60)
}

fn addr_of(k: &PrivateKey) -> String {
    PublicKey::from_private_key(k).to_address()
}

fn pk_of(k: &PrivateKey) -> String {
    PublicKey::from_private_key(k).to_hex()
}

fn chain() -> (String, String) {
    let id = ChainIdentity::active();
    (id.chain_id.clone(), id.network_id.clone())
}

/// A `StationRegistration`-carried owner-ledger tx around `data`.
fn ledger_tx(tag: &str, data: String) -> Transaction {
    let mut tx = build_tx(
        TransactionType::StationRegistration,
        &format!("ledger-{tag}"),
    );
    tx.data = Some(data);
    tx.set_id();
    tx
}

fn transfer_json(
    station: &str,
    from: &PrivateKey,
    to: &PrivateKey,
    nonce: u64,
    expires: u64,
    countersign: bool,
) -> String {
    let (c, n) = chain();
    let mut p = StationOwnerTransferPayload::new(
        station,
        addr_of(from),
        pk_of(from),
        addr_of(to),
        pk_of(to),
        nonce,
        expires,
    );
    p.sign_as_owner_with_domain(&c, &n, from);
    if countersign {
        p.countersign_with_domain(&c, &n, to);
    }
    serde_json::to_string(&p).unwrap()
}

fn action_json(
    station: &str,
    owner: &PrivateKey,
    kind: StationOwnerActionKind,
    nonce: u64,
    expires: u64,
) -> String {
    let (c, n) = chain();
    let mut p =
        StationOwnerActionPayload::new(station, addr_of(owner), pk_of(owner), kind, nonce, expires);
    p.sign_with_domain(&c, &n, owner);
    serde_json::to_string(&p).unwrap()
}

fn filing_json(
    station: &str,
    from: &PrivateKey,
    to: &PrivateKey,
    nonce: u64,
    governance: &PrivateKey,
    accept: bool,
) -> String {
    let (c, n) = chain();
    let mut p = StationOwnerRecoveryFilingPayload::new(
        station,
        addr_of(from),
        addr_of(to),
        pk_of(to),
        nonce,
    );
    p.sign_as_governance_with_domain(&c, &n, governance);
    if accept {
        p.accept_with_domain(&c, &n, to);
    }
    serde_json::to_string(&p).unwrap()
}

fn cancel_json(station: &str, owner: &PrivateKey, nonce: u64) -> String {
    let (c, n) = chain();
    let mut p =
        StationOwnerRecoveryCancelPayload::new(station, addr_of(owner), pk_of(owner), nonce);
    p.sign_with_domain(&c, &n, owner);
    serde_json::to_string(&p).unwrap()
}

fn execute_json(station: &str, nonce: u64) -> String {
    serde_json::to_string(&StationOwnerRecoveryExecutePayload::new(station, nonce)).unwrap()
}

/// Extract through the ONE shared constructor, then apply. Returns whether a
/// transition was extracted.
fn step(state: &mut State, tag: &str, data: String, height: u64) -> bool {
    match ledger_tx(tag, data).station_owner_ledger_transition(height) {
        Some(t) => {
            state.apply_transition(t).unwrap_or_else(|e| {
                panic!("[{tag}] an extracted owner step never fails the block: {e}")
            });
            true
        },
        None => false,
    }
}

fn registration(station: &str, owner: &PrivateKey, is_active: bool) -> StationRegistrationPayload {
    signed_registration_payload(
        station,
        &addr_of(owner),
        ContributionProvenanceClass::Real,
        is_active,
        owner,
        &governance_key(),
    )
}

/// A State with `station` registered (Real, active) to `owner`, with a
/// non-zero reputation so R2 can prove it is kept.
fn state_with_station(station: &str, owner: &PrivateKey) -> State {
    let mut state = State::new();
    state
        .apply_station_registration(Transaction::station_from_registration(&registration(
            station, owner, true,
        )))
        .unwrap();
    let mut record = state.station_registry.get(station).unwrap().clone();
    record.reputation_score = 777;
    state.station_registry.upsert(record).unwrap();
    state
}

fn entry(
    state: &State,
    station: &str,
) -> wrfcoin_shared_protocol::blockchain::station_owner::StationOwnerLedgerEntry {
    state
        .station_owner_ledger
        .get(station)
        .cloned()
        .unwrap_or_default()
}

fn owner(state: &State, station: &str) -> String {
    state
        .station_registry
        .get(station)
        .unwrap()
        .owner_address
        .clone()
}

fn every_appended_transition(station: &str, height: u64) -> Vec<StateTransition> {
    vec![
        StateTransition::ApplyStationOwnerTransfer {
            station_id: station.into(),
            from_owner_address: addr_of(&key(1)),
            to_owner_address: addr_of(&key(2)),
            transfer_nonce: 0,
            block_height: height,
        },
        StateTransition::ApplyStationOwnerAction {
            station_id: station.into(),
            owner_address: addr_of(&key(1)),
            action: StationOwnerActionKind::ReportStolen,
            action_nonce: 0,
            block_height: height,
        },
        StateTransition::ApplyStationOwnerRecoveryFiling {
            station_id: station.into(),
            from_owner_address: addr_of(&key(1)),
            to_owner_address: addr_of(&key(2)),
            recovery_nonce: 0,
            block_height: height,
        },
        StateTransition::ApplyStationOwnerRecoveryCancel {
            station_id: station.into(),
            owner_address: addr_of(&key(1)),
            recovery_nonce: 0,
            block_height: height,
        },
        StateTransition::ApplyStationOwnerRecoveryExecute {
            station_id: station.into(),
            recovery_nonce: 0,
            block_height: height,
        },
    ]
}

fn every_valid_kind(station: &str, expires: u64) -> Vec<(&'static str, String)> {
    let (a, b) = (key(1), key(2));
    vec![
        ("transfer", transfer_json(station, &a, &b, 0, expires, true)),
        (
            "action",
            action_json(
                station,
                &a,
                StationOwnerActionKind::ReportStolen,
                0,
                expires,
            ),
        ),
        (
            "file",
            filing_json(station, &a, &b, 0, &governance_key(), true),
        ),
        ("cancel", cancel_json(station, &a, 0)),
        ("execute", execute_json(station, 0)),
    ]
}

fn ledger_in_root(state: &State) -> bool {
    state
        .state_root_material()
        .windows(LEDGER_ROOT_TAG.len())
        .any(|w| w == LEDGER_ROOT_TAG)
}

/// R0 (unscheduled, or below H): nothing is extracted, every appended
/// transition is refused, the ledger stays empty and out of the root.
fn assert_inert_at(height: u64) {
    let station = "ws5000-r0";
    let mut state = state_with_station(station, &key(1));
    let before = state.state_root_material();
    for (kind, data) in every_valid_kind(station, height.saturating_add(100)) {
        assert!(
            ledger_tx(kind, data)
                .station_owner_ledger_transition(height)
                .is_none(),
            "R0: {kind} must not be extracted at h{height}"
        );
    }
    for t in every_appended_transition(station, height) {
        assert!(
            matches!(
                state.apply_transition(t),
                Err(StateError::InvalidTransition)
            ),
            "R0: an appended owner transition must be refused at h{height}"
        );
    }
    assert!(state.station_owner_ledger.is_empty());
    assert_eq!(
        state.state_root_material(),
        before,
        "R0: root material unchanged"
    );
    assert!(
        !ledger_in_root(&state),
        "R0: an empty ledger is absent from the root pre-image"
    );
}

// ─── R9: freeze suspends penalties ─────────────────────────────────────────

fn frozen_at(state: &mut State, station: &str, owner: &PrivateKey, height: u64) {
    let nonce = entry(state, station).owner_nonce;
    assert!(step(
        state,
        "freeze",
        action_json(
            station,
            owner,
            StationOwnerActionKind::ReportStolen,
            nonce,
            height + 10
        ),
        height,
    ));
    assert!(entry(state, station).frozen.is_some());
}

fn reveal_payload(
    station: &str,
    data: serde_json::Value,
    tag: &str,
) -> (CommitPayload, RevealPayload) {
    let ts = 1_700_000_100_i64;
    let nonce = format!("nonce-{tag}");
    let commit_id = Transaction::compute_commit_id(&data, &nonce, station, ts, "weather");
    (
        CommitPayload {
            commit_id: commit_id.clone(),
            station_id: station.into(),
            timestamp: ts,
            contribution_type: "weather".into(),
            auth_pubkey: None,
            auth_signature: None,
        },
        RevealPayload {
            commit_id,
            station_id: station.into(),
            timestamp: ts,
            contribution_type: "weather".into(),
            data,
            nonce,
            data_cid: None,
            data_hash: None,
            storage_provider: None,
            provenance: None,
            manifest: None,
            verification_receipt: None,
            compute_receipt: None,
            quality_tier: None,
        },
    )
}

/// Data-quality (#525 / #3214 hook): a physically impossible reveal.
fn data_quality_reveal(state: &mut State, station: &str, height: u64) -> Result<(), StateError> {
    let (commit, reveal) = reveal_payload(
        station,
        serde_json::json!({"temperature": 20.0, "humidity": 55.0, "pressure": 1013.0, "wind_speed": 200.0}),
        &format!("dq-{height}"),
    );
    state
        .apply_transition(StateTransition::RegisterCommit {
            payload: commit,
            block_height: height - 1,
        })
        .unwrap();
    state.apply_transition(StateTransition::ApplyReveal {
        block_timestamp: 1_700_000_100,
        payload: reveal,
        block_height: height,
    })
}

/// ZK-receipt hook: a verification receipt from an UNREGISTERED oracle builds
/// an `OracleNotRegistered` slashing trigger.
fn zk_receipt_reveal(state: &mut State, station: &str, height: u64) -> Result<(), StateError> {
    use wrfcoin_forecast_score::ForecastAccuracyMetrics;
    use wrfcoin_forecast_score::blockchain::{
        ForecastArtifactManifest, ForecastVerificationReceipt,
    };
    let (commit, mut reveal) = reveal_payload(
        station,
        serde_json::json!({"temperature": 20.0, "humidity": 55.0, "pressure": 1013.0, "wind_speed": 3.0}),
        &format!("zk-{height}"),
    );
    reveal.manifest = Some(ForecastArtifactManifest {
        schema_version: 1,
        model_id: "7".into(),
        input_hash: "11".repeat(32),
        output_hash: "22".repeat(32),
        produced_at: 1_700_000_000,
        region: "duluth".into(),
        horizon_hours: 6,
        input_variables: vec!["t".into()],
        output_variables: vec!["t".into()],
    });
    reveal.verification_receipt = Some(ForecastVerificationReceipt {
        manifest_digest: "aa".repeat(32),
        commit_id: reveal.commit_id.clone(),
        oracle_id: "oracle-not-registered-3347".into(),
        metrics: ForecastAccuracyMetrics {
            mae: 0.5,
            rmse: 0.75,
            bias: 0.0,
            skill_score: 0.5,
        },
        verified_at: 1_700_000_000_000,
        nonce: "bc".repeat(16),
        zk_proof: None,
        signature: None,
    });
    state
        .apply_transition(StateTransition::RegisterCommit {
            payload: commit,
            block_height: height - 1,
        })
        .unwrap();
    state.apply_transition(StateTransition::ApplyReveal {
        block_timestamp: 1_700_000_100,
        payload: reveal,
        block_height: height,
    })
}

fn is_slashed(result: &Result<(), StateError>) -> bool {
    matches!(result, Err(StateError::SlashingTriggered(_)))
}

/// Collusion hook: two OTHER stations ≥1 km apart, near `station`, recently
/// reported identical readings; `station`'s reveal is then penalised -10
/// reputation (testnet: reputation only). Returns the score delta.
fn collusion_reveal_delta(
    state: &mut State,
    station: &str,
    owner: &PrivateKey,
    height: u64,
) -> i128 {
    use wrfcoin_shared_protocol::weather::validation::WeatherObservation;
    for (sid, lat) in [("ws5000-coll-b", 46.80), ("ws5000-coll-c", 46.82)] {
        let mut p = registration(sid, owner, true);
        p.latitude = lat;
        state
            .apply_station_registration(Transaction::station_from_registration(&p))
            .unwrap();
        state.recent_weather_observations.insert(
            sid.to_string(),
            (
                WeatherObservation {
                    temperature_c: Some(12.0),
                    humidity_percent: Some(70.0),
                    pressure_hpa: Some(1010.0),
                    wind_speed_ms: Some(4.0),
                    ..Default::default()
                },
                height - 1,
            ),
        );
    }
    let before = state.oracle_registry().reputation_score(station) as i128;
    let (commit, reveal) = reveal_payload(
        station,
        serde_json::json!({"temperature": 12.5, "humidity": 69.0, "pressure": 1011.0, "wind_speed": 4.2}),
        &format!("coll-{height}"),
    );
    state
        .apply_transition(StateTransition::RegisterCommit {
            payload: commit,
            block_height: height - 1,
        })
        .unwrap();
    let _ = state.apply_transition(StateTransition::ApplyReveal {
        block_timestamp: 1_700_000_100,
        payload: reveal,
        block_height: height,
    });
    state.oracle_registry().reputation_score(station) as i128 - before
}

// ─── R10: production-path rewards ──────────────────────────────────────────

/// Production path: h1 register (Real, active) → h2 owner `kind` (freeze or
/// deactivate) → h3 commit → h4 reveal + 2-of-2 quorum. `kind = None` is the
/// anti-vacuity control. Returns (rewards_minted_count, earned).
async fn fresh_reveal_after(
    seed: u64,
    tag: &str,
    kind: Option<StationOwnerActionKind>,
) -> (u64, u128) {
    let _guard = lock_env().await;
    let station_id = format!("ws5000-ledger-{tag}");
    let harness = setup_chain_without_station(seed, &station_id);
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let mut pop = ProofOfPublishing::new(1, 60, 0, 16);
    pop.declare_no_genesis_oracles();
    let owner_key = &harness.contributor_key;
    let owner_addr = harness.contributor.to_string();
    let reg = signed_registration_payload(
        &station_id,
        &owner_addr,
        ContributionProvenanceClass::Real,
        true,
        owner_key,
        &governance_key(),
    );
    mine_block(
        &mut pop,
        &harness,
        &mempool,
        vec![registration_tx(&reg, tag)],
    )
    .await;
    let (c, n) = chain();
    let action_tx = |kind: StationOwnerActionKind, nonce: u64, t: &str| {
        let mut p = StationOwnerActionPayload::new(
            &station_id,
            &owner_addr,
            pk_of(owner_key),
            kind,
            nonce,
            1_000,
        );
        p.sign_with_domain(&c, &n, owner_key);
        ledger_tx(t, serde_json::to_string(&p).unwrap())
    };
    let txs = kind
        .map(|k| vec![action_tx(k, 0, &format!("{tag}-act"))])
        .unwrap_or_default();
    mine_block(&mut pop, &harness, &mempool, txs).await;
    {
        let chain = harness.blockchain.read().await;
        let e = chain
            .state
            .station_owner_ledger
            .get(&station_id)
            .cloned()
            .unwrap_or_default();
        match kind {
            Some(StationOwnerActionKind::ReportStolen) => {
                assert!(e.frozen.is_some(), "mined freeze applied")
            },
            Some(StationOwnerActionKind::Deactivate) => {
                assert!(e.owner_deactivated, "mined deactivate applied")
            },
            _ => assert!(chain.state.station_owner_ledger.is_empty()),
        }
    }
    let ts = 1_700_800_000i64 + seed as i64;
    let (commit, reveal, commit_id) = build_weather_commit_and_reveal(&station_id, ts, tag);
    mine_block(
        &mut pop,
        &harness,
        &mempool,
        vec![commit_tx_from(&commit, tag)],
    )
    .await;
    let block = mine_block(
        &mut pop,
        &harness,
        &mempool,
        vec![
            reveal_tx_from(&reveal, harness.contributor, tag),
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
    let chain = harness.blockchain.read().await;
    let earned = chain
        .state
        .get_account(&harness.contributor)
        .map(|a| a.earned_balance_total.0)
        .unwrap_or(0);
    (metadata_u64(&block, "rewards_minted_count"), earned)
}

#[derive(Debug)]
struct HoldRelease {
    planned: u128,
    finalized_while_frozen: bool,
    earned_while_frozen: u128,
    finalized_after_unfreeze: bool,
    earned_after_unfreeze: u128,
}

/// Q3 via the OWNER LEDGER: h1 register → h2 commit → h3 reveal without
/// quorum (planned) → h4 owner report_stolen → h5 quorum while frozen (HELD)
/// → h6 owner unfreeze (RELEASED).
async fn hold_release_via_freeze(seed: u64, tag: &str) -> HoldRelease {
    let _guard = lock_env().await;
    let station_id = format!("ws5000-ledger-{tag}");
    let harness = setup_chain_without_station(seed, &station_id);
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let mut pop = ProofOfPublishing::new(1, 60, 0, 16);
    pop.declare_no_genesis_oracles();
    let owner_key = &harness.contributor_key;
    let owner_addr = harness.contributor.to_string();
    let reg = signed_registration_payload(
        &station_id,
        &owner_addr,
        ContributionProvenanceClass::Real,
        true,
        owner_key,
        &governance_key(),
    );
    mine_block(
        &mut pop,
        &harness,
        &mempool,
        vec![registration_tx(&reg, tag)],
    )
    .await;
    let ts = 1_700_900_000i64 + seed as i64;
    let (commit, reveal, commit_id) = build_weather_commit_and_reveal(&station_id, ts, tag);
    mine_block(
        &mut pop,
        &harness,
        &mempool,
        vec![commit_tx_from(&commit, tag)],
    )
    .await;
    mine_block(
        &mut pop,
        &harness,
        &mempool,
        vec![reveal_tx_from(&reveal, harness.contributor, tag)],
    )
    .await;
    let planned = harness
        .blockchain
        .read()
        .await
        .state
        .contribution_reward_records
        .get(&commit_id)
        .map(|r| r.amount.0)
        .expect("the no-quorum reveal plans a reward record");
    let (c, n) = chain();
    let action_tx = |kind: StationOwnerActionKind, nonce: u64, t: &str| {
        let mut p = StationOwnerActionPayload::new(
            &station_id,
            &owner_addr,
            pk_of(owner_key),
            kind,
            nonce,
            1_000,
        );
        p.sign_with_domain(&c, &n, owner_key);
        ledger_tx(t, serde_json::to_string(&p).unwrap())
    };
    mine_block(
        &mut pop,
        &harness,
        &mempool,
        vec![action_tx(
            StationOwnerActionKind::ReportStolen,
            0,
            &format!("{tag}-freeze"),
        )],
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
    let observe = |chain: &Blockchain| {
        (
            chain
                .state
                .finalized_contribution_rewards
                .contains(&commit_id),
            chain
                .state
                .get_account(&harness.contributor)
                .map(|a| a.earned_balance_total.0)
                .unwrap_or(0),
        )
    };
    let (finalized_while_frozen, earned_while_frozen) = observe(&*harness.blockchain.read().await);
    mine_block(
        &mut pop,
        &harness,
        &mempool,
        vec![action_tx(
            StationOwnerActionKind::Unfreeze,
            1,
            &format!("{tag}-unfreeze"),
        )],
    )
    .await;
    let (finalized_after_unfreeze, earned_after_unfreeze) =
        observe(&*harness.blockchain.read().await);
    HoldRelease {
        planned,
        finalized_while_frozen,
        earned_while_frozen,
        finalized_after_unfreeze,
        earned_after_unfreeze,
    }
}

#[tokio::test]
async fn owner_ledger_rulings_2026_09_27() {
    std::env::set_var(GOVERNANCE_ENV, pk_of(&governance_key()));

    // ── R0a. Unscheduled: inert at every height. ───────────────────────────
    assert!(
        try_feature_schedule().is_none(),
        "this file owns its process"
    );
    for h in [0, H, 1_000_000] {
        assert_inert_at(h);
    }
    let name = ConsensusFeature::StationOwnerActions.name();
    assert_eq!(name, "station_owner_actions");
    assert!(KNOWN_FEATURES.contains(&name));
    let live_genesis: [(&str, &str); 0] = [];
    assert_eq!(
        upgrade_required_at(live_genesis),
        None,
        "unscheduled never halts"
    );
    let raw = format!(
        "0:v1,{H}:{},{H}:{name}",
        ConsensusFeature::StationOwnerConsent.name()
    );
    assert_eq!(upgrade_required_in_schedule(&raw), None);
    let pre_ledger: Vec<&str> = KNOWN_FEATURES
        .iter()
        .copied()
        .filter(|k| *k != name)
        .collect();
    let required = upgrade_required_in_schedule_with(&raw, &pre_ledger)
        .expect("a binary without station_owner_actions must report the #3093 upgrade");
    assert_eq!((required.feature.as_str(), required.height), (name, H));

    // ── Schedule both owner features at H (test-only, as the boot gate would).
    let metadata = [(GENESIS_ACTIVATION_SCHEDULE_KEY, raw.as_str())];
    let table = resolve_activation_schedule(metadata, SigningProtocolVersion::V1).unwrap();
    let features = resolve_feature_schedule(metadata, &table).unwrap();
    SigningProtocolVersion::install_active(SigningProtocolVersion::V1);
    install_activation_table(table);
    install_feature_schedule(features);
    assert!(feature_active(ConsensusFeature::StationOwnerConsent, H));
    assert!(feature_active(ConsensusFeature::StationOwnerActions, H));
    assert!(
        !feature_active(ConsensusFeature::StationActiveRewardGate, u64::MAX),
        "the ledger half of the reward predicate must work WITHOUT the #3351 R2 gate"
    );

    // ── R0b. Below H: the same inertness. ──────────────────────────────────
    assert_inert_at(H - 1);

    let (a, b, c, mallory) = (key(0xA1), key(0xB2), key(0xC3), key(0xE5));
    let s = "ws5000-ledger";
    let h = 1_000u64;

    // ── R1. The new owner's countersignature is mandatory. ─────────────────
    let mut state = state_with_station(s, &a);
    assert!(
        !step(
            &mut state,
            "t-nocs",
            transfer_json(s, &a, &b, 0, h + 5, false),
            h
        ),
        "R1: a transfer without the new owner's countersignature is not extracted"
    );
    assert_eq!(owner(&state, s), addr_of(&a));
    // Validity window (block heights only).
    assert!(
        !step(
            &mut state,
            "t-exp",
            transfer_json(s, &a, &b, 0, h - 1, true),
            h
        ),
        "expired"
    );
    assert!(
        !step(
            &mut state,
            "t-long",
            transfer_json(
                s,
                &a,
                &b,
                0,
                h + STATION_OWNER_TRANSFER_MAX_VALIDITY_BLOCKS + 1,
                true
            ),
            h
        ),
        "a validity window over 10,000 blocks is refused"
    );
    assert!(
        state.station_owner_ledger.is_empty(),
        "no refused step creates a ledger entry"
    );
    assert!(!ledger_in_root(&state));

    // ── R2. Transfer: only the owner moves; provenance + reputation kept. ──
    let before = state.station_registry.get(s).unwrap().clone();
    let counts_before = state.station_counts.clone();
    assert!(step(
        &mut state,
        "t-ok",
        transfer_json(s, &a, &b, 0, h + 5, true),
        h
    ));
    let after = state.station_registry.get(s).unwrap().clone();
    assert_eq!(
        after.owner_address,
        addr_of(&b),
        "R2: owner moved to the countersigner"
    );
    let mut expected = before.clone();
    expected.owner_address = addr_of(&b);
    assert_eq!(
        after, expected,
        "R2: every other field (provenance, reputation, …) is kept"
    );
    assert_eq!(after.provenance_class, ContributionProvenanceClass::Real);
    assert_eq!(after.reputation_score, 777);
    assert_eq!(state.station_counts, counts_before);
    assert_eq!(entry(&state, s).owner_nonce, 1);
    assert!(
        ledger_in_root(&state),
        "a non-empty ledger IS root material"
    );

    // ── R3. Replay safety. ─────────────────────────────────────────────────
    let root_before = state.state_root_material();
    assert!(step(
        &mut state,
        "t-replay",
        transfer_json(s, &a, &b, 0, h + 5, true),
        h + 1
    ));
    assert_eq!(
        state.state_root_material(),
        root_before,
        "R3: a replayed transfer is an inert no-op"
    );
    assert_eq!(
        state.apply_station_owner_transfer(s, &addr_of(&a), &addr_of(&b), 0),
        Err(StationOwnerLedgerRefusal::NotRegisteredOwner),
        "R3: the pinned from-owner kills the old signature"
    );
    assert_eq!(
        state.apply_station_owner_transfer(s, &addr_of(&b), &addr_of(&a), 0),
        Err(StationOwnerLedgerRefusal::NonceMismatch),
        "R3: a consumed nonce cannot be reused"
    );
    assert_eq!(
        state.apply_station_owner_action(
            s,
            &addr_of(&mallory),
            StationOwnerActionKind::ReportStolen,
            1,
            h
        ),
        Err(StationOwnerLedgerRefusal::NotRegisteredOwner),
        "only the registered owner acts"
    );

    // ── R4 + R5. Freeze: no transfer; governance cannot lift it. ───────────
    frozen_at(&mut state, s, &b, h + 2);
    assert_eq!(entry(&state, s).owner_nonce, 2);
    assert!(step(
        &mut state,
        "t-frozen",
        transfer_json(s, &b, &a, 2, h + 9, true),
        h + 3
    ));
    assert_eq!(
        owner(&state, s),
        addr_of(&b),
        "R4: a frozen station cannot be transferred"
    );
    assert_eq!(
        state.apply_station_owner_transfer(s, &addr_of(&b), &addr_of(&a), 2),
        Err(StationOwnerLedgerRefusal::Frozen)
    );
    for is_active in [true, false, true] {
        let reg = registration(s, &b, is_active);
        state
            .apply_transition(Transaction::station_registration_transition(&reg, h + 4))
            .unwrap();
    }
    assert!(
        entry(&state, s).frozen.is_some(),
        "R5: a governance registration cannot lift a freeze"
    );
    let hijack = registration(s, &mallory, true);
    state
        .apply_transition(Transaction::station_registration_transition(&hijack, h + 4))
        .unwrap();
    assert_eq!(
        owner(&state, s),
        addr_of(&b),
        "R5: governance cannot reassign (#3345 R1)"
    );
    // A non-owner "unfreeze": the key is bound to Mallory, not to the owner.
    assert!(step(
        &mut state,
        "unfreeze-mallory",
        action_json(s, &mallory, StationOwnerActionKind::Unfreeze, 2, h + 9),
        h + 5
    ));
    assert!(
        entry(&state, s).frozen.is_some(),
        "R5: only the OWNER's unfreeze lifts it"
    );
    // The ledger stops rewards while frozen (the predicate the PoP lanes use).
    assert!(state.station_owner_ledger_refuses_rewards(s, h + 5));
    assert!(state.station_penalties_suspended(s, h + 5));
    assert!(
        !state.station_owner_ledger_refuses_rewards(s, H - 1),
        "gated below H"
    );
    assert!(
        !state.station_penalties_suspended(s, H - 1),
        "gated below H"
    );
    assert!(step(
        &mut state,
        "unfreeze",
        action_json(s, &b, StationOwnerActionKind::Unfreeze, 2, h + 9),
        h + 5
    ));
    assert!(
        entry(&state, s).frozen.is_none(),
        "R5: the owner-signed unfreeze lifts it"
    );
    assert!(!state.station_owner_ledger_refuses_rewards(s, h + 5));

    // Deactivate / reactivate preconditions.
    assert!(step(
        &mut state,
        "deact",
        action_json(s, &b, StationOwnerActionKind::Deactivate, 3, h + 9),
        h + 6
    ));
    assert!(entry(&state, s).owner_deactivated);
    assert!(
        state.station_owner_ledger_refuses_rewards(s, h + 6),
        "owner deactivation stops rewards"
    );
    assert!(
        !state.station_penalties_suspended(s, h + 6),
        "deactivation alone does not suspend penalties"
    );
    assert_eq!(
        state.apply_station_owner_action(
            s,
            &addr_of(&b),
            StationOwnerActionKind::Deactivate,
            4,
            h + 6
        ),
        Err(StationOwnerLedgerRefusal::AlreadyDeactivated)
    );
    assert!(step(
        &mut state,
        "react",
        action_json(s, &b, StationOwnerActionKind::Reactivate, 4, h + 9),
        h + 7
    ));
    assert!(!entry(&state, s).owner_deactivated);
    assert_eq!(
        state.apply_station_owner_action(
            s,
            &addr_of(&b),
            StationOwnerActionKind::Unfreeze,
            5,
            h + 7
        ),
        Err(StationOwnerLedgerRefusal::NotFrozen)
    );
    assert_eq!(entry(&state, s).owner_nonce, 5);

    // ── R6. Recovery filing: governance + the new owner's acceptance. ──────
    assert!(
        !step(
            &mut state,
            "file-noaccept",
            filing_json(s, &b, &c, 5, &governance_key(), false),
            h + 8
        ),
        "R6: a filing without the new owner's countersignature is not extracted"
    );
    assert!(
        !step(
            &mut state,
            "file-notgov",
            filing_json(s, &b, &c, 5, &mallory, true),
            h + 8
        ),
        "R6: only an allowlisted governance key files"
    );
    let filed = h + 10;
    assert!(step(
        &mut state,
        "file",
        filing_json(s, &b, &c, 5, &governance_key(), true),
        filed
    ));
    let e = entry(&state, s);
    let pending = e
        .pending_recovery
        .clone()
        .expect("R6: a pending recovery is recorded");
    assert_eq!(
        (pending.filed_at_height, pending.recovery_nonce),
        (filed, 5)
    );
    assert_eq!(e.owner_nonce, 5, "filing does not consume the nonce");
    assert_eq!(
        owner(&state, s),
        addr_of(&b),
        "filing does not move the owner"
    );
    assert_eq!(
        state.apply_station_owner_recovery_filing(s, &addr_of(&b), &addr_of(&c), 5, filed + 1),
        Err(StationOwnerLedgerRefusal::RecoveryAlreadyPending)
    );
    assert_eq!(STATION_OWNER_RECOVERY_NOTICE_BLOCKS, 34_000);

    // ── R7. The current owner cancels only BEFORE filed + 34,000. ──────────
    assert_eq!(
        state.apply_station_owner_recovery_cancel(s, &addr_of(&mallory), 5, filed + 1),
        Err(StationOwnerLedgerRefusal::NotRegisteredOwner)
    );
    let mut late = state.clone();
    assert_eq!(
        late.apply_station_owner_recovery_cancel(
            s,
            &addr_of(&b),
            5,
            filed + STATION_OWNER_RECOVERY_NOTICE_BLOCKS
        ),
        Err(StationOwnerLedgerRefusal::NoticeWindowElapsed),
        "R7: no cancel at/after filed + 34,000"
    );
    let mut cancelled = state.clone();
    assert!(step(
        &mut cancelled,
        "cancel-last-block",
        cancel_json(s, &b, 5),
        filed + STATION_OWNER_RECOVERY_NOTICE_BLOCKS - 1
    ));
    assert!(
        entry(&cancelled, s).pending_recovery.is_none(),
        "R7: cancelled in the last notice block"
    );
    assert_eq!(
        entry(&cancelled, s).owner_nonce,
        6,
        "cancel consumes the nonce"
    );

    // Any owner step that consumes the nonce voids a pending recovery.
    let mut voided = state.clone();
    assert!(step(
        &mut voided,
        "void",
        action_json(s, &b, StationOwnerActionKind::Deactivate, 5, filed + 9),
        filed + 1
    ));
    assert!(entry(&voided, s).pending_recovery.is_none());
    assert_eq!(
        voided.apply_station_owner_recovery_execute(
            s,
            5,
            filed + STATION_OWNER_RECOVERY_NOTICE_BLOCKS
        ),
        Err(StationOwnerLedgerRefusal::NoPendingRecovery)
    );

    // ── R8. Execute: permissionless tx, only from filed + 34,000. ──────────
    // No hook: finalization past the window executes nothing.
    let mut idle = state.clone();
    idle.apply_block_finalization(filed + STATION_OWNER_RECOVERY_NOTICE_BLOCKS + 5);
    assert!(
        entry(&idle, s).pending_recovery.is_some(),
        "R8: no begin-block hook / scan"
    );
    assert_eq!(owner(&idle, s), addr_of(&b));
    // Freeze + deactivate the station first (a thief holds it), then re-file:
    // execution must clear both. (Deactivate, then report stolen.)
    let mut rec = cancelled.clone(); // nonce 6, nothing pending
    assert!(step(
        &mut rec,
        "rec-deact",
        action_json(s, &b, StationOwnerActionKind::Deactivate, 6, filed + 9),
        filed + 2
    ));
    frozen_at(&mut rec, s, &b, filed + 3);
    let refiled = filed + 4;
    assert!(step(
        &mut rec,
        "refile",
        filing_json(s, &b, &c, 8, &governance_key(), true),
        refiled
    ));
    let execute_tx = ledger_tx("execute", execute_json(s, 8));
    assert!(execute_tx.inputs.iter().all(|i| i.signature.is_none()));
    assert!(
        execute_tx
            .station_owner_ledger_transition(refiled)
            .is_some(),
        "R8: execute is extracted with no signature at all (permissionless)"
    );
    let too_early = refiled + STATION_OWNER_RECOVERY_NOTICE_BLOCKS - 1;
    assert_eq!(
        rec.clone()
            .apply_station_owner_recovery_execute(s, 8, too_early),
        Err(StationOwnerLedgerRefusal::NoticeWindowNotElapsed),
        "R8: not one block early"
    );
    assert!(step(&mut rec, "exec-early", execute_json(s, 8), too_early));
    assert_eq!(
        owner(&rec, s),
        addr_of(&b),
        "R8: an early execute is an inert no-op"
    );
    let at = refiled + STATION_OWNER_RECOVERY_NOTICE_BLOCKS;
    let before_exec = rec.station_registry.get(s).unwrap().clone();
    assert!(step(&mut rec, "exec", execute_json(s, 8), at));
    let after_exec = rec.station_registry.get(s).unwrap().clone();
    assert_eq!(
        after_exec.owner_address,
        addr_of(&c),
        "R8: executed at exactly filed + 34,000"
    );
    let mut expected = before_exec.clone();
    expected.owner_address = addr_of(&c);
    assert_eq!(after_exec, expected, "R8: provenance and reputation kept");
    let e = entry(&rec, s);
    assert!(e.frozen.is_none() && !e.owner_deactivated && e.pending_recovery.is_none());
    assert_eq!(e.owner_nonce, 9);
    assert!(!rec.station_owner_ledger_refuses_rewards(s, at));
    assert!(step(&mut rec, "exec-replay", execute_json(s, 8), at + 1));
    assert_eq!(
        owner(&rec, s),
        addr_of(&c),
        "an executed recovery cannot replay"
    );

    // ── R9. A freeze suspends penalties (and only from H). ─────────────────
    let p = "ws5000-penalty";
    let hp = 2_000u64;
    // Data-quality slashing.
    let mut control = state_with_station(p, &a);
    assert!(
        is_slashed(&data_quality_reveal(&mut control, p, hp)),
        "anti-vacuity: unfrozen is slashed"
    );
    let mut frozen = state_with_station(p, &a);
    frozen_at(&mut frozen, p, &a, hp - 5);
    let dq = data_quality_reveal(&mut frozen, p, hp);
    assert!(
        !is_slashed(&dq),
        "R9: a frozen station's data-quality violation is not penalised: {dq:?}"
    );
    // ZK-receipt slashing.
    let mut control = state_with_station(p, &a);
    assert!(
        is_slashed(&zk_receipt_reveal(&mut control, p, hp)),
        "anti-vacuity: unfrozen ZK receipt slashes"
    );
    let mut frozen = state_with_station(p, &a);
    frozen_at(&mut frozen, p, &a, hp - 5);
    let zk = zk_receipt_reveal(&mut frozen, p, hp);
    assert!(
        !is_slashed(&zk),
        "R9: a frozen station's bad ZK receipt is not penalised: {zk:?}"
    );
    // Collusion reputation penalty.
    let mut control = state_with_station(p, &a);
    assert!(
        collusion_reveal_delta(&mut control, p, &a, hp) < 0,
        "anti-vacuity: collusion penalises"
    );
    let mut frozen = state_with_station(p, &a);
    frozen_at(&mut frozen, p, &a, hp - 5);
    assert_eq!(
        collusion_reveal_delta(&mut frozen, p, &a, hp),
        0,
        "R9: no collusion penalty while frozen"
    );
    // Gate: a frozen ledger entry below H suspends nothing.
    let mut gated = state_with_station(p, &a);
    frozen_at(&mut gated, p, &a, hp - 5);
    let mut below = state_with_station(p, &a);
    below.station_owner_ledger.insert(
        p.into(),
        gated.station_owner_ledger.get(p).cloned().unwrap(),
    );
    let r = {
        let (commit, reveal) = reveal_payload(
            p,
            serde_json::json!({"temperature": 20.0, "humidity": 55.0, "pressure": 1013.0, "wind_speed": 200.0}),
            "dq-below",
        );
        below
            .apply_transition(StateTransition::RegisterCommit {
                payload: commit,
                block_height: 0,
            })
            .unwrap();
        below.apply_transition(StateTransition::ApplyReveal {
            block_timestamp: 1_700_000_100,
            payload: reveal,
            block_height: H - 1,
        })
    };
    assert!(
        is_slashed(&r),
        "R9 gate: below H a (smuggled) freeze suspends nothing"
    );

    // ── R10. Rewards on the PRODUCTION path (create_next_block). ──────────
    let (count, earned) = fresh_reveal_after(33_451, "control", None).await;
    assert!(
        count == 1 && earned > 0,
        "anti-vacuity: an unfrozen station is paid ({count}, {earned})"
    );
    let (count, earned) =
        fresh_reveal_after(33_452, "frozen", Some(StationOwnerActionKind::ReportStolen)).await;
    assert_eq!(
        (count, earned),
        (0, 0),
        "R10: a frozen station earns nothing"
    );
    let (count, earned) = fresh_reveal_after(
        33_453,
        "deactivated",
        Some(StationOwnerActionKind::Deactivate),
    )
    .await;
    assert_eq!(
        (count, earned),
        (0, 0),
        "R10: an owner-deactivated station earns nothing"
    );
    let hr = hold_release_via_freeze(33_454, "hold-release").await;
    assert!(hr.planned > 0, "precondition: a planned reward: {hr:?}");
    assert!(
        !hr.finalized_while_frozen && hr.earned_while_frozen == 0,
        "R10: a planned reward is HELD while frozen: {hr:?}"
    );
    assert!(
        hr.finalized_after_unfreeze && hr.earned_after_unfreeze > 0,
        "R10: the held reward is RELEASED on unfreeze: {hr:?}"
    );

    std::env::remove_var(GOVERNANCE_ENV);
}
