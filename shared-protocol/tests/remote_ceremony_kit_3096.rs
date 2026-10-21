//! #3096 — remote ceremony kit, library half: coordinator assembly refusals,
//! the producer possession proof, the custody report, and (with
//! `--features hsm`) the no-PIN token enumeration against a throwaway
//! SoftHSM token.

use std::collections::BTreeMap;

use rand::SeedableRng;
use rand::rngs::StdRng;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::crypto::signer::EnvKeySigner;
use wrfcoin_shared_protocol::oracle::ceremony::{
    AssembleError, GenesisOracleCeremony, GenesisOraclePossessionBundle, ProducerProofError,
    assemble_possession_bundle, bundle_registration_commitment, sign_oracle_possession_proof,
    sign_producer_possession_proof, verify_oracle_possession, verify_producer_possession_proof,
};
use wrfcoin_shared_protocol::security::key_custody::{
    CustodyEntry, CustodyFinding, CustodyManifest, CustodyReport, ManifestError, ObservedKey,
};

const CHAIN: &str = "777";
const NETWORK: &str = "testnet";

fn key(seed: u64) -> PrivateKey {
    PrivateKey::generate(&mut StdRng::seed_from_u64(seed))
}

/// A three-oracle bundle (quorum 2) with signing keys `key(11..=13)`.
fn bundle(ceremony_id: &str, submitted_at: i64) -> (GenesisOraclePossessionBundle, Vec<PrivateKey>) {
    bundle_at(ceremony_id, 1_800_000_000, submitted_at)
}

/// Same, with an explicit `activated_at` — one of the fields the registration
/// commitment covers (`submitted_at` deliberately is not: re-submitting the
/// same registration set must not invalidate proofs already collected).
fn bundle_at(ceremony_id: &str, activated_at: i64, submitted_at: i64) -> (GenesisOraclePossessionBundle, Vec<PrivateKey>) {
    let mut identities: Vec<String> = (1..=3)
        .map(|seed| PublicKey::from_private_key(&key(seed)).to_hex())
        .collect();
    identities.sort();
    let signing_keys = vec![key(11), key(12), key(13)];
    let signing_pubkeys: BTreeMap<String, String> = identities
        .iter()
        .zip(&signing_keys)
        .map(|(id, k)| (id.clone(), PublicKey::from_private_key(k).to_hex()))
        .collect();
    let ceremony = GenesisOracleCeremony::new(
        CHAIN,
        NETWORK,
        identities,
        2,
        signing_pubkeys,
        1,
        activated_at,
        submitted_at,
    )
    .expect("valid ceremony");
    (
        GenesisOraclePossessionBundle {
            ceremony_id: ceremony_id.into(),
            artifact: ceremony.build_artifact(&key(90)),
            possession_proofs: Vec::new(),
        },
        signing_keys,
    )
}

fn governance(b: &GenesisOraclePossessionBundle) -> Vec<String> {
    vec![b.artifact.governance_pubkey.clone()]
}

fn proof_from(b: &GenesisOraclePossessionBundle, k: &PrivateKey, ts: i64) -> wrfcoin_shared_protocol::blockchain::transaction::OracleAttestationPayload {
    let signer = EnvKeySigner::from_private_key(k.clone(), "test-only-oracle-key");
    sign_oracle_possession_proof(b, CHAIN, NETWORK, &governance(b), &signer, ts).expect("registered signer")
}

#[test]
fn two_software_signers_two_proofs_assemble_verify_count() {
    let (b, keys) = bundle("cut-3096", 1_800_000_100);
    let proofs: Vec<_> = keys.iter().map(|k| proof_from(&b, k, 1_800_000_200)).collect();
    let assembled = assemble_possession_bundle(&b, &proofs).expect("assemble");
    assert_eq!(assembled.possession_proofs.len(), 3);
    // Deterministic order: by oracle identity.
    let ids: Vec<&str> = assembled.possession_proofs.iter().map(|p| p.oracle_id.as_str()).collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
    let verified = verify_oracle_possession(&assembled, CHAIN, NETWORK, &governance(&b)).expect("verify");
    assert_eq!(verified.verified_operator_count, 3);
    // Proofs already in the bundle count; assembling the same two again is a duplicate.
    let partial = assemble_possession_bundle(&b, &proofs[..2]).unwrap();
    let again = assemble_possession_bundle(&partial, &proofs[2..]).unwrap();
    assert_eq!(again.possession_proofs.len(), 3);
    assert!(matches!(
        assemble_possession_bundle(&partial, &proofs[..1]),
        Err(AssembleError::DuplicateIdentity { .. })
    ));
}

#[test]
fn assemble_refuses_duplicate_foreign_and_mismatched_commitment_proofs() {
    let (b, keys) = bundle("cut-3096", 1_800_000_100);
    let p0 = proof_from(&b, &keys[0], 1_800_000_200);

    // Duplicate identity.
    assert_eq!(
        assemble_possession_bundle(&b, &[p0.clone(), p0.clone()]).unwrap_err(),
        AssembleError::DuplicateIdentity { oracle_id: p0.oracle_id.clone() }
    );

    // Identity the artifact does not register.
    let mut foreign = p0.clone();
    foreign.oracle_id = PublicKey::from_private_key(&key(99)).to_hex();
    assert_eq!(
        assemble_possession_bundle(&b, &[foreign.clone()]).unwrap_err(),
        AssembleError::UnknownIdentity { oracle_id: foreign.oracle_id }
    );

    // Signed over a different artifact: a re-cut bundle (new activated_at)
    // has a different registration commitment.
    let (refreshed, _) = bundle_at("cut-3096", 1_800_000_500, 1_800_001_000);
    let expected = bundle_registration_commitment(&refreshed).unwrap();
    let found = bundle_registration_commitment(&b).unwrap();
    assert_ne!(expected, found);
    // Re-submitting the same registration set later keeps the commitment, so
    // proofs collected before a resubmission stay valid.
    let (resubmitted, _) = bundle("cut-3096", 1_800_001_000);
    assert_eq!(bundle_registration_commitment(&resubmitted).unwrap(), found);
    assert_eq!(
        assemble_possession_bundle(&refreshed, &[p0.clone()]).unwrap_err(),
        AssembleError::WrongArtifact { oracle_id: p0.oracle_id.clone(), expected, found }
    );
}

#[test]
fn producer_proof_round_trip_and_refusals() {
    let (b, _) = bundle("cut-3096", 1_800_000_100);
    let producer = key(21);
    let signer = EnvKeySigner::from_private_key(producer.clone(), "test-only-producer-key");
    let proof = sign_producer_possession_proof(&b, &signer, 1_800_000_300).expect("sign");
    let hex = PublicKey::from_private_key(&producer).to_hex().to_ascii_lowercase();
    assert_eq!(proof.producer_pubkey, hex);
    assert_eq!(proof.registration_commitment, bundle_registration_commitment(&b).unwrap());
    assert_eq!(verify_producer_possession_proof(&b, &proof, &[]), Ok(()));
    assert_eq!(verify_producer_possession_proof(&b, &proof, &[hex.to_ascii_uppercase()]), Ok(()));

    assert!(matches!(
        verify_producer_possession_proof(&b, &proof, &["deadbeef".into()]),
        Err(ProducerProofError::UnexpectedProducer { .. })
    ));
    let (other, _) = bundle("cut-other", 1_800_000_100);
    assert!(matches!(
        verify_producer_possession_proof(&other, &proof, &[]),
        Err(ProducerProofError::WrongCeremony { .. })
    ));
    let (refreshed, _) = bundle_at("cut-3096", 1_800_000_500, 1_800_001_000);
    assert!(matches!(
        verify_producer_possession_proof(&refreshed, &proof, &[]),
        Err(ProducerProofError::WrongArtifact { .. })
    ));
    let mut tampered = proof.clone();
    tampered.timestamp += 1;
    assert!(matches!(
        verify_producer_possession_proof(&b, &tampered, &[]),
        Err(ProducerProofError::BadSignature { .. })
    ));
    let mut bad_hex = proof;
    bad_hex.signature = "zz".into();
    assert!(matches!(
        verify_producer_possession_proof(&b, &bad_hex, &[]),
        Err(ProducerProofError::BadSignature { .. })
    ));
}

fn entry(identity: &str, serial: &str, slot: &str, cka: &str, pk: &str) -> CustodyEntry {
    CustodyEntry {
        identity: identity.into(),
        role: "producer".into(),
        token_serial: serial.into(),
        piv_slot: slot.into(),
        public_label: "Public key for PIV Authentication".into(),
        cka_id: cka.into(),
        host: "test".into(),
        public_key: pk.into(),
    }
}

fn observed(serial: &str, cka: Option<&str>, label: &str, pk: &str) -> ObservedKey {
    ObservedKey {
        token_serial: serial.into(),
        token_label: format!("YubiKey PIV #{serial}"),
        object_label: label.into(),
        cka_id: cka.map(str::to_string),
        public_key: pk.into(),
    }
}

#[test]
fn custody_manifest_validation() {
    let good = PublicKey::from_private_key(&key(31)).to_hex();
    let ok = CustodyManifest { version: 1, entries: vec![entry("a", "1", "9A", "01", &good), entry("b", "1", "9C", "02", "")] };
    assert_eq!(ok.validate(), Ok(()));
    let dup = CustodyManifest { version: 1, entries: vec![entry("a", "1", "9A", "01", ""), entry("a", "2", "9A", "01", "")] };
    assert_eq!(dup.validate(), Err(ManifestError::DuplicateIdentity("a".into())));
    let bad_hex = CustodyManifest { version: 1, entries: vec![entry("a", "1", "9A", "zz", "")] };
    assert!(matches!(bad_hex.validate(), Err(ManifestError::BadHex { field: "cka_id", .. })));
    let short_key = CustodyManifest { version: 1, entries: vec![entry("a", "1", "9A", "01", "abcd")] };
    assert!(matches!(short_key.validate(), Err(ManifestError::BadHex { field: "public_key", .. })));
    let v2 = CustodyManifest { version: 2, entries: vec![] };
    assert_eq!(v2.validate(), Err(ManifestError::UnsupportedVersion(2)));
}

#[test]
fn custody_check_finds_matches_mismatches_absences_unfilled_and_unexpected() {
    let a = PublicKey::from_private_key(&key(31)).to_hex().to_ascii_lowercase();
    let b = PublicKey::from_private_key(&key(32)).to_hex().to_ascii_lowercase();
    let c = PublicKey::from_private_key(&key(33)).to_hex().to_ascii_lowercase();
    let manifest = CustodyManifest {
        version: 1,
        entries: vec![
            entry("match", "100", "9A", "01", &a),
            entry("mismatch", "100", "9C", "02", &c),
            entry("token-absent", "999", "9A", "01", &a),
            entry("object-absent", "100", "9D", "03", &a),
            entry("unfilled", "200", "9A", "01", ""),
            // No CKA_ID recorded: matched by label.
            CustodyEntry { cka_id: String::new(), ..entry("by-label", "200", "9C", "", &b) },
        ],
    };
    let seen = vec![
        observed("100", Some("01"), "Public key for PIV Authentication", &a),
        observed("100", Some("02"), "Public key for Digital Signature", &b),
        observed("200", Some("01"), "Public key for PIV Authentication", &c),
        observed("200", None, "Public key for PIV Authentication", &b),
        observed("300", Some("01"), "Public key for PIV Authentication", &c),
    ];
    let report = CustodyReport::check(&manifest, &seen);
    assert!(!report.is_consistent());
    assert_eq!(report.matches(), 2, "{:?}", report.findings);
    assert_eq!(report.unfilled(), 1);
    assert!(report.findings.contains(&CustodyFinding::Match { identity: "match".into() }));
    assert!(report.findings.contains(&CustodyFinding::Match { identity: "by-label".into() }));
    assert!(report.findings.contains(&CustodyFinding::Mismatch { identity: "mismatch".into(), expected: c.clone(), found: b.clone() }));
    assert!(report.findings.contains(&CustodyFinding::TokenAbsent { identity: "token-absent".into(), token_serial: "999".into() }));
    assert!(report.findings.iter().any(|f| matches!(f, CustodyFinding::ObjectAbsent { identity, .. } if identity == "object-absent")));
    assert!(report.findings.contains(&CustodyFinding::Unfilled { identity: "unfilled".into() }));
    assert!(report.findings.iter().any(|f| matches!(f, CustodyFinding::Unexpected { token_serial, .. } if token_serial == "300")));

    // One wrong slot alone: inconsistent; corrected: consistent.
    let wrong = CustodyManifest { version: 1, entries: vec![entry("x", "100", "9C", "02", &a)] };
    assert!(!CustodyReport::check(&wrong, &seen).is_consistent());
    let right = CustodyManifest { version: 1, entries: vec![entry("x", "100", "9A", "01", &a)] };
    assert!(CustodyReport::check(&right, &seen).is_consistent());
}

/// The live half of `custody --check`: every ed25519 public key on every
/// token the module presents, read WITHOUT a PIN. Runs against a throwaway
/// SoftHSM token (`SOFTHSM2_CONF` points at a temp dir, so real volumes are
/// unreachable by construction).
#[cfg(feature = "hsm")]
#[test]
fn enumerate_public_keys_reads_a_softhsm_token_without_a_pin() {
    use cryptoki::context::{CInitializeArgs, Pkcs11};
    use cryptoki::mechanism::Mechanism;
    use cryptoki::object::Attribute;
    use cryptoki::session::UserType;
    use cryptoki::types::AuthPin;
    use wrfcoin_shared_protocol::crypto::signer::enumerate_public_keys;

    let module = ["/usr/lib/softhsm/libsofthsm2.so", "/usr/lib/x86_64-linux-gnu/softhsm/libsofthsm2.so"]
        .iter()
        .map(std::path::PathBuf::from)
        .find(|p| p.exists());
    let Some(module) = module else {
        eprintln!("SoftHSM not installed; skipping the live enumeration test");
        return;
    };
    let dir = std::env::temp_dir().join(format!("wrfcoin-custody-3096-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let tokens = dir.join("tokens");
    std::fs::create_dir_all(&tokens).unwrap();
    let conf = dir.join("softhsm2.conf");
    std::fs::write(&conf, format!("directories.tokendir = {}\nobjectstore.backend = file\nlog.level = ERROR\n", tokens.display())).unwrap();
    std::env::set_var("SOFTHSM2_CONF", &conf);

    const TOKEN_LABEL: &str = "wrfcoin-custody-3096";
    const KEY_LABEL: &str = "custody-oracle-key";
    let ctx = Pkcs11::new(&module).expect("load SoftHSM");
    ctx.initialize(CInitializeArgs::OsThreads).expect("C_Initialize");
    let free_slot = *ctx.get_all_slots().unwrap().first().expect("a free slot");
    let so_pin = AuthPin::new("0102030405".into());
    ctx.init_token(free_slot, &so_pin, TOKEN_LABEL).expect("C_InitToken");
    let slot = ctx.get_slots_with_token().unwrap().into_iter()
        .find(|s| ctx.get_token_info(*s).map(|i| i.label().trim() == TOKEN_LABEL).unwrap_or(false))
        .expect("token by label");
    let session = ctx.open_rw_session(slot).unwrap();
    session.login(UserType::So, Some(&so_pin)).unwrap();
    session.init_pin(&AuthPin::new("1234".into())).unwrap();
    session.logout().unwrap();
    session.login(UserType::User, Some(&AuthPin::new("1234".into()))).unwrap();
    let ed25519_oid_der: Vec<u8> = vec![0x06, 0x03, 0x2b, 0x65, 0x70];
    let (public_handle, _private) = session.generate_key_pair(
        &Mechanism::EccEdwardsKeyPairGen,
        &[Attribute::Token(true), Attribute::Label(KEY_LABEL.as_bytes().to_vec()), Attribute::Id(vec![0x03]), Attribute::EcParams(ed25519_oid_der), Attribute::Verify(true)],
        &[Attribute::Token(true), Attribute::Label(KEY_LABEL.as_bytes().to_vec()), Attribute::Id(vec![0x03]), Attribute::Sensitive(true), Attribute::Extractable(false), Attribute::Sign(true)],
    ).expect("keypair");
    let _ = public_handle;
    session.logout().unwrap();
    drop(session);
    let serial = ctx.get_token_info(slot).unwrap().serial_number().trim().to_string();
    drop(ctx);

    // No PIN anywhere from here on.
    let keys = enumerate_public_keys(&module).expect("enumerate without login");
    let ours: Vec<_> = keys.iter().filter(|k| k.token_label == TOKEN_LABEL).collect();
    assert_eq!(ours.len(), 1, "{keys:?}");
    assert_eq!(ours[0].object_label, KEY_LABEL);
    assert_eq!(ours[0].cka_id.as_deref(), Some("03"));
    assert_eq!(ours[0].piv_slot.as_deref(), Some("9D (Key Management)"));
    assert_eq!(ours[0].public_key.len(), 64);
    assert_eq!(ours[0].token_serial, serial);

    // And the manifest check closes the loop: right key matches, wrong slot does not.
    let observed: Vec<ObservedKey> = keys.iter().map(|k| ObservedKey {
        token_serial: k.token_serial.clone(), token_label: k.token_label.clone(), object_label: k.object_label.clone(), cka_id: k.cka_id.clone(), public_key: k.public_key.clone(),
    }).collect();
    let right = CustodyManifest { version: 1, entries: vec![CustodyEntry { identity: "oracle-1".into(), role: "oracle".into(), token_serial: serial.clone(), piv_slot: "9D".into(), public_label: KEY_LABEL.into(), cka_id: "03".into(), host: "ct102".into(), public_key: ours[0].public_key.clone() }] };
    assert!(CustodyReport::check(&right, &observed).is_consistent());
    let wrong_slot = CustodyManifest { version: 1, entries: vec![CustodyEntry { cka_id: "01".into(), piv_slot: "9A".into(), ..right.entries[0].clone() }] };
    assert!(!CustodyReport::check(&wrong_slot, &observed).is_consistent());
    let _ = std::fs::remove_dir_all(&dir);
}
