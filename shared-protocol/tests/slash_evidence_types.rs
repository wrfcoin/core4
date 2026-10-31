use wrfcoin_shared_protocol::blockchain::transaction::TransactionType;
use wrfcoin_shared_protocol::crypto::domain_registry::SIGNED_OBJECT_REGISTRY;

#[path = "common/slash_evidence_vectors.rs"]
mod vectors;
use wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity;
use wrfcoin_shared_protocol::security::slash_evidence::{
    EvidenceError, MAX_REPORT_ATTESTATIONS, SlashEvidenceReport, SlashEvidenceReportPayload,
};

#[test]
fn appended_evidence_transaction_discriminant_is_36() {
    let encoded = 36u32.to_le_bytes();
    let parsed = bincode::deserialize::<TransactionType>(&encoded);
    assert!(
        parsed.is_ok(),
        "the appended evidence transaction is absent: {parsed:?}"
    );
    assert_eq!(parsed.unwrap().to_string(), "SlashEvidenceReport");
    assert_eq!(
        bincode::serialize(&TransactionType::FeatureScheduleExtension).unwrap(),
        35u32.to_le_bytes()
    );
    // Node S2 appends 37 without moving the existing evidence allocation.
    assert_eq!(bincode::deserialize::<TransactionType>(&37u32.to_le_bytes()).unwrap(), TransactionType::NodeBondLifecycle);
    assert_eq!(bincode::deserialize::<TransactionType>(&38u32.to_le_bytes()).unwrap(), TransactionType::NodeServiceBatch);
    assert_eq!(bincode::deserialize::<TransactionType>(&39u32.to_le_bytes()).unwrap(), TransactionType::NodeFraudReport);
    assert_eq!(bincode::deserialize::<TransactionType>(&40u32.to_le_bytes()).unwrap(), TransactionType::SessionKeyAction);
    // Security-key wallets (`security_key_spend`) append 41.
    assert_eq!(bincode::deserialize::<TransactionType>(&41u32.to_le_bytes()).unwrap(), TransactionType::SecurityKeyAction);
    assert!(bincode::deserialize::<TransactionType>(&42u32.to_le_bytes()).is_err());
}

#[test]
fn evidence_report_has_its_own_registered_domain() {
    let row = SIGNED_OBJECT_REGISTRY
        .iter()
        .find(|row| row.kind.as_str() == "SlashEvidenceReport");
    assert!(
        row.is_some(),
        "new evidence signed object is absent from the inventory"
    );
    assert_eq!(row.unwrap().tag, b"wrfcoin.slash_evidence_report.v1");
}

#[test]
fn staged_evidence_is_refused_without_state_or_mempool_effects() {
    use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
    use wrfcoin_shared_protocol::blockchain::transaction::{
        Transaction, TransactionInput, TransactionOutput,
    };
    use wrfcoin_shared_protocol::config::settings::Config;
    use wrfcoin_shared_protocol::crypto::hash::Hash;
    use wrfcoin_shared_protocol::mempool::pool::Mempool;
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: Hash::default(),
            vout: 0,
            signature: None,
            public_key: None,
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            amount: 1,
            address: "wrf-synthetic-evidence".into(),
            script: "OP_TRUE".into(),
            nft_data: None,
        }],
    );
    tx.tx_type = TransactionType::SlashEvidenceReport;
    tx.data = Some(serde_json::to_string(&vectors::payload()).unwrap());
    tx.set_id();
    let mut pool = Mempool::new(&Config::default());
    assert!(
        pool.add_transaction(tx.clone())
            .unwrap_err()
            .to_string()
            .contains("SP_SLASH_EVIDENCE_NOT_SUPPORTED")
    );
    assert_eq!(pool.size(), 0);
    let mut state = State::new();
    let before = state.state_root_material();
    assert!(
        state
            .apply_transition(StateTransition::ProcessGovernanceTransaction {
                tx,
                current_block: 1
            })
            .unwrap_err()
            .to_string()
            .contains("SP_SLASH_EVIDENCE_NOT_SUPPORTED")
    );
    assert_eq!(state.state_root_material(), before);
}

#[test]
fn canonical_golden_is_recomputed_and_exercised() {
    if std::env::var("WRFCOIN_REGEN_SLASH_EVIDENCE_VECTOR").as_deref() == Ok("1") {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/golden/slash_evidence_report_v1.jsonc");
        std::fs::write(path, vectors::fixture_text()).unwrap();
    }
    vectors::assert_pinned_vector();
}

#[test]
fn every_report_field_is_bound_and_rejected_after_tampering() {
    let mut original = vectors::payload();
    original
        .sign_for(&vectors::reporter(), &vectors::identity())
        .unwrap();
    original.verify_for(&vectors::identity()).unwrap();
    let changes: [fn(&mut SlashEvidenceReportPayload); 12] = [
        |p| p.version += 1,
        |p| p.commit_id = "11".repeat(32),
        |p| p.context_hash = "12".repeat(32),
        |p| p.signed_commit.timestamp += 1,
        |p| p.signed_commit.auth_signature = Some("13".repeat(64)),
        |p| p.reveal.data["temperature_c"] = serde_json::json!(21.0),
        |p| p.reveal.nonce.push('x'),
        |p| {
            p.attestations.push(serde_json::from_value(serde_json::json!({"commit_id":p.commit_id,"oracle_id":"14".repeat(32),"approved":false,"signature":"15".repeat(64),"confidence":0.0,"timestamp":0,"nonce":null})).unwrap())
        },
        |p| p.reporter_pubkey = "16".repeat(32),
        |p| p.reporter_nonce += 1,
        |p| p.fee += 1,
        |p| p.reporter_signature = None,
    ];
    for (index, change) in changes.into_iter().enumerate() {
        let mut changed = original.clone();
        change(&mut changed);
        assert!(
            changed.verify_for(&vectors::identity()).is_err(),
            "tampered field {index} verified"
        );
    }
    for identity in [
        ChainIdentity::new("778", "testnet"),
        ChainIdentity::new("777", "mainnet"),
    ] {
        assert!(original.verify_for(&identity).is_err());
    }
}

#[test]
fn signature_exclusion_and_evidence_dedup_are_independent_of_reporter() {
    let unsigned = vectors::payload();
    let bytes = unsigned.signing_preimage_for(&vectors::identity()).unwrap();
    let mut signed = unsigned.clone();
    signed
        .sign_for(&vectors::reporter(), &vectors::identity())
        .unwrap();
    assert_eq!(
        bytes,
        signed.signing_preimage_for(&vectors::identity()).unwrap()
    );
    signed.reporter_nonce += 100;
    signed.reporter_pubkey = "17".repeat(32);
    signed.fee += 2;
    assert_eq!(
        signed.evidence_id().unwrap(),
        unsigned.evidence_id().unwrap()
    );
}

#[test]
fn fee_signing_preserves_all_u128_bits() {
    let mut payload = vectors::payload();
    payload.fee = u128::MAX;
    payload
        .sign_for(&vectors::reporter(), &vectors::identity())
        .unwrap();
    payload.verify_for(&vectors::identity()).unwrap();
    let body = wrfcoin_shared_protocol::crypto::domain_signing::DomainSeparated::canonical_body(
        &SlashEvidenceReport::try_from(&payload).unwrap(),
    );
    assert_eq!(&body[body.len() - 16..], &u128::MAX.to_le_bytes());
    let round_trip: SlashEvidenceReportPayload =
        serde_json::from_slice(&serde_json::to_vec(&payload).unwrap()).unwrap();
    assert_eq!(round_trip.fee, u128::MAX);
}

#[test]
fn oversized_and_ambiguous_evidence_is_refused_before_signing() {
    let base = vectors::payload();
    let mut changed = base.clone();
    changed.reveal.nonce = "x".repeat(8 * 1024);
    assert_eq!(
        SlashEvidenceReport::try_from(&changed).unwrap_err(),
        EvidenceError::Bounds
    );
    changed = base.clone();
    changed.signed_commit.auth_pubkey = None;
    assert_eq!(
        SlashEvidenceReport::try_from(&changed).unwrap_err(),
        EvidenceError::Shape
    );
    changed = base.clone();
    changed.signed_commit.auth_signature = None;
    assert_eq!(
        SlashEvidenceReport::try_from(&changed).unwrap_err(),
        EvidenceError::Shape
    );
    changed = base.clone();
    changed.commit_id = changed.commit_id.to_uppercase();
    assert_eq!(
        SlashEvidenceReport::try_from(&changed).unwrap_err(),
        EvidenceError::Shape
    );
    let attestation = serde_json::from_value(serde_json::json!({"commit_id":base.commit_id,"oracle_id":"19".repeat(32),"approved":false,"signature":"20".repeat(64),"confidence":0.0,"timestamp":0,"nonce":null})).unwrap();
    changed = base.clone();
    changed.attestations = vec![attestation; MAX_REPORT_ATTESTATIONS + 1];
    assert_eq!(
        SlashEvidenceReport::try_from(&changed).unwrap_err(),
        EvidenceError::Bounds
    );
    changed.attestations.truncate(2);
    assert_eq!(
        SlashEvidenceReport::try_from(&changed).unwrap_err(),
        EvidenceError::Shape
    );
    changed.attestations.truncate(1);
    changed.attestations[0].confidence = Some(f64::NAN);
    assert_eq!(
        SlashEvidenceReport::try_from(&changed).unwrap_err(),
        EvidenceError::Shape
    );
}

#[test]
fn reporter_cannot_authenticate_an_unrelated_commit_or_reveal() {
    let changes: [fn(&mut SlashEvidenceReportPayload); 9] = [
        |p| p.commit_id = "11".repeat(32),
        |p| p.signed_commit.commit_id = "11".repeat(32),
        |p| p.reveal.commit_id = "11".repeat(32),
        |p| p.reveal.data["temperature_c"] = serde_json::json!(21.0),
        |p| p.reveal.nonce.push('x'),
        |p| p.reveal.timestamp += 1,
        |p| p.signed_commit.timestamp += 1,
        |p| p.reveal.station_id.push('x'),
        |p| p.reveal.contribution_type.push('x'),
    ];
    for (index, change) in changes.into_iter().enumerate() {
        let mut report = vectors::payload();
        change(&mut report);
        report
            .sign_for(&vectors::reporter(), &vectors::identity())
            .unwrap();
        assert!(
            report.verify_for(&vectors::identity()).is_err(),
            "reporter substituted evidence field{index}"
        );
    }
}

#[test]
fn reveal_json_numbers_cannot_carry_nonfinite_floats() {
    for nonfinite in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(serde_json::Number::from_f64(nonfinite).is_none());
    }
    let raw = serde_json::to_string(&vectors::payload()).unwrap();
    for token in ["NaN", "Infinity", "-Infinity", "1e9999"] {
        let changed = raw.replacen("20.0", token, 1);
        assert_ne!(changed, raw);
        assert!(serde_json::from_str::<SlashEvidenceReportPayload>(&changed).is_err());
    }
    let mut literal_null = vectors::payload();
    literal_null.reveal.data["temperature_c"] = serde_json::Value::Null;
    // A literal null is its own JSON value, never an in-memory nonfinite Number.
    // The commitment verifier refuses substituting it for the original number.
    literal_null
        .sign_for(&vectors::reporter(), &vectors::identity())
        .unwrap();
    assert!(literal_null.verify_for(&vectors::identity()).is_err());
}

#[test]
fn reporter_cannot_replace_original_commit_or_attester_signatures() {
    use wrfcoin_shared_protocol::blockchain::transaction::OracleAttestationPayload;
    use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
    let mut report = vectors::payload();
    report.signed_commit.auth_signature = Some("00".repeat(64));
    report
        .sign_for(&vectors::reporter(), &vectors::identity())
        .unwrap();
    assert!(report.verify_for(&vectors::identity()).is_err());

    let attester = PrivateKey::from_bytes(&[11; 32]).unwrap();
    let mut annotation = OracleAttestationPayload {
        commit_id: vectors::payload().commit_id,
        oracle_id: PublicKey::from_private_key(&attester).to_hex(),
        approved: false,
        signature: None,
        confidence: Some(0.8),
        timestamp: 1_700_000_000,
        nonce: Some("synthetic-attester-nonce".into()),
    };
    annotation.signature = Some(
        attester
            .sign(
                annotation
                    .canonical_signing_message_with_domain("777", "testnet")
                    .as_bytes(),
            )
            .to_hex(),
    );
    let mut good = vectors::payload();
    good.attestations = vec![annotation];
    good.sign_for(&vectors::reporter(), &vectors::identity())
        .unwrap();
    good.verify_for(&vectors::identity()).unwrap();
    let changes: [fn(&mut OracleAttestationPayload); 6] = [
        |p| p.signature = Some("00".repeat(64)),
        |p| p.approved = !p.approved,
        |p| p.confidence = Some(0.9),
        |p| p.timestamp += 1,
        |p| p.nonce = Some("other-nonce".into()),
        |p| p.commit_id = "11".repeat(32),
    ];
    for (index, change) in changes.into_iter().enumerate() {
        let mut report = good.clone();
        change(&mut report.attestations[0]);
        report
            .sign_for(&vectors::reporter(), &vectors::identity())
            .unwrap();
        assert!(
            report.verify_for(&vectors::identity()).is_err(),
            "reporter forged provenance field{index}"
        );
    }
}
