//! #3214 activation: `ConsensusFeature::StationOwnerSlashIdentity` scheduled
//! at H switches the data-quality slashing identity in `ApplyReveal` at
//! exactly H, through the REAL `State::apply_transition`:
//!   - below H the trigger keeps the pre-#3214 `station_id` placeholder, and
//!     finalization debits nobody (the path the live v1 testnet runs);
//!   - at H the trigger is bound to the station's registered, funded owner
//!     (canonical `wrf…` form accepted, stored as `0x…`), and finalization
//!     burns exactly 30 units and lowers the owner's reputation and records
//!     their cooldown;
//!   - at H an unregistered station fails closed with `SlashingApplication`
//!     and enqueues nothing.
//!
//! The feature ships UNSCHEDULED: the live 777/testnet genesis has no
//! schedule key, so there it is inactive at every height and never halts a
//! node. Activation is an owner governance decision.
//!
//! ONE test: the feature schedule is installed process-wide
//! (first-install-wins `OnceLock`), so this file owns its process, the same
//! discipline as `paymaster_activation_2171` and
//! `gross_outlier_guard_activation_3131`.

use wrfcoin_reputation_engine::ReputationEvent;
use wrfcoin_shared_protocol::blockchain::state::{State, StateError, StateTransition};
use wrfcoin_shared_protocol::blockchain::transaction::{CommitPayload, RevealPayload, Transaction};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ConsensusFeature, GENESIS_ACTIVATION_SCHEDULE_KEY, SigningProtocolVersion, feature_active,
    install_activation_table, install_feature_schedule, resolve_activation_schedule,
    resolve_feature_schedule, try_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::protocol_support::{
    KNOWN_FEATURES, upgrade_required_at, upgrade_required_in_schedule,
    upgrade_required_in_schedule_with,
};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::oracle::attestation::SlashingTrigger;
use wrfcoin_shared_protocol::station_registry::{
    ContributionProvenanceClass, DataQualityTier, StationLocation, WeatherStation,
};

const ACTIVATION_HEIGHT: u64 = 20;
const FUNDED: u128 = 100;
const PENALTY: u128 = 30;

fn station(station_id: &str, owner_address: String) -> WeatherStation {
    WeatherStation {
        station_id: station_id.to_string(),
        owner_address,
        location: StationLocation { latitude: 39.7392, longitude: -104.9903, elevation_meters: None },
        hardware_model: "BME280".to_string(),
        calibration_date: None,
        is_active: true,
        reputation_score: 50,
        data_quality_tier: DataQualityTier::Bronze,
        provenance_class: ContributionProvenanceClass::Real,
        ring_public_key: None,
    }
}

/// A deterministic owner whose `PublicKey::to_address()` is the canonical
/// `wrf…` form a station registration carries.
fn owner(seed: u8) -> (String, Address) {
    let private_key = PrivateKey::from_bytes(&[seed; 32]).expect("32-byte ed25519 seed");
    let public_key = PublicKey::from_private_key(&private_key);
    (public_key.to_address(), Address::from_public_key(&public_key))
}

fn seed_owner(state: &mut State, station_id: &str, seed: u8) -> Address {
    let (wrf_address, address) = owner(seed);
    assert!(wrf_address.starts_with("wrf"), "the registration uses the canonical wrf… form");
    state.create_account(address, Balance::new(FUNDED));
    state.station_registry.register(station(station_id, wrf_address)).unwrap();
    state.oracle_set.insert(address.to_string());
    state.oracle_registry_mut().apply_reputation_event(ReputationEvent {
        oracle_id: address.to_string(),
        delta: 50,
        reason: "seed".to_string(),
        timestamp: 0,
    });
    address
}

/// Commit at `height - 1` and reveal at `height` one physically impossible
/// reading (200 °C) for `station_id`.
fn violating_reveal(state: &mut State, station_id: &str, height: u64) -> Result<(), StateError> {
    let data = serde_json::json!({"temperature": 200.0, "humidity": 50.0});
    let nonce = format!("nonce-{station_id}-{height}");
    let timestamp = 1_700_000_000_i64 + height as i64;
    let contribution_type = "weather";
    let commit_id =
        Transaction::compute_commit_id(&data, &nonce, station_id, timestamp, contribution_type);
    state
        .apply_transition(StateTransition::RegisterCommit {
            payload: CommitPayload {
                commit_id: commit_id.clone(),
                station_id: station_id.to_string(),
                timestamp,
                contribution_type: contribution_type.to_string(),
                auth_pubkey: None,
                auth_signature: None,
            },
            block_height: height - 1,
        })
        .unwrap();
    state.apply_transition(StateTransition::ApplyReveal {
        payload: RevealPayload {
            commit_id,
            station_id: station_id.to_string(),
            timestamp,
            contribution_type: contribution_type.to_string(),
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
        block_height: height,
        block_timestamp: 0,
    })
}

fn triggered_identity(result: &Result<(), StateError>) -> Option<&str> {
    match result {
        Err(StateError::SlashingTriggered(SlashingTrigger::DataQualityViolation {
            oracle_id, ..
        })) => Some(oracle_id.as_str()),
        _ => None,
    }
}

#[test]
fn station_owner_slash_identity_binds_the_owner_exactly_from_activation() {
    // ── The live testnet (v1, no schedule key) never activates or halts. ────
    let name = ConsensusFeature::StationOwnerSlashIdentity.name();
    assert_eq!(name, "station_owner_slash_identity");
    assert_eq!(ConsensusFeature::parse(name), Some(ConsensusFeature::StationOwnerSlashIdentity));
    assert!(KNOWN_FEATURES.contains(&name));
    let live_genesis: [(&str, &str); 0] = [];
    assert_eq!(upgrade_required_at(live_genesis), None, "an unscheduled feature never halts a node");
    let raw = format!("0:v1,{ACTIVATION_HEIGHT}:{name}");
    assert_eq!(upgrade_required_in_schedule(&raw), None);
    let pre_3214_known: Vec<&str> =
        KNOWN_FEATURES.iter().copied().filter(|known| *known != name).collect();
    let required = upgrade_required_in_schedule_with(&raw, &pre_3214_known)
        .expect("a pre-#3214 binary must report the upgrade (#3093 clean halt)");
    assert_eq!((required.feature.as_str(), required.height), (name, ACTIVATION_HEIGHT));

    // ── Unscheduled (nothing installed yet): inactive at every height. ──────
    assert!(try_feature_schedule().is_none(), "this file owns its process; no schedule may pre-exist");
    assert!(!feature_active(ConsensusFeature::StationOwnerSlashIdentity, ACTIVATION_HEIGHT));
    assert!(!feature_active(ConsensusFeature::StationOwnerSlashIdentity, u64::MAX));

    // ── Schedule the feature at H, as the boot gate would (test-only). ──────
    let metadata = [(GENESIS_ACTIVATION_SCHEDULE_KEY, raw.as_str())];
    let table = resolve_activation_schedule(metadata, SigningProtocolVersion::V1).unwrap();
    let features = resolve_feature_schedule(metadata, &table).unwrap();
    SigningProtocolVersion::install_active(SigningProtocolVersion::V1);
    install_activation_table(table);
    install_feature_schedule(features);
    assert!(!feature_active(ConsensusFeature::StationOwnerSlashIdentity, ACTIVATION_HEIGHT - 1));
    assert!(feature_active(ConsensusFeature::StationOwnerSlashIdentity, ACTIVATION_HEIGHT));

    // ── H-1: the pre-#3214 path. Trigger keeps station_id; nobody debited. ──
    let mut state = State::new();
    let before_station = "station-3214-before";
    let before_owner = seed_owner(&mut state, before_station, 0x31);
    let before_reputation = state.oracle_registry().reputation_score(&before_owner.to_string());
    let result = violating_reveal(&mut state, before_station, ACTIVATION_HEIGHT - 1);
    assert_eq!(
        triggered_identity(&result),
        Some(before_station),
        "below activation the trigger keeps the station_id identity: {result:?}"
    );
    state.flush_deferred_slashing();
    let account = state.get_account(&before_owner).unwrap();
    assert_eq!(account.balance, Balance::new(FUNDED), "below activation the owner is not debited");
    assert_eq!(account.spendable_balance, Balance::new(FUNDED));
    assert_eq!(
        state.oracle_registry().reputation_score(&before_owner.to_string()),
        before_reputation
    );
    assert!(!state.oracle_registry().is_in_cooldown(&before_owner.to_string(), ACTIVATION_HEIGHT));

    // ── H: the trigger binds the registered, funded owner. ──────────────────
    let mut state = State::new();
    let station_id = "station-3214-active";
    let owner = seed_owner(&mut state, station_id, 0x32);
    let reputation_before = state.oracle_registry().reputation_score(&owner.to_string());
    let result = violating_reveal(&mut state, station_id, ACTIVATION_HEIGHT);
    assert_eq!(
        triggered_identity(&result),
        Some(owner.to_string().as_str()),
        "from activation the trigger carries the canonical 0x… owner: {result:?}"
    );
    state.flush_deferred_slashing();
    let account = state.get_account(&owner).unwrap();
    assert_eq!(
        account.balance,
        Balance::new(FUNDED - PENALTY),
        "exact 30-unit quality penalty must burn"
    );
    assert_eq!(account.spendable_balance, Balance::new(FUNDED - PENALTY));
    assert!(
        state.oracle_registry().reputation_score(&owner.to_string()) < reputation_before,
        "quality violation must reduce the accountable owner's reputation"
    );
    assert!(
        state.oracle_registry().is_in_cooldown(&owner.to_string(), ACTIVATION_HEIGHT + 1),
        "quality violation must record the accountable owner's cooldown"
    );

    // ── H: an unregistered station fails closed and enqueues nothing. ───────
    let mut state = State::new();
    let bystander = seed_owner(&mut state, "station-3214-bystander", 0x33);
    let result = violating_reveal(&mut state, "station-3214-unregistered", ACTIVATION_HEIGHT);
    assert!(
        matches!(result, Err(StateError::SlashingApplication(ref m)) if m.contains("unregistered station")),
        "unresolved station ownership must fail closed, got: {result:?}"
    );
    state.flush_deferred_slashing();
    assert_eq!(
        state.get_account(&bystander).unwrap().balance,
        Balance::new(FUNDED),
        "an unbound violation must not enter the deferred queue or debit anyone"
    );

    // E1 Stage A prep: H+1 still binds the canonical owner; a repeated flush
    // has no new trigger to debit. These are kernel controls, not proof that
    // a producer retains a rejected reveal (the native drill checks that).
    let mut state = State::new();
    let after_owner = seed_owner(&mut state, "e1-after", 0x34);
    let result = violating_reveal(&mut state, "e1-after", ACTIVATION_HEIGHT + 1);
    assert_eq!(
        triggered_identity(&result),
        Some(after_owner.to_string().as_str())
    );
    state.apply_block_finalization(ACTIVATION_HEIGHT + 1);
    assert_eq!(
        state.get_account(&after_owner).unwrap().balance,
        Balance::new(FUNDED - PENALTY)
    );
    state.flush_deferred_slashing();
    assert_eq!(
        state.get_account(&after_owner).unwrap().balance,
        Balance::new(FUNDED - PENALTY)
    );

    // Wrong/malformed owner and canonical-but-missing account each refuse
    // without queueing a bystander liability.
    for (sid, owner_text) in [
        ("e1-malformed", "not-an-address".to_string()),
        ("e1-missing-account", crate::owner(0x35).0),
    ] {
        let mut state = State::new();
        let bystander = seed_owner(&mut state, "e1-bystander", 0x36);
        state
            .station_registry
            .register(station(sid, owner_text))
            .unwrap();
        let result = violating_reveal(&mut state, sid, ACTIVATION_HEIGHT);
        assert!(matches!(result, Err(StateError::SlashingApplication(_))));
        state.flush_deferred_slashing();
        assert_eq!(
            state.get_account(&bystander).unwrap().balance,
            Balance::new(FUNDED)
        );
        assert_eq!(state.slashing_circuit_breaker.events_applied(), 0);
    }

    // The registry is the authority: a valid alternate account is the
    // liable owner; a contributor hint cannot select the funded bystander.
    let mut state = State::new();
    let actual = seed_owner(&mut state, "e1-canonical", 0x37);
    let bystander = seed_owner(&mut state, "e1-other", 0x38);
    let result = violating_reveal(&mut state, "e1-canonical", ACTIVATION_HEIGHT);
    assert_eq!(
        triggered_identity(&result),
        Some(actual.to_string().as_str())
    );
    state.flush_deferred_slashing();
    assert_eq!(
        state.get_account(&actual).unwrap().balance,
        Balance::new(FUNDED - PENALTY)
    );
    assert_eq!(
        state.get_account(&bystander).unwrap().balance,
        Balance::new(FUNDED)
    );

    // A zero-balance account satisfies identity existence but cannot burn
    // funds: the audit conservatively reports it as economic debt.
    let mut state = State::new();
    let zero = seed_owner(&mut state, "e1-zero", 0x39);
    state.get_account_mut(&zero).unwrap().balance = Balance::new(0);
    state.get_account_mut(&zero).unwrap().spendable_balance = Balance::new(0);
    let result = violating_reveal(&mut state, "e1-zero", ACTIVATION_HEIGHT);
    assert_eq!(triggered_identity(&result), Some(zero.to_string().as_str()));
    state.flush_deferred_slashing();
    assert_eq!(state.get_account(&zero).unwrap().balance, Balance::new(0));

    // Deterministic finalization caps 110 distinct physical-quality triggers
    // at the production 100 events; overflow neither wraps nor charges extra.
    let mut state = State::new();
    let mut owners = Vec::new();
    for seed in 1..=110 {
        let sid = format!("e1-cap-{seed:03}");
        let owner = seed_owner(&mut state, &sid, seed);
        assert!(
            triggered_identity(&violating_reveal(&mut state, &sid, ACTIVATION_HEIGHT)).is_some()
        );
        owners.push(owner);
    }
    state.flush_deferred_slashing();
    assert_eq!(state.slashing_circuit_breaker.events_applied(), 100);
    assert_eq!(state.slashing_circuit_breaker.events_skipped(), 10);
    assert_eq!(
        owners
            .iter()
            .filter(|a| state.get_account(a).unwrap().balance == Balance::new(FUNDED - PENALTY))
            .count(),
        100
    );
}
