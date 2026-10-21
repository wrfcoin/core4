#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/helpers.rs"]
mod helpers;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/routes/mining.rs"]
mod mining;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/state.rs"]
mod state;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/transition_extractor.rs"]
mod transition_extractor;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/rate_limit.rs"]
mod rate_limit;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/websocket.rs"]
mod websocket;

include!("support/launch_api_test_support.inc.rs");

use actix_web::{App, http::StatusCode, test, web};
use wrfcoin_native_chain::blockchain::block::Block;
use wrfcoin_native_chain::blockchain::transaction::Transaction;
use wrfcoin_shared_protocol::consensus::partition_policy::PartitionMode;

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
        .route("/blocks/mine", web::post().to(mining::mine_block))
        .route("/mining/stats", web::get().to(mining::get_mining_stats))
}

fn seeded_block(height: u64, prev_hash: wrfcoin_native_chain::crypto::hash::Hash) -> Block {
    let mut block = Block::new(
        1,
        prev_hash,
        vec![Transaction::new_coinbase(
            &test_address_string(height as u8),
            50,
            height,
        )],
        1,
        height,
    );
    block.metadata.insert(
        "rewards_minted_count".to_string(),
        "1".to_string(),
    );
    let hash = block.calculate_hash();
    block.set_hash(hash);
    block
}

#[actix_web::test]
async fn mine_block_requires_operator_on_production_route() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;

    let resp = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/blocks/mine")
            .set_json(serde_json::json!({
                "miner_address": test_address_string(0x61)
            }))
            .to_request(),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_enveloped_error::<mining::MineBlockResponse>(&body, "UNAUTHORIZED");
}

#[actix_web::test]
async fn mine_block_rejects_invalid_miner_address_on_production_route() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;

    let resp = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/blocks/mine")
            .insert_header(bearer(TEST_OPERATOR_TOKEN))
            .set_json(serde_json::json!({
                "miner_address": "not-an-address"
            }))
            .to_request(),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_enveloped_error::<mining::MineBlockResponse>(&body, "BAD_REQUEST");
}

#[actix_web::test]
async fn mine_block_returns_partition_read_only_error_on_production_route() {
    let state = test_state(None);
    {
        let mut mode = state.partition_mode.write().await;
        *mode = PartitionMode::ReadOnly;
    }
    {
        let mut mempool = state.mempool.write().await;
        mempool.set_partition_mode(PartitionMode::ReadOnly);
    }
    let app = test::init_service(test_app(state)).await;

    let resp = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/blocks/mine")
            .insert_header(bearer(TEST_OPERATOR_TOKEN))
            .set_json(serde_json::json!({
                "miner_address": test_address_string(0x62)
            }))
            .to_request(),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_enveloped_error::<mining::MineBlockResponse>(&body, "PARTITION_READ_ONLY");
}

#[actix_web::test]
async fn mining_stats_uses_production_route() {
    let state = test_state(None);
    {
        let mut blockchain = state.blockchain.write().await;
        let block_1 = seeded_block(1, [0u8; 32].into());
        let hash_1 = block_1.hash.expect("block 1 hash should be set");
        blockchain
            .add_block(block_1)
            .expect("first block should be added");

        let block_2 = seeded_block(2, hash_1);
        blockchain
            .add_block(block_2)
            .expect("second block should be added");
    }
    let app = test::init_service(test_app(state)).await;

    let resp = test::call_service(
        &app,
        test::TestRequest::get().uri("/mining/stats").to_request(),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["chain_height"], 2);
    assert_eq!(body["blocks_in_memory"], 2);
    assert_eq!(body["total_transactions"], 2);
    assert_eq!(body["difficulty"], 1);
}

/// #3365: a chain-777 block-1 cut with no faucet address is an operator/cut
/// input error (#3361 option B), the same class as the oracle-set refusal. It
/// must reach the operator as 422 `BLOCK_ONE_NO_FAUCET_ADDRESS`, not as the
/// #2138 redacted 500 ("node fault, retry") — a retry produces the same refusal.
#[actix_web::test]
async fn mine_block_returns_422_for_block_one_without_faucet_address_3365() {
    const CHILD: &str = "WRFCOIN_MINE_NO_FAUCET_CHILD_3365";
    // WRFCOIN_CHAIN_ID is process-global; run the chain-777 case in a child so
    // it cannot perturb the other tests in this binary.
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "mine_block_returns_422_for_block_one_without_faucet_address_3365",
            ])
            .env(CHILD, "1")
            .env("WRFCOIN_CHAIN_ID", "777")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}\n{stderr}");
        assert!(stdout.contains("1 passed"), "{stdout}\n{stderr}");
        return;
    }

    let state = test_state(None);
    assert_eq!(state.blockchain.read().await.height, 0, "fixture must cut block 1");
    let app = test::init_service(test_app(state)).await;

    let resp = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/blocks/mine")
            .insert_header(bearer(TEST_OPERATOR_TOKEN))
            .set_json(serde_json::json!({
                "miner_address": test_address_string(0x63)
            }))
            .to_request(),
    )
    .await;

    let status = resp.status();
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "body: {body}");
    assert_enveloped_error::<mining::MineBlockResponse>(&body, "BLOCK_ONE_NO_FAUCET_ADDRESS");
    // The actionable reason reaches the operator, not just the node log.
    assert!(
        body.to_string().contains("WRFCOIN_GENESIS_FAUCET_ADDRESS"),
        "the 422 must carry the actionable reason: {body}"
    );
}
