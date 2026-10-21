#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/helpers.rs"]
mod helpers;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/routes/oracle.rs"]
mod oracle;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/state.rs"]
mod state;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/rate_limit.rs"]
mod rate_limit;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/websocket.rs"]
mod websocket;

include!("support/launch_api_test_support.inc.rs");

use actix_web::{App, http::StatusCode, test, web};
use serde::Deserialize;
use wrfcoin_native_chain::blockchain::state::StateTransition;
use wrfcoin_native_chain::blockchain::transaction::OracleAttestationPayload;
use wrfcoin_native_chain::oracle::attestation::{AttestationRecord, AttestationSet};
use wrfcoin_native_chain::security::key_registry::SigningRole;

#[derive(Debug, Deserialize)]
struct AcceptedTransaction {
    id: String,
    tx_type: String,
}

fn test_app(
    state: web::Data<state::AppState>,
) -> actix_web::App<
    impl actix_web::dev::ServiceFactory<
        actix_web::dev::ServiceRequest,
        Config = (),
        Response = actix_web::dev::ServiceResponse<impl actix_web::body::MessageBody>,
        Error = actix_web::Error,
        InitError = (),
    >,
> {
    App::new()
        .app_data(state)
        .route(
            "/transactions/oracle/attestation",
            web::post().to(oracle::submit_oracle_attestation),
        )
        .route(
            "/transactions/oracle/operators",
            web::post().to(oracle::register_oracle_operator),
        )
        .route(
            "/transactions/oracle/config",
            web::get().to(oracle::get_oracle_config),
        )
        .route(
            "/transactions/oracle/status",
            web::get().to(oracle::get_oracle_status),
        )
        .route(
            "/oracle/leaderboard",
            web::get().to(oracle::get_oracle_leaderboard),
        )
        .route(
            "/transactions/oracle/config",
            web::put().to(oracle::update_oracle_config),
        )
}

#[actix_web::test]
async fn oracle_attestation_requires_oracle_role_on_production_route() {
    let (_oracle_sk, oracle_id) = keypair(401);
    let state = test_state(None);
    {
        let mut blockchain = state.blockchain.write().await;
        blockchain
            .apply_state_transition(StateTransition::SetOracleConfig {
                oracle_set: vec![oracle_id.clone()],
                oracle_quorum: 1,
            })
            .expect("oracle config should initialize");
    }
    let app = test::init_service(test_app(state)).await;

    let timestamp = chrono::Utc::now().timestamp();
    let commit_hash = "c".repeat(64);
    let payload = OracleAttestationPayload {
        commit_id: commit_hash.clone(),
        oracle_id: oracle_id.clone(),
        approved: true,
        signature: None,
        confidence: None,
        timestamp,
        nonce: None,
    };
    let body = serde_json::json!({
        "oracle_id": oracle_id,
        "commit_hash": commit_hash,
        "approved": true,
        "signature": keypair(402).0.sign(payload.signing_message().as_bytes()).to_string(),
        "timestamp": timestamp
    });

    let resp = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/transactions/oracle/attestation")
            .set_json(&body)
            .to_request(),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_enveloped_error::<AcceptedTransaction>(&body, "UNAUTHORIZED");
}

#[actix_web::test]
async fn oracle_attestation_accepts_valid_signature_on_production_route() {
    let (oracle_sk, oracle_id) = keypair(411);
    let state = test_state(None);
    {
        let mut blockchain = state.blockchain.write().await;
        blockchain
            .apply_state_transition(StateTransition::SetOracleConfig {
                oracle_set: vec![oracle_id.clone()],
                oracle_quorum: 1,
            })
            .expect("oracle config should initialize");
    }
    let app = test::init_service(test_app(state)).await;

    let timestamp = chrono::Utc::now().timestamp();
    let commit_hash = "d".repeat(64);
    let payload = OracleAttestationPayload {
        commit_id: commit_hash.clone(),
        oracle_id: oracle_id.clone(),
        approved: true,
        signature: None,
        confidence: Some(0.9),
        timestamp,
        nonce: Some("nonce-411".to_string()),
    };
    let signature = oracle_sk.sign(payload.signing_message().as_bytes()).to_string();

    let resp = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/transactions/oracle/attestation")
            .insert_header(bearer(TEST_ORACLE_TOKEN))
            .set_json(serde_json::json!({
                "oracle_id": oracle_id,
                "commit_hash": commit_hash,
                "approved": true,
                "signature": signature,
                "confidence": 0.9,
                "timestamp": timestamp,
                "nonce": "nonce-411"
            }))
            .to_request(),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: AcceptedTransaction = test::read_body_json(resp).await;
    assert!(!body.id.is_empty());
    assert_eq!(body.tx_type, "OracleAttestation");
}

#[actix_web::test]
async fn oracle_config_and_status_routes_use_exact_production_paths() {
    let (_oracle_sk, oracle_id) = keypair(421);
    let state = test_state(None);
    {
        let mut blockchain = state.blockchain.write().await;
        blockchain
            .apply_state_transition(StateTransition::SetOracleConfig {
                oracle_set: vec![oracle_id.clone()],
                oracle_quorum: 1,
            })
            .expect("oracle config should initialize");
        let mut attestation_set = AttestationSet::default();
        attestation_set.add(AttestationRecord {
            oracle_id: oracle_id.clone(),
            approved: true,
            confidence: 0.95,
            timestamp: chrono::Utc::now().timestamp(),
        });
        blockchain
            .state
            .attestations
            .insert("e".repeat(64), attestation_set);
    }
    let app = test::init_service(test_app(state)).await;

    let config_resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/transactions/oracle/config")
            .insert_header(bearer(TEST_READER_TOKEN))
            .to_request(),
    )
    .await;
    assert_eq!(config_resp.status(), StatusCode::OK);
    let config_body: oracle::OracleConfigResponse = test::read_body_json(config_resp).await;
    assert_eq!(config_body.oracle_set, vec![oracle_id.clone()]);
    assert_eq!(config_body.oracle_quorum, 1);

    let status_resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/transactions/oracle/status")
            .insert_header(bearer(TEST_READER_TOKEN))
            .to_request(),
    )
    .await;
    assert_eq!(status_resp.status(), StatusCode::OK);
    let status_body: oracle::OracleStatusResponse = test::read_body_json(status_resp).await;
    assert_eq!(status_body.oracle_set, vec![oracle_id]);
    assert_eq!(status_body.oracle_quorum, 1);
    assert_eq!(status_body.tracked_commit_count, 1);
    assert_eq!(status_body.commits_with_quorum, 1);
    assert_eq!(status_body.commits_pending_quorum, 0);
}

#[actix_web::test]
async fn oracle_leaderboard_uses_exact_production_route() {
    let (_oracle_sk, oracle_id) = keypair(431);
    let state = test_state(None);
    {
        let mut blockchain = state.blockchain.write().await;
        blockchain
            .apply_state_transition(StateTransition::SetOracleConfig {
                oracle_set: vec![oracle_id.clone()],
                oracle_quorum: 1,
            })
            .expect("oracle config should initialize");
    }
    let app = test::init_service(test_app(state)).await;

    let resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/oracle/leaderboard")
            .insert_header(bearer(TEST_READER_TOKEN))
            .to_request(),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["total_oracles"], 1);
    assert!(
        body["entries"]
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .any(|entry| entry["oracle_id"] == oracle_id),
        "leaderboard should expose the configured oracle"
    );
}

#[actix_web::test]
async fn update_oracle_config_requires_operator_on_production_route() {
    let (_oracle_sk, oracle_id) = keypair(441);
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;

    let resp = test::call_service(
        &app,
        test::TestRequest::put()
            .uri("/transactions/oracle/config")
            .set_json(serde_json::json!({
                "oracle_set": [oracle_id],
                "oracle_quorum": 1
            }))
            .to_request(),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_enveloped_error::<oracle::OracleConfigResponse>(&body, "UNAUTHORIZED");
}

#[actix_web::test]
async fn oracle_operator_registration_wires_signing_key_for_attestation_route() {
    let (_oracle_identity_sk, oracle_id) = keypair(461);
    let (operator_signing_sk, operator_public_key) = keypair(462);
    let activated_at = chrono::Utc::now().timestamp();
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;

    let register_resp = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/transactions/oracle/operators")
            .insert_header(bearer(TEST_OPERATOR_TOKEN))
            .set_json(serde_json::json!({
                "oracle_id": oracle_id,
                "public_key": operator_public_key,
                "epoch": 7,
                "activated_at": activated_at,
                "oracle_quorum": 1
            }))
            .to_request(),
    )
    .await;

    let register_status = register_resp.status();
    let register_bytes = test::read_body(register_resp).await;
    assert_eq!(
        register_status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&register_bytes)
    );
    let register_body: oracle::OracleOperatorRegisterResponse =
        serde_json::from_slice(&register_bytes).expect("registration body should deserialize");
    assert_eq!(register_body.oracle_id, oracle_id);
    assert_eq!(register_body.public_key, operator_public_key);
    assert_eq!(register_body.epoch, 7);
    assert_eq!(register_body.oracle_quorum, 1);
    assert_eq!(register_body.oracle_set, vec![oracle_id.clone()]);

    let timestamp = activated_at + 1;
    let commit_hash = "f".repeat(64);
    let payload = OracleAttestationPayload {
        commit_id: commit_hash.clone(),
        oracle_id: oracle_id.clone(),
        approved: true,
        signature: None,
        confidence: Some(0.97),
        timestamp,
        nonce: Some("operator-461".to_string()),
    };
    let signature = operator_signing_sk
        .sign(payload.signing_message().as_bytes())
        .to_string();

    let attestation_resp = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/transactions/oracle/attestation")
            .insert_header(bearer(TEST_ORACLE_TOKEN))
            .set_json(serde_json::json!({
                "oracle_id": oracle_id,
                "commit_hash": commit_hash,
                "approved": true,
                "signature": signature,
                "confidence": 0.97,
                "timestamp": timestamp,
                "nonce": "operator-461"
            }))
            .to_request(),
    )
    .await;

    let attestation_status = attestation_resp.status();
    let attestation_bytes = test::read_body(attestation_resp).await;
    assert_eq!(
        attestation_status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&attestation_bytes)
    );
    let body: AcceptedTransaction =
        serde_json::from_slice(&attestation_bytes).expect("attestation body should deserialize");
    assert!(!body.id.is_empty());
    assert_eq!(body.tx_type, "OracleAttestation");
}

#[actix_web::test]
async fn oracle_operator_registration_defaults_to_nonzero_epoch() {
    let (_oracle_identity_sk, oracle_id) = keypair(463);
    let (_operator_signing_sk, operator_public_key) = keypair(464);
    let activated_at = chrono::Utc::now().timestamp();
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;

    let register_resp = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/transactions/oracle/operators")
            .insert_header(bearer(TEST_OPERATOR_TOKEN))
            .set_json(serde_json::json!({
                "oracle_id": oracle_id,
                "public_key": operator_public_key,
                "activated_at": activated_at,
                "oracle_quorum": 1
            }))
            .to_request(),
    )
    .await;

    assert_eq!(register_resp.status(), StatusCode::CREATED);
    let register_body: oracle::OracleOperatorRegisterResponse =
        test::read_body_json(register_resp).await;
    assert_eq!(register_body.epoch, 1);
}

#[actix_web::test]
async fn oracle_operator_registration_rejects_epoch_zero_without_partial_state() {
    let (_oracle_identity_sk, oracle_id) = keypair(465);
    let (_operator_signing_sk, operator_public_key) = keypair(466);
    let state = test_state(None);
    let inspect_state = state.clone();
    let app = test::init_service(test_app(state)).await;

    let register_resp = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/transactions/oracle/operators")
            .insert_header(bearer(TEST_OPERATOR_TOKEN))
            .set_json(serde_json::json!({
                "oracle_id": oracle_id,
                "public_key": operator_public_key,
                "epoch": 0,
                "activated_at": chrono::Utc::now().timestamp(),
                "oracle_quorum": 1
            }))
            .to_request(),
    )
    .await;

    assert_eq!(register_resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body: serde_json::Value = test::read_body_json(register_resp).await;
    assert_enveloped_error::<oracle::OracleOperatorRegisterResponse>(&body, "UNPROCESSABLE_ENTITY");

    let blockchain = inspect_state.blockchain.read().await;
    assert!(
        !blockchain.state.oracle_set.contains(&oracle_id),
        "rejected epoch-0 registration must not mutate oracle_set"
    );
    assert!(
        blockchain
            .state
            .signing_keys
            .snapshot_for_role(SigningRole::Oracle)
            .is_empty(),
        "rejected epoch-0 registration must not seed legacy signing keys"
    );
}

#[actix_web::test]
async fn oracle_attestation_rejects_unregistered_oracle_id() {
    // An oracle_id that is a valid ed25519 public-key hex but is NOT registered
    // in the oracle_set / signing_keys must be rejected at the API layer with 422
    // rather than silently passing to the mempool (which would also reject it
    // via has_identity_role, but with a confusing error message).
    let (unregistered_sk, unregistered_id) = keypair(499);
    let state = test_state(None);
    // Deliberately do NOT call SetOracleConfig — oracle_id has no signing-key entry.
    let app = test::init_service(test_app(state)).await;

    let timestamp = chrono::Utc::now().timestamp();
    let commit_hash = "a".repeat(64);
    let payload = OracleAttestationPayload {
        commit_id: commit_hash.clone(),
        oracle_id: unregistered_id.clone(),
        approved: true,
        signature: None,
        confidence: None,
        timestamp,
        nonce: Some("nonce-499".to_string()),
    };
    let signature = unregistered_sk
        .sign(payload.signing_message().as_bytes())
        .to_string();

    let resp = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/transactions/oracle/attestation")
            .insert_header(bearer(TEST_ORACLE_TOKEN))
            .set_json(serde_json::json!({
                "oracle_id": unregistered_id,
                "commit_hash": commit_hash,
                "approved": true,
                "signature": signature,
                "timestamp": timestamp,
                "nonce": "nonce-499"
            }))
            .to_request(),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_enveloped_error::<AcceptedTransaction>(&body, "UNPROCESSABLE_ENTITY");
}

#[actix_web::test]
async fn update_oracle_config_updates_set_on_production_route() {
    let (_oracle_a_sk, oracle_a) = keypair(451);
    let (_oracle_b_sk, oracle_b) = keypair(452);
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;

    let resp = test::call_service(
        &app,
        test::TestRequest::put()
            .uri("/transactions/oracle/config")
            .insert_header(bearer(TEST_OPERATOR_TOKEN))
            .set_json(serde_json::json!({
                "oracle_set": [oracle_a.clone(), oracle_b.clone()],
                "oracle_quorum": 2
            }))
            .to_request(),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: oracle::OracleConfigResponse = test::read_body_json(resp).await;
    let mut expected = vec![oracle_a, oracle_b];
    expected.sort();
    assert_eq!(body.oracle_set, expected);
    assert_eq!(body.oracle_quorum, 2);
}
