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
        ForecastScorePayload, LateScoreVariable, ObservationAttestation, ObservationSignature,
        observation_attestation_message,
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
        let id = ChainIdentity::active();
        let attestation = attestation();
        let message = observation_attestation_message(&id.chain_id, &id.network_id, &attestation);
        let mut signatures: Vec<ObservationSignature> = signers
            .iter()
            .map(|k| ObservationSignature { oracle: oracle_id(k), ed25519: k.sign(&message).to_hex() })
            .collect();
        signatures.sort_by(|a, b| a.oracle.cmp(&b.oracle));
        if corrupt {
            signatures[0].ed25519 = oracle(0x99).sign(&message).to_hex();
        }
        ForecastScorePayload { attestation, signatures, forecast_commit_ids: vec!["f".into()] }
    }
    fn relay_tx(payload: &ForecastScorePayload) -> Transaction {
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: wrfcoin_shared_protocol::crypto::hash::hash_bytes(b"forecast-score-route"),
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
        assert_eq!(body["data"]["records"], 0);

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
        // Below the quorum of 2.
        let (status, body) = post!(&app, &relay_tx(&payload(&[&oracle(0x21)], false)));
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert!(body.to_string().contains("quorum is 2"), "{body}");
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

        // A well-formed quorum score is admitted.
        let tx = relay_tx(&payload(&[&oracle(0x21), &oracle(0x22)], false));
        let (status, body) = post!(&app, &tx);
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        assert_eq!(body["data"]["tx_id"], tx.id.unwrap().to_hex());
        assert_eq!(body["data"]["included"], false);
        let pending = server.mempool.read().await.get_all_transactions();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].tx_type, TransactionType::ForecastScore);

        // Reads: nothing is recorded in this fixture.
        let missing = test::call_service(
            &app,
            test::TestRequest::get().uri("/forecast/late-score/records/f").to_request(),
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
}
