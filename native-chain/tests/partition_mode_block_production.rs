//! HTTP integration tests for partition-mode block production guard (#1217).
//!
//! This is the **production-side** counterpart to
//! `partition_mode_routes.rs`. Where that file pins the HTTP endpoint
//! contract (the `/node/partition-status` read and the
//! `/operator/partition-mode` push), this file pins the invariant that
//! ACTUAL BLOCK PRODUCTION is refused end-to-end when the node sits in
//! `PartitionMode::ReadOnly`, and allowed in `Normal` and
//! `BlockProductionOnly`.
//!
//! ## Why a dedicated file
//!
//! The merged PR #1217 wired the partition policy into the mempool and
//! the HTTP routes that report / push the mode. The block production
//! path itself was still un-guarded at the lower layers — a direct
//! caller of `ProofOfPublishing::create_next_block` or
//! `PopConsensusAdapter::create_next_block` could have produced a
//! block from already-pooled transactions even in `ReadOnly`, because
//! the mempool guard only blocks *ingress*. This test suite drives the
//! HTTP `/blocks/mine` endpoint across all three modes to verify the
//! new adapter- and shared-protocol-level guards are wired to the
//! operator-facing 503 envelope.
//!
//! Together with the unit tests in:
//! - `shared-protocol::consensus::proof_of_publishing::tests` (direct
//!   call to `ProofOfPublishing::create_next_block`)
//! - `consensus-engines::algorithms::pop_adapter::tests` (direct call
//!   to `PopConsensusAdapter::create_next_block`)
//!
//! ...this file gives defense-in-depth coverage at every entrypoint a
//! block might be produced from.

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
use wrfcoin_native_chain::mempool::pool::Mempool;
use wrfcoin_native_chain::security::api_auth::{ApiRole, AuthPrincipal};
use wrfcoin_native_chain::storage::{DEFAULT_CHECKPOINT_INTERVAL, Storage};
use wrfcoin_shared_protocol::consensus::partition_policy::PartitionMode;

const TEST_OPERATOR_TOKEN: &str = "test-operator-token";
const TEST_MINER_ADDRESS: &str = "0x1111111111111111111111111111111111111111";

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

async fn set_partition_mode(state: &web::Data<state::AppState>, mode: PartitionMode) {
    // Mutate both the AppState field and the mempool in lockstep —
    // this is the same contract the /operator/partition-mode handler
    // upholds. Doing it directly here skips the operator-auth dance
    // and keeps each test focused on the production-path guard.
    {
        let mut current = state.partition_mode.write().await;
        *current = mode;
    }
    {
        let mut mempool = state.mempool.write().await;
        mempool.set_partition_mode(mode);
    }
}

// ---------------------------------------------------------------------------
// Core guarantee: ReadOnly refuses block production via /blocks/mine
// ---------------------------------------------------------------------------

#[actix_web::test]
async fn mine_block_refused_with_503_when_partition_is_read_only() {
    let state = test_state();
    set_partition_mode(&state, PartitionMode::ReadOnly).await;

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
        "ReadOnly partition mode MUST refuse /blocks/mine with 503 — \
         otherwise a minority node forks state when the partition heals"
    );

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["error"]["code"], "PARTITION_READ_ONLY");
    let msg = body["error"]["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("read_only") || msg.contains("partition"),
        "error message should explain the partition mode, got: {msg}"
    );
}

// ---------------------------------------------------------------------------
// BlockProductionOnly is the middle-partition case — it MUST succeed
// ---------------------------------------------------------------------------

#[actix_web::test]
async fn mine_block_allowed_in_block_production_only_mode() {
    // BlockProductionOnly is the middle-partition case: a node with
    // 1/3 < local_stake < 2/3 should continue producing blocks for
    // availability, even though it cannot cast finality votes. If the
    // guard here fires, we would have broken the whole point of the
    // mode. This test is a regression fence.
    let state = test_state();
    set_partition_mode(&state, PartitionMode::BlockProductionOnly).await;

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

    // The response MUST NOT be SERVICE_UNAVAILABLE/PARTITION_READ_ONLY.
    // It may be CREATED (block successfully produced) or some other
    // error for unrelated reasons (e.g. empty mempool under strict
    // policies), but it must never be the partition-refusal path.
    assert_ne!(
        resp.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "BlockProductionOnly must not trip the partition guard — the \
         mode is defined as 'produce blocks but don't finalize'. Got 503."
    );

    if resp.status() != StatusCode::CREATED {
        // Not a hard failure, but if the production path returned a
        // different error, it must not be the partition error code.
        let body: serde_json::Value = test::read_body_json(resp).await;
        if let Some(code) = body["error"]["code"].as_str() {
            assert_ne!(
                code, "PARTITION_READ_ONLY",
                "BlockProductionOnly must never surface PARTITION_READ_ONLY"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Normal mode still produces blocks — no regression from the new guard
// ---------------------------------------------------------------------------

#[actix_web::test]
async fn mine_block_allowed_in_normal_mode() {
    let state = test_state();
    // Default is Normal, but set it explicitly for clarity of intent.
    set_partition_mode(&state, PartitionMode::Normal).await;

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

    assert_ne!(
        resp.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "Normal mode must never trip the partition guard"
    );

    if resp.status() != StatusCode::CREATED {
        let body: serde_json::Value = test::read_body_json(resp).await;
        if let Some(code) = body["error"]["code"].as_str() {
            assert_ne!(
                code, "PARTITION_READ_ONLY",
                "Normal mode must never surface PARTITION_READ_ONLY"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Mode transition: Normal → ReadOnly flips production off live
// ---------------------------------------------------------------------------

#[actix_web::test]
async fn mine_block_production_toggles_with_partition_mode_transitions() {
    // Simulates the real operator workflow: the partition orchestrator
    // detects a quorum crossing and flips the mode from Normal to
    // ReadOnly mid-flight. The production path must start refusing
    // immediately — any block produced between the flip and the next
    // mining attempt becomes a fork-risk block.
    let state = test_state();

    let app = test::init_service(
        App::new()
            .app_data(state.clone())
            .route("/blocks/mine", web::post().to(mining::mine_block)),
    )
    .await;

    // Phase 1: Normal. Mining is allowed (status may be CREATED or an
    // unrelated error, but never 503).
    set_partition_mode(&state, PartitionMode::Normal).await;
    let req = test::TestRequest::post()
        .uri("/blocks/mine")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
        .set_json(serde_json::json!({ "miner_address": TEST_MINER_ADDRESS }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_ne!(
        resp.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "Normal phase: mining must not be refused by the partition guard"
    );

    // Phase 2: ReadOnly. The next mine attempt MUST be refused.
    set_partition_mode(&state, PartitionMode::ReadOnly).await;
    let req = test::TestRequest::post()
        .uri("/blocks/mine")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
        .set_json(serde_json::json!({ "miner_address": TEST_MINER_ADDRESS }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "After ReadOnly transition, the very next mine must be refused with 503"
    );
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["error"]["code"], "PARTITION_READ_ONLY");

    // Phase 3: Heal back to Normal. Mining is allowed again.
    set_partition_mode(&state, PartitionMode::Normal).await;
    let req = test::TestRequest::post()
        .uri("/blocks/mine")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
        .set_json(serde_json::json!({ "miner_address": TEST_MINER_ADDRESS }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_ne!(
        resp.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "After healing back to Normal, mining must no longer be refused"
    );
    if resp.status() != StatusCode::CREATED {
        let body: serde_json::Value = test::read_body_json(resp).await;
        if let Some(code) = body["error"]["code"].as_str() {
            assert_ne!(
                code, "PARTITION_READ_ONLY",
                "Healed Normal mode must not surface PARTITION_READ_ONLY"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Race regression: mempool flipping to ReadOnly between outer HTTP check
// and inner consensus call still gets refused by the deeper guards
// ---------------------------------------------------------------------------

#[actix_web::test]
async fn mine_block_refused_when_only_mempool_is_read_only() {
    // This test exercises the defense-in-depth layer: the outer HTTP
    // guard reads `state.partition_mode`, which is normally kept in
    // lockstep with the mempool mode via the operator endpoint. If a
    // future refactor or test harness forgets to update one of the
    // two, the inner guards (consensus-engines adapter +
    // shared-protocol PoP) must still refuse block production because
    // they read the mempool's own mode.
    //
    // Here we simulate that drift by flipping only the mempool.
    let state = test_state();
    {
        let mut mempool = state.mempool.write().await;
        mempool.set_partition_mode(PartitionMode::ReadOnly);
    }
    // Note: state.partition_mode is still Normal — the outer guard
    // will let the request through, and the inner guard must catch it.

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
        "Inner adapter/shared-protocol guards must refuse production \
         when the mempool is ReadOnly, even if AppState.partition_mode \
         was left Normal by a buggy caller"
    );
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["error"]["code"], "PARTITION_READ_ONLY");
}
