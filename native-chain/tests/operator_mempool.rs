#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/helpers.rs"]
mod helpers;
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
#[path = "../src/bin/launch_api/rate_limit.rs"]
mod rate_limit;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/websocket.rs"]
mod websocket;

use actix_web::{App, http::StatusCode, test, web};
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use wrfcoin_consensus_engines::{ConsensusFactory, ConsensusType, TypedConsensusManager};
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::blockchain::transaction::{
    Transaction, TransactionInput, TransactionOutput,
};
use wrfcoin_native_chain::bridge::safety_manager::BridgeSafetyManager;
use wrfcoin_native_chain::bridge::types::BridgeSafetyConfig;
use wrfcoin_native_chain::config::settings::Config;
use wrfcoin_native_chain::crypto::hash::{Hash, hash_string};
use wrfcoin_native_chain::crypto::signature::PrivateKey;
use wrfcoin_native_chain::mempool::pool::Mempool;
use wrfcoin_native_chain::security::api_auth::{ApiRole, AuthPrincipal};
use wrfcoin_native_chain::storage::{DEFAULT_CHECKPOINT_INTERVAL, Storage};

const TEST_OPERATOR_TOKEN: &str = "test-operator-token";

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

fn test_state(snapshot_signing_key: Option<PrivateKey>) -> web::Data<state::AppState> {
    use wrfcoin_shared_protocol::crypto::signer::{BlockSigner, EnvKeySigner};
    let snapshot_signing_key: Option<Arc<dyn BlockSigner>> = snapshot_signing_key
        .map(|pk| Arc::new(EnvKeySigner::from_private_key(pk, "test")) as Arc<dyn BlockSigner>);
    let tmp = std::env::temp_dir().join(format!("wrfcoin-test-{}.redb", fastrand::u64(..)));
    let storage =
        Storage::open(&tmp, DEFAULT_CHECKPOINT_INTERVAL).expect("test storage should open");
    let (finality_signal_tx, _rx) =
        wrfcoin_shared_protocol::consensus::finality::finality_signal_channel();
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
        web::scope("/operator/mempool")
            .route("/purge", web::post().to(operator::purge_mempool))
            .route("/clear", web::post().to(operator::clear_mempool)),
    )
}

fn build_transaction(tag: &str) -> Transaction {
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_string(&format!("mempool-input-{tag}")),
            vout: 0,
            signature: None,
            public_key: None,
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            amount: 1,
            address: format!("wrf-{tag}"),
            script: "OP_TRUE".to_string(),
            nft_data: None,
        }],
    );
    tx.fee = 1;
    tx.set_id();
    tx
}

async fn seed_transaction(
    state: &web::Data<state::AppState>,
    tag: &str,
    age_secs: Option<u64>,
) -> Hash {
    let tx = build_transaction(tag);
    let tx_id = tx.id.expect("transaction id should be set");
    let mut guard = state.mempool.write().await;
    guard
        .add_transaction(tx)
        .expect("transaction should be accepted");

    if let Some(age_secs) = age_secs {
        let timestamp = Instant::now()
            .checked_sub(Duration::from_secs(age_secs))
            .expect("test host uptime must exceed the requested backdating window");
        assert!(
            guard.set_transaction_timestamp_for_test(&tx_id, timestamp),
            "timestamp helper should find the inserted transaction"
        );
    }

    tx_id
}

#[actix_web::test]
async fn purge_requires_operator_role() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/operator/mempool/purge?max_age_secs=3600")
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[actix_web::test]
async fn purge_rejects_zero_age() {
    let state = test_state(None);
    let app = test::init_service(test_app(state.clone())).await;
    let req = test::TestRequest::post()
        .uri("/operator/mempool/purge?max_age_secs=0")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);

    // Sanity check: the request must not mutate the mempool on validation failure.
    assert_eq!(state.mempool.read().await.size(), 0);
}

#[actix_web::test]
async fn purge_removes_stale_transactions_and_keeps_fresh_ones() {
    let state = test_state(None);
    let stale_tx_id = seed_transaction(&state, "stale", Some(3_601)).await;
    let fresh_tx_id = seed_transaction(&state, "fresh", None).await;

    assert_eq!(state.mempool.read().await.size(), 2);

    let app = test::init_service(test_app(state.clone())).await;
    let req = test::TestRequest::post()
        .uri("/operator/mempool/purge?max_age_secs=3600")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["removed"], 1);
    assert_eq!(body["remaining"], 1);

    let guard = state.mempool.read().await;
    assert_eq!(guard.size(), 1);
    assert!(guard.get_transaction(&stale_tx_id).is_none());
    assert!(guard.get_transaction(&fresh_tx_id).is_some());
}

#[actix_web::test]
async fn clear_requires_operator_role() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/operator/mempool/clear")
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[actix_web::test]
async fn clear_empties_the_mempool() {
    let state = test_state(None);
    let tx_a = seed_transaction(&state, "clear-a", Some(3_601)).await;
    let tx_b = seed_transaction(&state, "clear-b", None).await;

    assert_eq!(state.mempool.read().await.size(), 2);

    let app = test::init_service(test_app(state.clone())).await;
    let req = test::TestRequest::post()
        .uri("/operator/mempool/clear")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["removed"], 2);
    assert_eq!(body["remaining"], 0);

    let guard = state.mempool.read().await;
    assert_eq!(guard.size(), 0);
    assert!(guard.get_transaction(&tx_a).is_none());
    assert!(guard.get_transaction(&tx_b).is_none());
}
