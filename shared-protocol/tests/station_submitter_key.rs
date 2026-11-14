//! `station_submitter_key` (#3565 design 1): owner-approved submitter keys.
//!
//! With `station_owner_consent`, `station_owner_actions`,
//! `device_submission_limit` and `station_submitter_key` scheduled from height
//! 1 (this binary owns its process-wide schedule):
//! - the `set_submitter_key` preimage and its stateless checks, and that the
//!   four older actions sign exactly the bytes they always did;
//! - an approved DEVICE key (a phone collect key, signing the same
//!   `wrfcoin_commit_auth_v2` message the app signs) may commit for the
//!   station; any other non-owner key may not; the signer class is recorded;
//! - replace, clear, and the automatic revocation on report-stolen,
//!   self-deactivation, owner transfer and executed recovery; no approval on a
//!   frozen or self-deactivated station; the nonce is the owner nonce;
//! - admission precheck, root section, pruning and snapshot round trip.

use sha3::{Digest, Sha3_256};
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::state_snapshot::export_state_entries;
use wrfcoin_shared_protocol::blockchain::station_owner::{
    STATION_OWNER_RECOVERY_NOTICE_BLOCKS, StationOwnerActionPayload, StationOwnerActionRefusal,
    StationOwnerLedgerRefusal, StationOwnerTransferPayload, StationOwnerTxRejection,
    StationSubmitterKeyPayload,
};
use wrfcoin_shared_protocol::blockchain::transaction::{
    CommitPayload, CommitSignerClass, StationOwnerActionKind, SubmitterClass, Transaction,
    TransactionInput, TransactionOutput, TransactionType,
    commit_authorization_message_with_domain, station_owner_action_message_with_domain,
    station_owner_set_submitter_message_with_domain,
};
use wrfcoin_shared_protocol::contributions::commit_reveal::APPLIED_REVEAL_RETENTION_BLOCKS;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ChainIdentity, ConsensusFeature, FeatureSchedule, feature_active, install_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::hash::hash_string;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::station_registry::{
    ContributionProvenanceClass, DataQualityTier, StationLocation, WeatherStation,
};

const T0: i64 = 1_762_646_400;
const STATION: &str = "st-submitter-1";
const ROOT_TAG: &[u8] = b"wrfcoin_station_submitters_v1";

fn schedule() {
    install_feature_schedule(
        FeatureSchedule::from_entries(vec![
            ("station_owner_consent".to_string(), 1),
            ("station_owner_actions".to_string(), 1),
            ("device_submission_limit".to_string(), 1),
            // From genesis, so the limit is in force from its own activation
            // (the 2,000-block lead rule, `device_submission_limit_active`).
            ("station_submitter_key".to_string(), 0),
        ])
        .unwrap(),
    );
}

fn key(seed: u8) -> PrivateKey {
    PrivateKey::from_bytes(&[seed; 32]).unwrap()
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

fn ledger_tx(tag: &str, data: String) -> Transaction {
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
    tx.data = Some(data);
    tx.set_id();
    tx
}

fn step(state: &mut State, tag: &str, data: String, height: u64) -> bool {
    match ledger_tx(tag, data).station_owner_ledger_transition(height) {
        Some(t) => {
            state.apply_transition(t).expect("an extracted owner step never fails");
            true
        },
        None => false,
    }
}

fn set_json(owner: &PrivateKey, submitter: Option<(&PrivateKey, SubmitterClass)>, nonce: u64) -> String {
    let (c, n) = chain();
    let mut p = StationSubmitterKeyPayload::new(
        STATION,
        addr_of(owner),
        pk_of(owner),
        submitter.map(|(k, class)| (pk_of(k), class)),
        nonce,
        5_000,
    );
    p.sign_with_domain(&c, &n, owner);
    serde_json::to_string(&p).unwrap()
}

fn action_json(owner: &PrivateKey, kind: StationOwnerActionKind, nonce: u64) -> String {
    let (c, n) = chain();
    let mut p = StationOwnerActionPayload::new(STATION, addr_of(owner), pk_of(owner), kind, nonce, 5_000);
    p.sign_with_domain(&c, &n, owner);
    serde_json::to_string(&p).unwrap()
}

fn station(owner: &PrivateKey) -> WeatherStation {
    WeatherStation {
        station_id: STATION.to_string(),
        owner_address: addr_of(owner),
        // A 0.1-degree grid point, not a real station.
        location: StationLocation { latitude: 45.0, longitude: -93.0, elevation_meters: Some(250.0) },
        hardware_model: "test-station".to_string(),
        calibration_date: None,
        is_active: true,
        reputation_score: 50,
        data_quality_tier: DataQualityTier::Gold,
        provenance_class: ContributionProvenanceClass::Real,
        ring_public_key: None,
    }
}

fn state_with_station(owner: &PrivateKey) -> State {
    let mut state = State::new();
    state.register_weather_station(station(owner)).expect("station registers");
    state
}

fn commit(signer: &PrivateKey, tag: &str) -> CommitPayload {
    let data = serde_json::json!({ "temperature_c": 12.5, "tag": tag });
    let nonce = format!("nonce-{tag}");
    let commit_id = Transaction::compute_commit_id(&data, &nonce, STATION, T0, "weather");
    let mut payload = CommitPayload {
        commit_id,
        station_id: STATION.to_string(),
        timestamp: T0,
        contribution_type: "weather".to_string(),
        auth_pubkey: None,
        auth_signature: None,
    };
    payload.sign_authorization(signer);
    payload
}

/// Apply a contribution commit; whether it applied.
fn try_commit(state: &mut State, signer: &PrivateKey, tag: &str, height: u64) -> bool {
    state
        .apply_transition(StateTransition::RegisterCommit { payload: commit(signer, tag), block_height: height })
        .is_ok()
}

fn signer_of(state: &State, signer: &PrivateKey, tag: &str) -> Option<CommitSignerClass> {
    state.station_submitters.commit_signers.get(&commit(signer, tag).commit_id).map(|r| r.class)
}

fn nonce(state: &State) -> u64 {
    state.station_owner_ledger.get(STATION).map(|e| e.owner_nonce).unwrap_or(0)
}

#[test]
fn preimage_and_stateless_checks() {
    schedule();
    let (c, n) = chain();
    let owner = key(0x11);
    let phone = key(0x22);
    // The four older actions sign exactly the old bytes, even if a payload
    // carries submitter fields (they are ignored for those actions).
    for kind in [
        StationOwnerActionKind::ReportStolen,
        StationOwnerActionKind::Unfreeze,
        StationOwnerActionKind::Deactivate,
        StationOwnerActionKind::Reactivate,
    ] {
        let mut p = StationOwnerActionPayload::new(STATION, addr_of(&owner), pk_of(&owner), kind, 7, 900);
        assert_eq!(
            p.message_with_domain(&c, &n),
            station_owner_action_message_with_domain(&c, &n, STATION, &addr_of(&owner), kind, 7, 900),
            "{kind:?} preimage unchanged"
        );
        p.sign_with_domain(&c, &n, &owner);
        assert_eq!(p.verify_stateless_with_domain(&c, &n, 10), Ok(()), "{kind:?} verifies");
        // Odd values under the new field names are unknown fields to the
        // legacy shape: still parsed, still extracted (all-flags-off parity is
        // pinned in station_submitter_key_inactive.rs).
        let body = serde_json::to_string(&p).unwrap();
        let odd = format!(r#"{},"submitter_class":"future_class","submitter_pubkey":17}}"#, body.trim_end_matches('}'));
        let back: StationOwnerActionPayload = serde_json::from_str(&odd).expect("legacy shape ignores them");
        assert_eq!(back, p);
    }
    // set_submitter_key: base bytes with discriminant 5, then flag, key, class.
    let base = station_owner_action_message_with_domain(
        &c, &n, STATION, &addr_of(&owner), StationOwnerActionKind::SetSubmitterKey, 3, 900,
    );
    let set = station_owner_set_submitter_message_with_domain(
        &c, &n, STATION, &addr_of(&owner), 3, 900, Some((&pk_of(&phone), SubmitterClass::Device)),
    );
    let mut want = base.clone();
    want.push(1);
    want.extend_from_slice(&64u32.to_le_bytes());
    want.extend_from_slice(pk_of(&phone).as_bytes());
    want.push(1);
    assert_eq!(set, want);
    let clear = station_owner_set_submitter_message_with_domain(&c, &n, STATION, &addr_of(&owner), 3, 900, None);
    assert_eq!(clear, [base.as_slice(), &[0]].concat());
    let relay = station_owner_set_submitter_message_with_domain(
        &c, &n, STATION, &addr_of(&owner), 3, 900, Some((&pk_of(&phone), SubmitterClass::Relay)),
    );
    assert_ne!(relay, set, "the class is signed");

    let mut good = StationSubmitterKeyPayload::new(
        STATION, addr_of(&owner), pk_of(&owner), Some((pk_of(&phone), SubmitterClass::Device)), 3, 900,
    );
    good.sign_with_domain(&c, &n, &owner);
    assert_eq!(good.verify_stateless_with_domain(&c, &n, 10), Ok(()));
    // Swapping the key or class after signing breaks the signature.
    let mut swapped = good.clone();
    swapped.submitter_class = Some(SubmitterClass::Relay);
    assert_eq!(
        swapped.verify_stateless_with_domain(&c, &n, 10),
        Err(StationOwnerActionRefusal::MissingOrBadOwnerSignature)
    );
    // Incomplete or non-canonical pairs are malformed.
    for (key, class) in [
        (Some(pk_of(&phone)), None),
        (None, Some(SubmitterClass::Device)),
        (Some(pk_of(&phone).to_uppercase()), Some(SubmitterClass::Device)),
        (Some("ab".repeat(31)), Some(SubmitterClass::Device)),
        (Some("zz".repeat(32)), Some(SubmitterClass::Device)),
    ] {
        let mut bad = good.clone();
        bad.submitter_pubkey = key;
        bad.submitter_class = class;
        assert_eq!(bad.verify_stateless_with_domain(&c, &n, 10), Err(StationOwnerActionRefusal::Malformed));
    }
    // The JSON shape: snake_case action and class; older actions omit the fields.
    let v: serde_json::Value = serde_json::to_value(&good).unwrap();
    assert_eq!(v["action"], "set_submitter_key");
    assert_eq!(v["submitter_class"], "device");
    let old = serde_json::to_value(StationOwnerActionPayload::new(
        STATION, addr_of(&owner), pk_of(&owner), StationOwnerActionKind::ReportStolen, 0, 9,
    ))
    .unwrap();
    assert!(old.get("submitter_pubkey").is_none() && old.get("submitter_class").is_none());
}

#[test]
fn approved_keys_commit_revoke_and_round_trip() {
    schedule();
    assert!(feature_active(ConsensusFeature::StationSubmitterKey, 1));
    let owner = key(0x11);
    let phone = key(0x22);
    let relay = key(0x23);
    let stranger = key(0x24);
    let mut state = state_with_station(&owner);
    let mut h = 10u64;

    // No key yet: only the owner may commit.
    assert!(try_commit(&mut state, &owner, "o1", h));
    assert_eq!(signer_of(&state, &owner, "o1"), Some(CommitSignerClass::Owner));
    assert!(!try_commit(&mut state, &phone, "p0", h));

    // Approve the phone's collect key (device).
    assert!(step(&mut state, "set-phone", set_json(&owner, Some((&phone, SubmitterClass::Device)), 0), h));
    assert_eq!(nonce(&state), 1, "the owner nonce is consumed");
    let sub = state.station_submitter(STATION).cloned().expect("approved");
    assert_eq!((sub.pubkey.as_str(), sub.class, sub.set_at_height), (pk_of(&phone).as_str(), SubmitterClass::Device, h));
    h += 1;
    // The phone signs the same commit-authorization message the app signs.
    let c = commit(&phone, "p1");
    let (cid, nid) = chain();
    assert!(PublicKey::from_hex(c.auth_pubkey.as_deref().unwrap())
        .unwrap()
        .verify(
            &commit_authorization_message_with_domain(&cid, &nid, STATION, &c.commit_id, "weather"),
            &wrfcoin_shared_protocol::crypto::signature::Signature::from_hex(c.auth_signature.as_deref().unwrap()).unwrap(),
        ));
    assert!(c.verify_authorization());
    assert!(try_commit(&mut state, &phone, "p1", h), "the approved collect key commits");
    assert_eq!(signer_of(&state, &phone, "p1"), Some(CommitSignerClass::Device));
    assert!(!try_commit(&mut state, &stranger, "s1", h), "any other key is refused");
    assert!(state.state_root_material().windows(ROOT_TAG.len()).any(|w| w == ROOT_TAG), "section in the root");
    h += 1;

    // Replay of the same approval: nonce mismatch, inert.
    assert!(step(&mut state, "set-phone-replay", set_json(&owner, Some((&phone, SubmitterClass::Device)), 0), h));
    assert_eq!(nonce(&state), 1);

    // Replace with a relay key: the phone stops, the relay works (class relay).
    assert!(step(&mut state, "set-relay", set_json(&owner, Some((&relay, SubmitterClass::Relay)), 1), h));
    h += 1;
    assert!(!try_commit(&mut state, &phone, "p2", h));
    assert!(try_commit(&mut state, &relay, "r1", h));
    assert_eq!(signer_of(&state, &relay, "r1"), Some(CommitSignerClass::Relay));

    // Clear: the relay stops; the owner still commits.
    assert!(step(&mut state, "clear", set_json(&owner, None, 2), h));
    h += 1;
    assert!(state.station_submitter(STATION).is_none());
    assert!(!try_commit(&mut state, &relay, "r2", h));
    assert!(try_commit(&mut state, &owner, "o2", h));

    // A stranger cannot approve a key (not the registered owner).
    assert!(step(&mut state, "stranger-set", set_json(&stranger, Some((&stranger, SubmitterClass::Device)), 3), h));
    assert!(state.station_submitter(STATION).is_none());
    assert_eq!(nonce(&state), 3);

    // Report stolen revokes at once; no approval while frozen; unfreeze, approve again.
    assert!(step(&mut state, "set-3", set_json(&owner, Some((&phone, SubmitterClass::Device)), 3), h));
    assert!(state.station_submitter(STATION).is_some());
    assert!(step(&mut state, "stolen", action_json(&owner, StationOwnerActionKind::ReportStolen, 4), h));
    assert!(state.station_submitter(STATION).is_none(), "report-stolen revokes the key");
    h += 1;
    assert!(!try_commit(&mut state, &phone, "p3", h), "a stolen phone's collect key stops");
    let before = nonce(&state);
    assert!(step(&mut state, "set-frozen", set_json(&owner, Some((&phone, SubmitterClass::Device)), before), h));
    assert!(state.station_submitter(STATION).is_none() && nonce(&state) == before, "no approval while frozen");
    let mut scratch = state.clone();
    assert_eq!(
        scratch.apply_station_submitter_key(STATION, &addr_of(&owner), before, None, h),
        Err(StationOwnerLedgerRefusal::Frozen)
    );
    assert!(step(&mut state, "unfreeze", action_json(&owner, StationOwnerActionKind::Unfreeze, 5), h));

    // Self-deactivation revokes; no approval while deactivated; reactivate.
    assert!(step(&mut state, "set-6", set_json(&owner, Some((&phone, SubmitterClass::Device)), 6), h));
    assert!(step(&mut state, "deactivate", action_json(&owner, StationOwnerActionKind::Deactivate, 7), h));
    assert!(state.station_submitter(STATION).is_none(), "deactivation revokes the key");
    assert!(step(&mut state, "set-deact", set_json(&owner, Some((&phone, SubmitterClass::Device)), 8), h));
    assert!(state.station_submitter(STATION).is_none() && nonce(&state) == 8);
    assert!(step(&mut state, "reactivate", action_json(&owner, StationOwnerActionKind::Reactivate, 8), h));
    h += 1;

    // Admission precheck of a good approval; then apply it.
    let tx = ledger_tx("pre", set_json(&owner, Some((&phone, SubmitterClass::Device)), 9));
    let (cid, nid) = chain();
    let admitted = state.precheck_station_owner_tx(&tx, h, &cid, &nid).expect("admitted");
    assert_eq!((admitted.kind, admitted.nonce), ("station_owner_action_v1", 9));
    let stale = ledger_tx("pre-stale", set_json(&owner, Some((&phone, SubmitterClass::Device)), 3));
    assert_eq!(
        state.precheck_station_owner_tx(&stale, h, &cid, &nid).unwrap_err().code(),
        StationOwnerTxRejection::Ledger(StationOwnerLedgerRefusal::NonceMismatch).code()
    );
    assert!(step(&mut state, "set-9", set_json(&owner, Some((&phone, SubmitterClass::Device)), 9), h));
    assert!(state.station_submitter(STATION).is_some());

    // Snapshot round trip keeps the keys and signer records.
    let entries = export_state_entries(&state).expect("export");
    assert!(entries.iter().any(|(k, _)| String::from_utf8_lossy(k).contains("station_submitters")));
    let mut restored = State::new();
    restored.apply_snapshot_entries(&entries).expect("import");
    assert_eq!(restored.station_submitters, state.station_submitters);
    assert_eq!(Sha3_256::digest(restored.state_root_material()), Sha3_256::digest(state.state_root_material()));

    // Owner transfer revokes (the new owner never approved it); the new owner may approve.
    let next_owner = key(0x33);
    let (c, n) = chain();
    let mut transfer = StationOwnerTransferPayload::new(
        STATION, addr_of(&owner), pk_of(&owner), addr_of(&next_owner), pk_of(&next_owner), 10, 5_000,
    );
    transfer.sign_as_owner_with_domain(&c, &n, &owner);
    transfer.countersign_with_domain(&c, &n, &next_owner);
    assert!(step(&mut state, "transfer", serde_json::to_string(&transfer).unwrap(), h));
    assert!(state.station_submitter(STATION).is_none(), "a transfer revokes the key");
    assert!(!try_commit(&mut state, &phone, "p4", h + 1));
    assert!(step(&mut state, "new-owner-set", set_json(&next_owner, Some((&relay, SubmitterClass::Relay)), 11), h));
    assert!(try_commit(&mut state, &relay, "r3", h + 1));

    // An executed recovery revokes too.
    let recovered = key(0x44);
    state
        .apply_station_owner_recovery_filing(STATION, &addr_of(&next_owner), &addr_of(&recovered), 12, h)
        .expect("filed");
    state
        .apply_station_owner_recovery_execute(STATION, 12, h + STATION_OWNER_RECOVERY_NOTICE_BLOCKS)
        .expect("executed");
    assert!(state.station_submitter(STATION).is_none(), "an executed recovery revokes the key");

    // Signer records age out with the applied-reveal retention.
    let newest = state.station_submitters.commit_signers.values().map(|r| r.committed_at).max().unwrap();
    state.prune_commit_signers(newest + APPLIED_REVEAL_RETENTION_BLOCKS);
    assert!(!state.station_submitters.commit_signers.is_empty(), "kept at the boundary");
    state.prune_commit_signers(newest + APPLIED_REVEAL_RETENTION_BLOCKS + 1);
    assert!(state.station_submitters.commit_signers.is_empty(), "dropped after it");
    assert!(state.station_submitters.is_empty());
    assert!(!state.state_root_material().windows(ROOT_TAG.len()).any(|w| w == ROOT_TAG), "absent again once empty");
}

#[test]
fn a_registration_that_changes_the_owner_revokes_the_key() {
    schedule();
    let owner = key(0x51);
    let phone = key(0x52);
    let new_owner = key(0x53);
    let mut state = state_with_station(&owner);
    assert!(step(&mut state, "set", set_json(&owner, Some((&phone, SubmitterClass::Device)), 0), 10));
    assert!(state.station_submitter(STATION).is_some());
    // A re-registration by the SAME owner (spelled differently) keeps it.
    let same = WeatherStation { owner_address: addr_of(&owner).to_uppercase().replacen("WRF", "wrf", 1), ..station(&owner) };
    state.apply_station_registration(same).expect("same-owner re-registration");
    assert!(state.station_submitter(STATION).is_some(), "same owner: the approval stays");
    // A governance re-registration to another owner (as when owner consent is
    // off) revokes it: the new owner never approved that key.
    state
        .apply_station_registration(WeatherStation { owner_address: addr_of(&new_owner), ..station(&owner) })
        .expect("owner-changing re-registration");
    assert!(state.station_submitter(STATION).is_none(), "an owner change revokes the key");
    assert!(!try_commit(&mut state, &phone, "after-owner-change", 11), "the old key's commit is refused");
    assert!(try_commit(&mut state, &new_owner, "new-owner", 11));
}

#[test]
fn routing_reads_the_action_with_the_legacy_enum_deserializer() {
    use wrfcoin_shared_protocol::blockchain::station_owner::owner_action_is_set_submitter;
    for (json, want) in [
        (r#"{"action":"report_stolen"}"#, Some(false)),
        (r#"{"action":{"report_stolen":null}}"#, Some(false)),
        (r#"{"action":{"reactivate":null},"submitter_class":"x"}"#, Some(false)),
        (r#"{"action":"set_submitter_key"}"#, Some(true)),
        (r#"{"action":{"set_submitter_key":null}}"#, Some(true)),
        (r#"{"action":"future_action"}"#, None),
        (r#"{"action":17}"#, None),
        (r#"{"station_id":"s"}"#, None),
        (r#"{"action":"report_stolen","action":"unfreeze"}"#, None),
    ] {
        assert_eq!(owner_action_is_set_submitter(json), want, "{json}");
    }
}
