//! Forecast late score routes (`forecast_late_score`): status read, score
//! relay gated on the feature, and the quorum dry run before admission.
//!
//! Each test runs in its own child process (`isolated_chain_domain`), so the
//! process-wide feature overlay it installs cannot leak into other tests.
use super::*;

#[cfg(test)]
mod tests {
    use super::*;
    use wrfcoin_shared_protocol::blockchain::transaction::{TransactionInput, TransactionOutput};
    use wrfcoin_shared_protocol::consensus::forecast_late_score::{
        ForecastRecord, ForecastScorePayload, LateScoreVariable, ObservationAttestation, ObservationSignature,
        forecast_score_message, score_outpoint,
    };
    use wrfcoin_shared_protocol::crypto::chain_binding::{
        ChainIdentity, ConsensusFeature, FORECAST_LATE_SCORE_KEY_BINDING_LEAD_BLOCKS,
        refresh_feature_schedule_extensions,
    };
    use wrfcoin_shared_protocol::security::key_registry::SigningRole;

    const SUBMIT: &str = "/forecast/late-score/submit";
    /// A tip at which the lane is in force at tip + 1 once `activate` ran.
    const TIP: u64 = FORECAST_LATE_SCORE_KEY_BINDING_LEAD_BLOCKS + 10;
    const T0: i64 = 1_762_646_400;

    fn oracle(seed: u8) -> PrivateKey {
        PrivateKey::from_bytes(&[seed; 32]).unwrap()
    }
    fn oracle_id(key: &PrivateKey) -> String {
        PublicKey::from_private_key(key).to_hex()
    }
    fn attestation() -> ObservationAttestation {
        ObservationAttestation {
            station_id: "st-route-1".into(),
            variable: LateScoreVariable::TemperatureC,
            lead_hours: 6,
            commit_height: 5,
            commit_time: T0,
            target: T0 + 6 * 3_600,
            truth: 2_000,
            truth_commit_id: "t".into(),
            reference: 300,
            mean24: 1_900,
            last: 2_100,
            window_hash: "5a".repeat(32),
            window_commit_ids: vec!["r".into()],
        }
    }
    fn payload(signers: &[&PrivateKey], corrupt: bool) -> ForecastScorePayload {
        payload_for(signers, corrupt, "f")
    }
    fn payload_for(signers: &[&PrivateKey], corrupt: bool, forecast: &str) -> ForecastScorePayload {
        let id = ChainIdentity::active();
        let attestation = attestation();
        let ids = vec![forecast.to_string()];
        let message = forecast_score_message(&id.chain_id, &id.network_id, &attestation, &ids);
        let mut signatures: Vec<ObservationSignature> = signers
            .iter()
            .map(|k| ObservationSignature { oracle: oracle_id(k), ed25519: k.sign(&message).to_hex() })
            .collect();
        signatures.sort_by(|a, b| a.oracle.cmp(&b.oracle));
        if corrupt {
            signatures[0].ed25519 = oracle(0x99).sign(&message).to_hex();
        }
        ForecastScorePayload { attestation, signatures, forecast_commit_ids: ids }
    }
    fn relay_tx(payload: &ForecastScorePayload) -> Transaction {
        let mut tx = Transaction::new(
            vec![TransactionInput {
                // The outpoint derived from the signed payload, as admission requires.
                txid: {
                    let id = ChainIdentity::active();
                    score_outpoint(&id.chain_id, &id.network_id, payload)
                },
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 0,
                address: wrfcoin_shared_protocol::common::types::Address([0x77; 20]).to_string(),
                script: "OP_TRUE".into(),
                nft_data: None,
            }],
        );
        tx.tx_type = TransactionType::ForecastScore;
        tx.sender_address = wrfcoin_shared_protocol::common::types::Address([0x77; 20]);
        tx.data = Some(serde_json::to_string(payload).unwrap());
        tx.set_id();
        tx
    }
    fn bearer() -> (&'static str, String) {
        ("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")) // secret-leak-ok: synthetic test bearer only.
    }
    fn activate() {
        refresh_feature_schedule_extensions(vec![
            (ConsensusFeature::ForecasterKeyBinding.name().to_string(), 0),
            (ConsensusFeature::ForecastLateScore.name().to_string(), 0),
        ]);
    }
    /// Oracles 0x21..=0x23 in the oracle set and the key registry, quorum 2.
    async fn seed_oracles(server: &web::Data<AppState>) {
        let mut chain = server.blockchain.write().await;
        chain.height = TIP;
        chain.state.oracle_set.clear();
        for seed in [0x21, 0x22, 0x23] {
            let id = oracle_id(&oracle(seed));
            chain.state.oracle_set.insert(id.clone());
            chain.state.signing_keys.ensure_legacy_identity_key(&id, SigningRole::Oracle, 0).unwrap();
        }
        chain.state.oracle_quorum = 2;
        // Recorded, unscored forecasts of the attested commit block.
        let a = attestation();
        for commit_id in ["f", "g", "h"] {
            chain.state.forecast_late_score.records.insert(
                commit_id.to_string(),
                ForecastRecord {
                    forecaster: wrfcoin_shared_protocol::common::types::Address([0x55; 20]),
                    station_id: a.station_id.clone(),
                    variable: a.variable,
                    value: 1_950,
                    lead_hours: a.lead_hours,
                    commit_height: a.commit_height,
                    commit_time: a.commit_time,
                    target: a.target,
                    reveal_height: a.commit_height + 1,
                    skill: None,
                },
            );
        }
    }
    /// POST `tx` to the submit route with a contributor token: (status, body).
    macro_rules! post {
        ($app:expr, $tx:expr) => {{
            let response = test::call_service(
                $app,
                test::TestRequest::post().uri(SUBMIT).insert_header(bearer()).set_json($tx).to_request(),
            )
            .await;
            let status = response.status();
            let body: serde_json::Value = test::read_body_json(response).await;
            (status, body)
        }};
    }

    #[actix_web::test]
    async fn inactive_lane_reports_status_and_refuses_a_score() {
        const NAME: &str =
            "tests::forecast_late_score_route::tests::inactive_lane_reports_status_and_refuses_a_score";
        if crate::helpers::test_env::isolated_chain_domain(NAME, "777", "testnet", "0x2727272727272727272727272727272727272727", "none") {
            return;
        }
        let server = test_state(None);
        seed_oracles(&server).await;
        let app = test::init_service(test_app(server.clone())).await;
        let status = test::call_service(&app, test::TestRequest::get().uri("/forecast/late-score").to_request()).await;
        assert_eq!(status.status(), StatusCode::OK);
        let body: serde_json::Value = test::read_body_json(status).await;
        assert_eq!(body["data"]["in_force_next_height"], false, "{body}");
        assert_eq!(body["data"]["chain_id"], "777");
        assert_eq!(body["data"]["oracle_quorum"], 2);
        assert_eq!(body["data"]["records"], 3);

        let tx = relay_tx(&payload(&[&oracle(0x21), &oracle(0x22)], false));
        let unauth = test::call_service(&app, test::TestRequest::post().uri(SUBMIT).set_json(&tx).to_request()).await;
        assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);
        let (status, body) = post!(&app, &tx);
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
        assert!(body.to_string().contains("FEATURE_NOT_ACTIVE"), "{body}");
        assert!(server.mempool.read().await.get_all_transactions().is_empty());
    }

    #[actix_web::test]
    async fn active_lane_admits_a_quorum_score_and_refuses_bad_ones() {
        const NAME: &str =
            "tests::forecast_late_score_route::tests::active_lane_admits_a_quorum_score_and_refuses_bad_ones";
        if crate::helpers::test_env::isolated_chain_domain(NAME, "777", "testnet", "0x2727272727272727272727272727272727272727", "none") {
            return;
        }
        activate();
        let server = test_state(None);
        seed_oracles(&server).await;
        let app = test::init_service(test_app(server.clone())).await;
        let status = test::call_service(&app, test::TestRequest::get().uri("/forecast/late-score").to_request()).await;
        let body: serde_json::Value = test::read_body_json(status).await;
        assert_eq!(body["data"]["in_force_next_height"], true, "{body}");

        // A forged signature.
        let (status, body) = post!(&app, &relay_tx(&payload(&[&oracle(0x21), &oracle(0x22)], true)));
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert!(body.to_string().contains("FORECAST_SCORE_REFUSED"), "{body}");
        assert!(body.to_string().contains("does not verify"), "{body}");
        // A signer outside the oracle set.
        let (status, body) = post!(&app, &relay_tx(&payload(&[&oracle(0x21), &oracle(0x29)], false)));
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert!(body.to_string().contains("not an authorized oracle"), "{body}");
        // Not a score, and a score whose data is not a payload.
        let mut other = relay_tx(&payload(&[&oracle(0x21), &oracle(0x22)], false));
        other.tx_type = TransactionType::AdvisoryGovernance;
        let (status, body) = post!(&app, &other);
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        let mut junk = relay_tx(&payload(&[&oracle(0x21), &oracle(0x22)], false));
        junk.data = Some("{\"attestation\":1}".into());
        let (status, body) = post!(&app, &junk);
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(body.to_string().contains("FORECAST_SCORE_PAYLOAD"), "{body}");
        assert!(server.mempool.read().await.get_all_transactions().is_empty());

        // Below the tip's quorum of 2: admitted and flagged, as the mempool and
        // the P2P path admit it (the including block may lower the quorum).
        let (status, body) = post!(&app, &relay_tx(&payload_for(&[&oracle(0x21)], false, "g")));
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        assert_eq!(body["data"]["below_quorum_at_tip"], true, "{body}");
        assert_eq!(server.mempool.read().await.get_all_transactions().len(), 1);

        // A well-formed quorum score is admitted.
        let tx = relay_tx(&payload(&[&oracle(0x21), &oracle(0x22)], false));
        let (status, body) = post!(&app, &tx);
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        assert_eq!(body["data"]["tx_id"], tx.id.unwrap().to_hex());
        assert_eq!(body["data"]["included"], false);
        assert_eq!(body["data"]["below_quorum_at_tip"], false, "{body}");
        let pending = server.mempool.read().await.get_all_transactions();
        assert_eq!(pending.len(), 2);
        assert!(pending.iter().all(|t| t.tx_type == TransactionType::ForecastScore));

        // A resend is refused before any signature work, and nothing is added.
        let (status, body) = post!(&app, &tx);
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert!(body.to_string().contains("ALREADY_PENDING"), "{body}");
        assert_eq!(server.mempool.read().await.get_all_transactions().len(), 2);
        // A reader token may not relay.
        let reader = test::call_service(
            &app,
            test::TestRequest::post()
                .uri(SUBMIT)
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: synthetic test bearer only.
                .set_json(&tx)
                .to_request(),
        )
        .await;
        assert_eq!(reader.status(), StatusCode::FORBIDDEN);
        // A body over the route's JSON limit is refused before parsing.
        let mut huge = relay_tx(&payload(&[&oracle(0x21), &oracle(0x22)], false));
        huge.data = Some("x".repeat(crate::routes::forecast_late_score::MAX_FORECAST_SCORE_JSON_BYTES + 1));
        let oversized = test::call_service(
            &app,
            test::TestRequest::post().uri(SUBMIT).insert_header(bearer()).set_json(&huge).to_request(),
        )
        .await;
        assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(server.mempool.read().await.get_all_transactions().len(), 2);

        // The same signed payload in another wrapper (relay label, time) is
        // not a second pool entry.
        let mut rewrapped = relay_tx(&payload(&[&oracle(0x21), &oracle(0x22)], false));
        rewrapped.sender_address = wrfcoin_shared_protocol::common::types::Address([0x78; 20]);
        rewrapped.timestamp += chrono::Duration::seconds(7);
        rewrapped.set_id();
        let (status, body) = post!(&app, &rewrapped);
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert!(body.to_string().contains("SP_FORECAST_LATE_SCORE_DUPLICATE"), "{body}");
        // The below-quorum score for "g" is replaced by one with more valid
        // signatures over the same payload.
        let weak_g = relay_tx(&payload_for(&[&oracle(0x21)], false, "g"));
        let strong_g = relay_tx(&payload_for(&[&oracle(0x21), &oracle(0x22)], false, "g"));
        let (status, body) = post!(&app, &strong_g);
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        let pending = server.mempool.read().await;
        assert!(!pending.contains(&weak_g.id.unwrap()) && pending.contains(&strong_g.id.unwrap()));
        drop(pending);
        // A forecast that is not recorded, or already scored, is refused.
        let (status, body) = post!(&app, &relay_tx(&payload_for(&[&oracle(0x21), &oracle(0x22)], false, "zz")));
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert!(body.to_string().contains("does not belong"), "{body}");
        server.blockchain.write().await.state.forecast_late_score.records.get_mut("h").unwrap().skill = Some(1);
        let (status, body) = post!(&app, &relay_tx(&payload_for(&[&oracle(0x21), &oracle(0x22)], false, "h")));
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert!(body.to_string().contains("already scored"), "{body}");
        assert_eq!(server.mempool.read().await.get_all_transactions().len(), 2);

        // Reads.
        let record = test::call_service(
            &app,
            test::TestRequest::get().uri("/forecast/late-score/records/f").to_request(),
        )
        .await;
        assert_eq!(record.status(), StatusCode::OK);
        let missing = test::call_service(
            &app,
            test::TestRequest::get().uri("/forecast/late-score/records/zz").to_request(),
        )
        .await;
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
        let missing = test::call_service(
            &app,
            test::TestRequest::get().uri("/forecast/late-score/buckets/nope").to_request(),
        )
        .await;
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    }

    #[actix_web::test]
    async fn one_token_is_rate_limited() {
        const NAME: &str = "tests::forecast_late_score_route::tests::one_token_is_rate_limited";
        if crate::helpers::test_env::isolated_chain_domain(NAME, "777", "testnet", "0x2727272727272727272727272727272727272727", "none") {
            return;
        }
        let server = test_state(None);
        let app = test::init_service(test_app(server.clone())).await;
        let tx = relay_tx(&payload(&[&oracle(0x21), &oracle(0x22)], false));
        for _ in 0..crate::routes::forecast_late_score::RATE_LIMIT_PER_MIN {
            let (status, body) = post!(&app, &tx);
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
        }
        let (status, body) = post!(&app, &tx);
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
        assert!(body.to_string().contains("RATE_LIMITED"), "{body}");
    }
}
