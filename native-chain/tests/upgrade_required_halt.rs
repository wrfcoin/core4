//! #3093 — old-binary clean halt at an unknown activation.
//!
//! With an upgrade deadline at H, the real production path (`/blocks/mine`)
//! produces through H − 1 and refuses block H with `UPGRADE_REQUIRED`,
//! requesting the clean halt; without a deadline nothing changes. The boot
//! refusal and rollback decisions are pure (`protocol_support`), and the
//! recorded protocol version round-trips through storage.
//!
//! The AppState scaffolding below is the same one
//! `partition_mode_block_production.rs` uses for the same route.

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
// #3132: `websocket.rs` resolves the client IP through `crate::rate_limit`
// (added by #3109), so every target that declares `mod websocket` must declare
// this one too. #3133 repairs the other 19 targets; this one landed after it.
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/rate_limit.rs"]
mod rate_limit;
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
    // Match launch_api's explicit `WRFCOIN_GENESIS_ORACLE_SET=none` posture:
    // without a declared posture block 1 refuses to be cut (#2523), and this
    // file needs to mine real blocks up to the deadline.
    pop_adapter.declare_no_genesis_oracles_blocking();
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
        forecast_quotas: Arc::new(RwLock::new(Default::default())),
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


use wrfcoin_native_chain::upgrade::{
    halt_requested, install_upgrade_deadline, production_refused_at, reset_for_tests,
    upgrade_deadline, UpgradeDeadline, EXIT_CODE_ROLLBACK_REFUSED, EXIT_CODE_UPGRADE_REQUIRED,
};
use wrfcoin_shared_protocol::crypto::protocol_support::{
    boot_refused, upgrade_required_in_schedule, UpgradeRequired, PROTOCOL_VERSION,
};

/// The deadline is process-wide; the route tests take turns.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// POST /blocks/mine as the operator; returns (status, body). A macro so the
/// service type never has to be spelled out.
macro_rules! mine_once {
    ($app:expr) => {{
        let req = test::TestRequest::post()
            .uri("/blocks/mine")
            .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
            .set_json(serde_json::json!({ "miner_address": TEST_MINER_ADDRESS }))
            .to_request();
        let resp = test::call_service(&$app, req).await;
        let status = resp.status();
        let body: serde_json::Value = test::read_body_json(resp).await;
        (status, body)
    }};
}

#[actix_web::test]
async fn the_producer_mines_through_h_minus_one_and_refuses_h_with_upgrade_required() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    reset_for_tests();
    const H: u64 = 4;
    install_upgrade_deadline(Some(UpgradeDeadline {
        feature: "future_feature".into(),
        height: H,
        running_protocol_version: PROTOCOL_VERSION,
    }));

    let state = test_state();
    let app = test::init_service(
        App::new()
            .app_data(state.clone())
            .route("/blocks/mine", web::post().to(mining::mine_block)),
    )
    .await;

    for expected_height in 1..H {
        let (status, body) = mine_once!(app);
        assert_eq!(status, StatusCode::CREATED, "height {expected_height}: {body}");
        assert_eq!(body["height"], expected_height, "{body}");
        assert!(!halt_requested(), "no halt before the deadline (height {expected_height})");
    }
    assert_eq!(state.blockchain.read().await.height, H - 1);

    let (status, body) = mine_once!(app);
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["error"]["code"], "UPGRADE_REQUIRED", "{body}");
    let message = body["error"]["message"].as_str().unwrap_or("");
    assert!(message.contains("EXIT_UPGRADE_REQUIRED"), "{message}");
    assert!(message.contains("\"future_feature\""), "{message}");
    assert!(message.contains(&format!("height {H}")), "{message}");
    assert_eq!(state.blockchain.read().await.height, H - 1, "block H was never produced");
    assert!(halt_requested(), "the clean halt is requested at the deadline");

    // A second attempt is refused the same way; the tip does not move.
    let (status, _) = mine_once!(app);
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(state.blockchain.read().await.height, H - 1);
    reset_for_tests();
}

#[actix_web::test]
async fn without_a_deadline_the_producer_is_ungated() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    reset_for_tests();
    assert_eq!(upgrade_deadline(), None);
    let state = test_state();
    let app = test::init_service(
        App::new()
            .app_data(state.clone())
            .route("/blocks/mine", web::post().to(mining::mine_block)),
    )
    .await;
    for expected_height in 1..=3u64 {
        let (status, body) = mine_once!(app);
        assert_eq!(status, StatusCode::CREATED, "{body}");
        assert_eq!(body["height"], expected_height);
    }
    assert!(!halt_requested());
}

#[actix_web::test]
async fn a_known_feature_installs_no_deadline_and_an_unknown_one_does() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    reset_for_tests();
    assert_eq!(upgrade_required_in_schedule("0:v1,100:v2,500:domain_separated_preimages"), None);
    let required = upgrade_required_in_schedule("0:v1,100:v2,10:not_in_this_binary").unwrap();
    assert_eq!(required, UpgradeRequired { feature: "not_in_this_binary".into(), height: 10 });
    install_upgrade_deadline(Some(UpgradeDeadline::from_required(&required)));
    assert_eq!(production_refused_at(9), None);
    assert_eq!(production_refused_at(10).map(|d| d.height), Some(10));
    // Boot-time refusal when the tip is already there.
    assert!(!boot_refused(&required, 9));
    assert!(boot_refused(&required, 10));
    reset_for_tests();
}

#[actix_web::test]
async fn exit_codes_are_distinct_and_nonzero() {
    assert_ne!(EXIT_CODE_UPGRADE_REQUIRED, 0);
    assert_ne!(EXIT_CODE_ROLLBACK_REFUSED, 0);
    assert_ne!(EXIT_CODE_UPGRADE_REQUIRED, EXIT_CODE_ROLLBACK_REFUSED);
    assert_ne!(EXIT_CODE_UPGRADE_REQUIRED, 1, "distinct from the generic failure exit");
}

#[actix_web::test]
async fn the_recorded_protocol_version_round_trips_through_storage() {
    let tmp = std::env::temp_dir().join(format!("wrfcoin-3093-{}.redb", fastrand::u64(..)));
    let storage = Storage::open(&tmp, DEFAULT_CHECKPOINT_INTERVAL).expect("open");
    assert_eq!(storage.load_recorded_protocol_version().unwrap(), None, "fresh volume");
    storage.record_protocol_version(PROTOCOL_VERSION).unwrap();
    assert_eq!(storage.load_recorded_protocol_version().unwrap(), Some(PROTOCOL_VERSION));
    storage.record_protocol_version(PROTOCOL_VERSION + 3).unwrap();
    assert_eq!(storage.load_recorded_protocol_version().unwrap(), Some(PROTOCOL_VERSION + 3));
    drop(storage);
    let reopened = Storage::open(&tmp, DEFAULT_CHECKPOINT_INTERVAL).expect("reopen");
    assert_eq!(reopened.load_recorded_protocol_version().unwrap(), Some(PROTOCOL_VERSION + 3));
    let _ = std::fs::remove_file(&tmp);
}
