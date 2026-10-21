#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/helpers.rs"]
mod helpers;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/routes/bridge.rs"]
mod bridge;
#[cfg(feature = "p2p")]
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/da_challenge_producer.rs"]
mod da_challenge_producer;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/state.rs"]
mod state;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/rate_limit.rs"]
mod rate_limit;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/websocket.rs"]
mod websocket;

use actix_web::{App, http::StatusCode, test, web};
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};
use tokio::sync::RwLock;
use wrfcoin_consensus_engines::{ConsensusFactory, ConsensusType, TypedConsensusManager};
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::bridge::safety_manager::BridgeSafetyManager;
use wrfcoin_native_chain::bridge::types::BridgeSafetyConfig;
use wrfcoin_native_chain::config::settings::Config;
use wrfcoin_native_chain::crypto::signature::PrivateKey;
use wrfcoin_native_chain::mempool::pool::Mempool;
use wrfcoin_native_chain::security::api_auth::{ApiRole, AuthPrincipal};
use wrfcoin_native_chain::storage::{DEFAULT_CHECKPOINT_INTERVAL, Storage};
use wrfcoin_shared_protocol::crypto::signer::{BlockSigner, EnvKeySigner};

const TEST_OPERATOR_TOKEN: &str = "test-operator-token";
const TEST_CONTRIBUTOR_TOKEN: &str = "test-contributor-token";
const TEST_READER_TOKEN: &str = "test-reader-token";

fn test_token_store() -> Arc<FxHashMap<String, AuthPrincipal>> {
    let mut store = FxHashMap::default();
    store.insert(
        TEST_OPERATOR_TOKEN.to_string(),
        AuthPrincipal::with_role(ApiRole::Operator),
    );
    store.insert(
        TEST_CONTRIBUTOR_TOKEN.to_string(),
        AuthPrincipal::with_role(ApiRole::Contributor),
    );
    store.insert(
        TEST_READER_TOKEN.to_string(),
        AuthPrincipal::with_role(ApiRole::Reader),
    );
    Arc::new(store)
}

fn test_consensus() -> Arc<TypedConsensusManager<wrfcoin_consensus_engines::PopConsensusAdapter>> {
    let pop_config = ConsensusFactory::default_config(ConsensusType::ProofOfPublishing);
    let pop_adapter = ConsensusFactory::create_pop_adapter(&pop_config);
    Arc::new(TypedConsensusManager::new(pop_adapter, pop_config))
}

fn test_state(snapshot_signing_key: Option<PrivateKey>) -> web::Data<state::AppState> {
    let tmp = std::env::temp_dir().join(format!("wrfcoin-test-{}.redb", fastrand::u64(..)));
    let storage =
        Storage::open(&tmp, DEFAULT_CHECKPOINT_INTERVAL).expect("test storage should open");
    let (finality_signal_tx, _rx) =
        wrfcoin_shared_protocol::consensus::finality::finality_signal_channel();
    let snapshot_signing_key: Option<Arc<dyn BlockSigner>> = snapshot_signing_key
        .map(|pk| Arc::new(EnvKeySigner::from_private_key(pk, "test")) as Arc<dyn BlockSigner>);
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
        snapshot_signing_key,
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
        partition_mode: Arc::new(RwLock::new(
            wrfcoin_shared_protocol::consensus::partition_policy::PartitionMode::default(),
        )),
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
        finality_signal_tx,
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
    App::new().app_data(state).service(
        web::scope("/bridge")
            .route("/safety/status", web::get().to(bridge::get_bridge_safety_status))
            .route("/safety/halt", web::post().to(bridge::halt_bridge))
            .route("/safety/resume", web::post().to(bridge::resume_bridge))
            .route("/safety/exposure", web::get().to(bridge::get_bridge_exposure))
            .route("/safety/audit-log", web::get().to(bridge::get_bridge_audit_log))
            .route("/challenge/open", web::post().to(bridge::open_bridge_challenge))
            .route("/challenge/submit", web::post().to(bridge::submit_bridge_challenge))
            .route("/challenge/resolve", web::post().to(bridge::resolve_bridge_challenge))
            .route("/caps/update", web::post().to(bridge::update_bridge_caps))
            .route("/scan-stuck", web::post().to(bridge::scan_bridge_stuck)),
    )
}

// ============================================================================
// get_bridge_safety_status (Reader since #1986 closed the public-exposure gap)
// ============================================================================

#[actix_web::test]
async fn get_safety_status_requires_auth() {
    // The NEGATIVE control for #1986's Reader gate: no Authorization header,
    // so a regression that makes this endpoint public again fails here. This
    // request deliberately carries no token — 9027d2ccc once "authenticated"
    // it mechanically, which turned the file's only unauthenticated denial
    // control into a contradiction of the Reader-positive test below (#2868).
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::get()
        .uri("/bridge/safety/status")
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[actix_web::test]
async fn get_safety_status_reader_token_allowed() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::get()
        .uri("/bridge/safety/status")
        .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert!(body.is_object());
}

// ============================================================================
// halt_bridge (Operator)
// ============================================================================

#[actix_web::test]
async fn halt_bridge_requires_operator() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/bridge/safety/halt")
        .set_json(serde_json::json!({
            "reason": "test halt"
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[actix_web::test]
async fn halt_bridge_validation_empty_reason() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/bridge/safety/halt")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
        .set_json(serde_json::json!({
            "reason": ""
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[actix_web::test]
async fn halt_bridge_happy_path() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/bridge/safety/halt")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
        .set_json(serde_json::json!({
            "reason": "emergency halt"
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert!(body.is_object());
}

// ============================================================================
// resume_bridge (Operator)
// ============================================================================

#[actix_web::test]
async fn resume_bridge_requires_operator() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/bridge/safety/resume")
        .set_json(serde_json::json!({
            "reason": "resume",
            "operator": "ops"
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[actix_web::test]
async fn resume_bridge_validation_empty_reason() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/bridge/safety/resume")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
        .set_json(serde_json::json!({
            "reason": "",
            "operator": "ops"
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[actix_web::test]
async fn resume_bridge_happy_path() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    // First halt, then resume
    let halt_req = test::TestRequest::post()
        .uri("/bridge/safety/halt")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
        .set_json(serde_json::json!({"reason": "test halt"}))
        .to_request();
    let _halt_resp = test::call_service(&app, halt_req).await;

    let resume_req = test::TestRequest::post()
        .uri("/bridge/safety/resume")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
        .set_json(serde_json::json!({
            "reason": "halt cleared",
            "operator": "emergency-ops"
        }))
        .to_request();
    let resp = test::call_service(&app, resume_req).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

// ============================================================================
// open_bridge_challenge (Operator)
// ============================================================================

#[actix_web::test]
async fn open_challenge_requires_operator() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/bridge/challenge/open")
        .set_json(serde_json::json!({
            "tx_id": "tx-001",
            "chain_id": "ethereum"
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[actix_web::test]
async fn open_challenge_validation_empty_tx_id() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/bridge/challenge/open")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
        .set_json(serde_json::json!({
            "tx_id": "",
            "chain_id": "ethereum"
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[actix_web::test]
async fn open_challenge_happy_path() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/bridge/challenge/open")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
        .set_json(serde_json::json!({
            "tx_id": "tx-abc123",
            "chain_id": "ethereum"
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
}

// ============================================================================
// submit_bridge_challenge (Contributor)
// ============================================================================

#[actix_web::test]
async fn submit_challenge_requires_contributor() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/bridge/challenge/submit")
        .set_json(serde_json::json!({
            "tx_id": "tx-001",
            "challenger_id": "challenger",
            "reason": "suspicious tx"
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[actix_web::test]
async fn submit_challenge_validation_empty_tx_id() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/bridge/challenge/submit")
        .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
        .set_json(serde_json::json!({
            "tx_id": "",
            "challenger_id": "challenger",
            "reason": "suspicious"
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[actix_web::test]
async fn submit_challenge_happy_path() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    // First open a challenge
    let open_req = test::TestRequest::post()
        .uri("/bridge/challenge/open")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
        .set_json(serde_json::json!({
            "tx_id": "tx-to-challenge",
            "chain_id": "ethereum"
        }))
        .to_request();
    let _open_resp = test::call_service(&app, open_req).await;
    // Then submit a challenge
    let req = test::TestRequest::post()
        .uri("/bridge/challenge/submit")
        .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
        .set_json(serde_json::json!({
            "tx_id": "tx-to-challenge",
            "challenger_id": "validator-1",
            "reason": "invalid proof"
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_ne!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ============================================================================
// resolve_bridge_challenge (Operator)
// ============================================================================

#[actix_web::test]
async fn resolve_challenge_requires_operator() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/bridge/challenge/resolve")
        .set_json(serde_json::json!({
            "tx_id": "tx-001",
            "upheld": true,
            "note": "challenge upheld"
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[actix_web::test]
async fn resolve_challenge_validation_empty_tx_id() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/bridge/challenge/resolve")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
        .set_json(serde_json::json!({
            "tx_id": "",
            "upheld": false,
            "note": "invalid"
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[actix_web::test]
async fn resolve_challenge_happy_path() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/bridge/challenge/resolve")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
        .set_json(serde_json::json!({
            "tx_id": "tx-challenge",
            "upheld": true,
            "note": "proof verified"
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_ne!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ============================================================================
// update_bridge_caps (Operator)
// ============================================================================

#[actix_web::test]
async fn update_caps_requires_operator() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/bridge/caps/update")
        .set_json(serde_json::json!({
            "per_chain_cap": 1000000,
            "global_cap": 10000000,
            "updated_by": "admin"
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[actix_web::test]
async fn update_caps_validation_zero_cap() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/bridge/caps/update")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
        .set_json(serde_json::json!({
            "per_chain_cap": 0,
            "global_cap": 10000000,
            "updated_by": "admin"
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[actix_web::test]
async fn update_caps_happy_path() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/bridge/caps/update")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
        .set_json(serde_json::json!({
            "per_chain_cap": 5000000,
            "global_cap": 50000000,
            "updated_by": "ops-team"
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert!(body.is_object());
}

// ============================================================================
// get_bridge_exposure (Reader)
// ============================================================================

#[actix_web::test]
async fn get_exposure_requires_auth() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::get()
        .uri("/bridge/safety/exposure")
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[actix_web::test]
async fn get_exposure_reader_token_allowed() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::get()
        .uri("/bridge/safety/exposure")
        .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert!(body.is_object());
}

// ============================================================================
// get_bridge_audit_log (Reader)
// ============================================================================

#[actix_web::test]
async fn get_audit_log_requires_auth() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::get()
        .uri("/bridge/safety/audit-log")
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[actix_web::test]
async fn get_audit_log_reader_token_allowed() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::get()
        .uri("/bridge/safety/audit-log")
        .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert!(body.is_array());
}

// ============================================================================
// scan_bridge_stuck (Operator)
// ============================================================================

#[actix_web::test]
async fn scan_stuck_requires_operator() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/bridge/scan-stuck")
        .set_json(serde_json::json!({
            "transactions": []
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[actix_web::test]
async fn scan_stuck_validation_exceeds_max_size() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    // Create 257 transactions (exceeds MAX_BRIDGE_SCAN_SIZE of 256)
    let transactions: Vec<serde_json::Value> = (0..257)
        .map(|i| serde_json::json!({
            "tx_id": format!("tx-{}", i),
            "status": "pending",
            "timestamp": 0i64
        }))
        .collect();
    let req = test::TestRequest::post()
        .uri("/bridge/scan-stuck")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
        .set_json(serde_json::json!({
            "transactions": transactions
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[actix_web::test]
async fn scan_stuck_happy_path() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/bridge/scan-stuck")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
        .set_json(serde_json::json!({
            "transactions": [
                {
                    "tx_id": "tx-stuck-1",
                    "status": "pending",
                    "timestamp": 0i64
                }
            ]
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert!(body.is_array());
}
