//! `GET /node/bonds`: read-only bond listing for the public producers page
//! (backend `/api/network/nodes`). The `/network/producers` test lives in
//! main.rs's test module because it appends blocks with `.add_block(`, which
//! `schedule_extension_node_paths_3277` reserves for reviewed node paths
//! outside test modules.
use super::*;
#[path = "../../../../../shared-protocol/tests/common/node_receipt_fixture.rs"]
mod fixture;

pub(super) fn reader_get(uri: &str) -> test::TestRequest {
    test::TestRequest::get()
        .uri(uri)
        .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: fixed synthetic test bearer, never a deployed credential
}

#[actix_web::test]
async fn node_bonds_list_is_paginated_bounded_and_reader_gated() {
    const NAME: &str =
        "tests::producers_bonds_list::node_bonds_list_is_paginated_bounded_and_reader_gated";
    if crate::helpers::test_env::isolated_chain_domain(
        NAME,
        "777",
        "testnet",
        "0x2727272727272727272727272727272727272727",
        "none",
    ) {
        return;
    }
    let _lock = crate::helpers::test_env::lock();
    let state = test_state(None);
    let app = test::init_service(test_app(state.clone())).await;
    // Feature unscheduled: same 404 as the guarded `/node/bonds/{id}`.
    let response = test::call_service(&app, reader_get("/node/bonds").to_request()).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let bonded = fixture::state(); // two deposits: ids [1;32] and [2;32]
    {
        let mut chain = state.blockchain.write().await;
        chain.state = bonded.clone();
        chain.height = fixture::H;
    }
    let response = test::call_service(
        &app,
        test::TestRequest::get().uri("/node/bonds").to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = test::call_service(&app, reader_get("/node/bonds").to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(response).await;
    assert_eq!(body["total"], 2);
    assert_eq!(body["returned"], 2);
    assert!(body["next_after"].is_null());
    let first = &body["bonds"][0];
    assert_eq!(first["deployment_id"], hex::encode([1u8; 32]));
    assert_eq!(first["status"], "REGISTERED");
    assert_eq!(first["principal_base_units"], "25000000000");
    assert_eq!(
        first["payout_account"],
        fixture::bonds::address(10).to_string()
    );
    assert_eq!(body["bonds"][1]["deployment_id"], hex::encode([2u8; 32]));

    // Page 1 of 1-row pages carries a cursor; page 2 ends the walk.
    let response = test::call_service(&app, reader_get("/node/bonds?limit=1").to_request()).await;
    let body: serde_json::Value = test::read_body_json(response).await;
    assert_eq!(body["returned"], 1);
    assert_eq!(body["next_after"], hex::encode([1u8; 32]));
    let next = format!("/node/bonds?limit=1&after={}", hex::encode([1u8; 32]));
    let response = test::call_service(&app, reader_get(&next).to_request()).await;
    let body: serde_json::Value = test::read_body_json(response).await;
    assert_eq!(body["returned"], 1);
    assert_eq!(body["bonds"][0]["deployment_id"], hex::encode([2u8; 32]));
    assert!(body["next_after"].is_null());

    // Bounds and malformed cursor.
    let response = test::call_service(&app, reader_get("/node/bonds?limit=0").to_request()).await;
    let body: serde_json::Value = test::read_body_json(response).await;
    assert_eq!(body["limit"], 1);
    let response = test::call_service(&app, reader_get("/node/bonds?limit=100000").to_request()).await;
    let body: serde_json::Value = test::read_body_json(response).await;
    assert_eq!(body["limit"], crate::routes::node_bonds::MAX_NODE_BOND_PAGE);
    let response = test::call_service(&app, reader_get("/node/bonds?after=zz").to_request()).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // Empty registry: an empty page, not an error.
    {
        let mut chain = state.blockchain.write().await;
        if let Some(registry) = chain.state.node_bond_registry.as_mut() {
            registry.registrations.clear();
        }
    }
    let response = test::call_service(&app, reader_get("/node/bonds").to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(response).await;
    assert_eq!(body["total"], 0);
    assert_eq!(body["bonds"], serde_json::json!([]));
    assert!(body["next_after"].is_null());
}
