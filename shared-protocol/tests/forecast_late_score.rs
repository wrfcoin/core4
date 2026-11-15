//! Forecast late score (`ConsensusFeature::ForecastLateScore`) through the
//! real `State` apply path: the activation boundary (dormant = nothing
//! written, root material byte-identical, both transitions refused), records
//! at reveal, every score check, bucket close and pay, the pool ledger,
//! rejection atomicity and the snapshot round trip.
//!
//! One test function: the feature schedule is process-wide.

use wrfcoin_shared_protocol::blockchain::state::{
    ContributionRewardRecord, State, StateError, StateTransition,
};
use wrfcoin_shared_protocol::blockchain::state_snapshot::export_state_entries;
use wrfcoin_shared_protocol::blockchain::transaction::{
    CommitPayload, ForecastArtifactManifest, OracleAttestationPayload, RevealPayload, Transaction,
    TransactionInput, TransactionOutput, TransactionType,
};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::consensus::forecast_late_score::{
    BUCKET_CAP, ForecastLateScoreError, ForecastScorePayload, LateScoreVariable, ObservationAttestation,
    ObservationSignature, POOL_EPOCH_BLOCKS, SCORE_OPENS_AFTER_SECS, SCORE_WINDOW_SECS,
    TRACK_RECORD_LEN, TrackRecord, bucket_closes_at, bucket_id, forecaster_key,
    forecast_score_message, score_outpoint, track_record_key,
};
use wrfcoin_shared_protocol::consensus::proof_of_publishing::{
    EmissionRole, FORECAST_REWARD_REASON_PREFIX,
};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ChainIdentity, ConsensusFeature, FORECAST_LATE_SCORE_KEY_BINDING_LEAD_BLOCKS, feature_active,
    forecast_late_score_active, forecast_late_score_active_with,
    forecast_late_score_schedule_allowed, refresh_feature_schedule_extensions,
};
use wrfcoin_shared_protocol::crypto::hash::hash_string;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::mempool::pool::Mempool;
use wrfcoin_shared_protocol::security::key_registry::{SigningKeyRegistry, SigningRole};
use wrfcoin_shared_protocol::station_registry::{
    ContributionProvenanceClass, DataQualityTier, StationLocation, WeatherStation,
};

/// `forecaster_key_binding` activates here.
const BOUND: u64 = 10;
/// `forecast_late_score` is SCHEDULED here, too early to take effect …
const SCHEDULED: u64 = 100;
/// … and is in force from here: the key binding has been active 2,000 blocks.
const ACTIVE: u64 = BOUND + FORECAST_LATE_SCORE_KEY_BINDING_LEAD_BLOCKS;
/// A whole hour.
const T0: i64 = 1_762_646_400;
const HOUR: i64 = 3_600;
const STATION: &str = "st-late-1";
const ROOT_TAG: &[u8] = b"wrfcoin_forecast_late_score_v1";

fn key(seed: u8) -> PrivateKey {
    PrivateKey::from_bytes(&[seed; 32]).unwrap()
}
fn address_of(k: &PrivateKey) -> Address {
    Address::from_public_key(&PublicKey::from_private_key(k))
}
fn oracle_id(k: &PrivateKey) -> String {
    PublicKey::from_private_key(k).to_hex()
}

fn register_station(state: &mut State, station_id: &str, owner: Address) {
    state
        .register_weather_station(WeatherStation {
            station_id: station_id.to_string(),
            owner_address: owner.to_string(),
            location: StationLocation {
                latitude: 46.7867,
                longitude: -92.1005,
                elevation_meters: Some(215.0),
            },
            hardware_model: "test-station".to_string(),
            calibration_date: None,
            is_active: true,
            reputation_score: 50,
            data_quality_tier: DataQualityTier::Gold,
            provenance_class: ContributionProvenanceClass::Real,
            ring_public_key: None,
        })
        .expect("register station");
}

/// A signed forecast commit and its reveal.
fn forecast(
    signer: &PrivateKey,
    station_id: &str,
    data: serde_json::Value,
    lead_hours: u32,
    tag: &str,
) -> (CommitPayload, RevealPayload) {
    let nonce = format!("nonce-{tag}");
    let commit_id = Transaction::compute_commit_id(&data, &nonce, station_id, T0, "forecast");
    let mut commit = CommitPayload {
        commit_id: commit_id.clone(),
        station_id: station_id.to_string(),
        timestamp: T0,
        contribution_type: "forecast".to_string(),
        auth_pubkey: None,
        auth_signature: None,
    };
    commit.sign_authorization(signer);
    let reveal = RevealPayload {
        commit_id,
        station_id: station_id.to_string(),
        timestamp: T0,
        contribution_type: "forecast".to_string(),
        data,
        nonce,
        data_cid: None,
        data_hash: None,
        storage_provider: None,
        provenance: None,
        manifest: Some(ForecastArtifactManifest {
            schema_version: 1,
            model_id: "test-model".to_string(),
            input_hash: "11".repeat(32),
            output_hash: "22".repeat(32),
            produced_at: T0,
            region: station_id.to_string(),
            horizon_hours: lead_hours,
            input_variables: vec!["temp_c".to_string()],
            output_variables: vec!["temp_c".to_string()],
        }),
        verification_receipt: None,
        compute_receipt: None,
        quality_tier: None,
    };
    (commit, reveal)
}

fn commit_at(state: &mut State, commit: &CommitPayload, height: u64) {
    state
        .apply_transition(StateTransition::RegisterCommit {
            payload: commit.clone(),
            block_height: height,
        })
        .expect("commit applies");
}
fn reveal_at(state: &mut State, reveal: &RevealPayload, height: u64, timestamp: i64) {
    state
        .apply_transition(StateTransition::ApplyReveal {
            payload: reveal.clone(),
            block_height: height,
            block_timestamp: timestamp,
        })
        .expect("reveal applies");
}

/// A station reading on chain with `signers` approving attestations.
fn attested_reading(
    state: &mut State,
    owner: &PrivateKey,
    station_id: &str,
    tag: &str,
    signers: &[&PrivateKey],
    height: u64,
) -> String {
    let data = serde_json::json!({
        "quality": 1.0, "volume": 1.0, "uniqueness": 1.0,
        "temperature_c": 20.0, "humidity_percent": 60.0, "pressure_hpa": 1014.0,
        "wind_speed_ms": 2.8, "precipitation_mm": 0.0, "dewpoint_c": 12.0
    });
    let nonce = format!("nonce-{tag}");
    let commit_id = Transaction::compute_commit_id(&data, &nonce, station_id, T0, "contribution");
    let mut commit = CommitPayload {
        commit_id: commit_id.clone(),
        station_id: station_id.to_string(),
        timestamp: T0,
        contribution_type: "contribution".to_string(),
        auth_pubkey: None,
        auth_signature: None,
    };
    commit.sign_authorization(owner);
    commit_at(state, &commit, height);
    let reveal = RevealPayload {
        commit_id: commit_id.clone(),
        station_id: station_id.to_string(),
        timestamp: T0,
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
    reveal_at(state, &reveal, height, T0);
    for (index, signer) in signers.iter().enumerate() {
        let mut payload = OracleAttestationPayload {
            commit_id: commit_id.clone(),
            oracle_id: oracle_id(signer),
            approved: true,
            signature: None,
            confidence: Some(1.0),
            timestamp: T0,
            nonce: Some(format!("att-{tag}-{index}")),
        };
        payload.signature = Some(
            signer
                .sign(payload.signing_message().as_bytes())
                .to_string(),
        );
        state
            .apply_transition(StateTransition::RecordOracleAttestation {
                payload,
                current_block: height,
            })
            .expect("attestation applies");
    }
    commit_id
}

/// `signers` approve `commit_id` (any applied reveal), as `attested_reading` does.
fn attest(state: &mut State, commit_id: &str, signers: &[&PrivateKey], tag: &str, height: u64) {
    for (index, signer) in signers.iter().enumerate() {
        let mut payload = OracleAttestationPayload {
            commit_id: commit_id.to_string(),
            oracle_id: oracle_id(signer),
            approved: true,
            signature: None,
            confidence: Some(1.0),
            timestamp: T0,
            nonce: Some(format!("att-{tag}-{index}")),
        };
        payload.signature = Some(
            signer
                .sign(payload.signing_message().as_bytes())
                .to_string(),
        );
        state
            .apply_transition(StateTransition::RecordOracleAttestation {
                payload,
                current_block: height,
            })
            .expect("attestation applies");
    }
}

/// The observation for the forecasts committed at `commit_height` /
/// `commit_time` (6 h lead): truth 20.00, reference 3.00, mean 19.00, last 21.00.
fn attestation(
    commit_height: u64,
    commit_time: i64,
    truth: &str,
    window: &str,
) -> ObservationAttestation {
    ObservationAttestation {
        station_id: STATION.to_string(),
        variable: LateScoreVariable::TemperatureC,
        lead_hours: 6,
        commit_height,
        commit_time,
        target: commit_time + 6 * HOUR,
        truth: 2_000,
        truth_commit_id: truth.to_string(),
        reference: 300,
        mean24: 1_900,
        last: 2_100,
        window_hash: "5a".repeat(32),
        window_commit_ids: vec![window.to_string()],
    }
}

fn score_payload(
    attestation: ObservationAttestation,
    signers: &[&PrivateKey],
    forecasts: &[&str],
) -> ForecastScorePayload {
    let named: Vec<(String, &PrivateKey)> = signers
        .iter()
        .map(|signer| (oracle_id(signer), *signer))
        .collect();
    score_payload_as(attestation, &named, forecasts)
}

/// A score whose signers are (oracle identity, the key that signs for it):
/// after a key rotation the two differ.
fn score_payload_as(
    attestation: ObservationAttestation,
    signers: &[(String, &PrivateKey)],
    forecasts: &[&str],
) -> ForecastScorePayload {
    let mut forecast_commit_ids: Vec<String> =
        forecasts.iter().map(|id| (*id).to_string()).collect();
    forecast_commit_ids.sort();
    let identity = ChainIdentity::active();
    let message = forecast_score_message(
        &identity.chain_id,
        &identity.network_id,
        &attestation,
        &forecast_commit_ids,
    );
    let mut signatures: Vec<ObservationSignature> = signers
        .iter()
        .map(|(oracle, signer)| ObservationSignature {
            oracle: oracle.clone(),
            ed25519: signer.sign(&message).to_hex(),
        })
        .collect();
    signatures.sort_by(|a, b| a.oracle.cmp(&b.oracle));
    ForecastScorePayload {
        attestation,
        signatures,
        forecast_commit_ids,
    }
}

fn score_at(
    state: &mut State,
    payload: &ForecastScorePayload,
    height: u64,
    timestamp: i64,
) -> Result<(), StateError> {
    state.apply_transition(StateTransition::ApplyForecastScore {
        payload: payload.clone(),
        block_height: height,
        block_timestamp: timestamp,
    })
}

/// A bucket close as a block applies it: with the emission ledger open at `height`.
fn close_at(state: &mut State, height: u64, timestamp: i64) -> Result<(), StateError> {
    state.apply_transitions_batch_at_height(
        height,
        [StateTransition::CloseForecastBuckets {
            block_height: height,
            block_timestamp: timestamp,
        }],
    )
}

fn refused(result: Result<(), StateError>, needle: &str) {
    let err = format!("{:?}", result.expect_err("must be refused"));
    assert!(err.contains(needle), "expected `{needle}`, got {err}");
}

fn has_root_section(state: &State) -> bool {
    state
        .state_root_material()
        .windows(ROOT_TAG.len())
        .any(|window| window == ROOT_TAG)
}

/// Give each forecaster 19 earlier positive scores for (STATION,
/// temperature, 6 h), so the score under test is the 20th and decides the
/// payment. The rule itself (window, eligibility, sliding) is covered in the
/// module tests and over real blocks in `forecast_late_score_parity`.
fn seed_track_records(state: &mut State, forecasters: &[&PrivateKey], at: i64) {
    for who in forecasters {
        state.forecast_late_score.track_records.insert(
            track_record_key(&address_of(who), STATION, LateScoreVariable::TemperatureC, 6),
            TrackRecord {
                skills: vec![500_000; TRACK_RECORD_LEN - 1],
                last_scored_at: at,
            },
        );
    }
}

fn balance(state: &State, address: &Address) -> u128 {
    state
        .get_account(address)
        .map_or(0, |account| account.balance.into_inner())
}

#[test]
fn forecast_late_score_end_to_end() {
    // ---- The gate: the feature AND 2,000 blocks of key binding. ----
    for (late, bound, height, expect) in [
        (Some(100u64), Some(10u64), 2_009u64, false),
        (Some(100), Some(10), 2_010, true),
        (Some(3_000), Some(10), 2_999, false),
        (Some(3_000), Some(10), 3_000, true),
        (Some(100), None, 1_000_000, false),
        (None, Some(10), 1_000_000, false),
        (Some(0), Some(0), 1_999, false),
        (Some(0), Some(0), 2_000, true),
    ] {
        let active = |feature: ConsensusFeature, at: u64| match feature {
            ConsensusFeature::ForecastLateScore => late.is_some_and(|h| at >= h),
            ConsensusFeature::ForecasterKeyBinding => bound.is_some_and(|h| at >= h),
            _ => false,
        };
        assert_eq!(
            forecast_late_score_active_with(active, height),
            expect,
            "{late:?} {bound:?} {height}"
        );
    }
    assert!(!forecast_late_score_schedule_allowed(5_000, None));
    assert!(!forecast_late_score_schedule_allowed(2_009, Some(10)));
    assert!(forecast_late_score_schedule_allowed(2_010, Some(10)));
    assert!(!forecast_late_score_schedule_allowed(
        u64::MAX,
        Some(u64::MAX)
    ));
    assert!(!ConsensusFeature::ForecastLateScore.schedulable(None));
    assert!(!ConsensusFeature::ForecastLateScore.schedulable(Some(&"ab".repeat(32))));

    // Unscheduled: inert at every height.
    for height in [0, 1, ACTIVE, u64::MAX] {
        assert!(!forecast_late_score_active(height));
    }
    // Mempool admission refuses the type while it is not in force.
    // A relayed score: any relay label and timestamp; the input outpoint is
    // the one derived from the signed payload unless `outpoint` overrides it.
    let wrap = |payload: &ForecastScorePayload, relay_byte: u8, ts_shift: i64, outpoint: Option<&str>| {
        let relay = Address([relay_byte; 20]);
        let identity = ChainIdentity::active();
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: outpoint.map(hash_string).unwrap_or_else(|| {
                    score_outpoint(&identity.chain_id, &identity.network_id, payload)
                }),
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 0,
                address: relay.to_string(),
                script: "OP_TRUE".into(),
                nft_data: None,
            }],
        );
        tx.tx_type = TransactionType::ForecastScore;
        tx.sender_address = relay;
        tx.timestamp += chrono::Duration::seconds(ts_shift);
        tx.data = Some(serde_json::to_string(payload).unwrap());
        tx.set_id();
        tx
    };
    let relay_tx = |payload: &ForecastScorePayload| wrap(payload, 0x77, 0, None);
    let admission_probe = score_payload(
        attestation(5, T0, "t", "r"),
        &[&key(0x21), &key(0x22)],
        &["f"],
    );
    // Admission is given the node's signing-key registry: here the two
    // signers are registered oracles.
    let mut admission_keys = SigningKeyRegistry::default();
    for seed in [0x21, 0x22] {
        admission_keys
            .ensure_legacy_identity_key(&oracle_id(&key(seed)), SigningRole::Oracle, 0)
            .unwrap();
    }
    let admit_with = |tx: Transaction, tip: u64, keys: Option<&SigningKeyRegistry>| {
        Mempool::new(&Config::default())
            .add_transaction_with_signing_keys_at_height(tx, keys, Some(tip))
            .map(|_| ())
            .map_err(|e| e.to_string())
    };
    let admit = |tx: Transaction, tip: u64| admit_with(tx, tip, Some(&admission_keys));
    let err = admit(relay_tx(&admission_probe), 1_000_000).unwrap_err();
    assert!(err.contains("SP_FORECAST_LATE_SCORE_INACTIVE"), "{err}");

    refresh_feature_schedule_extensions(vec![
        ("forecaster_key_binding".to_string(), BOUND),
        ("forecast_late_score".to_string(), SCHEDULED),
    ]);
    assert!(feature_active(
        ConsensusFeature::ForecastLateScore,
        SCHEDULED
    ));
    assert!(
        !forecast_late_score_active(SCHEDULED),
        "scheduled too early: dormant until the key binding has run 2,000 blocks"
    );
    assert!(!forecast_late_score_active(ACTIVE - 1));
    assert!(forecast_late_score_active(ACTIVE));
    // Admission asks about the NEXT height: refused at tip ACTIVE - 2,
    // admitted at tip ACTIVE - 1; a tampered or malformed payload never is.
    let err = admit(relay_tx(&admission_probe), ACTIVE - 2).unwrap_err();
    assert!(err.contains("SP_FORECAST_LATE_SCORE_INACTIVE"), "{err}");
    admit(relay_tx(&admission_probe), ACTIVE - 1).expect("a well-formed, signed score is admitted");
    let mut forged = admission_probe.clone();
    forged.attestation.truth += 1;
    let err = admit(relay_tx(&forged), ACTIVE).unwrap_err();
    assert!(
        err.contains("SP_FORECAST_LATE_SCORE_AUTHORIZATION"),
        "{err}"
    );
    // The oracle signature covers the full score payload: the same signed
    // observation with a different forecast list is not admitted.
    for other_list in [vec!["g".to_string()], vec!["f".to_string(), "g".to_string()]] {
        let mut relisted = admission_probe.clone();
        relisted.forecast_commit_ids = other_list;
        let err = admit(relay_tx(&relisted), ACTIVE).unwrap_err();
        assert!(
            err.contains("SP_FORECAST_LATE_SCORE_AUTHORIZATION") && err.contains("does not verify"),
            "{err}"
        );
    }
    // One pending score per signed payload, however it is wrapped: a second
    // relay label, timestamp or outpoint does not make a second pool entry.
    {
        let mut pool = Mempool::new(&Config::default());
        let mut add = |tx: Transaction| {
            pool.add_transaction_with_signing_keys_at_height(tx, Some(&admission_keys), Some(ACTIVE))
                .map(|_| ())
                .map_err(|e| e.to_string())
        };
        add(wrap(&admission_probe, 0x77, 0, None)).expect("the first wrapper is admitted");
        let err = add(wrap(&admission_probe, 0x78, 5, None)).unwrap_err();
        assert!(err.contains("SP_FORECAST_LATE_SCORE_DUPLICATE"), "{err}");
        let err = add(wrap(&admission_probe, 0x79, 9, Some("another-outpoint"))).unwrap_err();
        assert!(err.contains("SP_FORECAST_LATE_SCORE_OUTPOINT"), "{err}");
    }
    // A score with more valid signatures replaces a pending one with fewer
    // (same signed payload); a forged, equal or smaller signer set does not.
    {
        let mut pool = Mempool::new(&Config::default());
        let add = |pool: &mut Mempool, tx: Transaction| {
            pool.add_transaction_with_signing_keys_at_height(tx, Some(&admission_keys), Some(ACTIVE))
                .map(|_| ())
                .map_err(|e| e.to_string())
        };
        let mut weak = admission_probe.clone();
        weak.signatures.truncate(1);
        let weak_tx = wrap(&weak, 0x71, 0, None);
        add(&mut pool, weak_tx.clone()).expect("a one-signature score is admitted");
        let mut forged = admission_probe.clone();
        forged.signatures[1].ed25519 = "00".repeat(64);
        let err = add(&mut pool, wrap(&forged, 0x72, 1, None)).unwrap_err();
        assert!(err.contains("SP_FORECAST_LATE_SCORE_AUTHORIZATION"), "{err}");
        assert!(pool.contains(&weak_tx.id.unwrap()), "a forged superset leaves the pending score");
        let strong_tx = wrap(&admission_probe, 0x73, 2, None);
        add(&mut pool, strong_tx.clone()).expect("two valid signatures replace one");
        assert_eq!(pool.size(), 1);
        assert!(!pool.contains(&weak_tx.id.unwrap()), "the weaker score is gone");
        assert!(pool.contains(&strong_tx.id.unwrap()));
        let err = add(&mut pool, wrap(&admission_probe, 0x74, 3, None)).unwrap_err();
        assert!(err.contains("SP_FORECAST_LATE_SCORE_DUPLICATE"), "equal set: {err}");
        let err = add(&mut pool, wrap(&weak, 0x75, 4, None)).unwrap_err();
        assert!(err.contains("SP_FORECAST_LATE_SCORE_DUPLICATE"), "smaller set: {err}");
        assert_eq!(pool.size(), 1);
    }
    // Fail closed without a registry, and for a signer that is not a
    // registered oracle identity, however valid its own signature: a key
    // anyone can mint must not buy pool space.
    let err = admit_with(relay_tx(&admission_probe), ACTIVE, None).unwrap_err();
    assert!(
        err.contains("SP_FORECAST_LATE_SCORE_AUTHORIZATION"),
        "{err}"
    );
    let self_made = score_payload(
        attestation(5, T0, "t", "r"),
        &[&key(0x21), &key(0x29)],
        &["f"],
    );
    let err = admit(relay_tx(&self_made), ACTIVE).unwrap_err();
    assert!(err.contains("not an authorized oracle"), "{err}");
    let err = admit_with(
        relay_tx(&admission_probe),
        ACTIVE,
        Some(&SigningKeyRegistry::default()),
    )
    .unwrap_err();
    assert!(err.contains("not an authorized oracle"), "{err}");
    let mut malformed = relay_tx(&admission_probe);
    malformed.data = Some("{\"attestation\":1}".to_string());
    let err = admit(malformed, ACTIVE).unwrap_err();
    assert!(err.contains("SP_FORECAST_LATE_SCORE_PAYLOAD"), "{err}");

    let owner = key(0x01);
    let (alice, bob, carol, dave, erin) = (key(0x11), key(0x12), key(0x13), key(0x14), key(0x15));
    let (oracle_a, oracle_b, oracle_c, stranger) = (key(0x21), key(0x22), key(0x23), key(0x29));
    let mut state = State::new();
    register_station(&mut state, STATION, address_of(&owner));
    register_station(&mut state, "st-late-2", address_of(&owner));
    state
        .apply_transition(StateTransition::SetOracleConfig {
            oracle_set: vec![
                oracle_id(&oracle_a),
                oracle_id(&oracle_b),
                oracle_id(&oracle_c),
            ],
            oracle_quorum: 2,
        })
        .expect("oracle config");
    // Window readings: landed before any forecast commit.
    let reading = attested_reading(
        &mut state,
        &owner,
        STATION,
        "r1",
        &[&oracle_a, &oracle_b],
        ACTIVE - 6,
    );
    let weak_reading =
        attested_reading(&mut state, &owner, STATION, "r2", &[&oracle_a], ACTIVE - 6);
    let other_station_reading = attested_reading(
        &mut state,
        &owner,
        "st-late-2",
        "r3",
        &[&oracle_a, &oracle_b],
        ACTIVE - 6,
    );

    // ---- Dormant: below the effective height. ----
    let (commit, reveal) = forecast(
        &alice,
        STATION,
        serde_json::json!({"temp_c_centi": 2_040}),
        6,
        "dormant",
    );
    commit_at(&mut state, &commit, ACTIVE - 3);
    reveal_at(&mut state, &reveal, ACTIVE - 3, T0);
    assert!(
        state.forecast_late_score.is_empty(),
        "nothing is recorded before the feature is in force"
    );
    assert!(!has_root_section(&state));
    let before = state.state_root_material();
    let dormant_score = score_payload(
        attestation(ACTIVE - 3, T0, &reading, &reading),
        &[&oracle_a, &oracle_b],
        &[&commit.commit_id],
    );
    assert!(matches!(
        score_at(
            &mut state,
            &dormant_score,
            ACTIVE - 1,
            T0 + 6 * HOUR + SCORE_OPENS_AFTER_SECS
        ),
        Err(StateError::InvalidTransition)
    ));
    assert!(matches!(
        close_at(&mut state, ACTIVE - 1, T0 + 100 * HOUR),
        Err(StateError::InvalidTransition)
    ));
    assert!(state.forecast_late_score.is_empty());
    // The refused close opened the block's emission ledger, which is not root
    // material; everything hashed is byte-identical.
    assert_eq!(
        state.state_root_material(),
        before,
        "refused transitions leave root material byte-identical"
    );
    assert_eq!(
        export_state_entries(&state)
            .unwrap()
            .iter()
            .filter(|(k, _)| k.ends_with(b"forecast_late_score"))
            .count(),
        0
    );

    // ---- Active: records at reveal. ----
    // Block ACTIVE (time T0 + 100): Alice and Bob commit and reveal in it.
    // Block ACTIVE + 1 (time T0 + 200): Carol and Erin commit; they reveal in
    // the next block, so their commit time is the stamp the close left.
    // Block ACTIVE + 3 (time T0 + 400): Dave commits AFTER Alice's reveal,
    // 2 units from her value: a copy.
    let (t_a, t_c, t_d) = (T0 + 100, T0 + 200, T0 + 400);
    let commit_only = |state: &mut State,
                       signer: &PrivateKey,
                       value: serde_json::Value,
                       tag: &str,
                       height: u64| {
        let (commit, reveal) = forecast(signer, STATION, value, 6, tag);
        commit_at(state, &commit, height);
        reveal
    };
    let reveal = commit_only(
        &mut state,
        &alice,
        serde_json::json!({"temp_c_centi": 2_040}),
        "alice",
        ACTIVE,
    );
    let f_alice = reveal.commit_id.clone();
    reveal_at(&mut state, &reveal, ACTIVE, t_a);
    let reveal = commit_only(
        &mut state,
        &bob,
        serde_json::json!({"temp_c_centi": 2_150}),
        "bob",
        ACTIVE,
    );
    let f_bob = reveal.commit_id.clone();
    reveal_at(&mut state, &reveal, ACTIVE, t_a);
    close_at(&mut state, ACTIVE, t_a).expect("end of block");
    let carol_reveal = commit_only(
        &mut state,
        &carol,
        serde_json::json!({"temp_c_centi": 2_210}),
        "carol",
        ACTIVE + 1,
    );
    let erin_reveal = commit_only(
        &mut state,
        &erin,
        serde_json::json!({"temp_c_centi": 2_290}),
        "erin",
        ACTIVE + 1,
    );
    let (f_carol, f_erin) = (
        carol_reveal.commit_id.clone(),
        erin_reveal.commit_id.clone(),
    );
    assert_eq!(
        state.forecast_late_score.pending[&f_carol].commit_time,
        None
    );
    close_at(&mut state, ACTIVE + 1, t_c).expect("end of block");
    assert_eq!(
        state.forecast_late_score.pending[&f_carol].commit_time,
        Some(t_c),
        "the close stamps the commit block's time"
    );
    reveal_at(&mut state, &carol_reveal, ACTIVE + 2, t_c + 100);
    reveal_at(&mut state, &erin_reveal, ACTIVE + 2, t_c + 100);
    close_at(&mut state, ACTIVE + 2, t_c + 100).expect("end of block");
    let reveal = commit_only(
        &mut state,
        &dave,
        serde_json::json!({"temp_c_centi": 2_042}),
        "dave",
        ACTIVE + 3,
    );
    let f_dave = reveal.commit_id.clone();
    reveal_at(&mut state, &reveal, ACTIVE + 3, t_d);
    close_at(&mut state, ACTIVE + 3, t_d).expect("end of block");

    assert_eq!(state.forecast_late_score.records.len(), 5);
    seed_track_records(&mut state, &[&alice, &bob, &carol, &dave, &erin], t_d);
    assert!(
        state.forecast_late_score.pending.is_empty(),
        "a revealed commit leaves the pending map"
    );
    let alice_record = &state.forecast_late_score.records[&f_alice];
    assert_eq!(
        alice_record.forecaster,
        address_of(&alice),
        "the forecaster is the key that signed the commit"
    );
    assert_eq!((alice_record.value, alice_record.lead_hours), (2_040, 6));
    assert_eq!(
        (
            alice_record.commit_height,
            alice_record.commit_time,
            alice_record.target
        ),
        (ACTIVE, t_a, t_a + 6 * HOUR)
    );
    assert_eq!(
        (alice_record.reveal_height, alice_record.skill),
        (ACTIVE, None)
    );
    let carol_record = &state.forecast_late_score.records[&f_carol];
    assert_eq!(
        (
            carol_record.commit_height,
            carol_record.commit_time,
            carol_record.target,
            carol_record.reveal_height
        ),
        (ACTIVE + 1, t_c, t_c + 6 * HOUR, ACTIVE + 2),
        "the target comes from the COMMIT block's time, not the reveal's"
    );
    let hour = T0 + 6 * HOUR;
    let bucket = bucket_id(STATION, LateScoreVariable::TemperatureC, 6, hour).unwrap();
    assert_eq!(
        state.forecast_late_score.buckets[&bucket].commit_ids,
        vec![
            f_alice.clone(),
            f_bob.clone(),
            f_carol.clone(),
            f_erin.clone(),
            f_dave.clone()
        ]
    );
    assert!(has_root_section(&state));

    // Not recorded: a second forecast by the same wallet for the bucket, the
    // station owner's own forecast, a float, an out-of-range value, two
    // variables, an unsupported lead (3 h, and 1 h), an unregistered station.
    let records_before = state.forecast_late_score.records.len();
    let h = ACTIVE + 4;
    for (signer, value, tag) in [
        (
            &alice,
            serde_json::json!({"temp_c_centi": 2_000}),
            "alice-2",
        ),
        (&owner, serde_json::json!({"temp_c_centi": 2_001}), "owner"),
        (
            &key(0x31),
            serde_json::json!({"temp_c_centi": 20.4}),
            "float",
        ),
        (
            &key(0x36),
            serde_json::json!({"temp_c_centi": 10_001}),
            "too-hot",
        ),
        (
            &key(0x32),
            serde_json::json!({"temp_c_centi": 2_040, "pressure_pa": 101_325}),
            "two",
        ),
    ] {
        let reveal = commit_only(&mut state, signer, value, tag, h);
        reveal_at(&mut state, &reveal, h, t_d + 100);
    }
    for (signer, lead, station, tag) in [
        (key(0x33), 3u32, STATION, "lead-3"),
        // The 1 h lead is not scored (cheap baselines and honest
        // forecasters overlap there), so a 1 h forecast is not recorded.
        (key(0x37), 1, STATION, "lead-1"),
        (key(0x34), 6, "st-unregistered", "no-station"),
    ] {
        let (commit, reveal) = forecast(
            &signer,
            station,
            serde_json::json!({"temp_c_centi": 2_040}),
            lead,
            tag,
        );
        commit_at(&mut state, &commit, h);
        reveal_at(&mut state, &reveal, h, t_d + 100);
    }
    assert_eq!(state.forecast_late_score.records.len(), records_before);
    assert!(state.forecast_late_score.pending.is_empty());
    // A commit that is never revealed stays pending until its reveal window ends.
    let (never_revealed, _) = forecast(
        &key(0x35),
        STATION,
        serde_json::json!({"temp_c_centi": 1_234}),
        6,
        "never",
    );
    commit_at(&mut state, &never_revealed, h);
    assert_eq!(state.forecast_late_score.pending.len(), 1);
    // A second bucket (24 h lead) that nobody scores.
    let (commit_24, reveal_24) = forecast(
        &bob,
        STATION,
        serde_json::json!({"temp_c_centi": 2_101}),
        24,
        "bob-24h",
    );
    commit_at(&mut state, &commit_24, h);
    reveal_at(&mut state, &reveal_24, h, t_d + 100);
    close_at(&mut state, h, t_d + 100).expect("end of block");
    let unscored_bucket =
        bucket_id(STATION, LateScoreVariable::TemperatureC, 24, T0 + 24 * HOUR).unwrap();
    assert!(
        state
            .forecast_late_score
            .buckets
            .contains_key(&unscored_bucket)
    );

    // Readings that land AFTER the commits: the truth (landed near the
    // target, so after every commit) and a late reading for the window test.
    let truth = attested_reading(
        &mut state,
        &owner,
        STATION,
        "truth",
        &[&oracle_a, &oracle_b],
        ACTIVE + 280,
    );
    let late_reading = attested_reading(
        &mut state,
        &owner,
        STATION,
        "late",
        &[&oracle_a, &oracle_b],
        ACTIVE + 1,
    );

    // A forecast-type reveal is never a reading, even when its data is a
    // complete weather observation and the oracle quorum has attested it: it
    // holds no quality score, so it cannot be cited as truth or as a window
    // reading (checked with the score refusals below). A station reading
    // with the same data holds one: `truth` is cited by every good score.
    let weather_fields = serde_json::json!({
        "quality": 1.0, "volume": 1.0, "uniqueness": 1.0,
        "temperature_c": 20.0, "humidity_percent": 60.0, "pressure_hpa": 1014.0,
        "wind_speed_ms": 2.8, "precipitation_mm": 0.0, "dewpoint_c": 12.0
    });
    let (weatherish_commit, weatherish_reveal) =
        forecast(&erin, STATION, weather_fields, 6, "weatherish");
    commit_at(&mut state, &weatherish_commit, ACTIVE + 281);
    reveal_at(&mut state, &weatherish_reveal, ACTIVE + 281, T0);
    let weatherish = weatherish_commit.commit_id.clone();
    attest(
        &mut state,
        &weatherish,
        &[&oracle_a, &oracle_b],
        "weatherish",
        ACTIVE + 281,
    );

    // ---- Scoring: every refusal leaves the state untouched. ----
    let target_a = t_a + 6 * HOUR;
    let opens_at = target_a + SCORE_OPENS_AFTER_SECS;
    assert_eq!(
        SCORE_OPENS_AFTER_SECS,
        35 * 60,
        "15 minutes of readings plus 20 to land and reach quorum"
    );
    let closes_at = bucket_closes_at(hour);
    assert_eq!(closes_at, hour + HOUR + SCORE_WINDOW_SECS);
    let group_a = [f_alice.as_str(), f_bob.as_str()];
    let att_a = || attestation(ACTIVE, t_a, &truth, &reading);
    let good = score_payload(att_a(), &[&oracle_a, &oracle_b], &group_a);
    let h = ACTIVE + 400;
    let snapshot = state.state_root_material();
    refused(score_at(&mut state, &good, h, opens_at - 1), "too early");
    refused(score_at(&mut state, &good, h, closes_at), "too late");
    refused(
        score_at(
            &mut state,
            &score_payload(att_a(), &[&oracle_a], &group_a),
            h,
            opens_at,
        ),
        "quorum is 2",
    );
    refused(
        score_at(
            &mut state,
            &score_payload(att_a(), &[&oracle_a, &stranger], &group_a),
            h,
            opens_at,
        ),
        "not an authorized oracle",
    );
    let mut tampered = good.clone();
    tampered.attestation.truth = 2_040;
    refused(
        score_at(&mut state, &tampered, h, opens_at),
        "does not verify",
    );
    let mut unsorted = good.clone();
    unsorted.signatures.reverse();
    refused(
        score_at(&mut state, &unsorted, h, opens_at),
        "strictly ascending",
    );
    let mut duplicated = good.clone();
    duplicated.signatures = vec![good.signatures[0].clone(), good.signatures[0].clone()];
    refused(
        score_at(&mut state, &duplicated, h, opens_at),
        "strictly ascending",
    );
    // The oracle signature covers the full score payload: the observation
    // signed for `group_a` does not score a different forecast list, neither
    // a subset nor a superset.
    for other_list in [vec![f_alice.clone()], {
        let mut more: Vec<String> = group_a.iter().map(|s| s.to_string()).collect();
        more.push("zz-extra".to_string());
        more.sort();
        more
    }] {
        let mut relisted = good.clone();
        relisted.forecast_commit_ids = other_list;
        refused(
            score_at(&mut state, &relisted, h, opens_at),
            "does not verify",
        );
    }
    // Oracle keys are the chain's signing-key registry at the block's time.
    // Oracle A rotates to a new key from `opens_at + 100`: the new key signs
    // for A's identity from then on and not before; once that key epoch is
    // revoked it no longer signs. One identity counts once, whichever of its
    // keys signed. (On clones: the main state stays untouched.)
    {
        let rotated_key = key(0x2a);
        let a_id = oracle_id(&oracle_a);
        let as_a_rotated = score_payload_as(
            att_a(),
            &[
                (a_id.clone(), &rotated_key),
                (oracle_id(&oracle_b), &oracle_b),
            ],
            &group_a,
        );
        refused(
            score_at(&mut state.clone(), &as_a_rotated, h, opens_at + 100),
            "does not verify",
        );
        let mut rotated = state.clone();
        rotated
            .apply_transition(StateTransition::RegisterSigningKeyEpoch {
                identity: a_id.clone(),
                role: SigningRole::Oracle,
                epoch: 1,
                public_key: oracle_id(&rotated_key),
                activated_at: opens_at + 100,
            })
            .expect("rotation applies");
        refused(
            score_at(&mut rotated.clone(), &as_a_rotated, h, opens_at + 99),
            "does not verify",
        );
        score_at(&mut rotated.clone(), &as_a_rotated, h, opens_at + 100)
            .expect("the rotated-in key signs for the oracle identity");
        // The rotated key under its own name is nobody.
        refused(
            score_at(
                &mut rotated.clone(),
                &score_payload(att_a(), &[&rotated_key, &oracle_b], &group_a),
                h,
                opens_at + 100,
            ),
            "not an authorized oracle",
        );
        // Both of A's keys do not make two oracles.
        let mut both_keys = as_a_rotated.clone();
        both_keys.signatures = score_payload_as(
            att_a(),
            &[(a_id.clone(), &rotated_key), (a_id.clone(), &oracle_a)],
            &group_a,
        )
        .signatures;
        refused(
            score_at(&mut rotated.clone(), &both_keys, h, opens_at + 100),
            "strictly ascending",
        );
        rotated
            .apply_transition(StateTransition::RevokeSigningKeyEpoch {
                identity: a_id.clone(),
                role: SigningRole::Oracle,
                epoch: 1,
                revoked_at: opens_at + 200,
            })
            .expect("revocation applies");
        score_at(&mut rotated.clone(), &as_a_rotated, h, opens_at + 199)
            .expect("still valid one second before the revocation");
        refused(
            score_at(&mut rotated.clone(), &as_a_rotated, h, opens_at + 200),
            "does not verify",
        );
        // Revoke A's first key too: A cannot sign at all, though still in the set.
        rotated
            .apply_transition(StateTransition::RevokeSigningKeyEpoch {
                identity: a_id,
                role: SigningRole::Oracle,
                epoch: 0,
                revoked_at: opens_at + 200,
            })
            .expect("revocation applies");
        refused(
            score_at(&mut rotated, &good, h, opens_at + 200),
            "does not verify",
        );
    }
    // The attested forecast reveal with weather fields is not a reading. As
    // truth it landed after the commit and has quorum, so only the
    // station-reading rule refuses it.
    refused(
        score_at(
            &mut state,
            &score_payload(
                attestation(ACTIVE, t_a, &weatherish, &reading),
                &[&oracle_a, &oracle_b],
                &group_a,
            ),
            h,
            opens_at,
        ),
        "truth reading is not an attested",
    );
    // Cited readings: the truth and every window reading must be attested
    // readings of this station.
    for bad in [
        weak_reading.as_str(),
        other_station_reading.as_str(),
        "no-such-reading",
    ] {
        refused(
            score_at(
                &mut state,
                &score_payload(
                    attestation(ACTIVE, t_a, bad, &reading),
                    &[&oracle_a, &oracle_b],
                    &group_a,
                ),
                h,
                opens_at,
            ),
            "truth reading is not an attested",
        );
        refused(
            score_at(
                &mut state,
                &score_payload(
                    attestation(ACTIVE, t_a, &truth, bad),
                    &[&oracle_a, &oracle_b],
                    &group_a,
                ),
                h,
                opens_at,
            ),
            "window reading is not an attested",
        );
    }
    // A reading that landed at or after the commit height can never be in the
    // window, even fully attested.
    refused(
        score_at(
            &mut state,
            &score_payload(
                attestation(ACTIVE, t_a, &truth, &late_reading),
                &[&oracle_a, &oracle_b],
                &group_a,
            ),
            h,
            opens_at,
        ),
        "landed at or after",
    );
    // The truth is observed after the forecast: a reading that landed at or
    // below the commit height is not a truth, and the truth is never also a
    // window reading.
    for (early_truth, window) in [(&reading, &reading), (&truth, &truth)] {
        refused(
            score_at(
                &mut state,
                &score_payload(
                    attestation(ACTIVE, t_a, early_truth, window),
                    &[&oracle_a, &oracle_b],
                    &group_a,
                ),
                h,
                opens_at,
            ),
            "did not land after",
        );
    }
    // Out of range, a target that is not commit time + lead, another hour.
    let mut hot = att_a();
    hot.truth = 10_001;
    refused(
        score_at(
            &mut state,
            &score_payload(hot, &[&oracle_a, &oracle_b], &group_a),
            h,
            opens_at,
        ),
        "out of range",
    );
    let mut shifted = att_a();
    shifted.target += 60;
    refused(
        score_at(
            &mut state,
            &score_payload(shifted, &[&oracle_a, &oracle_b], &group_a),
            h,
            opens_at,
        ),
        "target is not commit_time",
    );
    refused(
        score_at(
            &mut state,
            &score_payload(
                attestation(ACTIVE, t_a + 3 * HOUR, &truth, &reading),
                &[&oracle_a, &oracle_b],
                &group_a,
            ),
            h,
            opens_at + 3 * HOUR,
        ),
        "no forecast is recorded",
    );
    // The right bucket but another commit block's time or height: refused.
    refused(
        score_at(
            &mut state,
            &score_payload(
                attestation(ACTIVE + 1, t_c, &truth, &reading),
                &[&oracle_a, &oracle_b],
                &group_a,
            ),
            h,
            opens_at + HOUR,
        ),
        "does not belong",
    );
    refused(
        score_at(
            &mut state,
            &score_payload(
                attestation(ACTIVE, t_a + 1, &truth, &reading),
                &[&oracle_a, &oracle_b],
                &group_a,
            ),
            h,
            opens_at + HOUR,
        ),
        "does not belong",
    );
    assert_eq!(
        state.state_root_material(),
        snapshot,
        "refused scores change nothing"
    );

    // The read-only admission pre-check a node may run (not consensus):
    // every listed forecast recorded, unscored, of the attested block.
    let lane = &state.forecast_late_score;
    assert_eq!(lane.check_admissible_score(&good), Ok(()));
    let mut unknown = good.clone();
    unknown.forecast_commit_ids.push("zz-not-recorded".into());
    assert_eq!(
        lane.check_admissible_score(&unknown),
        Err(ForecastLateScoreError::ForecastNotInBucket("zz-not-recorded".into()))
    );
    let mut other_block = good.clone();
    other_block.attestation.commit_height += 1;
    assert!(matches!(
        lane.check_admissible_score(&other_block),
        Err(ForecastLateScoreError::ForecastNotInBucket(_))
    ));

    // One of the two first, by one relay; then both by another (the one
    // already scored is skipped).
    score_at(
        &mut state,
        &score_payload(att_a(), &[&oracle_b, &oracle_c], &[&f_alice]),
        h,
        opens_at,
    )
    .expect("first score applies");
    assert_eq!(
        state.forecast_late_score.records[&f_alice].skill,
        Some(866_667)
    );
    assert_eq!(state.forecast_late_score.records[&f_bob].skill, None);
    // Apply skips the scored one; the admission pre-check refuses the list.
    assert_eq!(
        state.forecast_late_score.check_admissible_score(&good),
        Err(ForecastLateScoreError::AlreadyScored(f_alice.clone()))
    );
    assert_eq!(
        state.forecast_late_score.buckets[&bucket]
            .observations
            .len(),
        1
    );
    // That commit block is pinned to that observation: a different one is
    // refused, even with a full quorum.
    let mut conflicting = att_a();
    conflicting.truth = 2_200;
    refused(
        score_at(
            &mut state,
            &score_payload(conflicting, &[&oracle_a, &oracle_b], &group_a),
            h + 1,
            opens_at,
        ),
        "different observation",
    );
    score_at(&mut state, &good, h + 1, closes_at - 1)
        .expect("the rest score up to the last second");
    assert_eq!(
        state.forecast_late_score.records[&f_bob].skill,
        Some(500_000)
    );
    assert_eq!(
        state.forecast_late_score.records[&f_alice].skill,
        Some(866_667),
        "a scored forecast is not rescored"
    );
    // Replay: nothing left to score.
    refused(
        score_at(&mut state, &good, h + 2, opens_at),
        "no listed forecast is waiting",
    );
    // The other commit blocks have their own observations (same numbers here).
    let group_c = [f_carol.as_str(), f_erin.as_str()];
    let opens_c = t_c + 6 * HOUR + SCORE_OPENS_AFTER_SECS;
    let score_c = score_payload(
        attestation(ACTIVE + 1, t_c, &truth, &reading),
        &[&oracle_a, &oracle_b],
        &group_c,
    );
    refused(
        score_at(&mut state, &score_c, h + 2, opens_c - 1),
        "too early",
    );
    score_at(&mut state, &score_c, h + 2, opens_c).expect("second commit block scores");
    assert_eq!(
        state.forecast_late_score.records[&f_carol].skill,
        Some(300_000)
    );
    assert_eq!(
        state.forecast_late_score.records[&f_erin].skill,
        Some(33_334)
    );
    // Dave's window may include the reading that landed at ACTIVE + 1: it is
    // below his commit height (ACTIVE + 3).
    let score_d = score_payload(
        attestation(ACTIVE + 3, t_d, &truth, &late_reading),
        &[&oracle_a, &oracle_c],
        &[&f_dave],
    );
    score_at(
        &mut state,
        &score_d,
        h + 3,
        t_d + 6 * HOUR + SCORE_OPENS_AFTER_SECS,
    )
    .expect("third commit block scores");
    assert_eq!(
        state.forecast_late_score.records[&f_dave].skill,
        Some(860_000)
    );
    assert_eq!(
        state.forecast_late_score.buckets[&bucket]
            .observations
            .len(),
        3
    );

    // ---- No baseline credit for a forecast reveal once in force. ----
    refused(
        state.apply_transition(StateTransition::ApplyContributionRewardDecision {
            record: ContributionRewardRecord {
                address: address_of(&alice),
                amount: Balance::new(1),
                reason: format!("{FORECAST_REWARD_REASON_PREFIX}{f_alice}"),
                station_id: STATION.to_string(),
                provenance_class: ContributionProvenanceClass::Real,
            },
            current_block: h + 3,
        }),
        "forecast reveal credits are off",
    );

    // ---- Snapshot round trip with a populated lane. ----
    {
        let mut source = state.clone();
        source.recalculate_state_root();
        let entries = export_state_entries(&source).expect("export");
        let mut synced = State::default();
        synced.apply_snapshot_entries(&entries).expect("import");
        assert_eq!(synced.forecast_late_score, source.forecast_late_score);
        assert_eq!(synced.state_root_material(), source.state_root_material());
    }

    // ---- Close and pay. ----
    let all = [
        f_alice.as_str(),
        f_bob.as_str(),
        f_carol.as_str(),
        f_erin.as_str(),
        f_dave.as_str(),
    ];
    let close_height = 3_000u64;
    let supply_before = state.total_minted_supply();
    // One second early: only the expired pending commit goes.
    close_at(&mut state, close_height - 1, closes_at - 1)
        .expect("early close is a no-op for buckets");
    assert!(
        state.forecast_late_score.pending.is_empty(),
        "the unrevealed commit expired"
    );
    assert_eq!(state.forecast_late_score.buckets.len(), 2);
    assert_eq!(state.total_minted_supply(), supply_before);
    // A close for a height other than the open block is refused.
    assert!(matches!(
        state.apply_transition(StateTransition::CloseForecastBuckets {
            block_height: close_height + 7,
            block_timestamp: closes_at
        }),
        Err(StateError::InvalidTransition)
    ));

    // A credit that cannot be made never stops the block: with Bob's
    // earned total at the ceiling the close still applies, the others are
    // paid, and Bob's amount goes back to unissued, unminted.
    {
        let mut full = state.clone();
        let bob_address = address_of(&bob);
        full.create_account(bob_address, Balance::new(0))
            .earned_balance_total = Balance::new(u128::MAX);
        close_at(&mut full, close_height, closes_at)
            .expect("the close applies and the block goes on");
        assert_eq!(balance(&full, &bob_address), 0);
        assert_eq!(balance(&full, &address_of(&alice)), 86_666_700);
        assert_eq!(balance(&full, &address_of(&carol)), 30_000_000);
        assert_eq!(full.total_minted_supply(), supply_before + 116_666_700);
        assert_eq!(
            full.emission_ledger.spent(EmissionRole::Contribution),
            116_666_700
        );
        let ledger = &full.forecast_late_score.pool[&(close_height / POOL_EPOCH_BLOCKS)];
        assert_eq!(ledger.paid, Balance::new(116_666_700));
        assert_eq!(ledger.unissued, Balance::new(50_000_000));
        assert_eq!(
            ledger.paid_by_forecaster[&forecaster_key(&bob_address)],
            Balance::new(0)
        );
        assert!(full.forecast_late_score.buckets.get(&bucket).is_none());
    }
    close_at(&mut state, close_height, closes_at).expect("close applies");
    // Top three by skill among non-copiers: Alice, Bob, Carol. Dave (a copy)
    // and Erin (fourth) earn nothing.
    let paid = [
        (&alice, 86_666_700u128),
        (&bob, 50_000_000),
        (&carol, 30_000_000),
    ];
    for (who, amount) in paid {
        assert_eq!(balance(&state, &address_of(who)), amount);
    }
    assert_eq!(balance(&state, &address_of(&dave)), 0);
    assert_eq!(balance(&state, &address_of(&erin)), 0);
    let total: u128 = paid.iter().map(|(_, amount)| amount).sum();
    assert!(total <= BUCKET_CAP);
    assert_eq!(
        state.total_minted_supply(),
        supply_before + total,
        "exactly the payments are minted"
    );
    assert_eq!(
        state.emission_ledger.spent(EmissionRole::Contribution),
        total,
        "charged to the Contribution slice of the closing block"
    );
    let ledger = &state.forecast_late_score.pool[&(close_height / POOL_EPOCH_BLOCKS)];
    assert_eq!(ledger.paid, Balance::new(total));
    assert_eq!(ledger.unissued, Balance::new(0));
    assert_eq!(
        ledger.paid_by_forecaster[&forecaster_key(&address_of(&alice))],
        Balance::new(86_666_700)
    );
    let slice = state
        .emission_ledger
        .partition()
        .slice(EmissionRole::Contribution);
    assert_eq!(
        ledger.budget,
        Balance::new(slice * 1_000 * 500 / 10_000),
        "5% of the slice over the epoch"
    );
    // The bucket and its records are gone; the unscored one is still open.
    assert!(!state.forecast_late_score.buckets.contains_key(&bucket));
    assert!(
        all.iter()
            .all(|id| !state.forecast_late_score.records.contains_key(*id))
    );
    assert!(
        state
            .forecast_late_score
            .buckets
            .contains_key(&unscored_bucket)
    );
    // A closed bucket cannot be scored or paid again.
    refused(
        score_at(&mut state, &good, close_height + 1, closes_at - 1),
        "no forecast is recorded",
    );
    let after_close = state.total_minted_supply();
    close_at(&mut state, close_height + 1, closes_at + 1).expect("second close");
    assert_eq!(state.total_minted_supply(), after_close);

    // The unscored bucket closes unpaid and is deleted: fail closed.
    close_at(
        &mut state,
        close_height + 2,
        bucket_closes_at(T0 + 24 * HOUR),
    )
    .expect("unscored close");
    assert!(state.forecast_late_score.buckets.is_empty());
    assert!(state.forecast_late_score.records.is_empty());
    assert_eq!(state.total_minted_supply(), after_close);

    // Old pool epochs and idle track records (at an epoch boundary) are
    // dropped; then the lane is empty again and leaves the root material.
    close_at(
        &mut state,
        (close_height / POOL_EPOCH_BLOCKS + 10) * POOL_EPOCH_BLOCKS,
        T0 + 1_000 * HOUR,
    )
    .expect("late close");
    assert!(state.forecast_late_score.is_empty());
    assert!(!has_root_section(&state));

    reorg_and_rejection_atomicity();
}

/// Reorg and rejection atomicity. Runs inside the end-to-end test because the
/// feature schedule is process-wide (it is active here).
///
/// A node reorgs by restoring the fork-point `State` and replaying the new
/// branch. So: build a fork point with live lane state, apply branch A,
/// restore the fork point from its checkpoint-codec bytes, apply branch B,
/// and require the result to equal a node that only ever saw B.
fn reorg_and_rejection_atomicity() {
    let owner = key(0x01);
    let (alice, bob, carol) = (key(0x41), key(0x42), key(0x43));
    let (oracle_a, oracle_b) = (key(0x21), key(0x22));
    let fork_height = ACTIVE + 10;
    let (t_x, t_fork) = (T0 + 50, T0 + 150);

    struct Common {
        reading: String,
        f_alice: String,
        bob_reveal: RevealPayload,
    }
    // Everything up to and including the fork-point block. At the fork point
    // the lane holds a record (Alice), a stamped pending commit (Bob) and a
    // bucket.
    let prefix = |state: &mut State| -> Common {
        register_station(state, STATION, address_of(&owner));
        state
            .apply_transition(StateTransition::SetOracleConfig {
                oracle_set: vec![oracle_id(&oracle_a), oracle_id(&oracle_b)],
                oracle_quorum: 2,
            })
            .expect("oracle config");
        let reading = attested_reading(
            state,
            &owner,
            STATION,
            "reorg-r",
            &[&oracle_a, &oracle_b],
            ACTIVE,
        );
        let (commit, reveal) = forecast(
            &alice,
            STATION,
            serde_json::json!({"temp_c_centi": 2_040}),
            6,
            "reorg-alice",
        );
        commit_at(state, &commit, fork_height - 1);
        reveal_at(state, &reveal, fork_height - 1, t_x);
        close_at(state, fork_height - 1, t_x).expect("end of block");
        let (bob_commit, bob_reveal) = forecast(
            &bob,
            STATION,
            serde_json::json!({"temp_c_centi": 2_150}),
            6,
            "reorg-bob",
        );
        commit_at(state, &bob_commit, fork_height);
        seed_track_records(state, &[&alice, &bob, &carol], t_fork);
        close_at(state, fork_height, t_fork).expect("end of fork block");
        state.recalculate_state_root();
        Common {
            reading,
            f_alice: commit.commit_id,
            bob_reveal,
        }
    };
    let hour = T0 + 6 * HOUR;
    let closes_at = bucket_closes_at(hour);
    let observation = |common: &Common, truth_id: &str, truth: i64| {
        let mut a = attestation(fork_height - 1, t_x, truth_id, &common.reading);
        a.truth = truth;
        score_payload(a, &[&oracle_a, &oracle_b], &[&common.f_alice])
    };
    // Branch A: Bob never reveals; Carol forecasts; Alice is scored against
    // truth 20.00 and paid at close.
    let branch_a = |state: &mut State, common: &Common| {
        let (commit, reveal) = forecast(
            &carol,
            STATION,
            serde_json::json!({"temp_c_centi": 2_210}),
            6,
            "reorg-carol",
        );
        commit_at(state, &commit, fork_height + 1);
        reveal_at(state, &reveal, fork_height + 1, t_fork + 100);
        close_at(state, fork_height + 1, t_fork + 100).expect("A: end of block");
        let truth = attested_reading(
            state,
            &owner,
            STATION,
            "reorg-truth-a",
            &[&oracle_a, &oracle_b],
            fork_height + 200,
        );
        score_at(
            state,
            &observation(common, &truth, 2_000),
            fork_height + 300,
            t_x + 6 * HOUR + SCORE_OPENS_AFTER_SECS,
        )
        .expect("A: score");
        close_at(state, fork_height + 400, closes_at).expect("A: close and pay");
        state.recalculate_state_root();
    };
    // Branch B: Bob reveals (his stamp comes from the fork block); Alice is
    // scored against a different truth; nothing has closed yet.
    let branch_b = |state: &mut State, common: &Common| {
        reveal_at(state, &common.bob_reveal, fork_height + 1, t_fork + 77);
        close_at(state, fork_height + 1, t_fork + 77).expect("B: end of block");
        let truth = attested_reading(
            state,
            &owner,
            STATION,
            "reorg-truth-b",
            &[&oracle_a, &oracle_b],
            fork_height + 200,
        );
        score_at(
            state,
            &observation(common, &truth, 2_100),
            fork_height + 300,
            t_x + 6 * HOUR + SCORE_OPENS_AFTER_SECS,
        )
        .expect("B: score");
        state.recalculate_state_root();
    };

    // The node that followed A and then reorgs to B.
    let mut node = State::new();
    let common = prefix(&mut node);
    assert_eq!(node.forecast_late_score.records.len(), 1);
    assert_eq!(
        node.forecast_late_score
            .pending
            .values()
            .next()
            .unwrap()
            .commit_time,
        Some(t_fork)
    );
    let fork_bytes = bincode::serialize(&node).expect("checkpoint codec");
    let fork_material = node.state_root_material();
    branch_a(&mut node, &common);
    assert!(
        node.forecast_late_score.buckets.is_empty(),
        "A closed the bucket"
    );
    assert_eq!(
        node.forecast_late_score.pool.len(),
        1,
        "A paid from the pool"
    );
    assert!(balance(&node, &address_of(&alice)) > 0);
    let after_a = node.state_root_material();

    let mut node: State = bincode::deserialize(&fork_bytes).expect("restore the fork point");
    assert_eq!(
        node.state_root_material(),
        fork_material,
        "the restored fork point is the fork point"
    );
    branch_b(&mut node, &common);

    // A node that only ever saw B.
    let mut fresh = State::new();
    let fresh_common = prefix(&mut fresh);
    assert_eq!(fresh_common.f_alice, common.f_alice);
    branch_b(&mut fresh, &fresh_common);

    assert_eq!(
        node.forecast_late_score, fresh.forecast_late_score,
        "no residue of branch A in the lane"
    );
    assert_eq!(node.state_root_material(), fresh.state_root_material());
    assert_eq!(node.state_root, fresh.state_root);
    assert_ne!(node.state_root_material(), after_a);
    let lane = &node.forecast_late_score;
    assert!(lane.pool.is_empty(), "A's pool ledger is gone");
    assert_eq!(
        balance(&node, &address_of(&alice)),
        0,
        "A's payment is gone"
    );
    assert_eq!(
        lane.records.len(),
        2,
        "Alice and Bob; Carol's record from A is gone"
    );
    assert_eq!(
        lane.records[&common.bob_reveal.commit_id].commit_time, t_fork,
        "Bob keeps the fork block's stamp"
    );
    assert_eq!(
        lane.records[&common.f_alice].skill,
        Some(800_000),
        "scored against B's truth, not A's"
    );
    assert!(lane.pending.is_empty());

    // Rejection atomicity: after each kind of lane write is in the state, a
    // refused transition of every lane-writing kind changes nothing.
    let mut checked = 0;
    let mut probe = |state: &mut State, label: &str| {
        let before = state.state_root_material();
        let lane_before = state.forecast_late_score.clone();
        let height = fork_height + 500;
        // A duplicate commit id, a reveal with no commit, a forged score and a
        // close for the wrong height.
        let (dup_commit, _) = forecast(
            &alice,
            STATION,
            serde_json::json!({"temp_c_centi": 2_040}),
            6,
            "reorg-alice",
        );
        let known = state
            .forecast_late_score
            .records
            .contains_key(&dup_commit.commit_id);
        let dup = state.apply_transition(StateTransition::RegisterCommit {
            payload: dup_commit,
            block_height: height,
        });
        assert_eq!(
            dup.is_err(),
            known,
            "{label}: a commit id already revealed is refused"
        );
        let (_, orphan) = forecast(
            &carol,
            STATION,
            serde_json::json!({"temp_c_centi": 1_111}),
            6,
            "reorg-orphan",
        );
        assert!(
            state
                .apply_transition(StateTransition::ApplyReveal {
                    payload: orphan,
                    block_height: height,
                    block_timestamp: t_fork
                })
                .is_err()
        );
        let mut forged = observation(&common, &common.reading, 2_000);
        forged.attestation.truth += 1;
        assert!(score_at(state, &forged, height, closes_at - 1).is_err());
        assert!(
            state
                .apply_transition(StateTransition::CloseForecastBuckets {
                    block_height: height + 9_999,
                    block_timestamp: closes_at
                })
                .is_err()
        );
        if known {
            assert_eq!(
                state.forecast_late_score, lane_before,
                "{label}: lane untouched"
            );
            assert_eq!(
                state.state_root_material(),
                before,
                "{label}: root material untouched"
            );
        }
        checked += 1;
    };
    // After a record + a stamped pending commit (the fork point) …
    let mut at_fork: State = bincode::deserialize(&fork_bytes).unwrap();
    probe(&mut at_fork, "record and stamped pending");
    // … after a score and a second record (branch B) …
    probe(&mut node, "scored");
    // … and after a close with a payment and a pool ledger (branch A).
    let mut paid: State = bincode::deserialize(&fork_bytes).unwrap();
    branch_a(&mut paid, &common);
    assert!(!paid.forecast_late_score.pool.is_empty());
    let (pool_before, material_before) =
        (paid.forecast_late_score.clone(), paid.state_root_material());
    let mut forged = observation(&common, &common.reading, 2_000);
    forged.attestation.truth += 1;
    assert!(score_at(&mut paid, &forged, fork_height + 500, closes_at + 1).is_err());
    assert!(
        paid.apply_transition(StateTransition::CloseForecastBuckets {
            block_height: 1,
            block_timestamp: closes_at + 9
        })
        .is_err()
    );
    assert_eq!(
        paid.forecast_late_score, pool_before,
        "pool ledger untouched by refusals"
    );
    assert_eq!(paid.state_root_material(), material_before);
    assert_eq!(checked, 2);
}
