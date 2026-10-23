//! #3345 — a station's owner changes only with the current owner's consent.
//!
//! What this file proves, in order, inside ONE test (the feature schedule is
//! installed process-wide through a first-install-wins `OnceLock`, so this
//! file owns its process — the same discipline as
//! `station_owner_slash_identity_activation_3214`):
//!
//! 1. CHARACTERISATION of the defect on the production path (no schedule, i.e.
//!    the live v1 chain): a governance-signed `StationRegistration` for an
//!    EXISTING `station_id` with a DIFFERENT owner, mined by the real
//!    `ProofOfPublishing::create_next_block`, silently replaces the owner. The
//!    previous owner signs nothing. Since #2743 (`STATION_OWNERSHIP_BINDING_ACTIVATION_HEIGHT`) the registered owner
//!    is the reward payee, so this redirects the station's earnings. This
//!    assertion is expected to keep passing below activation: it is the
//!    "pre-activation behaviour unchanged" half of the acceptance criteria.
//! 2. `ConsensusFeature::StationOwnerConsent` is known, unscheduled, never
//!    halts an unscheduled genesis, and a pre-#3345 binary reports the #3093
//!    upgrade when a schedule names it.
//! 3. With the feature scheduled at H (test-only):
//!    - at H-1 the shared constructor still emits the legacy transition and an
//!      owner change still applies (roots byte-identical below activation);
//!    - at H an owner change is an inert no-op: the record AND the whole
//!      `state_root_material` are unchanged;
//!    - at H a same-owner re-registration (moved coordinates, and the same
//!      address re-spelled `0x…` vs `wrf…`) still applies;
//!    - at H a brand-new station still registers;
//!    - the owner-consent transition is refused (`InvalidTransition`) below
//!      activation, so it cannot be smuggled into pre-activation state.
//! 4. The DRAFT transfer preimage is byte-pinned and binds every field.
//!
//! RED control (recorded in the PR): with the owner-change refusal in
//! `State::apply_transition_inner` removed, step 3 fails at the "owner change
//! at H is refused" assertion.
//!
//! Run from core4: `cargo test -p wrfcoin-shared-protocol --test
//! station_owner_consent_3345`

use std::sync::Arc;

use rand::SeedableRng;
use rand::rngs::StdRng;
use tokio::sync::RwLock;
use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::state::{
    State, StateError, StateTransition, same_station_owner,
};
use wrfcoin_shared_protocol::blockchain::transaction::{
    STATION_OWNER_TRANSFER_ACCEPT_DOMAIN, STATION_OWNER_TRANSFER_DOMAIN,
    StationRegistrationPayload, Transaction, TransactionInput, TransactionOutput, TransactionType,
    station_owner_transfer_message_with_domain,
};
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
const ACTIVATION_HEIGHT: u64 = 20;

fn key(seed: u8) -> PrivateKey {
    PrivateKey::from_bytes(&[seed; 32]).expect("32-byte ed25519 seed")
}

/// The canonical, pubkey-bound `wrf…` owner address for `owner_key`.
fn bound_address(owner_key: &PrivateKey) -> String {
    PublicKey::from_private_key(owner_key).to_address()
}

fn signed_registration(
    station_id: &str,
    owner_address: &str,
    latitude: f64,
    owner_key: &PrivateKey,
    governance_key: &PrivateKey,
) -> StationRegistrationPayload {
    let mut payload = StationRegistrationPayload::new(
        station_id,
        owner_address,
        latitude,
        -92.1005,
        "Ambient WS-5000",
        DataQualityTier::Gold,
        ContributionProvenanceClass::Real,
    );
    payload.sign_ownership(owner_key);
    payload.sign(governance_key);
    payload
}

fn registration_tx(payload: &StationRegistrationPayload, tag: &str) -> Transaction {
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
    tx.tx_type = TransactionType::StationRegistration;
    tx.data = Some(serde_json::to_string(payload).expect("serialize registration payload"));
    tx.set_id();
    tx
}

/// Step 1: the defect, on the production block path, with no schedule.
async fn governance_reregistration_reassigns_owner_on_the_production_path(
    governance_key: &PrivateKey,
) {
    let station_id = "ws5000-3345-characterisation";
    let victim_key = key(0xA1);
    let thief_key = key(0xB2);
    let victim = bound_address(&victim_key);
    let thief = bound_address(&thief_key);

    let mut rng = StdRng::seed_from_u64(3345);
    let miner_key = PrivateKey::generate(&mut rng);
    let miner_hex = format!(
        "0x{}",
        hex::encode(
            wrfcoin_shared_protocol::common::types::Address::from_public_key(
                &PublicKey::from_private_key(&miner_key)
            )
            .0
        )
    );
    let chain = Arc::new(RwLock::new(Blockchain::default()));
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let mut pop = ProofOfPublishing::new(1, 60, 0, 16);
    pop.declare_no_genesis_oracles();

    for (tag, owner_key, owner) in [
        ("victim", &victim_key, &victim),
        ("thief", &thief_key, &thief),
    ] {
        let payload = signed_registration(station_id, owner, 46.7867, owner_key, governance_key);
        let tx = registration_tx(&payload, tag);
        assert!(
            tx.verify_station_registration_authorization(),
            "fixture must authorize ({tag})"
        );
        mempool
            .write()
            .await
            .add_transaction(tx)
            .expect("enqueue registration");
        pop.create_next_block(chain.clone(), mempool.clone(), &miner_hex)
            .await
            .unwrap_or_else(|e| panic!("mine {tag} registration block: {e}"));
        let registered = chain
            .read()
            .await
            .state
            .station_registry
            .get(station_id)
            .map(|s| s.owner_address.clone());
        assert_eq!(
            registered.as_deref(),
            Some(owner.as_str()),
            "after the {tag} block"
        );
    }
    // The victim never signed anything in the second block, yet the station
    // (and, from the ownership-binding height, its reward payee) now belongs to the thief.
    let chain = chain.read().await;
    assert!(
        chain.block_at_height(2).is_some(),
        "both registrations were mined"
    );
    assert!(
        chain.block_at_height(ACTIVATION_HEIGHT).is_none(),
        "characterisation must run below any activation height"
    );
}

#[tokio::test]
async fn station_owner_changes_only_with_current_owner_consent_from_activation() {
    let governance_key = key(0x60);
    // SAFETY-OF-TEST: this file owns its process; nothing else reads this env.
    std::env::set_var(
        GOVERNANCE_ENV,
        PublicKey::from_private_key(&governance_key).to_hex(),
    );

    // ── 1. The defect today (no schedule installed = live v1 chain). ────────
    assert!(
        try_feature_schedule().is_none(),
        "this file owns its process; no schedule may pre-exist"
    );
    governance_reregistration_reassigns_owner_on_the_production_path(&governance_key).await;

    // ── 2. The feature: known, unscheduled, halts only an old binary. ───────
    let name = ConsensusFeature::StationOwnerConsent.name();
    assert_eq!(name, "station_owner_consent");
    assert_eq!(
        ConsensusFeature::parse(name),
        Some(ConsensusFeature::StationOwnerConsent)
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
    let pre_3345_known: Vec<&str> = KNOWN_FEATURES
        .iter()
        .copied()
        .filter(|k| *k != name)
        .collect();
    let required = upgrade_required_in_schedule_with(&raw, &pre_3345_known)
        .expect("a pre-#3345 binary must report the upgrade (#3093 clean halt)");
    assert_eq!(
        (required.feature.as_str(), required.height),
        (name, ACTIVATION_HEIGHT)
    );
    assert!(!feature_active(
        ConsensusFeature::StationOwnerConsent,
        u64::MAX
    ));

    let owner_key = key(0x11);
    let other_key = key(0x22);
    let owner = bound_address(&owner_key);
    let other = bound_address(&other_key);
    let station_id = "ws5000-3345";
    let original = signed_registration(station_id, &owner, 46.7867, &owner_key, &governance_key);
    let hijack = signed_registration(station_id, &other, 46.7867, &other_key, &governance_key);

    // Unscheduled: the shared constructor emits the legacy variant at every height.
    for h in [0, ACTIVATION_HEIGHT, u64::MAX] {
        assert!(matches!(
            Transaction::station_registration_transition(&original, h),
            StateTransition::ApplyStationRegistration { .. }
        ));
    }

    // ── 3. Schedule the feature at H, as the boot gate would (test-only). ──
    let metadata = [(GENESIS_ACTIVATION_SCHEDULE_KEY, raw.as_str())];
    let table = resolve_activation_schedule(metadata, SigningProtocolVersion::V1).unwrap();
    let features = resolve_feature_schedule(metadata, &table).unwrap();
    SigningProtocolVersion::install_active(SigningProtocolVersion::V1);
    install_activation_table(table);
    install_feature_schedule(features);
    assert!(!feature_active(
        ConsensusFeature::StationOwnerConsent,
        ACTIVATION_HEIGHT - 1
    ));
    assert!(feature_active(
        ConsensusFeature::StationOwnerConsent,
        ACTIVATION_HEIGHT
    ));

    // H-1: legacy variant; an owner change still applies (pre-activation
    // unchanged).
    let mut state = State::new();
    state
        .apply_transition(Transaction::station_registration_transition(
            &original,
            ACTIVATION_HEIGHT - 1,
        ))
        .unwrap();
    let below = Transaction::station_registration_transition(&hijack, ACTIVATION_HEIGHT - 1);
    assert!(matches!(
        below,
        StateTransition::ApplyStationRegistration { .. }
    ));
    state.apply_transition(below).unwrap();
    assert_eq!(
        state
            .station_registry
            .get(station_id)
            .unwrap()
            .owner_address,
        other
    );

    // The owner-consent variant cannot be applied below activation.
    let mut state = State::new();
    let smuggled = StateTransition::ApplyStationRegistrationWithOwnerConsent {
        station: Transaction::station_from_registration(&original),
        block_height: ACTIVATION_HEIGHT - 1,
    };
    assert!(matches!(
        state.apply_transition(smuggled),
        Err(StateError::InvalidTransition)
    ));
    assert!(state.station_registry.get(station_id).is_none());

    // H: a brand-new station registers through the owner-consent variant.
    let mut state = State::new();
    let first = Transaction::station_registration_transition(&original, ACTIVATION_HEIGHT);
    assert!(matches!(
        first,
        StateTransition::ApplyStationRegistrationWithOwnerConsent { .. }
    ));
    state.apply_transition(first).unwrap();
    assert_eq!(
        state
            .station_registry
            .get(station_id)
            .unwrap()
            .owner_address,
        owner
    );

    // H: an owner change without the current owner's consent is refused, and
    // refused completely — the whole state-root material is untouched.
    let material_before = state.state_root_material();
    state
        .apply_transition(Transaction::station_registration_transition(
            &hijack,
            ACTIVATION_HEIGHT,
        ))
        .expect("a refused owner change is an inert no-op, never a block-failing error");
    assert_eq!(
        state
            .station_registry
            .get(station_id)
            .unwrap()
            .owner_address,
        owner,
        "owner change at H is refused: governance alone can no longer reassign a station"
    );
    assert_eq!(
        state.state_root_material(),
        material_before,
        "a refused change must not touch state"
    );

    // H: a same-owner metadata correction still applies (moved coordinates).
    let moved = signed_registration(station_id, &owner, 46.9, &owner_key, &governance_key);
    state
        .apply_transition(Transaction::station_registration_transition(
            &moved,
            ACTIVATION_HEIGHT + 1,
        ))
        .unwrap();
    let record = state.station_registry.get(station_id).unwrap();
    assert_eq!(
        (record.owner_address.as_str(), record.location.latitude),
        (owner.as_str(), 46.9)
    );

    // H: the same owner re-spelled `0x…` is the same owner, not a transfer.
    let respelled = format!("0x{}", owner.trim_start_matches("wrf"));
    assert!(same_station_owner(&owner, &respelled));
    let respelled_reg =
        signed_registration(station_id, &respelled, 47.0, &owner_key, &governance_key);
    state
        .apply_transition(Transaction::station_registration_transition(
            &respelled_reg,
            ACTIVATION_HEIGHT + 2,
        ))
        .unwrap();
    assert_eq!(
        state
            .station_registry
            .get(station_id)
            .unwrap()
            .location
            .latitude,
        47.0
    );
    assert!(!same_station_owner(&owner, &other));
    assert!(!same_station_owner("not-an-address", "also-not"));

    // ── 4. The DRAFT transfer preimage: byte-pinned, every field bound. ────
    let base = || {
        (
            "777".to_string(),
            "testnet".to_string(),
            station_id.to_string(),
            owner.clone(),
            other.clone(),
            PublicKey::from_private_key(&other_key).to_hex(),
            7u64,
            1_000u64,
        )
    };
    let build = |domain: &[u8], f: &(String, String, String, String, String, String, u64, u64)| {
        station_owner_transfer_message_with_domain(
            domain, &f.0, &f.1, &f.2, &f.3, &f.4, &f.5, f.6, f.7,
        )
    };
    let pinned = build(STATION_OWNER_TRANSFER_DOMAIN, &base());
    // Independent layout check: the documented encoding, built by hand.
    let mut expected = b"wrfcoin_station_owner_transfer_v1".to_vec();
    let f = base();
    for field in [&f.0, &f.1, &f.2, &f.3, &f.4, &f.5] {
        expected.extend_from_slice(&(field.len() as u32).to_le_bytes());
        expected.extend_from_slice(field.as_bytes());
    }
    expected.extend_from_slice(&7u64.to_le_bytes());
    expected.extend_from_slice(&1_000u64.to_le_bytes());
    assert_eq!(
        pinned, expected,
        "preimage must match the design doc's field-by-field encoding"
    );
    assert_eq!(
        blake3::hash(&pinned).to_hex().to_string(),
        PINNED_TRANSFER_PREIMAGE_BLAKE3,
        "the #3345 transfer preimage bytes changed; that invalidates every signature made over them"
    );
    assert_ne!(
        pinned,
        build(STATION_OWNER_TRANSFER_ACCEPT_DOMAIN, &base()),
        "domains must differ"
    );
    let mutations: [fn(&mut (String, String, String, String, String, String, u64, u64)); 8] = [
        |f| f.0.push('x'),
        |f| f.1.push('x'),
        |f| f.2.push('x'),
        |f| f.3.push('x'),
        |f| f.4.push('x'),
        |f| f.5.push('x'),
        |f| f.6 += 1,
        |f| f.7 += 1,
    ];
    for (i, mutate) in mutations.iter().enumerate() {
        let mut fields = base();
        mutate(&mut fields);
        assert_ne!(
            build(STATION_OWNER_TRANSFER_DOMAIN, &fields),
            pinned,
            "field {i} must be bound"
        );
    }

    std::env::remove_var(GOVERNANCE_ENV);
}

/// blake3 of the transfer preimage for the fixed inputs in step 4. Pinned from
/// the first run of this test; any change is a signature-breaking change.
const PINNED_TRANSFER_PREIMAGE_BLAKE3: &str =
    "53de3d8002686c583363c649155fd782c6aed3261c959d253ae3e8207b43e032"; // pragma: allowlist secret (blake3 of a public test preimage)
