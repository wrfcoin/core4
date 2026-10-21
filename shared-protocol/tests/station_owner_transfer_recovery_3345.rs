//! #3345 owner rulings 2026-09-27 — the stateless half of the owner-signed
//! transfer (Q4 countersignature, Q5 record preserved) and of the time-locked
//! owner-key recovery (Q1; reused by #3347 Q1 to lift a freeze).
//!
//! Pure signature / block-height checks: no schedule, no `State`, no block
//! production, so this file does not need to own its process. The stateful
//! half (owner-ledger nonce, "not frozen", storing and executing a pending
//! recovery) needs the owner ledger State field and is NOT covered here.
//!
//! RED control (recorded in the PR): with the countersignature check, the
//! notice-window comparison, the cancel-window guard and the Q5 record rule
//! mutated in `shared-protocol/src/blockchain/station_owner.rs`, the matching
//! tests below fail.
//!
//! Run from core4: `cargo test -p wrfcoin-shared-protocol --test
//! station_owner_transfer_recovery_3345`

use wrfcoin_shared_protocol::blockchain::station_owner::{
    PendingStationOwnerRecovery, STATION_OWNER_RECOVERY_ACCEPT_DOMAIN,
    STATION_OWNER_RECOVERY_FILE_DOMAIN, STATION_OWNER_RECOVERY_NOTICE_BLOCKS,
    STATION_OWNER_TRANSFER_MAX_VALIDITY_BLOCKS, StationOwnerRecoveryCancelPayload,
    StationOwnerRecoveryFilingPayload, StationOwnerRecoveryRefusal, StationOwnerTransferPayload,
    StationOwnerTransferRefusal, station_owner_recovery_cancel_message_with_domain,
    station_owner_recovery_message_with_domain, transferred_station_record,
};
use wrfcoin_shared_protocol::blockchain::transaction::{
    STATION_OWNER_TRANSFER_ACCEPT_DOMAIN, STATION_OWNER_TRANSFER_DOMAIN,
    StationRegistrationPayload, Transaction, TransactionInput, TransactionOutput, TransactionType,
};
use wrfcoin_shared_protocol::crypto::hash::hash_string;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::station_registry::{
    ContributionProvenanceClass, DataQualityTier, StationLocation, WeatherStation,
};

const CHAIN: &str = "777";
const NET: &str = "testnet";
const STATION: &str = "KDLH-ambient-1";

fn key(seed: u8) -> PrivateKey {
    PrivateKey::from_bytes(&[seed; 32]).expect("32-byte ed25519 seed")
}
fn pk_hex(k: &PrivateKey) -> String {
    PublicKey::from_private_key(k).to_hex()
}
fn addr(k: &PrivateKey) -> String {
    PublicKey::from_private_key(k).to_address()
}

/// A transfer A→B at nonce 3, valid until h1_000, signed by A and countersigned
/// by B.
fn signed_transfer(owner: &PrivateKey, new_owner: &PrivateKey) -> StationOwnerTransferPayload {
    let mut t = StationOwnerTransferPayload::new(
        STATION,
        addr(owner),
        pk_hex(owner),
        addr(new_owner),
        pk_hex(new_owner),
        3,
        1_000,
    );
    t.sign_as_owner_with_domain(CHAIN, NET, owner);
    t.countersign_with_domain(CHAIN, NET, new_owner);
    t
}

#[test]
fn transfer_requires_the_new_owners_countersignature_q4() {
    let (a, b, c) = (key(1), key(2), key(3));
    let ok = signed_transfer(&a, &b);
    assert_eq!(ok.verify_stateless_with_domain(CHAIN, NET, 900), Ok(()));

    // Q4: the current owner's signature ALONE is not enough.
    let mut no_counter = ok.clone();
    no_counter.to_owner_signature = None;
    assert_eq!(
        no_counter.verify_stateless_with_domain(CHAIN, NET, 900),
        Err(StationOwnerTransferRefusal::MissingOrBadCountersignature),
        "a transfer without the new owner's countersignature must be refused"
    );

    // Countersigned by a key that does not own the destination address.
    let mut wrong_counter = ok.clone();
    wrong_counter.countersign_with_domain(CHAIN, NET, &c);
    assert_eq!(
        wrong_counter.verify_stateless_with_domain(CHAIN, NET, 900),
        Err(StationOwnerTransferRefusal::MissingOrBadCountersignature)
    );

    // The new owner signing the SENDER domain cannot stand in for acceptance.
    let mut domain_swap = ok.clone();
    domain_swap.to_owner_signature = Some(
        b.sign(&ok.message_with_domain(STATION_OWNER_TRANSFER_DOMAIN, CHAIN, NET))
            .to_hex(),
    );
    assert_eq!(
        domain_swap.verify_stateless_with_domain(CHAIN, NET, 900),
        Err(StationOwnerTransferRefusal::MissingOrBadCountersignature)
    );

    // A countersignature from another chain does not verify here.
    let mut other_chain = ok.clone();
    other_chain.countersign_with_domain("778", NET, &b);
    assert_eq!(
        other_chain.verify_stateless_with_domain(CHAIN, NET, 900),
        Err(StationOwnerTransferRefusal::MissingOrBadCountersignature)
    );
}

#[test]
fn transfer_requires_the_current_owner_and_bound_keys() {
    let (a, b, c) = (key(1), key(2), key(3));
    let ok = signed_transfer(&a, &b);

    let mut no_owner_sig = ok.clone();
    no_owner_sig.from_owner_signature = None;
    assert_eq!(
        no_owner_sig.verify_stateless_with_domain(CHAIN, NET, 900),
        Err(StationOwnerTransferRefusal::MissingOrBadOwnerSignature)
    );

    // The acceptor's signature under the ACCEPT tag cannot stand in for the
    // owner's.
    let mut owner_slot_swap = ok.clone();
    owner_slot_swap.from_owner_signature = Some(
        a.sign(&ok.message_with_domain(STATION_OWNER_TRANSFER_ACCEPT_DOMAIN, CHAIN, NET))
            .to_hex(),
    );
    assert_eq!(
        owner_slot_swap.verify_stateless_with_domain(CHAIN, NET, 900),
        Err(StationOwnerTransferRefusal::MissingOrBadOwnerSignature)
    );

    // Someone else's key claiming to be the owner: pubkey does not derive to
    // from_owner_address.
    let mut impostor = ok.clone();
    impostor.from_owner_pubkey = pk_hex(&c);
    assert_eq!(
        impostor.verify_stateless_with_domain(CHAIN, NET, 900),
        Err(StationOwnerTransferRefusal::FromKeyNotBound)
    );

    // A typo'd destination (no key derives to it) is refused — the Q4 rationale.
    let mut typo = ok.clone();
    typo.to_owner_address = addr(&c);
    assert_eq!(
        typo.verify_stateless_with_domain(CHAIN, NET, 900),
        Err(StationOwnerTransferRefusal::ToKeyNotBound)
    );

    // A→A (even re-spelled 0x… vs wrf…) is not a transfer.
    let mut same = ok.clone();
    same.to_owner_address = format!("0x{}", addr(&a).trim_start_matches("wrf"));
    same.to_owner_pubkey = pk_hex(&a);
    assert_eq!(
        same.verify_stateless_with_domain(CHAIN, NET, 900),
        Err(StationOwnerTransferRefusal::SameOwner)
    );
}

#[test]
fn transfer_validity_window_is_in_block_heights() {
    let (a, b) = (key(1), key(2));
    let ok = signed_transfer(&a, &b); // expires_at_height = 1_000
    assert_eq!(ok.verify_stateless_with_domain(CHAIN, NET, 1_000), Ok(()));
    assert_eq!(
        ok.verify_stateless_with_domain(CHAIN, NET, 1_001),
        Err(StationOwnerTransferRefusal::Expired)
    );
    // Parked too far ahead: expires_at_height > h + MAX_VALIDITY.
    let mut far = StationOwnerTransferPayload::new(
        STATION,
        addr(&a),
        pk_hex(&a),
        addr(&b),
        pk_hex(&b),
        3,
        20_000,
    );
    far.sign_as_owner_with_domain(CHAIN, NET, &a);
    far.countersign_with_domain(CHAIN, NET, &b);
    let too_early = 20_000 - STATION_OWNER_TRANSFER_MAX_VALIDITY_BLOCKS - 1;
    assert_eq!(
        far.verify_stateless_with_domain(CHAIN, NET, too_early),
        Err(StationOwnerTransferRefusal::ValidityTooLong)
    );
    assert_eq!(
        far.verify_stateless_with_domain(CHAIN, NET, too_early + 1),
        Ok(())
    );
}

#[test]
fn transfer_keeps_provenance_and_reputation_q5() {
    let (a, b) = (key(1), key(2));
    let existing = WeatherStation {
        station_id: STATION.to_string(),
        owner_address: addr(&a),
        location: StationLocation {
            latitude: 46.8369,
            longitude: -92.1833,
            elevation_meters: Some(432.0),
        },
        hardware_model: "Ambient WS-5000".to_string(),
        calibration_date: Some("2026-09-01".to_string()),
        is_active: true,
        reputation_score: 8_750,
        data_quality_tier: DataQualityTier::Gold,
        provenance_class: ContributionProvenanceClass::Real,
        ring_public_key: Some("ab".repeat(32)),
    };
    let moved = transferred_station_record(&existing, &addr(&b));
    assert_eq!(moved.owner_address, addr(&b));
    assert_eq!(moved.provenance_class, ContributionProvenanceClass::Real);
    assert_eq!(moved.reputation_score, 8_750);
    let mut expected = existing.clone();
    expected.owner_address = addr(&b);
    assert_eq!(moved, expected, "only owner_address changes on a transfer");
}

#[test]
fn recovery_executes_only_after_the_notice_window_q1() {
    assert_eq!(
        STATION_OWNER_RECOVERY_NOTICE_BLOCKS, 34_000,
        "owner ruling: ~14 days"
    );
    let (a, b) = (key(1), key(2));
    let filing = StationOwnerRecoveryFilingPayload::new(STATION, addr(&a), addr(&b), pk_hex(&b), 5);
    let filed_at = 21_000;
    let pending = PendingStationOwnerRecovery::filed(&filing, filed_at);
    assert_eq!(pending.executable_at_height(), filed_at + 34_000);
    assert!(!pending.is_executable_at(filed_at));
    assert!(
        !pending.is_executable_at(filed_at + 34_000 - 1),
        "a recovery must not execute before the full notice window"
    );
    assert!(pending.is_executable_at(filed_at + 34_000));
    // Saturates instead of wrapping near u64::MAX.
    let late = PendingStationOwnerRecovery::filed(&filing, u64::MAX - 5);
    assert!(!late.is_executable_at(u64::MAX - 1));
}

#[test]
fn current_owner_can_cancel_a_recovery_only_before_it_executes_q1() {
    let (a, b) = (key(1), key(2));
    let filing = StationOwnerRecoveryFilingPayload::new(STATION, addr(&a), addr(&b), pk_hex(&b), 5);
    let filed_at = 21_000;
    let pending = PendingStationOwnerRecovery::filed(&filing, filed_at);

    let mut cancel = StationOwnerRecoveryCancelPayload::new(STATION, addr(&a), pk_hex(&a), 5);
    cancel.sign_with_domain(CHAIN, NET, &a);
    assert_eq!(cancel.verify_stateless_with_domain(CHAIN, NET), Ok(()));
    assert!(pending.is_cancelled_by(STATION, &cancel, filed_at + 1));
    assert!(pending.is_cancelled_by(STATION, &cancel, filed_at + 34_000 - 1));
    assert!(
        !pending.is_cancelled_by(STATION, &cancel, filed_at + 34_000),
        "once executable, a late cancel no longer applies"
    );

    // Another recovery's nonce, or another station, does not cancel this one.
    let mut other_nonce = cancel.clone();
    other_nonce.recovery_nonce = 6;
    assert!(!pending.is_cancelled_by(STATION, &other_nonce, filed_at + 1));
    assert!(!pending.is_cancelled_by("other-station", &cancel, filed_at + 1));

    // The would-be NEW owner cannot "cancel" as if it were the owner.
    let mut by_new_owner = StationOwnerRecoveryCancelPayload::new(STATION, addr(&b), pk_hex(&b), 5);
    by_new_owner.sign_with_domain(CHAIN, NET, &b);
    assert_eq!(
        by_new_owner.verify_stateless_with_domain(CHAIN, NET),
        Ok(())
    );
    assert!(!pending.is_cancelled_by(STATION, &by_new_owner, filed_at + 1));

    // A cancel claiming the owner's address with another key fails the binding.
    let mut forged = StationOwnerRecoveryCancelPayload::new(STATION, addr(&a), pk_hex(&b), 5);
    forged.sign_with_domain(CHAIN, NET, &b);
    assert_eq!(
        forged.verify_stateless_with_domain(CHAIN, NET),
        Err(StationOwnerRecoveryRefusal::OwnerKeyNotBound)
    );
    let mut unsigned = cancel.clone();
    unsigned.owner_signature = None;
    assert_eq!(
        unsigned.verify_stateless_with_domain(CHAIN, NET),
        Err(StationOwnerRecoveryRefusal::MissingOrBadOwnerSignature)
    );
}

#[test]
fn recovery_filing_needs_governance_and_the_new_owners_acceptance() {
    let (a, b, gov, rogue) = (key(1), key(2), key(9), key(8));
    // SAFETY-ish: this is the only test in this file that reads the
    // allowlist, and it sets a single fixed value.
    std::env::set_var("WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS", pk_hex(&gov));

    let mut filing =
        StationOwnerRecoveryFilingPayload::new(STATION, addr(&a), addr(&b), pk_hex(&b), 5);
    filing.accept_with_domain(CHAIN, NET, &b);
    filing.sign_as_governance_with_domain(CHAIN, NET, &gov);
    assert_eq!(filing.verify_stateless_with_domain(CHAIN, NET), Ok(()));

    let mut by_rogue = filing.clone();
    by_rogue.sign_as_governance_with_domain(CHAIN, NET, &rogue);
    assert_eq!(
        by_rogue.verify_stateless_with_domain(CHAIN, NET),
        Err(StationOwnerRecoveryRefusal::NotGovernance)
    );

    let mut unaccepted = filing.clone();
    unaccepted.to_owner_signature = None;
    assert_eq!(
        unaccepted.verify_stateless_with_domain(CHAIN, NET),
        Err(StationOwnerRecoveryRefusal::MissingOrBadAcceptance)
    );

    // Governance cannot redirect a signed filing to another owner.
    let mut redirected = filing.clone();
    redirected.to_owner_address = addr(&a);
    assert_eq!(
        redirected.verify_stateless_with_domain(CHAIN, NET),
        Err(StationOwnerRecoveryRefusal::SameOwner)
    );
    let mut renonced = filing.clone();
    renonced.recovery_nonce = 6;
    assert_eq!(
        renonced.verify_stateless_with_domain(CHAIN, NET),
        Err(StationOwnerRecoveryRefusal::MissingOrBadGovernanceSignature)
    );
}

#[test]
fn recovery_preimages_are_distinct_and_bind_every_field() {
    let base = |domain: &[u8], nonce: u64| {
        station_owner_recovery_message_with_domain(
            domain, CHAIN, NET, STATION, "wrfA", "wrfB", "pkB", nonce,
        )
    };
    let file = base(STATION_OWNER_RECOVERY_FILE_DOMAIN, 5);
    let accept = base(STATION_OWNER_RECOVERY_ACCEPT_DOMAIN, 5);
    assert_ne!(file, accept);
    assert_ne!(file, base(STATION_OWNER_RECOVERY_FILE_DOMAIN, 6));
    assert!(file.starts_with(b"wrfcoin_station_owner_recovery_file_v1"));
    assert!(file.ends_with(&5u64.to_le_bytes()));
    let cancel = station_owner_recovery_cancel_message_with_domain(CHAIN, NET, STATION, "wrfA", 5);
    assert!(cancel.starts_with(b"wrfcoin_station_owner_recovery_cancel_v1"));
    assert_ne!(
        cancel,
        station_owner_recovery_cancel_message_with_domain("778", NET, STATION, "wrfA", 5)
    );
}

#[test]
fn transfer_and_registration_parsers_never_confuse_each_other() {
    let (a, b) = (key(1), key(2));
    let build = |data: String| {
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: hash_string("input-3345-parse"),
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 1,
                address: "wrf-3345-parse".to_string(),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        tx.tx_type = TransactionType::StationRegistration;
        tx.data = Some(data);
        tx.set_id();
        tx
    };
    let transfer_tx = build(serde_json::to_string(&signed_transfer(&a, &b)).unwrap());
    assert!(transfer_tx.parse_station_owner_transfer().is_some());
    assert!(
        transfer_tx.parse_station_registration().is_none(),
        "an old/new registration parser treats a transfer as inert"
    );
    let registration = StationRegistrationPayload::new(
        STATION,
        addr(&a),
        46.8,
        -92.1,
        "Ambient WS-5000",
        DataQualityTier::Gold,
        ContributionProvenanceClass::Real,
    );
    let registration_tx = build(serde_json::to_string(&registration).unwrap());
    assert!(registration_tx.parse_station_owner_transfer().is_none());
}
