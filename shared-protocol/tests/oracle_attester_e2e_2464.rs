// #2464 — the attester path carries one real observation to a paid state.
//
// `station_registration_production_path_2537.rs` proved the earn loop pays
// when attestations exist; its attestations were hand-rolled in the test. This
// file proves the ATTESTER pipeline — the exact artifacts the
// `oracle_attester` service bin executes — produces those attestations:
//
//   PendingObservation (as the #2464 read surface serves it, from the unmined
//   reveal) → MechanicalPolicy::evaluate (physics + freshness + bands) →
//   Attester::attest (#2472 signer role, commit-id verification, signing,
//   self-verification) → AttestationSubmission wire round-trip (the JSON body
//   POST /transactions/oracle/attestation accepts) → mempool → block.
//
// What it proves, end to end:
//
//   1. THE HEADLINE (prompt lane): two attester instances (the two-host
//      deployment shape: oracle 1 on node container, oracle 2 on the workstation)
//      discover the unmined reveal, approve it mechanically, and the reveal
//      block pays — `rewards_provenance_real_decision_count >= 1` in block
//      metadata and the record reaches `real_paid` at quorum. This is the
//      acceptance criterion for the increment.
//   2. THE DEFERRED LANE (timing rule, #2508): the same loop with the reveal
//      mined BEFORE any attestation exists pays nothing in the reveal block
//      (`real_pending`), then pays once attester-produced attestations mine
//      later — proving a missed prompt window settles instead of stranding.
//   3. THE MECHANICAL CONTROL: a physically impossible reading is refused by
//      the policy, both attesters submit `approved=false`, and nothing is
//      ever paid. This is what stops the pipeline from being a rubber stamp.
//
// Run from the repo root:
//   cargo test -p wrfcoin-shared-protocol --test oracle_attester_e2e_2464

use std::sync::Arc;

use rand::rngs::StdRng;
use rand::{SeedableRng, random};
use rustc_hash::FxHashMap;
use tokio::sync::{Mutex, MutexGuard, RwLock};

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
use wrfcoin_shared_protocol::crypto::signer::EnvKeySigner;
use wrfcoin_shared_protocol::mempool::pool::Mempool;
use wrfcoin_shared_protocol::oracle::attester::{
    Attester, AttestationSubmission, PendingObservation, discover_pending,
};
use wrfcoin_shared_protocol::oracle::mechanical::{
    MechanicalPolicy, MechanicalVerdict, UNCHECKED_CONFIDENCE_CAP,
};
use wrfcoin_shared_protocol::station_registry::{ContributionProvenanceClass, DataQualityTier};
use wrfcoin_shared_protocol::weather::station_reference::{
    StationReferenceObservation, pair_station_reference,
};
use wrfcoin_shared_protocol::weather::validation::weather_observation_from_json;
use wrfcoin_shared_protocol::weather::quality_tier::ReadingQualityTier;

const GOVERNANCE_ENV: &str = "WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS";

/// Serializes tests that install the governance allowlist env var, same as
/// the sibling e2e files.
static ENV_LOCK: Mutex<()> = Mutex::const_new(());

async fn lock_env() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().await
}

// ---------------------------------------------------------------------------
// Chain scaffolding (same shapes as station_registration_production_path_2537)
// ---------------------------------------------------------------------------

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

fn metadata_u64(block: &wrfcoin_shared_protocol::blockchain::block::Block, key: &str) -> u64 {
    block
        .metadata
        .get(key)
        .and_then(|raw| raw.parse::<u64>().ok())
        .unwrap_or_else(|| panic!("block metadata must contain numeric '{key}'"))
}

/// One in-process "attester instance": the identity key wrapped in the same
/// `Attester` the service bin builds, so the signing recipe under test is the
/// production one. `EnvKeySigner::from_private_key` is the debug-build test
/// key — precisely the identity seam the deployment doc describes.
struct AttesterInstance {
    attester: Attester,
    oracle_id: String,
}

fn attester_instance(seed: u64) -> AttesterInstance {
    let mut rng = StdRng::seed_from_u64(seed);
    let private_key = PrivateKey::generate(&mut rng);
    let oracle_id = PublicKey::from_private_key(&private_key).to_hex();
    let attester = Attester::new(Arc::new(EnvKeySigner::from_private_key(
        private_key,
        "WRFCOIN_ORACLE_SIGNER_PRIVATE_KEY",
    )));
    AttesterInstance { attester, oracle_id }
}

struct Harness {
    blockchain: Arc<RwLock<Blockchain>>,
    mempool: Arc<RwLock<Mempool>>,
    pop: ProofOfPublishing,
    contributor: Address,
    miner_hex: String,
    oracle_1: AttesterInstance,
    oracle_2: AttesterInstance,
    station_id: String,
    governance_key: PrivateKey,
}

async fn setup(seed: u64, tag: &str) -> Harness {
    let mut rng = StdRng::seed_from_u64(seed);
    let contributor_key = PrivateKey::generate(&mut rng);
    let contributor = Address::from_public_key(&PublicKey::from_private_key(&contributor_key));
    let miner_key = PrivateKey::generate(&mut rng);
    let miner_address = Address::from_public_key(&PublicKey::from_private_key(&miner_key));
    let miner_hex = format!("0x{}", hex::encode(miner_address.0));
    let governance_key = PrivateKey::generate(&mut rng);

    let station_id = format!("attester-e2e-{tag}-{}", random::<u16>());

    let oracle_1 = attester_instance(seed + 10);
    let oracle_2 = attester_instance(seed + 20);

    let mut chain = Blockchain::default();
    chain.state.create_account(contributor, Balance::new(0));
    // Two attester instances, quorum 2 — the two-host deployment shape
    // (oracle 3 stays cold and is deliberately absent).
    chain
        .apply_state_transition(StateTransition::SetOracleConfig {
            oracle_set: vec![oracle_1.oracle_id.clone(), oracle_2.oracle_id.clone()],
            oracle_quorum: 2,
        })
        .expect("set oracle quorum");
    chain
        .state
        .governance
        .params
        .contributor_tier_policy
        .trusted_provider_station_ids
        .insert(station_id.clone());

    let mut pop = ProofOfPublishing::new(1, 60, 0, 16);
    pop.declare_no_genesis_oracles();

    let mut harness = Harness {
        blockchain: Arc::new(RwLock::new(chain)),
        mempool: Arc::new(RwLock::new(Mempool::new(&Config::default()))),
        pop,
        contributor,
        miner_hex,
        oracle_1,
        oracle_2,
        station_id,
        governance_key,
    };

    // The allowlist must exist before the registration block is mined, or
    // the registration silently fails authorization. Caller holds ENV_LOCK.
    std::env::set_var(
        GOVERNANCE_ENV,
        PublicKey::from_private_key(&harness.governance_key).to_hex(),
    );

    // Block 1: the governance-authorized Real station registration, on the
    // production transaction path.
    let mut payload = StationRegistrationPayload::new(
        &harness.station_id,
        &harness.contributor.to_string(),
        46.7867,
        -92.1005,
        "attester-e2e-station",
        DataQualityTier::Gold,
        ContributionProvenanceClass::Real,
    );
    payload.sign_ownership(&contributor_key);
    payload.sign(&harness.governance_key);
    let mut reg_tx = build_tx(TransactionType::StationRegistration, &format!("reg-{tag}"));
    reg_tx.data = Some(serde_json::to_string(&payload).expect("serialize registration"));
    reg_tx.set_id();
    {
        let mut mp = harness.mempool.write().await;
        mp.add_transaction(reg_tx).expect("enqueue registration");
    }
    mine(&mut harness).await;
    assert_eq!(
        harness
            .blockchain
            .read()
            .await
            .state
            .station_registry
            .get(&harness.station_id)
            .expect("station registered")
            .provenance_class,
        ContributionProvenanceClass::Real
    );

    harness
}

async fn mine(harness: &mut Harness) -> wrfcoin_shared_protocol::blockchain::block::Block {
    harness
        .pop
        .create_next_block(
            harness.blockchain.clone(),
            harness.mempool.clone(),
            &harness.miner_hex,
        )
        .await
        .expect("mine block")
}

fn weather_commit_and_reveal(
    station_id: &str,
    data: serde_json::Value,
    timestamp: i64,
    tag: &str,
) -> (CommitPayload, RevealPayload, String) {
    let nonce = format!("nonce-{tag}");
    let commit_id =
        Transaction::compute_commit_id(&data, &nonce, station_id, timestamp, "contribution");

    let seed = tag.bytes().fold(7u64, |acc, b| acc.saturating_add(b as u64));
    let mut rng = StdRng::seed_from_u64(seed);
    let auth_key = PrivateKey::generate(&mut rng);
    let mut commit_payload = CommitPayload {
        commit_id: commit_id.clone(),
        station_id: station_id.to_string(),
        timestamp,
        contribution_type: "contribution".to_string(),
        auth_pubkey: None,
        auth_signature: None,
    };
    commit_payload.sign_authorization(&auth_key);

    let reveal_payload = RevealPayload {
        commit_id: commit_id.clone(),
        station_id: station_id.to_string(),
        timestamp,
        contribution_type: "contribution".to_string(),
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
    };
    (commit_payload, reveal_payload, commit_id)
}

fn commit_tx(payload: &CommitPayload, tag: &str) -> Transaction {
    let mut tx = build_tx(TransactionType::ContributionCommit, &format!("commit-{tag}"));
    tx.data = Some(serde_json::to_string(payload).expect("serialize commit"));
    tx.set_id();
    tx
}

fn reveal_tx(payload: &RevealPayload, sender: Address, tag: &str) -> Transaction {
    let mut tx = build_tx(TransactionType::ContributionReveal, &format!("reveal-{tag}"));
    tx.data = Some(serde_json::to_string(payload).expect("serialize reveal"));
    tx.sender_address = sender;
    tx.fee = 1;
    tx.set_id();
    tx
}

/// What the #2464 read surface serves for an unmined reveal: the full
/// preimage. Built from the reveal payload exactly as the route builds it.
fn pending_observation_from(reveal: &RevealPayload) -> PendingObservation {
    PendingObservation {
        commit_id: reveal.commit_id.clone(),
        station_id: reveal.station_id.clone(),
        nonce: reveal.nonce.clone(),
        timestamp: reveal.timestamp,
        data: reveal.data.clone(),
        contribution_type: reveal.contribution_type.clone(),
    }
}

/// Run ONE attester instance over one observation, producing the transaction
/// that reaches the mempool — through every artifact the service bin uses:
/// discovery filter, mechanical policy, signing with self-verification, and
/// the wire submission round trip.
#[allow(clippy::too_many_arguments)]
fn attester_produces_tx(
    instance: &AttesterInstance,
    observation: &PendingObservation,
    tracked: &FxHashMap<String, wrfcoin_shared_protocol::oracle::attestation::AttestationSet>,
    quorum: usize,
    references: &[StationReferenceObservation],
    now: i64,
    tag: &str,
    policy: &MechanicalPolicy,
) -> Option<(Transaction, MechanicalVerdict)> {
    let observations = std::slice::from_ref(observation);
    let work = discover_pending(observations, tracked, &instance.oracle_id, quorum);
    let observation = work.first()?;

    // Mirror the service bin exactly: pair against the local reference set,
    // then evaluate. An empty/unpairable reference set is the daemon's
    // PHYSICS-ONLY path — a visibly capped attestation, never a silent pass.
    let comparison = weather_observation_from_json(&observation.data).and_then(|parsed| {
        pair_station_reference(
            &parsed,
            observation.timestamp,
            references,
            wrfcoin_shared_protocol::weather::station_reference::DEFAULT_MAX_PAIR_DELTA_SECS,
        )
    });
    let verdict = policy.evaluate(observation, comparison.as_ref(), now);
    let payload = instance
        .attester
        .attest(
            observation,
            verdict.approved,
            Some(verdict.confidence),
            now,
            &format!("att-{tag}-{}", &instance.oracle_id[..8]),
        )
        .expect("attester must sign a commit-verified observation");

    // The wire round trip: render the POST body, parse it back as the node's
    // payload type — the same asymmetry (commit_hash vs commit_id) the HTTP
    // route crosses.
    let body = AttestationSubmission::from(&payload).to_json();
    let parsed: OracleAttestationPayload =
        serde_json::from_str(&body).expect("submission body parses as the protocol payload");
    assert!(
        parsed.has_valid_signature(),
        "signature must survive the wire round trip"
    );

    let mut tx = build_tx(
        TransactionType::OracleAttestation,
        &format!("att-{tag}-{}", &instance.oracle_id[..8]),
    );
    tx.data = Some(serde_json::to_string(&parsed).expect("serialize attestation"));
    tx.set_id();
    Some((tx, verdict))
}

/// Both instances attest `observation` (paired against `references`, empty =
/// physics-only) and the txs land in the mempool. Returns the verdicts, so
/// tests can assert on approval AND confidence.
async fn both_attesters_attest(
    harness: &Harness,
    observation: &PendingObservation,
    references: &[StationReferenceObservation],
    now: i64,
    tag: &str,
) -> Vec<MechanicalVerdict> {
    both_attesters_attest_under(harness, observation, references, now, tag, &MechanicalPolicy::default())
        .await
}

/// [`both_attesters_attest`] under an explicit policy — how the service bin
/// runs once the node reports `reward_gross_outlier_guard` active (#3131).
async fn both_attesters_attest_under(
    harness: &Harness,
    observation: &PendingObservation,
    references: &[StationReferenceObservation],
    now: i64,
    tag: &str,
    policy: &MechanicalPolicy,
) -> Vec<MechanicalVerdict> {
    let (tracked, quorum, signing_keys) = {
        let chain = harness.blockchain.read().await;
        (
            chain.state.attestations.clone(),
            chain.state.oracle_quorum.max(1),
            chain.state.signing_keys.clone(),
        )
    };
    let mut verdicts = Vec::new();
    for instance in [&harness.oracle_1, &harness.oracle_2] {
        let Some((tx, verdict)) =
            attester_produces_tx(instance, observation, &tracked, quorum, references, now, tag, policy)
        else {
            continue;
        };
        verdicts.push(verdict);
        harness
            .mempool
            .write()
            .await
            .add_transaction_with_signing_keys(tx, Some(&signing_keys))
            .expect("attestation admitted to mempool");
    }
    verdicts
}

fn plausible_reading() -> serde_json::Value {
    serde_json::json!({
        "quality": 1.0,
        "volume": 1.0,
        "uniqueness": 1.0,
        "temperature_c": 21.5,
        "humidity_percent": 60.0,
        "pressure_hpa": 1014.0,
        "wind_speed_ms": 2.8,
        "precipitation_mm": 0.0,
        "dewpoint_c": 13.5
    })
}

// ---------------------------------------------------------------------------
// 1. THE HEADLINE — prompt lane: attester-produced attestations mine WITH the
//    reveal, and the observation is paid in that block.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn attester_pipeline_carries_one_observation_to_real_paid_in_the_reveal_block() {
    let _guard = lock_env().await;
    let mut harness = setup(91_100 + random::<u16>() as u64, "prompt").await;
    let ts = 1_700_800_000i64;
    let (commit, reveal, commit_id) =
        weather_commit_and_reveal(&harness.station_id, plausible_reading(), ts, "prompt");

    {
        let mut mp = harness.mempool.write().await;
        mp.add_transaction(commit_tx(&commit, "prompt")).expect("enqueue commit");
    }
    mine(&mut harness).await;

    // The reveal enters the mempool; the attesters see it there — the read
    // surface's "mempool" lane — and attest BEFORE it mines. Freshness is
    // evaluated at the reading's own epoch, as the daemon would at capture
    // time.
    {
        let mut mp = harness.mempool.write().await;
        mp.add_transaction(reveal_tx(&reveal, harness.contributor, "prompt"))
            .expect("enqueue reveal");
    }
    let observation = pending_observation_from(&reveal);
    // The local reference (KDLH-style) agrees with the reading, so this is
    // the reference-CHECKED prompt path: full confidence, no cap.
    let references = vec![StationReferenceObservation {
        timestamp: ts + 30,
        temperature_c: Some(21.0),
        pressure_hpa: Some(1013.5),
        humidity_pct: Some(58.0),
        wind_speed_ms: Some(3.0),
    }];
    let verdicts =
        both_attesters_attest(&harness, &observation, &references, ts + 60, "prompt").await;
    assert!(
        verdicts.iter().all(|v| v.approved),
        "a plausible fresh reading must approve: {verdicts:?}"
    );
    assert_eq!(verdicts.len(), 2);
    assert!(
        verdicts.iter().all(|v| v.confidence > 0.95),
        "a reference-checked agreeing reading must carry full confidence, \
         not the unchecked cap: {verdicts:?}"
    );

    // One block carries reveal + both attestations: the "before or in the
    // reveal block" ordering the reward path requires.
    let reveal_block = mine(&mut harness).await;
    std::env::remove_var(GOVERNANCE_ENV);

    // THE acceptance criterion: a Real provenance decision was made…
    let real_decisions = metadata_u64(&reveal_block, "rewards_provenance_real_decision_count");
    assert!(
        real_decisions >= 1,
        "rewards_provenance_real_decision_count must be >= 1, got {real_decisions}"
    );

    // …and the record reached real_paid at quorum.
    let chain = harness.blockchain.read().await;
    let aggregate = chain.state.contribution_reward_aggregate(&harness.contributor);
    assert!(
        aggregate.real_paid.0 > 0,
        "the attested observation must reach real_paid, got {:?}",
        aggregate
    );
    assert!(
        chain.state.finalized_contribution_rewards.contains(&commit_id),
        "the reward record must be finalized at quorum"
    );
    assert_eq!(
        chain
            .state
            .attestations
            .get(&commit_id)
            .expect("attestation set tracked")
            .approvals(),
        2,
        "both attester instances' approvals must be on chain"
    );
}

// ---------------------------------------------------------------------------
// 2. THE DEFERRED LANE — the reveal mines with NO attestation; the decision is
//    recorded but unpaid; attester-produced attestations in a LATER block
//    settle it (#2508). This is the safety net behind the timing rule.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_reveal_that_mines_unattested_settles_when_the_attesters_catch_up() {
    let _guard = lock_env().await;
    let mut harness = setup(92_200 + random::<u16>() as u64, "late").await;
    let ts = 1_700_800_000i64;
    let (commit, reveal, commit_id) =
        weather_commit_and_reveal(&harness.station_id, plausible_reading(), ts, "late");

    {
        let mut mp = harness.mempool.write().await;
        mp.add_transaction(commit_tx(&commit, "late")).expect("enqueue commit");
    }
    mine(&mut harness).await;
    {
        let mut mp = harness.mempool.write().await;
        mp.add_transaction(reveal_tx(&reveal, harness.contributor, "late"))
            .expect("enqueue reveal");
    }
    let reveal_block = mine(&mut harness).await;

    // The decision is recorded — this is where
    // rewards_provenance_real_decision_count first moves — but nothing is
    // paid: quorum has not arrived.
    assert!(
        metadata_u64(&reveal_block, "rewards_provenance_real_decision_count") >= 1,
        "the Real decision must be recorded in the reveal block even without quorum"
    );
    {
        let chain = harness.blockchain.read().await;
        let aggregate = chain.state.contribution_reward_aggregate(&harness.contributor);
        assert_eq!(aggregate.real_paid.0, 0, "no quorum, no payment");
        assert!(
            aggregate.real_pending.0 > 0,
            "the plan must be pending, not vanished: {aggregate:?}"
        );
    }

    // The attesters catch up. On a live node they would read this commit from
    // the read surface's "block" lane (state still serves the preimage via
    // the reveal's block). Then the deferred settlement pays.
    let observation = pending_observation_from(&reveal);
    // No reference set here: the deferred lane doubles as the PHYSICS-ONLY
    // deployment shape — approvals unaffected, confidence visibly capped.
    let verdicts = both_attesters_attest(&harness, &observation, &[], ts + 120, "late").await;
    assert!(verdicts.iter().all(|v| v.approved), "{verdicts:?}");
    assert_eq!(verdicts.len(), 2);
    assert!(
        verdicts.iter().all(|v| v.confidence <= UNCHECKED_CONFIDENCE_CAP + 1e-9),
        "an unchecked attestation must not exceed the cap: {verdicts:?}"
    );
    mine(&mut harness).await;
    // Settlement runs in the block AFTER quorum lands in state.
    mine(&mut harness).await;
    std::env::remove_var(GOVERNANCE_ENV);

    let chain = harness.blockchain.read().await;
    let aggregate = chain.state.contribution_reward_aggregate(&harness.contributor);
    assert!(
        aggregate.real_paid.0 > 0,
        "late quorum must settle to real_paid (#2508), got {aggregate:?}"
    );
    assert!(chain.state.finalized_contribution_rewards.contains(&commit_id));
}

// ---------------------------------------------------------------------------
// 3. THE MECHANICAL CONTROL — an impossible reading is refused by policy,
//    attested approved=false by both instances, and never paid. Without this,
//    the pipeline could be a rubber stamp and tests 1–2 would still pass.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_physically_impossible_reading_is_rejected_and_never_paid() {
    let _guard = lock_env().await;
    let mut harness = setup(93_300 + random::<u16>() as u64, "reject").await;
    let ts = 1_700_800_000i64;
    let mut impossible = plausible_reading();
    impossible["temperature_c"] = serde_json::json!(120.0); // hotter than Earth's record by 63 C
    let (commit, reveal, commit_id) =
        weather_commit_and_reveal(&harness.station_id, impossible, ts, "reject");

    {
        let mut mp = harness.mempool.write().await;
        mp.add_transaction(commit_tx(&commit, "reject")).expect("enqueue commit");
    }
    mine(&mut harness).await;
    {
        let mut mp = harness.mempool.write().await;
        mp.add_transaction(reveal_tx(&reveal, harness.contributor, "reject"))
            .expect("enqueue reveal");
    }

    let observation = pending_observation_from(&reveal);
    let verdicts = both_attesters_attest(&harness, &observation, &[], ts + 60, "reject").await;
    assert_eq!(verdicts.len(), 2);
    assert!(
        verdicts.iter().all(|v| !v.approved),
        "the mechanical policy must refuse an impossible reading: {verdicts:?}"
    );

    mine(&mut harness).await;
    mine(&mut harness).await;
    std::env::remove_var(GOVERNANCE_ENV);

    let chain = harness.blockchain.read().await;
    let aggregate = chain.state.contribution_reward_aggregate(&harness.contributor);
    assert_eq!(
        aggregate.real_paid.0, 0,
        "a rejected reading must never be paid: {aggregate:?}"
    );
    assert!(
        !chain.state.finalized_contribution_rewards.contains(&commit_id),
        "a rejection quorum must not finalize a payment"
    );
    let set = chain.state.attestations.get(&commit_id).expect("attestations tracked");
    assert_eq!(set.approvals(), 0, "no approval may exist for a refused reading");
    assert_eq!(set.by_oracle.len(), 2, "both rejections must be recorded on chain");
}

// ---------------------------------------------------------------------------
// 4. THE REFERENCE CONTROL (round-2 item 1 acceptance): a physically-plausible
//    but WRONG reading — 19.6 °C submitted while the local reference (KDLH)
//    reports 30 °C — is materially down-weighted. Before the owner's ruling
//    was implemented this reading attested at FULL confidence (the daemon
//    hardcoded `comparison: None` and evaluate treated None as a free Good);
//    the assertions below are impossible under that behavior, and the
//    RED-control runs live in the unit layers (station_reference: the pairing
//    tiered Good pre-fixture; mechanical: unchecked confidence was 0.9917
//    pre-cap).
//
//    #3131 (owner ruling 2026-09-24, option b): the #3098 second-worst fold
//    drops a lone Outlier variable, so this single-variable 10.4 °C miss
//    earned Degraded (0.5×). From the `reward_gross_outlier_guard` activation a lone
//    Outlier beyond its variable's `gross_max` (nws_station temperature: 9 °C)
//    collapses the reading to Outlier again. Before activation the verdict is
//    the #3098 one, unchanged; both are pinned below.
// ---------------------------------------------------------------------------

/// The wrong reading, committed and revealed on a fresh harness, plus the
/// reference observation that exposes it.
async fn wrong_reading_setup(
    tag: &str,
) -> (Harness, PendingObservation, Vec<StationReferenceObservation>, String, i64) {
    let harness = setup(94_400 + random::<u16>() as u64, tag).await;
    let ts = 1_700_800_000i64;
    // Plausible in absolute terms (passes WMO physics bounds), wrong for the
    // actual weather: 19.6 °C against a 30 °C airport observation 3 km away.
    let wrong_reading = serde_json::json!({
        "quality": 1.0,
        "volume": 1.0,
        "uniqueness": 1.0,
        "temperature_c": 19.6,
        "humidity_percent": 52.0,
        "pressure_hpa": 1013.0,
        "wind_speed_ms": 4.0,
        "precipitation_mm": 0.0,
        "dewpoint_c": 9.0
    });
    let (commit, reveal, commit_id) =
        weather_commit_and_reveal(&harness.station_id, wrong_reading, ts, tag);
    let mut harness = harness;
    {
        let mut mp = harness.mempool.write().await;
        mp.add_transaction(commit_tx(&commit, tag)).expect("enqueue commit");
    }
    mine(&mut harness).await;
    {
        let mut mp = harness.mempool.write().await;
        mp.add_transaction(reveal_tx(&reveal, harness.contributor, tag))
            .expect("enqueue reveal");
    }
    let references = vec![StationReferenceObservation {
        timestamp: ts + 30,
        temperature_c: Some(30.0),
        pressure_hpa: Some(1013.2),
        humidity_pct: Some(50.0),
        wind_speed_ms: Some(4.2),
    }];
    (harness, pending_observation_from(&reveal), references, commit_id, ts)
}

/// Mine the attestations and return the confidences the chain recorded.
async fn recorded_confidences(harness: &mut Harness, commit_id: &str) -> Vec<f64> {
    mine(harness).await;
    std::env::remove_var(GOVERNANCE_ENV);
    let chain = harness.blockchain.read().await;
    let set = chain.state.attestations.get(commit_id).expect("attestations tracked");
    assert_eq!(set.approvals(), 2, "both down-weighted approvals are on chain");
    set.by_oracle.values().map(|record| record.confidence).collect()
}

#[tokio::test]
async fn a_plausible_but_wrong_reading_is_down_weighted_by_the_local_reference() {
    let _guard = lock_env().await;
    let (mut harness, observation, references, commit_id, ts) = wrong_reading_setup("wrong").await;
    // Post-activation: the node reports `reward_gross_outlier_guard` active and the
    // service evaluates under the guard.
    let policy = MechanicalPolicy { gross_outlier_guard: true, ..MechanicalPolicy::default() };
    let verdicts =
        both_attesters_attest_under(&harness, &observation, &references, ts + 60, "wrong", &policy)
            .await;

    assert_eq!(verdicts.len(), 2);
    for verdict in &verdicts {
        // The chain philosophy holds: the tier never flips approval…
        assert!(verdict.approved, "an outlier is down-weighted, not refused: {verdict:?}");
        // …but a 10.4 °C residual is past the nws_station gross limit (9 °C),
        // so the confidence collapses to the outlier multiplier (0.1 × fresh).
        assert_eq!(verdict.tier, ReadingQualityTier::Outlier, "{verdict:?}");
        assert!(
            verdict.confidence <= 0.11,
            "a reading 10.4 °C from the local reference must carry collapsed \
             confidence, got {verdict:?}"
        );
        assert!(
            verdict.reasons.iter().any(|r| r.contains("outlier")),
            "the audit trail must name the tier: {verdict:?}"
        );
        assert!(
            verdict.reasons.iter().any(|r| r.contains("gross outlier guard")),
            "the audit trail must name the guard: {verdict:?}"
        );
    }

    // The collapsed confidence is what lands on chain, not a local artifact.
    for confidence in recorded_confidences(&mut harness, &commit_id).await {
        assert!(confidence <= 0.11, "on-chain confidence {confidence}");
    }
}

#[tokio::test]
async fn before_the_gross_guard_activates_the_wrong_reading_keeps_the_3098_degraded_verdict() {
    let _guard = lock_env().await;
    let (mut harness, observation, references, commit_id, ts) =
        wrong_reading_setup("wrong-pre").await;
    // Pre-activation: the default policy, i.e. the #3098 fold unchanged.
    let verdicts =
        both_attesters_attest(&harness, &observation, &references, ts + 60, "wrong-pre").await;
    assert_eq!(verdicts.len(), 2);
    for verdict in &verdicts {
        // Exactly what main produced before #3131 (issue #3131's panic text).
        assert!(verdict.approved);
        assert_eq!(verdict.tier, ReadingQualityTier::Degraded, "{verdict:?}");
        assert_eq!(verdict.confidence, 0.4958, "{verdict:?}");
        assert!(!verdict.reasons.iter().any(|r| r.contains("gross")), "{verdict:?}");
    }
    for confidence in recorded_confidences(&mut harness, &commit_id).await {
        assert_eq!(confidence, 0.4958);
    }
}
