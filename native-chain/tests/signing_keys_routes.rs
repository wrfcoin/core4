#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/helpers.rs"]
mod helpers;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/routes/signing_keys.rs"]
mod signing_keys;
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
use wrfcoin_native_chain::blockchain::state::StateTransition;

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
            "/transactions/signing-keys/register",
            web::post().to(signing_keys::register_signing_key_epoch),
        )
        .route(
            "/transactions/signing-keys/revoke",
            web::post().to(signing_keys::revoke_signing_key_epoch),
        )
        .route(
            "/transactions/signing-keys/{role}",
            web::get().to(signing_keys::get_signing_keys_by_role),
        )
        .route(
            "/network/signing-keys/register",
            web::post().to(signing_keys::register_signing_key_epoch),
        )
        .route(
            "/network/signing-keys/revoke",
            web::post().to(signing_keys::revoke_signing_key_epoch),
        )
        .route(
            "/network/signing-keys/{role}",
            web::get().to(signing_keys::get_signing_keys_by_role),
        )
}

#[actix_web::test]
async fn transactions_register_requires_operator_on_production_route() {
    let (_identity_sk, identity) = keypair(101);
    let (_rotated_sk, rotated_pk) = keypair(102);
    let activated_at = chrono::Utc::now().timestamp();
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;

    let resp = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/transactions/signing-keys/register")
            .set_json(serde_json::json!({
                "identity": identity,
                "role": "oracle",
                "epoch": 1,
                "public_key": rotated_pk,
                "activated_at": activated_at
            }))
            .to_request(),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_enveloped_error::<signing_keys::SigningKeyMutationResponse>(&body, "UNAUTHORIZED");
}

#[actix_web::test]
async fn transactions_register_and_list_routes_use_exact_production_paths() {
    let (_identity_sk, identity) = keypair(111);
    let (_rotated_sk, rotated_pk) = keypair(112);
    let activated_at = chrono::Utc::now().timestamp();
    let state = test_state(None);
    {
        let mut blockchain = state.blockchain.write().await;
        blockchain
            .apply_state_transition(StateTransition::SetOracleConfig {
                oracle_set: vec![identity.clone()],
                oracle_quorum: 1,
            })
            .expect("oracle identity should be allowed to rotate keys");
    }
    let app = test::init_service(test_app(state)).await;

    let register_resp = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/transactions/signing-keys/register")
            .insert_header(bearer(TEST_OPERATOR_TOKEN))
            .set_json(serde_json::json!({
                "identity": identity,
                "role": "oracle",
                "epoch": 1,
                "public_key": rotated_pk,
                "activated_at": activated_at
            }))
            .to_request(),
    )
    .await;

    assert_eq!(register_resp.status(), StatusCode::OK);
    let register_body: signing_keys::SigningKeyMutationResponse =
        test::read_body_json(register_resp).await;
    assert_eq!(register_body.role, "oracle");
    assert_eq!(register_body.epoch, 1);
    assert_eq!(register_body.identity, identity);

    let list_resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/transactions/signing-keys/oracle")
            .insert_header(bearer(TEST_OPERATOR_TOKEN))
            .to_request(),
    )
    .await;

    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body: signing_keys::SigningKeyRoleSnapshotResponse =
        test::read_body_json(list_resp).await;
    let identity_epochs = list_body
        .identities
        .iter()
        .find(|item| item.identity == identity)
        .expect("registered identity should be listed");
    assert_eq!(list_body.role, "oracle");
    assert!(
        identity_epochs
            .epochs
            .iter()
            .any(|entry| entry.epoch == 1 && entry.revoked_at.is_none()),
        "epoch 1 should be visible in the oracle signing-key snapshot"
    );
}

#[actix_web::test]
async fn network_register_and_list_aliases_use_exact_production_paths() {
    let (_identity_sk, identity) = keypair(121);
    let (_rotated_sk, rotated_pk) = keypair(122);
    let activated_at = chrono::Utc::now().timestamp();
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;

    let register_resp = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/network/signing-keys/register")
            .insert_header(bearer(TEST_OPERATOR_TOKEN))
            .set_json(serde_json::json!({
                "identity": identity,
                "role": "calibrator",
                "epoch": 2,
                "public_key": rotated_pk,
                "activated_at": activated_at
            }))
            .to_request(),
    )
    .await;

    assert_eq!(register_resp.status(), StatusCode::OK);

    let list_resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/network/signing-keys/calibrator")
            .insert_header(bearer(TEST_OPERATOR_TOKEN))
            .to_request(),
    )
    .await;

    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body: signing_keys::SigningKeyRoleSnapshotResponse =
        test::read_body_json(list_resp).await;
    let identity_epochs = list_body
        .identities
        .iter()
        .find(|item| item.identity == identity)
        .expect("registered identity should be listed");
    assert_eq!(list_body.role, "calibrator");
    assert_eq!(identity_epochs.epochs[0].epoch, 2);
    assert_eq!(identity_epochs.epochs[0].public_key, rotated_pk);
}

#[actix_web::test]
async fn transactions_revoke_route_marks_epoch_revoked() {
    let (_identity_sk, identity) = keypair(131);
    let (_rotated_sk, rotated_pk) = keypair(132);
    let activated_at = chrono::Utc::now().timestamp();
    let revoked_at = activated_at + 10;
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;

    let register_resp = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/transactions/signing-keys/register")
            .insert_header(bearer(TEST_OPERATOR_TOKEN))
            .set_json(serde_json::json!({
                "identity": identity,
                "role": "validator",
                "epoch": 3,
                "public_key": rotated_pk,
                "activated_at": activated_at
            }))
            .to_request(),
    )
    .await;
    assert_eq!(register_resp.status(), StatusCode::OK);

    let revoke_resp = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/transactions/signing-keys/revoke")
            .insert_header(bearer(TEST_OPERATOR_TOKEN))
            .set_json(serde_json::json!({
                "identity": identity,
                "role": "validator",
                "epoch": 3,
                "revoked_at": revoked_at
            }))
            .to_request(),
    )
    .await;

    assert_eq!(revoke_resp.status(), StatusCode::OK);
    let revoke_body: signing_keys::SigningKeyMutationResponse =
        test::read_body_json(revoke_resp).await;
    assert_eq!(revoke_body.epoch, 3);
    assert_eq!(revoke_body.role, "validator");

    let list_resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/transactions/signing-keys/validator")
            .insert_header(bearer(TEST_OPERATOR_TOKEN))
            .to_request(),
    )
    .await;

    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body: signing_keys::SigningKeyRoleSnapshotResponse =
        test::read_body_json(list_resp).await;
    let identity_epochs = list_body
        .identities
        .iter()
        .find(|item| item.identity == identity)
        .expect("revoked identity should still be listed");
    assert_eq!(identity_epochs.epochs[0].revoked_at, Some(revoked_at));
}

#[actix_web::test]
async fn network_revoke_route_marks_epoch_revoked() {
    let (_identity_sk, identity) = keypair(141);
    let (_rotated_sk, rotated_pk) = keypair(142);
    let activated_at = chrono::Utc::now().timestamp();
    let revoked_at = activated_at + 10;
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;

    let register_resp = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/network/signing-keys/register")
            .insert_header(bearer(TEST_OPERATOR_TOKEN))
            .set_json(serde_json::json!({
                "identity": identity,
                "role": "governance_operator",
                "epoch": 4,
                "public_key": rotated_pk,
                "activated_at": activated_at
            }))
            .to_request(),
    )
    .await;
    assert_eq!(register_resp.status(), StatusCode::OK);

    let revoke_resp = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/network/signing-keys/revoke")
            .insert_header(bearer(TEST_OPERATOR_TOKEN))
            .set_json(serde_json::json!({
                "identity": identity,
                "role": "governance_operator",
                "epoch": 4,
                "revoked_at": revoked_at
            }))
            .to_request(),
    )
    .await;

    assert_eq!(revoke_resp.status(), StatusCode::OK);

    let list_resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/network/signing-keys/governance_operator")
            .insert_header(bearer(TEST_OPERATOR_TOKEN))
            .to_request(),
    )
    .await;

    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body: signing_keys::SigningKeyRoleSnapshotResponse =
        test::read_body_json(list_resp).await;
    let identity_epochs = list_body
        .identities
        .iter()
        .find(|item| item.identity == identity)
        .expect("revoked identity should still be listed");
    assert_eq!(list_body.role, "governance_operator");
    assert_eq!(identity_epochs.epochs[0].revoked_at, Some(revoked_at));
}
