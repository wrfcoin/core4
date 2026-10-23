use rand::{SeedableRng, rngs::StdRng};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::de::DeserializeOwned;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};
use tokio::sync::RwLock;
use wrfcoin_consensus_engines::{ConsensusFactory, ConsensusType, TypedConsensusManager};
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::bridge::safety_manager::BridgeSafetyManager;
use wrfcoin_native_chain::bridge::types::BridgeSafetyConfig;
use wrfcoin_native_chain::common::types::Address;
use wrfcoin_native_chain::config::settings::Config;
use wrfcoin_native_chain::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_native_chain::mempool::pool::Mempool;
use wrfcoin_native_chain::security::api_auth::{ApiRole, AuthPrincipal};
use wrfcoin_native_chain::storage::{DEFAULT_CHECKPOINT_INTERVAL, Storage};
use wrfcoin_shared_protocol::crypto::signer::{BlockSigner, EnvKeySigner};

#[cfg(feature = "p2p")]
#[allow(dead_code, unused_imports)]
#[path = "../../src/bin/launch_api/da_challenge_producer.rs"]
mod da_challenge_producer;

const TEST_OPERATOR_TOKEN: &str = "test-operator-token";
const TEST_CONTRIBUTOR_TOKEN: &str = "test-contributor-token";
const TEST_ORACLE_TOKEN: &str = "test-oracle-token";
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
        TEST_ORACLE_TOKEN.to_string(),
        AuthPrincipal::with_role(ApiRole::Oracle),
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
    #[cfg(feature = "zk")]
    {
        let zk_verifier: Arc<
            dyn wrfcoin_shared_protocol::consensus::zk_verification::ZkVerifier,
        > = Arc::new(wrfcoin_native_chain::zk_service::ArkworksZkVerifier);
        pop_adapter.set_zk_verifier_blocking(zk_verifier);
    }
    Arc::new(TypedConsensusManager::new(pop_adapter, pop_config))
}

fn test_state(snapshot_signing_key: Option<PrivateKey>) -> actix_web::web::Data<state::AppState> {
    let snapshot_signing_key: Option<Arc<dyn BlockSigner>> = snapshot_signing_key
        .map(|pk| Arc::new(EnvKeySigner::from_private_key(pk, "test")) as Arc<dyn BlockSigner>);
    let tmp = std::env::temp_dir().join(format!("wrfcoin-test-{}.redb", fastrand::u64(..)));
    let storage =
        Storage::open(&tmp, DEFAULT_CHECKPOINT_INTERVAL).expect("test storage should open");
    let (finality_signal_tx, _rx) =
        wrfcoin_shared_protocol::consensus::finality::finality_signal_channel();
    actix_web::web::Data::new(state::AppState {
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
        debug_seed_api_enabled: true,
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
        weather_gossip_signing_key: Arc::new(PrivateKey::generate(&mut StdRng::seed_from_u64(0))),
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

// Shared test scaffolding: this `.inc.rs` is included into many route-test
// targets, each of which uses a different subset of these helpers, so the ones
// a given target does not call are legitimately "unused" there. Mark them
// `#[allow(dead_code)]` rather than deleting — they are intentional shared
// helpers, not leftovers (deleting would remove coverage other targets rely on).
#[allow(dead_code)]
fn bearer(token: &str) -> (&'static str, String) {
    ("Authorization", format!("Bearer {token}")) // secret-leak-ok: test-only bearer header built from an in-memory fixture token, never a real credential
}

#[allow(dead_code)]
fn keypair(seed: u64) -> (PrivateKey, String) {
    let mut rng = StdRng::seed_from_u64(seed);
    let private_key = PrivateKey::generate(&mut rng);
    let public_key = PublicKey::from_private_key(&private_key).to_string();
    (private_key, public_key)
}

#[allow(dead_code)]
fn test_address(byte: u8) -> Address {
    Address([byte; 20])
}

#[allow(dead_code)]
fn test_address_string(byte: u8) -> String {
    test_address(byte).to_string()
}

fn assert_enveloped_error<T: DeserializeOwned>(body: &serde_json::Value, code: &str) {
    assert!(serde_json::from_value::<T>(body.clone()).is_err());
    assert_eq!(body["error"]["code"], code);
}
