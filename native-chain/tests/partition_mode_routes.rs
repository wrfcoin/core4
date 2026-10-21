//! HTTP integration tests for partition-mode gating (#1217).
//!
//! Verifies that the `POST /blocks/mine` handler refuses block production
//! and that `GET /node/partition-status` reports the current mode once
//! the orchestrator has pushed a transition via
//! `POST /operator/partition-mode`.
//!
//! These tests exercise the full HTTP handler path the same way the
//! #1179 operator_mempool tests do, with an in-process actix-web app
//! and a minimal `AppState` built by the shared test harness.

#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/helpers.rs"]
mod helpers;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/routes/mining.rs"]
mod mining;
#[cfg(feature = "p2p")]
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/peer_reputation.rs"]
mod peer_reputation;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/routes/network.rs"]
mod network;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/routes/operator.rs"]
mod operator;
#[cfg(feature = "p2p")]
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/da_challenge_producer.rs"]
mod da_challenge_producer;
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

use actix_web::{http::StatusCode, test, web, App};
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};
use tokio::sync::RwLock;
use wrfcoin_consensus_engines::{ConsensusFactory, ConsensusType, TypedConsensusManager};
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::bridge::safety_manager::BridgeSafetyManager;
use wrfcoin_native_chain::bridge::types::BridgeSafetyConfig;
use wrfcoin_native_chain::config::settings::Config;
use wrfcoin_native_chain::mempool::pool::Mempool;
use wrfcoin_native_chain::security::api_auth::{ApiRole, AuthPrincipal};
use wrfcoin_native_chain::storage::{Storage, DEFAULT_CHECKPOINT_INTERVAL};
use wrfcoin_shared_protocol::consensus::partition_policy::PartitionMode;

const TEST_OPERATOR_TOKEN: &str = "test-operator-token";
const TEST_MINER_ADDRESS: &str = "0x0000000000000000000000000000000000000001";

fn test_token_store() -> Arc<FxHashMap<String, AuthPrincipal>> {
    let mut store = FxHashMap::default();
    store.insert(
        TEST_OPERATOR_TOKEN.to_string(),
        AuthPrincipal::with_role(ApiRole::Operator),
    );
    Arc::new(store)
}

fn test_consensus() -> Arc<TypedConsensusManager<wrfcoin_consensus_engines::PopConsensusAdapter>> {
    let pop_config = ConsensusFactory::default_config(ConsensusType::ProofOfPublishing);
    let pop_adapter = ConsensusFactory::create_pop_adapter(&pop_config);
    Arc::new(TypedConsensusManager::new(pop_adapter, pop_config))
}

fn test_state() -> web::Data<state::AppState> {
    let tmp = std::env::temp_dir().join(format!("wrfcoin-test-{}.redb", fastrand::u64(..)));
    let storage =
        Storage::open(&tmp, DEFAULT_CHECKPOINT_INTERVAL).expect("test storage should open");
    web::Data::new(state::AppState {
        consensus: test_consensus(),
        blockchain: Arc::new(RwLock::new(Blockchain::default())),
        snapshot_service: Arc::new(state::snapshot_service::SnapshotServiceState::default()),
        genesis_hash: state::GenesisHashCell::new(None),
        producer_authority_boot: Arc::new(state::ProducerAuthorityBootStatus::default()),
        p2p_startup: Arc::new(state::P2pStartupStatus::default()),
        #[cfg(feature = "p2p")]
        tx_gossip_backpressure_streak: Arc::new(std::sync::Mutex::new(0)),
        #[cfg(feature = "p2p")]
        p2p_consumer_health: state::P2pConsumerHealth::new(false),
        mempool: Arc::new(RwLock::new(Mempool::new(&Config::default()))),
        snapshot_sync_seen: Arc::new(RwLock::new(FxHashSet::default())),
        snapshot_signing_key: None,
        snapshot_trusted_signers: Arc::new(FxHashSet::default()),
        bridge_safety: Arc::new(RwLock::new(BridgeSafetyManager::new(
            BridgeSafetyConfig::default(),
        ))),
        token_store: test_token_store(),
        legacy_submit_rate_limit: Arc::new(RwLock::new(FxHashMap::default())),
        permissionless_submit_rate_limit: Arc::new(RwLock::new(FxHashMap::default())),
        contribution_commit_rate_limit: Arc::new(RwLock::new(FxHashMap::default())),
        permissionless_replay_seen: Arc::new(RwLock::new(FxHashMap::default())),
        storage: Arc::new(RwLock::new(storage)),
        evidence_store: Arc::new(
            wrfcoin_consensus_engines::equivocation::InMemoryEvidenceStore::new(),
        ),
        failed_transactions_total: Arc::new(AtomicU64::new(0)),
        zk_proofs_per_block_cache: Arc::new(std::sync::Mutex::new(FxHashMap::default())),
        trust_reward_metrics_cache: Arc::new(std::sync::Mutex::new(FxHashMap::default())),
        mining_tx_count_cache: Arc::new(std::sync::Mutex::new(FxHashMap::default())),
        stake_metrics_cache: Arc::new(std::sync::Mutex::new(None)),
        block_production_lock: Arc::new(tokio::sync::Mutex::new(())),
        nft_mutation_api_enabled: true,
        oracle_admin_api_enabled: true,
        debug_seed_api_enabled: false,
        partition_mode: Arc::new(RwLock::new(PartitionMode::default())),
        node_capability: Arc::new(RwLock::new(
            wrfcoin_native_chain::network_failover::NodeCapabilitySnapshot::default(),
        )),
        failover_monitor: Arc::new(RwLock::new(
            wrfcoin_native_chain::network_failover::FailoverMonitor::new(
                wrfcoin_native_chain::network_failover::FailoverConfig::default(),
            ),
        )),
        state_diverged: Arc::new(AtomicBool::new(false)),
        event_tx: websocket::new_event_channel().0,
        finality_signal_tx: wrfcoin_shared_protocol::consensus::finality::finality_signal_channel().0,
        #[cfg(feature = "p2p")]
        p2p: None,
        #[cfg(feature = "p2p")]
        weather_gossip_signing_key: Arc::new(
            wrfcoin_shared_protocol::crypto::signature::PrivateKey::generate(&mut rand::thread_rng()),
        ),
        #[cfg(feature = "p2p")]
        da_challenge_runtime: da_challenge_producer::DaChallengeRuntime::new(),
        #[cfg(feature = "privacy")]
        key_image_registry: Arc::new(wrfcoin_privacy::KeyImageRegistry::new()),
        #[cfg(feature = "privacy")]
        privacy_key_registry: Arc::new(RwLock::new(FxHashMap::default())),
        #[cfg(feature = "zk")]
        zk_verifier: Arc::new(wrfcoin_native_chain::zk_service::ArkworksZkVerifier),
        #[cfg(feature = "zk")]
        zk_prover_gate: Arc::new(tokio::sync::Semaphore::new(1)),
        #[cfg(feature = "execution")]
        execution_registry: Arc::new(wrfcoin_execution_engine::registry::default_registry()),
        #[cfg(feature = "execution")]
        execution_runtime: match wrfcoin_execution_engine::docker::DockerRunner::new() {
            Ok(runner) => Arc::new(runner),
            Err(_) => Arc::new(wrfcoin_execution_engine::docker::NoOpContainerRuntime),
        },
        #[cfg(feature = "execution")]
        execution_results: Arc::new(RwLock::new(FxHashMap::default())),
        #[cfg(feature = "execution")]
        execution_token_quotas: Arc::new(RwLock::new(FxHashMap::default())),
        #[cfg(feature = "execution")]
        execution_quota_config: state::ExecutionQuotaConfig::default(),
        #[cfg(feature = "execution")]
        forecast_task_assignments: Arc::new(RwLock::new(FxHashMap::default())),
        #[cfg(feature = "execution")]
        forecast_node_heartbeats: Arc::new(RwLock::new(FxHashMap::default())),
        #[cfg(feature = "execution")]
        forecast_task_results: Arc::new(RwLock::new(FxHashMap::default())),
    })
}

#[actix_web::test]
async fn partition_status_default_reports_normal() {
    let state = test_state();
    let app = test::init_service(
        App::new()
            .app_data(state.clone())
            .route(
                "/node/partition-status",
                web::get().to(network::get_partition_status),
            ),
    )
    .await;

    let req = test::TestRequest::get()
        .uri("/node/partition-status")
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["mode"], "normal");
    assert_eq!(body["allows_block_production"], true);
    assert_eq!(body["accepts_mempool_writes"], true);
    assert_eq!(body["allows_finality_voting"], true);
}

#[actix_web::test]
async fn partition_status_reflects_operator_push_to_read_only() {
    let state = test_state();
    let app = test::init_service(
        App::new()
            .app_data(state.clone())
            .route(
                "/node/partition-status",
                web::get().to(network::get_partition_status),
            )
            .route(
                "/operator/partition-mode",
                web::post().to(network::set_partition_mode),
            ),
    )
    .await;

    // Operator pushes ReadOnly.
    let set_req = test::TestRequest::post()
        .uri("/operator/partition-mode")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
        .set_json(serde_json::json!({ "mode": "ReadOnly" }))
        .to_request();
    let set_resp = test::call_service(&app, set_req).await;
    assert_eq!(
        set_resp.status(),
        StatusCode::OK,
        "operator push should succeed"
    );

    // Read-back confirms the transition.
    let get_req = test::TestRequest::get()
        .uri("/node/partition-status")
        .to_request();
    let get_resp = test::call_service(&app, get_req).await;
    let body: serde_json::Value = test::read_body_json(get_resp).await;
    assert_eq!(body["mode"], "read_only");
    assert_eq!(body["allows_block_production"], false);
    assert_eq!(body["accepts_mempool_writes"], false);
}

#[actix_web::test]
async fn operator_partition_mode_requires_operator_role() {
    let state = test_state();
    let app = test::init_service(
        App::new()
            .app_data(state.clone())
            .route(
                "/operator/partition-mode",
                web::post().to(network::set_partition_mode),
            ),
    )
    .await;

    // Missing auth header → rejected.
    let req = test::TestRequest::post()
        .uri("/operator/partition-mode")
        .set_json(serde_json::json!({ "mode": "ReadOnly" }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert!(
        resp.status().is_client_error() || resp.status() == StatusCode::UNAUTHORIZED,
        "unauthenticated push must be rejected, got {}",
        resp.status()
    );

    // State must not have flipped.
    let mode = *state.partition_mode.read().await;
    assert_eq!(mode, PartitionMode::Normal, "state must not flip on unauthorized push");
}

#[actix_web::test]
async fn mine_block_returns_503_when_partition_is_read_only() {
    let state = test_state();
    // Directly set ReadOnly to bypass the operator auth dance for this test.
    {
        let mut mode = state.partition_mode.write().await;
        *mode = PartitionMode::ReadOnly;
    }
    {
        let mut mempool = state.mempool.write().await;
        mempool.set_partition_mode(PartitionMode::ReadOnly);
    }

    let app = test::init_service(
        App::new()
            .app_data(state.clone())
            .route("/blocks/mine", web::post().to(mining::mine_block)),
    )
    .await;

    let req = test::TestRequest::post()
        .uri("/blocks/mine")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
        .set_json(serde_json::json!({ "miner_address": TEST_MINER_ADDRESS }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "ReadOnly mode must refuse mining with 503"
    );

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["error"]["code"], "PARTITION_READ_ONLY");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("read_only"),
        "error message should mention read_only mode, got: {body}"
    );
}

#[actix_web::test]
async fn mine_block_also_refused_under_minority_without_explicit_manual_set() {
    // The point of this test is to exercise the PartitionMode default
    // pathway — a freshly-constructed AppState is Normal and mining
    // proceeds. Once the orchestrator pushes ReadOnly via the operator
    // endpoint, subsequent mining is refused. The test is the
    // integration counterpart to the unit tests in
    // `shared-protocol::consensus::partition_policy::tests` and
    // `shared-protocol::mempool::pool::tests`.
    let state = test_state();
    let app = test::init_service(
        App::new()
            .app_data(state.clone())
            .route("/blocks/mine", web::post().to(mining::mine_block))
            .route(
                "/operator/partition-mode",
                web::post().to(network::set_partition_mode),
            ),
    )
    .await;

    // Operator push: ReadOnly.
    let set_req = test::TestRequest::post()
        .uri("/operator/partition-mode")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
        .set_json(serde_json::json!({ "mode": "ReadOnly" }))
        .to_request();
    let _ = test::call_service(&app, set_req).await;

    // Mine attempt must now be refused with 503.
    let mine_req = test::TestRequest::post()
        .uri("/blocks/mine")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
        .set_json(serde_json::json!({ "miner_address": TEST_MINER_ADDRESS }))
        .to_request();
    let mine_resp = test::call_service(&app, mine_req).await;
    assert_eq!(mine_resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[actix_web::test]
async fn partition_mode_middle_allows_block_production_but_refuses_finality() {
    // BlockProductionOnly is the middle-partition case: produce blocks
    // for availability but do not cast finality votes. The HTTP status
    // endpoint should report the correct flags for each capability.
    let state = test_state();
    {
        let mut mode = state.partition_mode.write().await;
        *mode = PartitionMode::BlockProductionOnly;
    }

    let app = test::init_service(
        App::new()
            .app_data(state.clone())
            .route(
                "/node/partition-status",
                web::get().to(network::get_partition_status),
            ),
    )
    .await;

    let req = test::TestRequest::get()
        .uri("/node/partition-status")
        .to_request();
    let resp = test::call_service(&app, req).await;
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["mode"], "block_production_only");
    assert_eq!(body["allows_block_production"], true);
    assert_eq!(body["allows_finality_voting"], false);
    assert_eq!(body["accepts_mempool_writes"], true);
    assert_eq!(body["serves_queries"], true);
}
