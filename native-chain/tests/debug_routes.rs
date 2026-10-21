#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/helpers.rs"]
mod helpers;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/routes/debug.rs"]
mod debug;
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

#[derive(Debug, Deserialize)]
struct SeedAccountResponse {
    status: String,
    address: String,
    amount: u128,
    stake: u128,
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
        .route("/debug/seed-account", web::post().to(debug::seed_account))
}

#[actix_web::test]
async fn seed_account_requires_operator_on_production_route() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;

    let resp = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/debug/seed-account")
            .set_json(serde_json::json!({
                "address": test_address_string(0x11),
                "amount": 75,
                "stake": 25
            }))
            .to_request(),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_enveloped_error::<SeedAccountResponse>(&body, "UNAUTHORIZED");
}

#[actix_web::test]
async fn seed_account_applies_balance_and_stake_on_production_route() {
    let state = test_state(None);
    let app = test::init_service(test_app(state.clone())).await;
    let address = test_address_string(0x22);

    let resp = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/debug/seed-account")
            .insert_header(bearer(TEST_OPERATOR_TOKEN))
            .set_json(serde_json::json!({
                "address": address,
                "amount": 75,
                "stake": 25
            }))
            .to_request(),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: SeedAccountResponse = test::read_body_json(resp).await;
    assert_eq!(body.status, "success");
    assert_eq!(body.address, test_address_string(0x22));
    assert_eq!(body.amount, 75);
    assert_eq!(body.stake, 25);

    let blockchain = state.blockchain.read().await;
    let account = blockchain
        .state
        .get_account(&test_address(0x22))
        .expect("seeded account should exist");
    assert_eq!(account.balance.into_inner(), 75);
    assert_eq!(account.stake_locked.into_inner(), 25);
    assert_eq!(account.spendable_balance.into_inner(), 50);
}
