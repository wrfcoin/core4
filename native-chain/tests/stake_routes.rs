#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/helpers.rs"]
mod helpers;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/routes/stake.rs"]
mod stake;
// stake.rs serializes monetary fields via `crate::routes::decimal` (#2680);
// mount that module at the same path in this test crate root.
#[allow(dead_code)]
#[path = "../src/bin/launch_api/routes"]
mod routes {
    pub mod decimal;
}
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
use wrfcoin_native_chain::common::types::Balance;

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
            "/transactions/stake/metrics",
            web::get().to(stake::get_stake_metrics),
        )
        .route(
            "/transactions/stake/{address}",
            web::get().to(stake::get_stake_status),
        )
}

#[actix_web::test]
async fn stake_status_rejects_invalid_address_on_production_route() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;

    let resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/transactions/stake/not-an-address")
            .insert_header(bearer(TEST_READER_TOKEN))
            .to_request(),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_enveloped_error::<stake::StakeStatusResponse>(&body, "BAD_REQUEST");
}

/// Denial-path control for the Reader-gated stake endpoints (#1986
/// hardening): pins that neither the per-address status nor the metrics
/// endpoint is reachable unauthenticated.
#[actix_web::test]
async fn stake_status_and_metrics_require_auth() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;

    for uri in [
        "/transactions/stake/0x3333333333333333333333333333333333333333",
        "/transactions/stake/metrics",
    ] {
        let resp = test::call_service(
            &app,
            test::TestRequest::get().uri(uri).to_request(),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "{uri} must reject unauthenticated requests"
        );
    }
}

#[actix_web::test]
async fn stake_routes_return_state_backed_fields_and_consistent_metrics() {
    let state = test_state(None);
    let primary = test_address(0x33);
    let secondary = test_address(0x44);

    let expected_metrics = {
        let mut blockchain = state.blockchain.write().await;
        blockchain.state.create_account(primary, Balance::new(500));
        blockchain.state.create_account(secondary, Balance::new(250));

        if let Some(account) = blockchain.state.accounts.get_mut(&primary) {
            account.stake_required = Balance::new(200);
            account.stake_locked = Balance::new(150);
            account.spendable_balance = Balance::new(350);
            account.earned_balance_total = Balance::new(42);
        }
        if let Some(account) = blockchain.state.accounts.get_mut(&secondary) {
            account.stake_required = Balance::new(100);
            account.stake_locked = Balance::new(20);
            account.spendable_balance = Balance::new(230);
            account.earned_balance_total = Balance::new(7);
        }

        (
            blockchain.state.accounts.len() as u64,
            300u64,
            170u64,
            580u64,
            49u64,
        )
    };

    let app = test::init_service(test_app(state)).await;

    let stake_resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/transactions/stake/0x3333333333333333333333333333333333333333")
            .insert_header(bearer(TEST_READER_TOKEN))
            .to_request(),
    )
    .await;

    assert_eq!(stake_resp.status(), StatusCode::OK);
    // Monetary fields serialize as decimal STRINGS (#2680, routes::decimal),
    // so the response no longer round-trips through the struct's derived
    // Deserialize — assert on the JSON wire form instead.
    let stake_body: serde_json::Value = test::read_body_json(stake_resp).await;
    assert_eq!(
        stake_body.get("address").and_then(|v| v.as_str()),
        Some("0x3333333333333333333333333333333333333333")
    );
    assert_eq!(stake_body.get("stake_required").and_then(|v| v.as_str()), Some("200"));
    assert_eq!(stake_body.get("stake_locked").and_then(|v| v.as_str()), Some("150"));
    assert_eq!(stake_body.get("spendable_balance").and_then(|v| v.as_str()), Some("350"));
    assert_eq!(stake_body.get("earned_balance_total").and_then(|v| v.as_str()), Some("42"));

    let metrics_resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/transactions/stake/metrics")
            .insert_header(bearer(TEST_READER_TOKEN))
            .to_request(),
    )
    .await;

    assert_eq!(metrics_resp.status(), StatusCode::OK);
    let metrics_body: serde_json::Value = test::read_body_json(metrics_resp).await;
    assert_eq!(
        metrics_body.get("account_count").and_then(|v| v.as_u64()),
        Some(expected_metrics.0)
    );
    assert_eq!(
        metrics_body.get("total_stake_required").and_then(|v| v.as_str()),
        Some(expected_metrics.1.to_string().as_str())
    );
    assert_eq!(
        metrics_body.get("total_stake_locked").and_then(|v| v.as_str()),
        Some(expected_metrics.2.to_string().as_str())
    );
    assert_eq!(
        metrics_body.get("total_spendable_balance").and_then(|v| v.as_str()),
        Some(expected_metrics.3.to_string().as_str())
    );
    assert_eq!(
        metrics_body.get("total_earned_balance").and_then(|v| v.as_str()),
        Some(expected_metrics.4.to_string().as_str())
    );
}
