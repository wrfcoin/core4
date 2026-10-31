//! `GET /network/producers` and `GET /node/bonds`: read-only listings for the
//! public producers page (backend `/api/network/nodes`).
use super::*;
#[path = "../../../../../shared-protocol/tests/common/node_receipt_fixture.rs"]
mod fixture;

use wrfcoin_shared_protocol::consensus::producer_set::{ProducerMember, ProducerSetChange};

fn reader_get(uri: &str) -> test::TestRequest {
    test::TestRequest::get()
        .uri(uri)
        .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: fixed synthetic test bearer, never a deployed credential
}

fn member(id: &str, byte: u8) -> ProducerMember {
    let key = PrivateKey::from_bytes(&[byte; 32]).unwrap();
    let public = wrfcoin_shared_protocol::crypto::signature::PublicKey::from_private_key(&key);
    ProducerMember {
        producer_id: id.to_string(),
        payout_address: wrfcoin_shared_protocol::common::types::Address::from_public_key(&public)
            .to_string(),
        public_key: public.as_bytes().try_into().unwrap(),
    }
}

#[actix_web::test]
async fn network_producers_lists_active_set_with_bounded_activity() {
    let state = test_state(None);
    let app = test::init_service(test_app(state.clone())).await;

    // Reader auth, like the other node read models.
    let response = test::call_service(
        &app,
        test::TestRequest::get().uri("/network/producers").to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // Empty history: an honest empty set, not a fabricated roster.
    let response = test::call_service(&app, reader_get("/network/producers").to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(response).await;
    assert_eq!(body["producers"], serde_json::json!([]));
    assert!(body["set_epoch"].is_null());
    assert!(body["producer_set_hash"].is_null());

    let (alpha, bravo) = (member("alpha", 41), member("bravo", 42));
    {
        let mut chain = state.blockchain.write().await;
        chain.state.producer_set_history = vec![
            ProducerSetChange::new(0, 0, vec![bravo.clone(), alpha.clone()]),
            // Scheduled far ahead: must not be reported as the active set.
            ProducerSetChange::new(1, 1_000, vec![member("charlie", 43)]),
        ];
        let mut prev = Hash::default();
        for (height, key) in [(1, alpha.public_key), (2, bravo.public_key), (3, alpha.public_key)] {
            let mut block = Block::new(1, prev, Vec::new(), 1, height);
            block.producer_pubkey = key;
            block.set_hash(block.calculate_hash());
            prev = block.hash.unwrap();
            chain.add_block(block).unwrap();
        }
    }

    let response = test::call_service(&app, reader_get("/network/producers").to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(response).await;
    assert_eq!(body["tip_height"], 3);
    assert_eq!(body["authorizes_height"], 4);
    assert_eq!(body["set_epoch"], 0);
    assert_eq!(body["set_effective_height"], 0);
    let producers = body["producers"].as_array().unwrap();
    assert_eq!(producers.len(), 2);
    // Canonical (producer_id) order regardless of history order.
    assert_eq!(producers[0]["producer_id"], "alpha");
    assert_eq!(producers[0]["public_key"], hex::encode(alpha.public_key));
    assert_eq!(producers[0]["payout_address"], alpha.payout_address);
    assert_eq!(producers[0]["last_produced_height"], 3);
    assert_eq!(producers[0]["blocks_in_window"], 2);
    assert_eq!(producers[1]["producer_id"], "bravo");
    assert_eq!(producers[1]["last_produced_height"], 2);
    assert_eq!(producers[1]["blocks_in_window"], 1);
    assert_eq!(body["activity_window"]["blocks_scanned"], 3);
    assert_eq!(body["activity_window"]["from_height"], 1);
    assert_eq!(body["activity_window"]["to_height"], 3);

    // A narrow window only sees the newest block.
    let response = test::call_service(&app, reader_get("/network/producers?window=1").to_request()).await;
    let body: serde_json::Value = test::read_body_json(response).await;
    assert_eq!(body["activity_window"]["blocks_scanned"], 1);
    assert_eq!(body["producers"][0]["blocks_in_window"], 1);
    assert!(body["producers"][1]["last_produced_height"].is_null());
    assert_eq!(body["producers"][1]["blocks_in_window"], 0);

    // Bounds: 0 clamps up to 1, huge clamps down to the cap.
    let response = test::call_service(&app, reader_get("/network/producers?window=0").to_request()).await;
    let body: serde_json::Value = test::read_body_json(response).await;
    assert_eq!(body["activity_window"]["requested"], 1);
    let response =
        test::call_service(&app, reader_get("/network/producers?window=99999999").to_request()).await;
    let body: serde_json::Value = test::read_body_json(response).await;
    assert_eq!(
        body["activity_window"]["requested"],
        crate::routes::network::MAX_PRODUCER_ACTIVITY_WINDOW
    );

    // A malformed window is refused, not panicked on.
    let response = test::call_service(&app, reader_get("/network/producers?window=abc").to_request()).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
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
