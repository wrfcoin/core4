//! HTTP integration tests for `/network/info` endpoint (#1329).
//!
//! Verifies that `GET /network/info` returns the `genesis_hash` field
//! alongside network_id, chain_id, and other node introspection data.

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

use actix_web::{http::StatusCode, test, web, App};
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};
use tokio::sync::RwLock;
use wrfcoin_consensus_engines::{ConsensusFactory, ConsensusType, TypedConsensusManager};
use wrfcoin_native_chain::blockchain::block::Block;
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::bridge::safety_manager::BridgeSafetyManager;
use wrfcoin_native_chain::bridge::types::BridgeSafetyConfig;
use wrfcoin_native_chain::config::settings::Config;
use wrfcoin_native_chain::mempool::pool::Mempool;
use wrfcoin_native_chain::security::api_auth::{ApiRole, AuthPrincipal};
use wrfcoin_native_chain::storage::{Storage, DEFAULT_CHECKPOINT_INTERVAL};
use wrfcoin_shared_protocol::consensus::partition_policy::PartitionMode;
use wrfcoin_shared_protocol::crypto::hash::Hash;
use chrono::Utc;

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

/// Build the height-1 genesis block for this chain (the genesis is height 1
/// with `prev_block_hash == Hash::default()`, NOT height 0 — see #1714/#1715).
fn make_genesis_block() -> (Block, Hash) {
    // Block::new(version, prev_block_hash, txs, difficulty, height)
    let mut genesis = Block::new(1, Hash::default(), Vec::new(), 1, 1);
    genesis.timestamp = Utc::now();
    let genesis_hash = genesis.calculate_hash();
    genesis.set_hash(genesis_hash);
    (genesis, genesis_hash)
}

fn test_state() -> web::Data<state::AppState> {
    // Default test state: genesis (height-1) block present in the in-memory
    // window AND a cached genesis_hash, mirroring a freshly booted node.
    let mut blockchain = Blockchain::default();
    let (genesis, genesis_hash) = make_genesis_block();
    blockchain.add_block(genesis).expect("genesis block should be added");
    test_state_with(blockchain, Some(format!("0x{}", genesis_hash.to_hex())))
}

/// Build an `AppState` from a caller-supplied blockchain and cached
/// `genesis_hash`, so tests can simulate a long-running node whose genesis
/// block has been evicted from the in-memory window (#1714).
fn test_state_with(
    blockchain: Blockchain,
    genesis_hash: Option<String>,
) -> web::Data<state::AppState> {
    let tmp = std::env::temp_dir().join(format!("wrfcoin-test-{}.redb", fastrand::u64(..)));
    let storage =
        Storage::open(&tmp, DEFAULT_CHECKPOINT_INTERVAL).expect("test storage should open");

    web::Data::new(state::AppState {
        consensus: test_consensus(),
        blockchain: Arc::new(RwLock::new(blockchain)),
        snapshot_service: Arc::new(state::snapshot_service::SnapshotServiceState::default()),
        genesis_hash: state::GenesisHashCell::new(genesis_hash),
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
async fn network_info_includes_genesis_hash() {
    let state = test_state();
    let app = test::init_service(
        App::new()
            .app_data(state.clone())
            .route("/network/info", web::get().to(network::get_network_info)),
    )
    .await;

    let req = test::TestRequest::get().uri("/network/info").to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "/network/info should return 200 OK"
    );

    let body: serde_json::Value = test::read_body_json(resp).await;

    // Verify all expected fields are present
    assert!(
        body.get("network_id").is_some(),
        "response should include network_id"
    );
    assert!(
        body.get("chain_id").is_some(),
        "response should include chain_id"
    );
    assert!(
        body.get("node_version").is_some(),
        "response should include node_version"
    );
    assert!(
        body.get("genesis_hash").is_some(),
        "response should include genesis_hash"
    );
    assert!(
        body.get("peer_count").is_some(),
        "response should include peer_count"
    );
    assert!(
        body.get("sync_status").is_some(),
        "response should include sync_status"
    );
    assert!(
        body.get("chain_height").is_some(),
        "response should include chain_height"
    );

    // Verify genesis_hash is a non-empty string
    let genesis_hash = body["genesis_hash"].as_str().expect("genesis_hash should be a string");
    assert!(
        !genesis_hash.is_empty() && genesis_hash != "unknown",
        "genesis_hash should be populated with actual hash, got: {genesis_hash}"
    );
}

#[actix_web::test]
async fn network_info_genesis_hash_is_hex_string() {
    let state = test_state();
    let app = test::init_service(
        App::new()
            .app_data(state)
            .route("/network/info", web::get().to(network::get_network_info)),
    )
    .await;

    let req = test::TestRequest::get().uri("/network/info").to_request();
    let resp = test::call_service(&app, req).await;

    let body: serde_json::Value = test::read_body_json(resp).await;
    let genesis_hash = body["genesis_hash"].as_str().expect("genesis_hash should be a string");

    // Verify it's a valid hex string (0x prefix followed by hex chars)
    assert!(
        genesis_hash.starts_with("0x"),
        "genesis_hash should start with '0x', got: {genesis_hash}"
    );
    assert!(
        genesis_hash.len() > 2,
        "genesis_hash should have content after '0x', got: {genesis_hash}"
    );
    assert!(
        genesis_hash[2..].chars().all(|c| c.is_ascii_hexdigit()),
        "genesis_hash should contain only hex digits after '0x', got: {genesis_hash}"
    );
}

/// Regression test for #1714: on a long-running node the genesis (height-1)
/// block is evicted from the ~200-block in-memory window. Before the fix the
/// per-request lookup returned `None` and `/network/info` reported
/// `genesis_hash: "unknown"`. The hash is now resolved once at boot and cached
/// in `AppState`, so it must still be reported even when the genesis block is no
/// longer in the in-memory window.
#[actix_web::test]
async fn network_info_reports_cached_genesis_hash_when_genesis_evicted() {
    // Simulate a long-running node: the in-memory window holds only a *later*
    // block (height 5000), not the height-1 genesis. The cached value stands in
    // for the hash resolved at boot from the durable archive.
    let mut blockchain = Blockchain::default();
    let mut later = Block::new(1, Hash::default(), Vec::new(), 1, 5000);
    later.timestamp = Utc::now();
    let later_hash = later.calculate_hash();
    later.set_hash(later_hash);
    blockchain.add_block(later).expect("later block should be added");

    // Precondition: the lowest in-memory block is NOT the height-1 genesis
    // anchor, so the handler's in-memory fallback cannot resolve it.
    let lowest = blockchain
        .blocks()
        .values()
        .next()
        .expect("window has a block");
    assert_ne!(lowest.height, 1, "precondition: genesis evicted from window");

    let cached = "0x29cd3f89fc3d3e6e8f6f820c85de41ae51f40bed70f8d5234caf7ddd82bae205";
    let state = test_state_with(blockchain, Some(cached.to_string()));
    let app = test::init_service(
        App::new()
            .app_data(state)
            .route("/network/info", web::get().to(network::get_network_info)),
    )
    .await;

    let req = test::TestRequest::get().uri("/network/info").to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = test::read_body_json(resp).await;
    let genesis_hash = body["genesis_hash"]
        .as_str()
        .expect("genesis_hash should be a string");

    assert_ne!(
        genesis_hash, "unknown",
        "genesis_hash must not be 'unknown' when a cached value exists despite eviction"
    );
    assert_eq!(
        genesis_hash, cached,
        "genesis_hash should report the cached boot-time value"
    );
}

/// Regression test for #2797: a node that fresh-syncs over P2P boots with an
/// EMPTY datadir, so the boot-time cache is `None`. Once sync completes, the
/// height-1 genesis has long been evicted from the ~200-block in-memory
/// window — before the fix the handler's window-only fallback reported
/// `genesis_hash: "unknown"` forever. The handler must fall back to the
/// durable archive (the same resolver the boot path uses) and report the
/// real hash.
#[actix_web::test]
async fn network_info_resolves_genesis_from_archive_when_boot_cache_is_empty() {
    // In-memory window: only a later block — the post-fresh-sync shape.
    let mut blockchain = Blockchain::default();
    let mut later = Block::new(1, Hash::default(), Vec::new(), 1, 5000);
    later.timestamp = Utc::now();
    let later_hash = later.calculate_hash();
    later.set_hash(later_hash);
    blockchain.add_block(later).expect("later block should be added");

    // No boot cache: the empty-datadir startup resolved nothing.
    let state = test_state_with(blockchain, None);

    // Durable archive: the synced chain's height-1 genesis is persisted.
    let (genesis, genesis_hash) = make_genesis_block();
    let expected = format!("0x{}", genesis_hash.to_hex());
    {
        let checkpoint_source = Blockchain::default();
        let mut storage = state.storage.write().await;
        let checkpoint = storage
            .checkpoint_state_bytes(genesis.height, &checkpoint_source.state)
            .expect("checkpoint bytes");
        storage
            .persist_block(genesis, vec![], checkpoint)
            .await
            .expect("persist genesis into archive");
    }

    let app = test::init_service(
        App::new()
            .app_data(state)
            .route("/network/info", web::get().to(network::get_network_info)),
    )
    .await;

    let req = test::TestRequest::get().uri("/network/info").to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = test::read_body_json(resp).await;
    let reported = body["genesis_hash"]
        .as_str()
        .expect("genesis_hash should be a string");
    assert_ne!(
        reported, "unknown",
        "fresh-synced node must not report 'unknown' once the archive holds genesis (#2797)"
    );
    assert_eq!(
        reported, expected,
        "genesis_hash should be resolved from the durable archive"
    );
}

/// Negative control (#2797 review): a truncated/malformed archive whose lowest
/// persisted block is NOT the height-1 genesis anchor must yield "unknown" —
/// naming an arbitrary first-row hash `genesis_hash` would convert a corrupt
/// datadir into a plausible wrong chain identity.
#[actix_web::test]
async fn network_info_refuses_non_anchor_lowest_archive_block() {
    let mut blockchain = Blockchain::default();
    let mut later = Block::new(1, Hash::default(), Vec::new(), 1, 5000);
    later.timestamp = Utc::now();
    let later_hash = later.calculate_hash();
    later.set_hash(later_hash);
    blockchain.add_block(later).expect("later block should be added");

    let state = test_state_with(blockchain, None);

    // Archive's lowest block is height 7 — a truncated-history shape.
    let mut orphan = Block::new(1, Hash::default(), Vec::new(), 1, 7);
    orphan.timestamp = Utc::now();
    let orphan_hash = orphan.calculate_hash();
    orphan.set_hash(orphan_hash);
    {
        let checkpoint_source = Blockchain::default();
        let mut storage = state.storage.write().await;
        let checkpoint = storage
            .checkpoint_state_bytes(orphan.height, &checkpoint_source.state)
            .expect("checkpoint bytes");
        storage
            .persist_block(orphan, vec![], checkpoint)
            .await
            .expect("persist non-anchor block into archive");
    }

    let app = test::init_service(
        App::new()
            .app_data(state)
            .route("/network/info", web::get().to(network::get_network_info)),
    )
    .await;

    let req = test::TestRequest::get().uri("/network/info").to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(
        body["genesis_hash"].as_str().expect("string"),
        "unknown",
        "a non-height-1 lowest archive block must never be reported as genesis"
    );
}

/// Repeated-poll control (#2797 review): the unresolved-cache path runs on
/// every request (`watch -n5` operator usage), so ten consecutive polls must
/// answer consistently and without deadlock while the archive branch resolves
/// off-guard on the blocking pool.
#[actix_web::test]
async fn network_info_survives_repeated_polls_with_unresolved_cache() {
    let mut blockchain = Blockchain::default();
    let mut later = Block::new(1, Hash::default(), Vec::new(), 1, 5000);
    later.timestamp = Utc::now();
    let later_hash = later.calculate_hash();
    later.set_hash(later_hash);
    blockchain.add_block(later).expect("later block should be added");

    let (genesis, genesis_hash) = make_genesis_block();
    let expected = format!("0x{}", genesis_hash.to_hex());
    let state = test_state_with(blockchain, None);
    {
        let checkpoint_source = Blockchain::default();
        let mut storage = state.storage.write().await;
        let checkpoint = storage
            .checkpoint_state_bytes(genesis.height, &checkpoint_source.state)
            .expect("checkpoint bytes");
        storage
            .persist_block(genesis, vec![], checkpoint)
            .await
            .expect("persist genesis into archive");
    }

    let app = test::init_service(
        App::new()
            .app_data(state)
            .route("/network/info", web::get().to(network::get_network_info)),
    )
    .await;

    for attempt in 0..10 {
        let req = test::TestRequest::get().uri("/network/info").to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK, "poll {attempt}");
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(
            body["genesis_hash"].as_str().expect("string"),
            expected,
            "poll {attempt} must resolve the archive genesis"
        );
    }
}

/// When no cached value exists but the height-1 genesis block IS still in the
/// in-memory window, `/network/info` resolves the hash from the genesis anchor
/// (fresh-node fallback). The cached fast path and the in-memory fallback must
/// agree on the hash.
#[actix_web::test]
async fn network_info_resolves_genesis_anchor_from_memory_without_cache() {
    let mut blockchain = Blockchain::default();
    let (genesis, genesis_hash) = make_genesis_block();
    blockchain.add_block(genesis).expect("genesis should be added");
    let expected = format!("0x{}", genesis_hash.to_hex());

    // No cached value: force the handler's in-memory genesis-anchor fallback.
    let state = test_state_with(blockchain, None);
    let app = test::init_service(
        App::new()
            .app_data(state)
            .route("/network/info", web::get().to(network::get_network_info)),
    )
    .await;

    let req = test::TestRequest::get().uri("/network/info").to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(
        body["genesis_hash"].as_str(),
        Some(expected.as_str()),
        "in-memory genesis anchor should resolve to the height-1 block hash"
    );
}

/// When no cached value exists AND the genesis block is not in the in-memory
/// window, `/network/info` falls back to the legacy `"unknown"` sentinel rather
/// than panicking — confirming the cache is an additive fast path.
#[actix_web::test]
async fn network_info_falls_back_to_unknown_without_cache_or_memory() {
    let blockchain = Blockchain::default();
    let state = test_state_with(blockchain, None);
    let app = test::init_service(
        App::new()
            .app_data(state)
            .route("/network/info", web::get().to(network::get_network_info)),
    )
    .await;

    let req = test::TestRequest::get().uri("/network/info").to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(
        body["genesis_hash"].as_str(),
        Some("unknown"),
        "without cache or in-memory genesis, genesis_hash falls back to 'unknown'"
    );
}

/// #2762 residual (follow-up to #2797/#2805): the durable archive may be
/// consulted **at most once** per process for the genesis identity. Answer
/// stability across polls is already covered by
/// `network_info_survives_repeated_polls_with_unresolved_cache` and stays
/// green with or without promotion, so this test observes the *read* instead:
/// after the first successful poll the archive is swapped for an empty one.
/// Code that re-reads storage on every poll (the pre-promotion behaviour)
/// degrades to `"unknown"` from poll 2 onward; code that promoted the
/// anchor-checked answer into the boot cache keeps serving it from memory.
#[actix_web::test]
async fn network_info_promotes_archive_genesis_and_stops_reading_the_archive() {
    // A node past the in-memory window: lowest in-memory block is NOT the
    // height-1 anchor, boot cache is empty (fresh-P2P-synced joiner shape).
    let mut blockchain = Blockchain::default();
    let mut later = Block::new(1, Hash::default(), Vec::new(), 1, 5000);
    later.timestamp = Utc::now();
    let later_hash = later.calculate_hash();
    later.set_hash(later_hash);
    blockchain.add_block(later).expect("later block should be added");

    let (genesis, genesis_hash) = make_genesis_block();
    let expected = format!("0x{}", genesis_hash.to_hex());
    let state = test_state_with(blockchain, None);
    {
        let checkpoint_source = Blockchain::default();
        let mut storage = state.storage.write().await;
        let checkpoint = storage
            .checkpoint_state_bytes(genesis.height, &checkpoint_source.state)
            .expect("checkpoint bytes");
        storage
            .persist_block(genesis, vec![], checkpoint)
            .await
            .expect("persist genesis into archive");
    }

    let app = test::init_service(
        App::new()
            .app_data(state.clone())
            .route("/network/info", web::get().to(network::get_network_info)),
    )
    .await;

    // Poll 1: the only poll that is allowed to read the durable archive.
    let req = test::TestRequest::get().uri("/network/info").to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK, "poll 1");
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(
        body["genesis_hash"].as_str().expect("string"),
        expected,
        "poll 1 must resolve genesis from the archive"
    );

    // Take the archive away. Any later poll that still consults storage now
    // sees an empty archive and can only answer "unknown" — which makes the
    // per-poll read observable without instrumenting production code.
    {
        let tmp = std::env::temp_dir()
            .join(format!("wrfcoin-test-empty-{}.redb", fastrand::u64(..)));
        let empty =
            Storage::open(&tmp, DEFAULT_CHECKPOINT_INTERVAL).expect("empty storage should open");
        *state.storage.write().await = empty;
    }

    for attempt in 2..=10 {
        let req = test::TestRequest::get().uri("/network/info").to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK, "poll {attempt}");
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(
            body["genesis_hash"].as_str().expect("string"),
            expected,
            "poll {attempt} must serve the promoted genesis from memory, \
             not re-read the archive (#2762 residual)"
        );
    }
}
