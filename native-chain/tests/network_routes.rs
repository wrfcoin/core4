#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/helpers.rs"]
mod helpers;
#[cfg(feature = "p2p")]
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/peer_reputation.rs"]
mod peer_reputation;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/routes/network.rs"]
mod network;
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
use wrfcoin_native_chain::blockchain::state::{AuditEventKind, AuditEventRecord, StateTransition};
use wrfcoin_native_chain::blockchain::transaction::{
    Transaction, TransactionInput, TransactionOutput,
};
use wrfcoin_native_chain::crypto::hash::hash_string;
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
        .route("/network/audit-log", web::get().to(network::get_network_audit_log))
        .route("/network/info", web::get().to(network::get_network_info))
        .route("/node/sync/status", web::get().to(network::get_node_sync_status))
        .route("/node/resources", web::get().to(network::get_node_resources))
        .route("/node/partition-status", web::get().to(network::get_partition_status))
        .route(
            "/operator/partition-mode",
            web::post().to(network::set_partition_mode),
        )
}

#[cfg(feature = "p2p")]
fn test_app_with_reputation(
    state: web::Data<state::AppState>,
    reputation: web::Data<peer_reputation::SharedPeerReputation>,
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
        .app_data(reputation)
        .route(
            "/network/peers/reputation",
            web::get().to(network::get_network_peer_reputation),
        )
}

fn build_transaction(tag: &str) -> Transaction {
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_string(&format!("network-route-input-{tag}")),
            vout: 0,
            signature: None,
            public_key: None,
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            amount: 1,
            address: format!("wrf-network-{tag}"),
            script: "OP_TRUE".to_string(),
            nft_data: None,
        }],
    );
    tx.fee = 1;
    tx.set_id();
    tx
}

#[actix_web::test]
async fn network_audit_log_requires_operator_on_production_route() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;

    let resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/network/audit-log?kind=governance_proposal_executed")
            .to_request(),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_enveloped_error::<network::AuditLogResponse>(&body, "UNAUTHORIZED");
}

#[actix_web::test]
async fn network_audit_log_returns_filtered_records_on_production_route() {
    let state = test_state(None);
    {
        let mut blockchain = state.blockchain.write().await;
        blockchain
            .apply_state_transition(StateTransition::RecordAuditEvent {
                event: AuditEventRecord {
                    event_id: 0,
                    block_height: 9,
                    timestamp: 1_700_000_001,
                    kind: AuditEventKind::BridgeSafetyHalt,
                    subject_id: Some("bridge-1".to_string()),
                    actor_id: Some("operator-a".to_string()),
                    details: Some(serde_json::json!({ "reason": "manual" })),
                },
            })
            .expect("first audit event should be recorded");
        blockchain
            .apply_state_transition(StateTransition::RecordAuditEvent {
                event: AuditEventRecord {
                    event_id: 0,
                    block_height: 10,
                    timestamp: 1_700_000_002,
                    kind: AuditEventKind::GovernanceProposalExecuted,
                    subject_id: Some("prop-1".to_string()),
                    actor_id: Some("operator-a".to_string()),
                    details: Some(serde_json::json!({ "yes_votes": 5 })),
                },
            })
            .expect("second audit event should be recorded");
    }
    let app = test::init_service(test_app(state)).await;

    let resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/network/audit-log?kind=governance_proposal_executed")
            .insert_header(bearer(TEST_OPERATOR_TOKEN))
            .to_request(),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: network::AuditLogResponse = test::read_body_json(resp).await;
    assert_eq!(body.total, 1);
    assert_eq!(body.items[0].event_id, 2);
    assert_eq!(body.items[0].subject_id.as_deref(), Some("prop-1"));
}

#[actix_web::test]
async fn network_info_uses_production_route() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;

    let resp = test::call_service(
        &app,
        test::TestRequest::get().uri("/network/info").to_request(),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["chain_height"], 0);
    assert_eq!(body["sync_status"], "genesis");
    assert!(body["node_version"].as_str().is_some());
}

#[actix_web::test]
async fn node_sync_status_uses_production_route() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;

    let resp = test::call_service(
        &app,
        test::TestRequest::get().uri("/node/sync/status").to_request(),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["sync_status"], "genesis");
    assert_eq!(body["chain_height"], 0);
    assert_eq!(body["blocks_in_memory"], 0);
}

#[actix_web::test]
async fn node_resources_uses_production_route() {
    let state = test_state(None);
    {
        let mut mempool = state.mempool.write().await;
        mempool
            .add_transaction(build_transaction("resources"))
            .expect("test transaction should seed the mempool");
    }
    let app = test::init_service(test_app(state)).await;

    let resp = test::call_service(
        &app,
        test::TestRequest::get().uri("/node/resources").to_request(),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["blocks_in_memory"], 0);
    assert_eq!(body["pending_transactions"], 1);
    assert!(body["uptime_secs"].as_u64().is_some());
}

#[actix_web::test]
async fn operator_partition_mode_requires_operator_on_production_route() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;

    let resp = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/operator/partition-mode")
            .set_json(serde_json::json!({ "mode": "ReadOnly" }))
            .to_request(),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["error"]["code"], "UNAUTHORIZED");
}

#[actix_web::test]
async fn node_partition_status_and_operator_partition_mode_use_production_routes() {
    let state = test_state(None);
    let app = test::init_service(test_app(state.clone())).await;

    let set_resp = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/operator/partition-mode")
            .insert_header(bearer(TEST_OPERATOR_TOKEN))
            .set_json(serde_json::json!({ "mode": "ReadOnly" }))
            .to_request(),
    )
    .await;

    assert_eq!(set_resp.status(), StatusCode::OK);
    let set_body: serde_json::Value = test::read_body_json(set_resp).await;
    assert_eq!(set_body["mode"], "read_only");
    assert_eq!(set_body["allows_block_production"], false);
    assert_eq!(set_body["accepts_mempool_writes"], false);
    assert_eq!(*state.partition_mode.read().await, PartitionMode::ReadOnly);

    let get_resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/node/partition-status")
            .to_request(),
    )
    .await;

    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_body: serde_json::Value = test::read_body_json(get_resp).await;
    assert_eq!(get_body["mode"], "read_only");
    assert_eq!(get_body["allows_block_production"], false);
    assert_eq!(get_body["allows_finality_voting"], false);
}

#[cfg(feature = "p2p")]
#[actix_web::test]
async fn peer_reputation_requires_operator_token() {
    use peer_reputation::{PeerReputationTracker, apply_peer_penalty};
    use libp2p::PeerId;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    let tracker = Arc::new(RwLock::new(PeerReputationTracker::new()));
    {
        let mut guard = tracker.write().await;
        let _ = apply_peer_penalty(&mut guard, &PeerId::random(), "peer_banned", true);
    }
    let reputation = web::Data::new(tracker);
    let state = test_state(None);
    let app = test::init_service(test_app_with_reputation(state, reputation)).await;

    let unauth_resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/network/peers/reputation")
            .to_request(),
    )
    .await;
    assert_eq!(unauth_resp.status(), StatusCode::UNAUTHORIZED);

    let reader_resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/network/peers/reputation")
            .insert_header(bearer(TEST_READER_TOKEN))
            .to_request(),
    )
    .await;
    assert_eq!(reader_resp.status(), StatusCode::FORBIDDEN);
}

#[cfg(feature = "p2p")]
#[actix_web::test]
async fn peer_reputation_operator_returns_snapshot_worst_first() {
    use peer_reputation::{PeerReputationTracker, apply_peer_penalty};
    use libp2p::PeerId;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    let tracker = Arc::new(RwLock::new(PeerReputationTracker::new()));
    let banned = PeerId::random();
    let mild = PeerId::random();
    {
        let mut guard = tracker.write().await;
        let _ = apply_peer_penalty(&mut guard, &banned, "peer_banned", true);
        let _ = apply_peer_penalty(&mut guard, &mild, "invalid_message", false);
    }
    let reputation = web::Data::new(tracker);
    let state = test_state(None);
    let app = test::init_service(test_app_with_reputation(state, reputation)).await;

    let resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/network/peers/reputation")
            .insert_header(bearer(TEST_OPERATOR_TOKEN))
            .to_request(),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["total"], 2);
    assert_eq!(body["graylisted"], 1);
    let peers = body["peers"].as_array().expect("peers array present");
    assert_eq!(peers.len(), 2);
    assert_eq!(peers[0]["peer_id"], banned.to_string());
    assert_eq!(peers[0]["graylisted"], true);
    assert_eq!(peers[0]["ban_count"], 1);
    assert_eq!(peers[1]["peer_id"], mild.to_string());
    assert_eq!(peers[1]["graylisted"], false);
}
