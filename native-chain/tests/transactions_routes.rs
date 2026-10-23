#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/helpers.rs"]
mod helpers;
// Include the production route table so wiring regressions fail this harness.
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/routes/mod.rs"]
mod routes;
use routes::transactions;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/transition_extractor.rs"]
mod transition_extractor;
#[cfg(feature = "p2p")]
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/peer_reputation.rs"]
mod peer_reputation;
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
use rand::SeedableRng;
use rand::rngs::StdRng;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};
use tokio::sync::RwLock;
use wrfcoin_consensus_engines::{ConsensusFactory, ConsensusType, TypedConsensusManager};
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::blockchain::transaction::commit_authorization_message;
use wrfcoin_shared_protocol::contributions::commit_reveal::CommitRecord;
use wrfcoin_native_chain::bridge::safety_manager::BridgeSafetyManager;
use wrfcoin_native_chain::bridge::types::BridgeSafetyConfig;
use wrfcoin_native_chain::config::settings::Config;
use wrfcoin_native_chain::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_native_chain::mempool::pool::Mempool;
use wrfcoin_native_chain::security::api_auth::{ApiRole, AuthPrincipal};
use wrfcoin_native_chain::storage::{DEFAULT_CHECKPOINT_INTERVAL, Storage};
use wrfcoin_shared_protocol::crypto::signer::{BlockSigner, EnvKeySigner};

/// Generate auth_pubkey/auth_signature for a ContributionCommit payload (#1848).
fn commit_auth_fields(station_id: &str, commit_hash: &str, seed: u64) -> (String, String) {
    let mut rng = StdRng::seed_from_u64(seed);
    let sk = PrivateKey::generate(&mut rng);
    let pk = PublicKey::from_private_key(&sk);
    let msg = commit_authorization_message(station_id, commit_hash, "contribution");
    let sig = sk.sign(&msg);
    (pk.to_hex(), sig.to_hex())
}

const TEST_CONTRIBUTOR_TOKEN: &str = "test-contributor-token";
const TEST_READER_TOKEN: &str = "test-reader-token";
const TEST_ORACLE_TOKEN: &str = "test-oracle-token";

fn test_token_store() -> Arc<FxHashMap<String, AuthPrincipal>> {
    let mut store = FxHashMap::default();
    store.insert(
        TEST_CONTRIBUTOR_TOKEN.to_string(),
        AuthPrincipal::with_role(ApiRole::Contributor),
    );
    store.insert(
        TEST_READER_TOKEN.to_string(),
        AuthPrincipal::with_role(ApiRole::Reader),
    );
    store.insert(TEST_ORACLE_TOKEN.to_string(), AuthPrincipal::with_role(ApiRole::Oracle));
    Arc::new(store)
}

fn bearer(token: &str) -> (&'static str, String) {
    ("Authorization", format!("Bearer {token}"))
}

fn test_consensus() -> Arc<TypedConsensusManager<wrfcoin_consensus_engines::PopConsensusAdapter>> {
    let pop_config = ConsensusFactory::default_config(ConsensusType::ProofOfPublishing);
    let pop_adapter = ConsensusFactory::create_pop_adapter(&pop_config);
    Arc::new(TypedConsensusManager::new(pop_adapter, pop_config))
}

fn test_state(snapshot_signing_key: Option<PrivateKey>) -> web::Data<state::AppState> {
    test_state_with_tokens(snapshot_signing_key, test_token_store())
}

/// `test_state` with a caller-supplied token store, for bearers that carry
/// identity claims (a wallet-bound forecaster, #3070, or contributor, #2148).
fn test_state_with_tokens(
    snapshot_signing_key: Option<PrivateKey>,
    token_store: Arc<FxHashMap<String, AuthPrincipal>>,
) -> web::Data<state::AppState> {
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
        token_store,
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
            wrfcoin_shared_protocol::crypto::signature::PrivateKey::generate(
                &mut rand::thread_rng(),
            ),
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
        web::scope("/transactions")
            .route("/contribution/commit", web::post().to(transactions::submit_contribution_commit))
            .route("/contribution/reveal", web::post().to(transactions::submit_contribution_reveal))
            .route("/forecast/commit", web::post().to(transactions::submit_forecast_commit))
            .route("/forecast/reveal", web::post().to(transactions::submit_forecast_reveal)),
    )
}

#[actix_web::test]
async fn contribution_commit_requires_auth() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/transactions/contribution/commit")
        .set_json(serde_json::json!({
            "contributor": "wrf1abc123",
            "station_id": "station-001",
            "commit_hash": "abcdef0123456789"
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[actix_web::test]
async fn contribution_commit_happy_path() {
    let state = test_state(None);
    let app = test::init_service(test_app(state.clone())).await;
    let commit_hash = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
    let station_id = "station-001";
    let (auth_pubkey, auth_signature) = commit_auth_fields(station_id, commit_hash, 1001);
    let req = test::TestRequest::post()
        .uri("/transactions/contribution/commit")
        .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
        .set_json(serde_json::json!({
            "contributor": "wrf1abc123",
            "station_id": station_id,
            "commit_hash": commit_hash,
            "auth_pubkey": auth_pubkey,
            "auth_signature": auth_signature,
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert!(body["id"].is_string());
    assert_eq!(body["tx_type"], "ContributionCommit");
}

#[cfg(feature = "p2p")]
#[actix_web::test]
async fn contribution_commit_broadcasts_weatherdata_v2_command() {
    use libp2p::PeerId;
    use wrfcoin_p2p::{P2pClient, P2pCommand};
    use tokio::time::{timeout, Duration};

    let (p2p, mut command_rx) = P2pClient::for_testing_with_receiver(PeerId::random());
    let mut state_arc = test_state(None).into_inner();
    if let Some(inner) = std::sync::Arc::get_mut(&mut state_arc) {
        inner.p2p = Some(p2p);
    }
    let state = web::Data::from(state_arc);

    let app = test::init_service(test_app(state.clone())).await;
    let commit_hash = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789".to_string();
    let station_id = "station-001";
    let (auth_pubkey, auth_signature) = commit_auth_fields(station_id, &commit_hash, 1002);
    let req = test::TestRequest::post()
        .uri("/transactions/contribution/commit")
        .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
        .set_json(serde_json::json!({
            "contributor": "wrf1abc123",
            "station_id": station_id,
            "commit_hash": commit_hash,
            "auth_pubkey": auth_pubkey,
            "auth_signature": auth_signature,
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    let mut weather_broadcast_seen = false;
    for _ in 0..4 {
        let command = timeout(Duration::from_millis(500), command_rx.recv())
            .await
            .expect("p2p command should be emitted")
            .expect("p2p command sender should still be alive");

        if let P2pCommand::BroadcastWeatherDataV2 {
            station_id,
            data,
            content_ref,
            station_signing_key,
        } = command
        {
            assert_eq!(station_id, "station-001");
            assert_eq!(
                data,
                b"abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789".to_vec()
            );
            assert!(content_ref.is_none());
            // #1969: the node must attach a usable signing key so the V2
            // broadcast carries a verifiable station signature. Sanity-check
            // that signing with it produces a signature verifiable under the
            // derived pubkey.
            use wrfcoin_shared_protocol::crypto::signature::PublicKey;
            let sig = station_signing_key.sign(b"probe");
            let pubkey = PublicKey::from_private_key(&station_signing_key);
            assert!(
                pubkey.verify(b"probe", &sig),
                "node-supplied weather gossip key must produce verifiable signatures"
            );
            weather_broadcast_seen = true;
            break;
        }
    }
    assert!(
        weather_broadcast_seen,
        "expected BroadcastWeatherDataV2 command after contribution commit"
    );
}

#[actix_web::test]
async fn contribution_commit_rejects_empty_contributor() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/transactions/contribution/commit")
        .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
        .set_json(serde_json::json!({
            "contributor": "",
            "station_id": "station-001",
            "commit_hash": "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[actix_web::test]
async fn contribution_commit_rejects_empty_station_id() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/transactions/contribution/commit")
        .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
        .set_json(serde_json::json!({
            "contributor": "wrf1abc123",
            "station_id": "",
            "commit_hash": "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[actix_web::test]
async fn contribution_commit_rejects_invalid_commit_hash() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/transactions/contribution/commit")
        .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
        .set_json(serde_json::json!({
            "contributor": "wrf1abc123",
            "station_id": "station-001",
            "commit_hash": "not-hex"
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[actix_web::test]
async fn contribution_reveal_requires_auth() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/transactions/contribution/reveal")
        .set_json(serde_json::json!({
            "contributor": "wrf1abc123",
            "station_id": "station-001",
            "commit_hash": "abcdef0123456789",
            "nonce": "1234567890",
            "payload": { "temperature": 72.5 }
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[actix_web::test]
async fn contribution_reveal_happy_path() {
    let state = test_state(None);
    let app = test::init_service(test_app(state.clone())).await;

    let data = serde_json::json!({ "temperature": 72.5 });
    let nonce = "1234567890";
    let station_id = "station-001";
    let timestamp = chrono::Utc::now().timestamp();
    let contribution_type = "contribution";

    let computed_hash = wrfcoin_native_chain::blockchain::transaction::Transaction::compute_commit_id(
        &data,
        nonce,
        station_id,
        timestamp,
        contribution_type,
    );

    let req = test::TestRequest::post()
        .uri("/transactions/contribution/reveal")
        .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
        .set_json(serde_json::json!({
            "contributor": "wrf1abc123",
            "station_id": station_id,
            "commit_hash": computed_hash,
            "nonce": nonce,
            "payload": data,
            "timestamp": timestamp,
            "compute_receipt": {
                "task_id": "task-2026-01",
                "worker_id": "ff00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
                "work_hash": "c0ffee00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
                "work_factor": 1000,
                "completed_at": 1730500000,
                "nonce": "compute-nonce-001"
            }
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert!(body["id"].is_string());
    assert_eq!(body["tx_type"], "ContributionReveal");

    // The compute_receipt sent in the request must actually land in the queued
    // transaction's RevealPayload. Without this assertion a regression that drops
    // request.compute_receipt back to None would still pass the 201 check.
    let mempool = state.mempool.read().await;
    let reveals = mempool.get_transactions_by_type(
        wrfcoin_native_chain::blockchain::transaction::TransactionType::ContributionReveal,
        10,
    );
    assert_eq!(reveals.len(), 1, "expected exactly one reveal in mempool");
    let payload: serde_json::Value = serde_json::from_str(
        reveals[0]
            .data
            .as_deref()
            .expect("reveal tx must carry a payload"),
    )
    .expect("reveal payload must be valid JSON");
    let receipt = &payload["compute_receipt"];
    assert_eq!(receipt["task_id"], "task-2026-01");
    assert_eq!(receipt["work_factor"], 1000);
    assert_eq!(receipt["nonce"], "compute-nonce-001");
}

#[actix_web::test]
async fn contribution_reveal_rejects_mismatched_hash() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/transactions/contribution/reveal")
        .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
        .set_json(serde_json::json!({
            "contributor": "wrf1abc123",
            "station_id": "station-001",
            "commit_hash": "wronghash0123456789",
            "nonce": "1234567890",
            "payload": { "temperature": 72.5 },
            "timestamp": chrono::Utc::now().timestamp()
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[actix_web::test]
async fn contribution_reveal_rejects_non_object_payload() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/transactions/contribution/reveal")
        .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
        .set_json(serde_json::json!({
            "contributor": "wrf1abc123",
            "station_id": "station-001",
            "commit_hash": "abcdef0123456789",
            "nonce": "1234567890",
            "payload": "not an object",
            "timestamp": chrono::Utc::now().timestamp()
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[actix_web::test]
async fn contribution_reveal_rejects_invalid_nonce() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/transactions/contribution/reveal")
        .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
        .set_json(serde_json::json!({
            "contributor": "wrf1abc123",
            "station_id": "station-001",
            "commit_hash": "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
            "nonce": "",
            "payload": { "temperature": 72.5 }
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[actix_web::test]
async fn contribution_reveal_rejects_empty_contributor() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/transactions/contribution/reveal")
        .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
        .set_json(serde_json::json!({
            "contributor": "",
            "station_id": "station-001",
            "commit_hash": "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
            "nonce": "1234567890",
            "payload": { "temperature": 72.5 }
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

fn test_app_status(
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
        web::scope("/transactions")
            .route(
                "/contributions/{station_id}/observations",
                web::get().to(transactions::get_station_observations),
            )
            .route(
                "/contributions/status",
                web::get().to(transactions::get_contribution_statuses),
            ),
    )
}

#[actix_web::test]
async fn contribution_statuses_returns_bounded_paginated_response() {
    let state = test_state(None);
    {
        let mut blockchain = state.blockchain.write().await;
        for (id, ts) in [("commit-charlie", 300u64), ("commit-alpha", 100), ("commit-bravo", 200)] {
            blockchain.state.pending_commits.insert(
                id.to_string(),
                CommitRecord {
                    commit_id: id.to_string(),
                    station_id: "station-001".to_string(),
                    contribution_type: "weather".to_string(),
                    provenance_class: Default::default(),
                    committed_at: ts,
                    commit_expires_at: ts + 1000,
                    reveal_expires_at: ts + 2000,
                },
            );
        }
    }
    let app = test::init_service(test_app_status(state)).await;

    // Page 1: descending-by-timestamp → charlie(300), bravo(200).
    let req = test::TestRequest::get()
        .uri("/transactions/contributions/status?limit=2&offset=0")
        .insert_header(bearer(TEST_READER_TOKEN))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: transactions::ContributionStatusListResponse = test::read_body_json(resp).await;
    assert_eq!(body.total, 3);
    assert_eq!(body.limit, 2);
    assert_eq!(body.offset, 0);
    assert_eq!(body.items.len(), 2);
    assert_eq!(body.items[0].commit_id, "commit-charlie");
    assert_eq!(body.items[1].commit_id, "commit-bravo");

    // Page 2: offset=2 → alpha(100).
    let req2 = test::TestRequest::get()
        .uri("/transactions/contributions/status?limit=2&offset=2")
        .insert_header(bearer(TEST_READER_TOKEN))
        .to_request();
    let resp2 = test::call_service(&app, req2).await;
    assert_eq!(resp2.status(), StatusCode::OK);
    let body2: transactions::ContributionStatusListResponse = test::read_body_json(resp2).await;
    assert_eq!(body2.total, 3);
    assert_eq!(body2.items.len(), 1);
    assert_eq!(body2.items[0].commit_id, "commit-alpha");
}


#[actix_web::test]
async fn forecast_cli_payloads_are_accepted_by_routes() {
    use wrfcoin_cli::forecast_submission::{ForecastInput, commit_body, reveal_body};
    use wrfcoin_shared_protocol::crypto::domain_signing::{signing_chain_id, signing_network_id};
    use wrfcoin_native_chain::blockchain::state::StateTransition;
    let (_, fixture, signer) = forecast_route_fixture();
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data.json");
    let manifest = dir.path().join("manifest.json");
    let receipt = dir.path().join("receipt.json");
    for (path, value) in [(&data, &fixture["payload"]), (&manifest, &fixture["manifest"]),
        (&receipt, &fixture["verification_receipt"])] {
        std::fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
    }
    let input = ForecastInput { forecaster: "wrf1forecast".into(), station_id: "station-forecast".into(),
        data_file: data, nonce: "reveal-nonce".into(), timestamp: fixture["timestamp"].as_i64().unwrap() };
    let commit = commit_body(&input, &signer, &signing_chain_id(), &signing_network_id()).unwrap();
    let reveal = reveal_body(&input, &manifest, Some(&receipt)).unwrap();
    assert_eq!(commit["commit_hash"], reveal["commit_hash"]);
    let state = forecast_state();
    state.blockchain.write().await.apply_state_transition(StateTransition::SetOracleConfig {
        oracle_set: vec![fixture["verification_receipt"]["oracle_id"].as_str().unwrap().into()],
        oracle_quorum: 1,
    }).unwrap();
    let app = test::init_service(test_app(state.clone())).await;
    for (route, body) in [("commit", commit), ("reveal", reveal)] {
        let request = test::TestRequest::post().uri(&format!("/transactions/forecast/{route}"))
            .insert_header(bearer(TEST_FORECASTER_TOKEN)).set_json(body).to_request();
        let response = test::call_service(&app, request).await;
        assert_eq!(response.status(), StatusCode::CREATED, "{route}");
        let body: serde_json::Value = test::read_body_json(response).await;
        assert!(body["id"].is_string(), "{body}");
    }
}

/// A Contributor bearer whose wallet claim is the fixture's forecaster
/// (`wrf1forecast`), spelled in a different case to prove the comparison is
/// case-insensitive like the `wallet:` websocket channel (#3070).
const TEST_FORECASTER_TOKEN: &str = "test-forecaster-token";
const TEST_FORECASTER_WALLET: &str = "WRF1Forecast";
/// An Operator bearer, which acts for the node and may submit for anyone.
const TEST_OPERATOR_TOKEN: &str = "test-operator-token";

fn forecast_token_store() -> Arc<FxHashMap<String, AuthPrincipal>> {
    use wrfcoin_native_chain::security::api_auth::Identity;
    let mut store = (*test_token_store()).clone();
    store.insert(TEST_OPERATOR_TOKEN.to_string(), AuthPrincipal::with_role(ApiRole::Operator));
    store.insert(
        TEST_FORECASTER_TOKEN.to_string(),
        AuthPrincipal::with_identity(ApiRole::Contributor, Identity::wallet(TEST_FORECASTER_WALLET)),
    );
    Arc::new(store)
}

fn forecast_state() -> actix_web::web::Data<state::AppState> {
    test_state_with_tokens(None, forecast_token_store())
}

/// #3070 item 1: the bearer's wallet claim decides whom a forecast may be
/// submitted for, on both routes, through the production route table.
#[actix_web::test]
async fn forecast_routes_bind_the_forecaster_to_the_bearer_wallet() {
    let (commit, reveal, _) = forecast_route_fixture();
    let other_commit = { let mut c = commit.clone(); c["forecaster"] = serde_json::json!("wrf1other"); c };
    let other_reveal = { let mut r = reveal.clone(); r["forecaster"] = serde_json::json!("wrf1other"); r };
    // (token, body, expected status, expected error code)
    let cases: Vec<(&str, &str, serde_json::Value, StatusCode, Option<&str>)> = vec![
        // The bound bearer submits for its own wallet, case-insensitively.
        ("bound/own", TEST_FORECASTER_TOKEN, commit.clone(), StatusCode::CREATED, None),
        // The same bearer cannot submit for another forecaster on either route.
        ("bound/other-commit", TEST_FORECASTER_TOKEN, other_commit.clone(), StatusCode::FORBIDDEN, Some("FORECASTER_NOT_BOUND_TO_BEARER")),
        ("bound/other-reveal", TEST_FORECASTER_TOKEN, other_reveal, StatusCode::FORBIDDEN, Some("FORECASTER_NOT_BOUND_TO_BEARER")),
        // A Contributor bearer with no wallet claim cannot submit for anyone,
        // not even the fixture's forecaster.
        ("unbound", TEST_CONTRIBUTOR_TOKEN, commit.clone(), StatusCode::FORBIDDEN, Some("FORECASTER_BEARER_UNBOUND")),
        // An Operator bearer acts for the node and may submit for anyone.
        ("operator/other", TEST_OPERATOR_TOKEN, other_commit, StatusCode::CREATED, None),
    ];
    for (label, token, body, expected, code) in cases {
        let state = forecast_state();
        let app = test::init_service(App::new().app_data(state.clone())
            .configure(routes::configure_launch_routes)).await;
        let path = if body.get("payload").is_some() { "reveal" } else { "commit" };
        let resp = test::call_service(&app, test::TestRequest::post()
            .uri(&format!("/transactions/forecast/{path}")).insert_header(bearer(token))
            .set_json(&body).to_request()).await;
        assert_eq!(resp.status(), expected, "{label}");
        let enqueued = state.mempool.read().await.get_all_transactions().len();
        match code {
            None => assert_eq!(enqueued, 1, "{label} must enqueue"),
            Some(code) => {
                let error: serde_json::Value = test::read_body_json(resp).await;
                assert!(error.to_string().contains(code), "{label}: {error}");
                assert_eq!(enqueued, 0, "{label} must not enqueue");
            },
        }
    }
}

/// A distinct, validly signed forecast commit body for `forecaster` (one per `n`).
fn distinct_forecast_commit(n: u64, forecaster: &str) -> serde_json::Value {
    use wrfcoin_native_chain::blockchain::transaction::{CommitPayload, Transaction};
    let sk = PrivateKey::generate(&mut StdRng::seed_from_u64(70_000 + n));
    let timestamp = chrono::Utc::now().timestamp();
    let data = serde_json::json!({"temperature": 20.0, "n": n});
    let commit_id = Transaction::compute_commit_id(&data, &format!("nonce-{n}"), "station-forecast", timestamp, "forecast");
    let mut commit = CommitPayload {
        commit_id: commit_id.clone(), station_id: "station-forecast".into(), timestamp,
        contribution_type: "forecast".into(), auth_pubkey: None, auth_signature: None,
    };
    commit.sign_authorization(&sk);
    serde_json::json!({
        "forecaster": forecaster, "station_id": commit.station_id, "commit_hash": commit_id,
        "auth_pubkey": commit.auth_pubkey, "auth_signature": commit.auth_signature,
    })
}

/// Review of core4#3114: the binding compares the wallet case-insensitively,
/// so everything downstream must use ONE canonical forecaster. Otherwise every
/// case or whitespace spelling of the wallet gets a fresh per-forecaster
/// rate-limit bucket and a distinct sender. The reviewer's probe sequence,
/// at the default limit (30/min; the env override is process-global, so it is
/// not changed here): fill the bucket with one spelling, then the next
/// request under that spelling AND every variant spelling must get 429.
#[actix_web::test]
async fn forecaster_case_and_whitespace_variants_share_one_rate_limit_bucket() {
    let state = forecast_state();
    let app = test::init_service(App::new().app_data(state.clone())
        .configure(routes::configure_launch_routes)).await;
    let limit = helpers::contribution_commit_max_requests() as u64;
    let post = |n: u64, forecaster: &str| test::TestRequest::post()
        .uri("/transactions/forecast/commit").insert_header(bearer(TEST_FORECASTER_TOKEN))
        .set_json(distinct_forecast_commit(n, forecaster)).to_request();
    for n in 0..limit {
        let resp = test::call_service(&app, post(n, "wrf1forecast")).await;
        assert_eq!(resp.status(), StatusCode::CREATED, "commit {n} within the limit");
    }
    // Every accepted commit is sent under the canonical identity (the
    // bearer's claim), not the request's spelling.
    for tx in state.mempool.read().await.get_all_transactions() {
        assert_eq!(tx.outputs[0].address, TEST_FORECASTER_WALLET);
    }
    for (i, variant) in ["wrf1forecast", "WRF1forecast", "wRf1FoReCaSt", " wrf1forecast ", "WRF1FORECAST\t"]
        .into_iter().enumerate()
    {
        let resp = test::call_service(&app, post(limit + i as u64, variant)).await;
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS, "{variant:?} must share the bucket");
    }
    assert_eq!(state.mempool.read().await.get_all_transactions().len() as u64, limit);
}

/// Real signatures and the production handlers; no oracle-verifier mock.
fn forecast_route_fixture() -> (serde_json::Value, serde_json::Value, PrivateKey) {
    use wrfcoin_native_chain::blockchain::transaction::{
        CommitPayload, Transaction, ForecastArtifactManifest, ForecastAccuracyMetrics,
        ForecastVerificationReceipt,
    };
    let sk = PrivateKey::generate(&mut StdRng::seed_from_u64(3063));
    let oracle_id = PublicKey::from_private_key(&sk).to_hex();
    let timestamp = chrono::Utc::now().timestamp();
    let data = serde_json::json!({"temperature": 20.0});
    let commit_id = Transaction::compute_commit_id(&data, "reveal-nonce", "station-forecast", timestamp, "forecast");
    let manifest = ForecastArtifactManifest {
        schema_version: 1, model_id: "forecast-route-test".into(),
        input_hash: "a".repeat(64), output_hash: "b".repeat(64),
        produced_at: timestamp, region: "test".into(), horizon_hours: 6,
        input_variables: vec!["temperature".into()], output_variables: vec!["temperature".into()],
    };
    let mut receipt = ForecastVerificationReceipt {
        manifest_digest: manifest.manifest_digest(), commit_id: commit_id.clone(),
        oracle_id, metrics: ForecastAccuracyMetrics { mae: 1.0, rmse: 1.0, bias: 0.0, skill_score: 0.8 },
        verified_at: timestamp, nonce: "receipt-nonce".into(), zk_proof: None, signature: None,
    };
    receipt.signature = Some(sk.sign(receipt.signing_message().as_bytes()).to_hex());
    let mut commit = CommitPayload {
        commit_id: commit_id.clone(), station_id: "station-forecast".into(), timestamp,
        contribution_type: "forecast".into(), auth_pubkey: None, auth_signature: None,
    };
    commit.sign_authorization(&sk);
    let commit_json = serde_json::json!({
        "forecaster": "wrf1forecast", "station_id": commit.station_id, "commit_hash": commit_id,
        "auth_pubkey": commit.auth_pubkey, "auth_signature": commit.auth_signature,
    });
    let reveal_json = serde_json::json!({
        "forecaster": "wrf1forecast", "station_id": "station-forecast", "commit_hash": commit_id,
        "payload": data, "nonce": "reveal-nonce", "timestamp": timestamp,
        "manifest": manifest, "verification_receipt": receipt,
    });
    (commit_json, reveal_json, sk)
}

#[actix_web::test]
async fn forecast_routes_require_contributor_role() {
    let (commit, reveal, _) = forecast_route_fixture();
    let app = test::init_service(test_app(test_state(None))).await;
    for (path, body) in [("commit", commit), ("reveal", reveal)] {
        for (token, expected) in [(None, StatusCode::UNAUTHORIZED),
            (Some(TEST_READER_TOKEN), StatusCode::FORBIDDEN),
            (Some(TEST_ORACLE_TOKEN), StatusCode::FORBIDDEN),
            // #3070: the role is necessary, not sufficient — a Contributor
            // bearer with no wallet claim cannot submit for anyone.
            (Some(TEST_CONTRIBUTOR_TOKEN), StatusCode::FORBIDDEN)] {
            let mut req = test::TestRequest::post().uri(&format!("/transactions/forecast/{path}")).set_json(&body);
            if let Some(token) = token { req = req.insert_header(bearer(token)); }
            let resp = test::call_service(&app, req.to_request()).await;
            assert_eq!(resp.status(), expected, "{path} with {token:?}");
        }
    }
}

#[actix_web::test]
async fn forecast_commit_authorization_is_required_at_activation() {
    use wrfcoin_shared_protocol::consensus::proof_of_publishing::TX_SIGNATURE_VERIFICATION_ACTIVATION_HEIGHT;
    let state = forecast_state();
    state.blockchain.write().await.height = TX_SIGNATURE_VERIFICATION_ACTIVATION_HEIGHT;
    let app = test::init_service(test_app(state.clone())).await;
    let (valid, _, _) = forecast_route_fixture();
    for kind in ["missing", "missing_key", "forged", "wrong_type"] {
        let mut body = valid.clone();
        if kind == "missing" {
            body.as_object_mut().unwrap().remove("auth_signature");
        } else if kind == "missing_key" {
            body.as_object_mut().unwrap().remove("auth_pubkey");
        } else if kind == "forged" {
            body["auth_signature"] = serde_json::json!("00".repeat(64));
        } else {
            let (pk, sig) = commit_auth_fields("station-forecast", body["commit_hash"].as_str().unwrap(), 3063);
            body["auth_pubkey"] = serde_json::json!(pk);
            body["auth_signature"] = serde_json::json!(sig);
        }
        let resp = test::call_service(&app, test::TestRequest::post()
            .uri("/transactions/forecast/commit").insert_header(bearer(TEST_FORECASTER_TOKEN))
            .set_json(body).to_request()).await;
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY, "{kind}");
        let error: serde_json::Value = test::read_body_json(resp).await;
        assert!(error.to_string().contains("INVALID_FORECAST_AUTHORIZATION"));
    }
    assert!(state.mempool.read().await.get_all_transactions().is_empty());
    let resp = test::call_service(&app, test::TestRequest::post()
        .uri("/transactions/forecast/commit").insert_header(bearer(TEST_FORECASTER_TOKEN))
        .set_json(valid).to_request()).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[actix_web::test]
async fn forecast_reveal_rejects_unusable_receipts_before_enqueue() {
    use wrfcoin_native_chain::blockchain::transaction::ForecastVerificationReceipt;
    use wrfcoin_native_chain::blockchain::state::StateTransition;
    let state = forecast_state();
    let (_, valid, sk) = forecast_route_fixture();
    let oracle = valid["verification_receipt"]["oracle_id"].as_str().unwrap().to_string();
    state.blockchain.write().await.apply_state_transition(StateTransition::SetOracleConfig {
        oracle_set: vec![oracle], oracle_quorum: 1,
    }).unwrap();
    let app = test::init_service(test_app(state.clone())).await;
    for kind in ["unsigned", "malformed_signature", "wrong_signature", "foreign_binding", "commit", "manifest", "blank_nonce", "unknown_oracle", "reveal_nonce"] {
        let mut body = valid.clone();
        let mut receipt: ForecastVerificationReceipt = serde_json::from_value(body["verification_receipt"].clone()).unwrap();
        match kind {
            "unsigned" => receipt.signature = None,
            "malformed_signature" => receipt.signature = Some("not-hex".into()),
            "wrong_signature" => receipt.signature = Some("00".repeat(64)),
            "foreign_binding" => {
                let message = receipt.signing_message();
                let mut fields: Vec<_> = message.split('|').map(str::to_string).collect();
                assert_eq!(fields.len(), 8);
                fields[2] = format!("{}-foreign", fields[2]);
                receipt.signature = Some(sk.sign(fields.join("|").as_bytes()).to_hex());
            },
            "commit" => receipt.commit_id = "c".repeat(64),
            "manifest" => receipt.manifest_digest = "d".repeat(64),
            "blank_nonce" => receipt.nonce = " ".into(),
            "unknown_oracle" => {
                let other = PrivateKey::generate(&mut StdRng::seed_from_u64(3064));
                receipt.oracle_id = PublicKey::from_private_key(&other).to_hex();
                receipt.signature = Some(other.sign(receipt.signing_message().as_bytes()).to_hex());
            },
            "reveal_nonce" => body["nonce"] = serde_json::json!("tampered"),
            _ => unreachable!(),
        }
        if matches!(kind, "commit" | "manifest" | "blank_nonce") {
            receipt.signature = Some(sk.sign(receipt.signing_message().as_bytes()).to_hex());
            assert!(receipt.has_valid_signature(), "binding checks must reject even a signed receipt");
        }
        body["verification_receipt"] = serde_json::to_value(receipt).unwrap();
        let resp = test::call_service(&app, test::TestRequest::post()
            .uri("/transactions/forecast/reveal").insert_header(bearer(TEST_FORECASTER_TOKEN))
            .set_json(body).to_request()).await;
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY, "{kind}");
        let error: serde_json::Value = test::read_body_json(resp).await;
        assert!(error.to_string().contains("message"), "clear error body for {kind}");
        assert!(state.mempool.read().await.get_all_transactions().is_empty(), "{kind} must not enqueue");
    }
    let resp = test::call_service(&app, test::TestRequest::post()
        .uri("/transactions/forecast/reveal").insert_header(bearer(TEST_FORECASTER_TOKEN))
        .set_json(&valid).to_request()).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    // A different outer transaction id must not allow replay of the same reveal.
    let resp = test::call_service(&app, test::TestRequest::post()
        .uri("/transactions/forecast/reveal").insert_header(bearer(TEST_FORECASTER_TOKEN))
        .set_json(valid).to_request()).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}


#[actix_web::test]
async fn forecast_reveal_without_receipt_retains_baseline_submission() {
    let (_, mut body, _) = forecast_route_fixture();
    body.as_object_mut().unwrap().remove("verification_receipt");
    let app = test::init_service(test_app(forecast_state())).await;
    let resp = test::call_service(&app, test::TestRequest::post()
        .uri("/transactions/forecast/reveal").insert_header(bearer(TEST_FORECASTER_TOKEN))
        .set_json(body).to_request()).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
}

/// #2456 — the per-commit reward-evidence read model is fail-closed: every
/// leg of `record -> quorum/credit -> block -> finality -> account` must be
/// proven before `rewarded` flips, and each missing leg is named.
#[actix_web::test]
async fn reward_evidence_is_fail_closed_until_every_leg_is_proven_2456() {
    use wrfcoin_native_chain::blockchain::block::Block;
    use wrfcoin_native_chain::blockchain::state::{ContributionRewardRecord, StateTransition};
    use wrfcoin_native_chain::common::types::{Address, Balance};

    let state = test_state(None);
    let app = test::init_service(App::new().app_data(state.clone())
        .configure(routes::configure_launch_routes)).await;
    let get = |commit: &str| test::TestRequest::get()
        .uri(&format!("/transactions/contributions/{commit}/reward-evidence"))
        .insert_header(bearer(TEST_READER_TOKEN)).to_request();

    // Unknown commit: 404, no evidence object at all.
    let resp = test::call_service(&app, get("no-such-commit")).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    // No bearer: the route is authenticated.
    let resp = test::call_service(&app, test::TestRequest::get()
        .uri("/transactions/contributions/x/reward-evidence").to_request()).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let payee = Address([0x24; 20]);
    let real = "commit-real-2456";
    let synthetic = "commit-synthetic-2456";
    {
        let mut chain = state.blockchain.write().await;
        // A synthetic decision is recorded at zero and finalized immediately.
        chain.apply_state_transition(StateTransition::ApplyContributionRewardDecision {
            record: ContributionRewardRecord {
                address: payee, amount: Balance::new(0),
                reason: format!("contribution:{synthetic}"),
                station_id: "station-2456".into(),
                provenance_class: wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass::Synthetic,
            },
            current_block: 0,
        }).unwrap();
        // A real decision without quorum stays pending: recorded, not credited.
        chain.apply_state_transition(StateTransition::ApplyContributionRewardDecision {
            record: ContributionRewardRecord {
                address: payee, amount: Balance::new(45),
                reason: format!("contribution:{real}"),
                station_id: "station-2456".into(),
                provenance_class: wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass::Real,
            },
            current_block: 0,
        }).unwrap();
    }
    let body: serde_json::Value = test::read_body_json(test::call_service(&app, get(synthetic)).await).await;
    assert_eq!(body["rewarded"], false);
    assert_eq!(body["rewarded_blocker"], "provenance_not_reward_eligible");
    assert_eq!(body["reward"]["amount"], "0");
    assert_eq!(body["contribution"]["reward_status"], "excluded_non_real");

    let body: serde_json::Value = test::read_body_json(test::call_service(&app, get(real)).await).await;
    assert_eq!(body["rewarded"], false, "{body}");
    assert_eq!(body["rewarded_blocker"], "reward_not_finalized");
    assert_eq!(body["reward"]["finalized"], false);
    assert_eq!(body["contribution"]["reward_status"], "pending_quorum");
    assert!(body["account"].is_null(), "no credit yet: {body}");
    assert_eq!(body["chain"]["height"], 0);

    // Quorum and the credit land (the state legs), but no block carries the
    // record yet: still not rewarded, and the blocker says so.
    {
        let mut chain = state.blockchain.write().await;
        chain.state.finalized_contribution_rewards.insert(real.to_string());
        let account = chain.state.create_account(payee, Balance::new(45));
        account.earned_balance_total = Balance::new(45);
    }
    let body: serde_json::Value = test::read_body_json(test::call_service(&app, get(real)).await).await;
    assert_eq!(body["rewarded"], false, "{body}");
    assert_eq!(body["rewarded_blocker"], "block_not_found_in_scan_window");
    assert_eq!(body["account"]["earned_balance_total"], "45");
    assert_eq!(body["account"]["credit_covers_reward"], true);
    assert_eq!(body["block_search"]["complete"], true);

    // Three blocks; the second carries the record in its reward_entries.
    for height in 1..=3u64 {
        let mut chain = state.blockchain.write().await;
        let mut block = Block::new(1, chain.tip, vec![], 1, height);
        if height == 2 {
            let records = vec![ContributionRewardRecord {
                address: payee, amount: Balance::new(45),
                reason: format!("contribution:{real}"),
                station_id: "station-2456".into(),
                provenance_class: wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass::Real,
            }];
            block.metadata.insert("reward_entries".into(), serde_json::to_string(&records).unwrap());
        }
        block.set_hash(block.calculate_hash());
        chain.add_block(block).unwrap();
    }
    let body: serde_json::Value = test::read_body_json(test::call_service(&app, get(real)).await).await;
    assert_eq!(body["rewarded"], false, "{body}");
    assert_eq!(body["rewarded_blocker"], "block_not_finalized");
    assert_eq!(body["block"]["height"], 2);
    assert_eq!(body["block"]["finalized"], false);
    assert_eq!(body["chain"]["height"], 3);
    assert_eq!(body["chain"]["finalized_height"], 0);
    assert_eq!(body["block_search"]["scanned_to"], 3);

    // Finality reaches the block: every leg is proven.
    state.blockchain.write().await.set_finalized_height(2);
    let body: serde_json::Value = test::read_body_json(test::call_service(&app, get(real)).await).await;
    assert_eq!(body["rewarded"], true, "{body}");
    assert!(body["rewarded_blocker"].is_null());
    assert_eq!(body["block"]["finalized"], true);
    assert_eq!(body["reward"]["amount"], "45");
    assert_eq!(body["reward"]["reason"], format!("contribution:{real}"));
    assert_eq!(body["contribution"]["reward_status"], "paid");
    assert_eq!(body["chain"]["finalized_height"], 2);
    // Registration-only evidence can never reach here: the synthetic record
    // is still not rewarded on the same chain.
    let body: serde_json::Value = test::read_body_json(test::call_service(&app, get(synthetic)).await).await;
    assert_eq!(body["rewarded"], false);
}

/// #2148 — Sybil caps on the contribution commit edge: an identity is capped
/// at its station count through the production route, a station is capped at
/// its commits per window, the identity is the bearer's wallet claim when the
/// token carries one, and a refused commit never reaches the mempool.
#[actix_web::test]
async fn contribution_commits_are_capped_per_identity_and_per_station_2148() {
    use std::sync::Mutex;
    use wrfcoin_native_chain::security::api_auth::Identity;
    use wrfcoin_native_chain::security::sybil::SybilLimiter;

    let state = test_state(None);
    let app = test::init_service(App::new().app_data(state.clone())
        .configure(routes::configure_launch_routes)).await;
    let contributor = "wrf1sybil2148";
    let max = wrfcoin_native_chain::security::sybil::ContributionSybilPolicy::default()
        .max_stations_per_identity as usize;
    assert!(max < 30, "the per-contributor commit limiter (30/min) must not trip first");
    let post = |station: String, seed: u64| {
        let commit_hash = format!("{:064x}", seed);
        let (auth_pubkey, auth_signature) = commit_auth_fields(&station, &commit_hash, seed);
        test::TestRequest::post()
            .uri("/transactions/contribution/commit")
            .insert_header(bearer(TEST_CONTRIBUTOR_TOKEN))
            .set_json(serde_json::json!({
                "contributor": contributor,
                "station_id": station,
                "commit_hash": commit_hash,
                "auth_pubkey": auth_pubkey,
                "auth_signature": auth_signature,
            }))
            .to_request()
    };
    // Up to the cap: accepted. The cap-plus-one station: refused, nothing enqueued.
    for i in 0..max {
        let resp = test::call_service(&app, post(format!("station-2148-{i}"), 2148_00 + i as u64)).await;
        assert_eq!(resp.status(), StatusCode::CREATED, "station {i}");
    }
    let before = state.mempool.read().await.get_all_transactions().len();
    let resp = test::call_service(&app, post(format!("station-2148-{max}"), 2148_99)).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let error: serde_json::Value = test::read_body_json(resp).await;
    assert!(error.to_string().contains("SYBIL_STATION_LIMIT"), "{error}");
    assert_eq!(state.mempool.read().await.get_all_transactions().len(), before, "refused commit must not enqueue");
    // A station this identity already commits for is still accepted.
    let resp = test::call_service(&app, post("station-2148-0".to_string(), 2148_98)).await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Per-station cap and identity attribution, on a private limiter with a
    // tight policy (the node-wide one keeps the testnet defaults).
    let limiter = Mutex::new(SybilLimiter::new(routes::sybil::policy_from_values(Some(8), Some(2), Some(60), None)));
    let req = test::TestRequest::post().uri("/transactions/contribution/commit")
        .insert_header(bearer(TEST_CONTRIBUTOR_TOKEN)).to_http_request();
    routes::sybil::enforce_with(&limiter, &req, &state, "wrf1a", "station-shared", 1_000).unwrap();
    routes::sybil::enforce_with(&limiter, &req, &state, "wrf1b", "station-shared", 1_010).unwrap();
    let refused = routes::sybil::enforce_with(&limiter, &req, &state, "wrf1c", "station-shared", 1_020).unwrap_err();
    assert_eq!(refused.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(refused.headers().get("Retry-After").and_then(|v| v.to_str().ok()), Some("40"));
    let body = actix_web::body::to_bytes(refused.into_body()).await.unwrap();
    assert!(String::from_utf8_lossy(&body).contains("SYBIL_STATION_COMMIT_CAP"));
    routes::sybil::enforce_with(&limiter, &req, &state, "wrf1c", "station-shared", 1_060).unwrap();

    // Identity: the wallet claim wins over the self-asserted contributor id,
    // case-folded; without a claim the contributor id is the identity.
    let mut store = (*test_token_store()).clone();
    store.insert("bound-2148".to_string(),
        AuthPrincipal::with_identity(ApiRole::Contributor, Identity::wallet("WRF1Wallet2148")));
    let bound_state = test_state_with_tokens(None, Arc::new(store));
    let bound_req = test::TestRequest::post().uri("/transactions/contribution/commit")
        .insert_header(bearer("bound-2148")).to_http_request();
    assert_eq!(routes::sybil::contribution_identity(&bound_req, &bound_state, "wrf1whatever"), "wallet:wrf1wallet2148");
    assert_eq!(routes::sybil::contribution_identity(&req, &state, " WRF1Self "), "contributor:wrf1self");
}

/// #2148 review F3: the shared OPERATOR token acts for many contributors, so a
/// wallet claim on it (`WRFCOIN_OPERATOR_WALLET_ADDRESS`) must not become the
/// Sybil identity. If it did, every station the node submits for would
/// collapse into one identity capped at `max_stations_per_identity` (8).
#[actix_web::test]
async fn operator_bearer_with_a_wallet_keys_the_sybil_identity_on_the_contributor_2148_f3() {
    use std::sync::Mutex;
    use wrfcoin_native_chain::security::api_auth::Identity;
    use wrfcoin_native_chain::security::sybil::SybilLimiter;

    const OPERATOR_WITH_WALLET: &str = "operator-with-wallet-2148";
    let mut store = (*test_token_store()).clone();
    store.insert(OPERATOR_WITH_WALLET.to_string(),
        AuthPrincipal::with_identity(ApiRole::Operator, Identity::wallet("WRF1OperatorWallet")));
    let state = test_state_with_tokens(None, Arc::new(store));
    let req = test::TestRequest::post().uri("/transactions/contribution/commit")
        .insert_header(bearer(OPERATOR_WITH_WALLET)).to_http_request();

    assert_eq!(routes::sybil::contribution_identity(&req, &state, " WRF1Alice "), "contributor:wrf1alice");

    // Shipped defaults (8 stations per identity): 3 x the cap of distinct
    // contributors, each with its own station, all through the operator bearer.
    let limiter = Mutex::new(SybilLimiter::new(routes::sybil::policy_from_values(None, None, None, None)));
    let cap = routes::sybil::policy_from_values(None, None, None, None).max_stations_per_identity as usize;
    assert_eq!(cap, 8);
    for i in 0..cap * 3 {
        let admitted = routes::sybil::enforce_with(
            &limiter, &req, &state, &format!("wrf1contributor{i}"), &format!("station-f3-{i}"), 10_000 + i as i64);
        assert!(admitted.is_ok(), "contributor {i} of {} refused through the operator bearer", cap * 3);
    }
}


fn observation_tx(station: &str, tag: &str) -> wrfcoin_native_chain::blockchain::transaction::Transaction {
    use wrfcoin_native_chain::blockchain::transaction::{Transaction, TransactionType};
    let data = serde_json::json!({"temperature_c": 20.5, "humidity_percent": 45.0,
        "ownerAddress": "must-not-leak", "totalRewards": "must-not-leak", "MAC": "must-not-leak"});
    let commit = Transaction::compute_commit_id(&data, tag, station, 1700000000, "contribution");
    let mut tx = Transaction::new(vec![], vec![]);
    tx.tx_type = TransactionType::ContributionReveal;
    tx.data = Some(serde_json::to_string(&serde_json::json!({
        "commit_id": commit, "station_id": station, "timestamp": 1700000000,
        "contribution_type": "contribution", "data": data, "nonce": tag,
    })).unwrap());
    tx.set_id();
    tx
}

async fn observation_blocks(state: &web::Data<state::AppState>, count: u64, per_block: usize) {
    use wrfcoin_native_chain::blockchain::block::Block;
    for height in 1..=count {
        let mut chain = state.blockchain.write().await;
        let txs = (0..per_block).map(|i| observation_tx("station-read", &format!("{height}-{i}"))).collect();
        let mut block = Block::new(1, chain.tip, txs, 1, height);
        block.set_hash(block.calculate_hash());
        chain.add_block(block.clone()).unwrap();
        drop(chain);
        state.storage.write().await.persist_block(block, vec![], None).await.unwrap();
    }
}

#[actix_web::test]
async fn observations_require_reader_and_do_not_scan_unavailable_history_for_denied_callers() {
    let state = test_state(None);
    state.blockchain.write().await.height = 1;
    let app = test::init_service(test_app_status(state)).await;
    for (token, status) in [(None, StatusCode::UNAUTHORIZED),
        (Some("invalid-reader-token"), StatusCode::FORBIDDEN),
        // Contributor inherits Reader in the existing role policy.
        (Some(TEST_CONTRIBUTOR_TOKEN), StatusCode::SERVICE_UNAVAILABLE),
        (Some(TEST_READER_TOKEN), StatusCode::SERVICE_UNAVAILABLE)] {
        let mut req = test::TestRequest::get().uri("/transactions/contributions/station-read/observations");
        if let Some(token) = token { req = req.insert_header(bearer(token)); }
        assert_eq!(test::call_service(&app, req.to_request()).await.status(), status);
    }
}

#[actix_web::test]
async fn observations_paginate_within_height_and_redact_non_weather_data() {
    let state = test_state(None);
    observation_blocks(&state, 2, 2).await;
    let app = test::init_service(test_app_status(state)).await;
    let mut after = 0;
    let mut offset = 0;
    let mut commits = std::collections::HashSet::new();
    for expected_height in [1, 1, 2, 2] {
        let req = test::TestRequest::get()
            .uri(&format!("/transactions/contributions/station-read/observations?after={after}&offset={offset}&limit=1"))
            .insert_header(bearer(TEST_READER_TOKEN)).to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = test::read_body_json(resp).await;
        let data = &body["data"];
        assert_eq!(data["items"].as_array().unwrap().len(), 1);
        let item = &data["items"][0];
        assert_eq!(item["height"], expected_height);
        assert_eq!(item["values"]["temperature_c"], 20.5);
        assert_eq!(item["observed_at"], 1700000000);
        assert!(item["attestation"].is_null(), "absence is unknown, not a fabricated attestation");
        assert!(item["provenance_class"].is_null());
        assert!(commits.insert(item["commit_id"].as_str().unwrap().to_owned()));
        assert!(!body.to_string().contains("must-not-leak"));
        for forbidden in ["ownerAddress", "totalRewards", "MAC", "sender_address", "radio_metadata"] {
            assert!(!body.to_string().contains(forbidden));
        }
        after = data["next_after"].as_u64().unwrap();
        offset = data["next_offset"].as_u64().unwrap();
    }
    assert_eq!((after, offset), (2, 0));
}

#[actix_web::test]
async fn observations_read_persisted_history_and_bound_pages_and_sparse_scans() {
    let state = test_state(None);
    *state.blockchain.write().await = Blockchain::with_capacity(1);
    observation_blocks(&state, 2, 101).await;
    assert!(state.blockchain.read().await.block_at_height(1).is_none());
    let app = test::init_service(test_app_status(state)).await;
    let resp = test::call_service(&app, test::TestRequest::get()
        .uri("/transactions/contributions/station-read/observations?limit=999999")
        .insert_header(bearer(TEST_READER_TOKEN)).to_request()).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["data"]["items"].as_array().unwrap().len(), 100);
    assert_eq!(body["data"]["next_after"], 0);
    assert_eq!(body["data"]["next_offset"], 100);

    let sparse = test_state(None);
    observation_blocks(&sparse, 65, 0).await;
    let app = test::init_service(test_app_status(sparse)).await;
    let resp = test::call_service(&app, test::TestRequest::get()
        .uri("/transactions/contributions/station-read/observations")
        .insert_header(bearer(TEST_READER_TOKEN)).to_request()).await;
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["data"]["items"], serde_json::json!([]));
    assert_eq!(body["data"]["next_after"], 64);
    assert_eq!(body["data"]["has_more"], true);
}

#[actix_web::test]
async fn observations_validate_cursor_and_filter_station() {
    let state = test_state(None);
    observation_blocks(&state, 1, 1).await;
    let app = test::init_service(test_app_status(state)).await;
    for suffix in ["?offset=2", "?after=-1", "?limit=bad"] {
        let resp = test::call_service(&app, test::TestRequest::get()
            .uri(&format!("/transactions/contributions/station-read/observations{suffix}"))
            .insert_header(bearer(TEST_READER_TOKEN)).to_request()).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
    let resp = test::call_service(&app, test::TestRequest::get()
        .uri("/transactions/contributions/another-station/observations")
        .insert_header(bearer(TEST_READER_TOKEN)).to_request()).await;
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["data"]["items"], serde_json::json!([]));
    assert_eq!(body["data"]["has_more"], false);
}

#[actix_web::test]
async fn forecast_routes_are_wired_through_configure_launch_routes() {
    let state = forecast_state();
    let (commit, reveal, _) = forecast_route_fixture();
    let app = test::init_service(App::new().app_data(state.clone())
        .configure(routes::configure_launch_routes)).await;
    for (path, body) in [("commit", commit.clone()), ("reveal", reveal)] {
        let resp = test::call_service(&app, test::TestRequest::post()
            .uri(&format!("/transactions/forecast/{path}"))
            .set_json(body).to_request()).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "{path} must be wired and require bearer");
    }
    assert!(state.mempool.read().await.get_all_transactions().is_empty());
    let resp = test::call_service(&app, test::TestRequest::post()
        .uri("/transactions/forecast/commit").insert_header(bearer(TEST_FORECASTER_TOKEN))
        .set_json(commit).to_request()).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    assert_eq!(state.mempool.read().await.get_all_transactions().len(), 1);
}

#[actix_web::test]
async fn forecast_edge_tracks_the_consensus_oracle_set_after_rotation() {
    use wrfcoin_native_chain::blockchain::state::StateTransition;
    let (_, body, _) = forecast_route_fixture();
    let oracle = body["verification_receipt"]["oracle_id"].as_str().unwrap().to_string();
    for keep_oracle in [false, true] {
        let state = forecast_state();
        {
            let mut chain = state.blockchain.write().await;
            chain.apply_state_transition(StateTransition::SetOracleConfig {
                oracle_set: vec![oracle.clone()], oracle_quorum: 1,
            }).unwrap();
            // PoP's adapter reads this current chain-managed set for each block.
            chain.apply_state_transition(StateTransition::SetOracleConfig {
                oracle_set: if keep_oracle { vec![oracle.clone()] } else {
                    let replacement = PrivateKey::generate(&mut StdRng::seed_from_u64(3070));
                    vec![PublicKey::from_private_key(&replacement).to_hex()]
                }, oracle_quorum: 1,
            }).unwrap();
            assert_eq!(chain.state.oracle_registry().oracle_set().contains(&oracle), keep_oracle);
        }
        let app = test::init_service(App::new().app_data(state.clone())
            .configure(routes::configure_launch_routes)).await;
        let resp = test::call_service(&app, test::TestRequest::post()
            .uri("/transactions/forecast/reveal").insert_header(bearer(TEST_FORECASTER_TOKEN))
            .set_json(&body).to_request()).await;
        assert_eq!(resp.status(), if keep_oracle { StatusCode::CREATED } else { StatusCode::UNPROCESSABLE_ENTITY });
        assert_eq!(state.mempool.read().await.get_all_transactions().len(), usize::from(keep_oracle));
    }
}

#[actix_web::test]
async fn forecast_wallet_quotas_preserve_bodies_and_return_clear_429s() {
    let state = forecast_state();
    state.forecast_quotas.write().await.config.limit = 1;
    let app = test::init_service(test_app(state.clone())).await;
    let (commit, mut reveal, _) = forecast_route_fixture();
    reveal["verification_receipt"] = serde_json::Value::Null;
    for (kind, body) in [("commit", commit), ("reveal", reveal)] {
        let post = |body: &serde_json::Value| test::TestRequest::post()
            .uri(&format!("/transactions/forecast/{kind}"))
            .insert_header(bearer(TEST_FORECASTER_TOKEN)).set_json(body).to_request();
        assert_eq!(test::call_service(&app, post(&body)).await.status(), StatusCode::CREATED);
        let mut alias = body.clone();
        alias["forecaster"] = serde_json::json!(" WRF1FORECAST ");
        // Changing station cannot acquire a fresh wallet-wide budget.
        alias["station_id"] = serde_json::json!("another-station");
        let response = test::call_service(&app, post(&alias)).await;
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(response.headers().get("retry-after").unwrap().to_str().unwrap().parse::<u64>().unwrap() > 0);
        let error: serde_json::Value = test::read_body_json(response).await;
        assert!(error.to_string().contains("FORECAST_QUOTA_EXCEEDED"));
    }
    assert_eq!(state.mempool.read().await.get_all_transactions().len(), 2);
}
