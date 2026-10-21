#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/helpers.rs"]
mod helpers;
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

use actix_web::{App, HttpServer, web};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use rustc_hash::{FxHashMap, FxHashSet};
use std::io;
use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::RwLock;
use tokio::time::{Duration, sleep, timeout};
use wrfcoin_consensus_engines::{ConsensusFactory, ConsensusType, TypedConsensusManager};
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::bridge::safety_manager::BridgeSafetyManager;
use wrfcoin_native_chain::bridge::types::BridgeSafetyConfig;
use wrfcoin_native_chain::config::settings::Config;
use wrfcoin_native_chain::mempool::pool::Mempool;
use wrfcoin_native_chain::security::api_auth::{ApiRole, AuthPrincipal, Identity};
use wrfcoin_native_chain::storage::{DEFAULT_CHECKPOINT_INTERVAL, Storage};

const CONTRIBUTOR_TOKEN: &str = "contributor-wallet-token";
const READER_TOKEN: &str = "reader-token";
const CONTRIBUTOR_WALLET: &str = "0xabc123";
const OTHER_WALLET: &str = "0xdef456";
const CONTRIBUTOR_ID: &str = "station-17";

fn test_consensus() -> Arc<TypedConsensusManager<wrfcoin_consensus_engines::PopConsensusAdapter>> {
    let pop_config = ConsensusFactory::default_config(ConsensusType::ProofOfPublishing);
    let pop_adapter = ConsensusFactory::create_pop_adapter(&pop_config);
    Arc::new(TypedConsensusManager::new(pop_adapter, pop_config))
}

fn test_token_store() -> Arc<FxHashMap<String, AuthPrincipal>> {
    let mut store = FxHashMap::default();
    store.insert(
        CONTRIBUTOR_TOKEN.to_string(),
        AuthPrincipal::with_identity(ApiRole::Contributor, Identity::wallet(CONTRIBUTOR_WALLET)),
    );
    store.insert(
        READER_TOKEN.to_string(),
        AuthPrincipal::with_role(ApiRole::Reader),
    );
    Arc::new(store)
}

fn test_state() -> web::Data<state::AppState> {
    let tmp = std::env::temp_dir().join(format!("wrfcoin-ws-{}.redb", fastrand::u64(..)));
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
        #[cfg(feature = "zk")]
        zk_verifier: Arc::new(wrfcoin_native_chain::zk_service::ArkworksZkVerifier),
        #[cfg(feature = "zk")]
        zk_prover_gate: Arc::new(tokio::sync::Semaphore::new(1)),
        #[cfg(feature = "privacy")]
        key_image_registry: Arc::new(wrfcoin_privacy::KeyImageRegistry::new()),
        #[cfg(feature = "privacy")]
        privacy_key_registry: Arc::new(RwLock::new(FxHashMap::default())),
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

struct WsSession {
    stream: TcpStream,
    buffer: Vec<u8>,
}

impl WsSession {
    async fn connect(addr: SocketAddr, token: &str) -> io::Result<Self> {
        let mut stream = TcpStream::connect(addr).await?;
        let key = STANDARD.encode([0x42_u8; 16]);
        let request = format!(
            "GET /ws HTTP/1.1\r\n\
             Host: {addr}\r\n\
             Connection: Upgrade\r\n\
             Upgrade: websocket\r\n\
             Sec-WebSocket-Version: 13\r\n\
             Sec-WebSocket-Key: {key}\r\n\
             Authorization: Bearer {token}\r\n\
             \r\n"
        );
        stream.write_all(request.as_bytes()).await?;
        stream.flush().await?;

        let mut session = Self {
            stream,
            buffer: Vec::new(),
        };
        let response = session.read_http_response().await?;
        assert!(
            response.starts_with("HTTP/1.1 101 Switching Protocols"),
            "websocket handshake failed: {response}"
        );
        Ok(session)
    }

    async fn read_http_response(&mut self) -> io::Result<String> {
        let delimiter = b"\r\n\r\n";
        loop {
            if let Some(offset) = find_subslice(&self.buffer, delimiter) {
                let response = self
                    .buffer
                    .drain(..offset + delimiter.len())
                    .collect::<Vec<_>>();
                return String::from_utf8(response)
                    .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.utf8_error()));
            }

            let mut chunk = [0_u8; 1024];
            let read = self.stream.read(&mut chunk).await?;
            if read == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "connection closed before HTTP response completed",
                ));
            }
            self.buffer.extend_from_slice(&chunk[..read]);
        }
    }

    async fn read_exact_into(&mut self, out: &mut [u8]) -> io::Result<()> {
        while self.buffer.len() < out.len() {
            let mut chunk = [0_u8; 1024];
            let read = self.stream.read(&mut chunk).await?;
            if read == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "connection closed while reading websocket frame",
                ));
            }
            self.buffer.extend_from_slice(&chunk[..read]);
        }

        out.copy_from_slice(&self.buffer[..out.len()]);
        self.buffer.drain(..out.len());
        Ok(())
    }

    async fn read_text_frame(&mut self) -> io::Result<String> {
        let mut head = [0_u8; 2];
        self.read_exact_into(&mut head).await?;

        assert_eq!(head[0] & 0x0f, 0x01, "expected a websocket text frame");
        assert_eq!(head[0] & 0x80, 0x80, "expected a final websocket frame");
        assert_eq!(head[1] & 0x80, 0, "server frames must not be masked");

        let len = match head[1] & 0x7f {
            126 => {
                let mut ext = [0_u8; 2];
                self.read_exact_into(&mut ext).await?;
                u16::from_be_bytes(ext) as usize
            },
            127 => {
                let mut ext = [0_u8; 8];
                self.read_exact_into(&mut ext).await?;
                u64::from_be_bytes(ext) as usize
            },
            n => n as usize,
        };

        let mut payload = vec![0_u8; len];
        self.read_exact_into(&mut payload).await?;
        String::from_utf8(payload)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.utf8_error()))
    }

    async fn send_text(&mut self, text: &str) -> io::Result<()> {
        let payload = text.as_bytes();
        let mask = [0x12_u8, 0x34_u8, 0x56_u8, 0x78_u8];
        let mut frame = Vec::with_capacity(payload.len() + 16);
        frame.push(0x81);

        match payload.len() {
            0..=125 => frame.push(0x80 | payload.len() as u8),
            126..=65_535 => {
                frame.push(0x80 | 126);
                frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
            },
            _ => {
                frame.push(0x80 | 127);
                frame.extend_from_slice(&(payload.len() as u64).to_be_bytes());
            },
        }

        frame.extend_from_slice(&mask);
        frame.extend(
            payload
                .iter()
                .enumerate()
                .map(|(index, byte)| byte ^ mask[index % 4]),
        );

        self.stream.write_all(&frame).await?;
        self.stream.flush().await
    }
}

async fn spawn_server() -> (SocketAddr, actix_web::dev::ServerHandle) {
    let state = test_state();
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("listener should bind");
    let addr = listener
        .local_addr()
        .expect("listener should expose an address");
    let server = HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .route("/ws", web::get().to(websocket::ws_index))
    })
    // actix waits up to 30 s (its default) for the still-open websocket
    // session to close on a graceful stop; each test only needs its own
    // assertions to have run, so don't make every test sit out that wait (#3024).
    .shutdown_timeout(1)
    .listen(listener)
    .expect("server should listen")
    .run();

    let handle = server.handle();
    tokio::spawn(server);
    sleep(Duration::from_millis(50)).await;
    (addr, handle)
}

async fn connect_ws(addr: SocketAddr, token: &str) -> WsSession {
    WsSession::connect(addr, token)
        .await
        .expect("websocket handshake should succeed")
}

async fn assert_subscribed(session: &mut WsSession, channel: &str) {
    let frame = timeout(Duration::from_secs(2), session.read_text_frame())
        .await
        .expect("subscription ack should arrive within timeout")
        .expect("subscription ack frame should decode");
    let value: serde_json::Value =
        serde_json::from_str(&frame).expect("subscription ack should be valid JSON");
    assert_eq!(value["event"], "subscribed");
    assert_eq!(value["payload"]["channel"], channel);
}

async fn assert_forbidden(session: &mut WsSession, channel: &str) {
    let frame = timeout(Duration::from_secs(2), session.read_text_frame())
        .await
        .expect("error frame should arrive within timeout")
        .expect("error frame should decode");
    let value: serde_json::Value =
        serde_json::from_str(&frame).expect("error frame should be valid JSON");
    assert_eq!(value["event"], "error");
    assert_eq!(value["payload"]["code"], "FORBIDDEN");
    assert_eq!(value["payload"]["channel"], channel);
}

async fn prime_connection(session: &mut WsSession) {
    let frame = timeout(Duration::from_secs(2), session.read_text_frame())
        .await
        .expect("connected frame should arrive within timeout")
        .expect("connected frame should decode");
    let value: serde_json::Value =
        serde_json::from_str(&frame).expect("connected frame should be valid JSON");
    assert_eq!(value["event"], "connected");
    assert_eq!(value["payload"]["authenticated"], true);
}

fn subscribe_message(channel: &str) -> String {
    serde_json::json!({
        "action": "subscribe",
        "channel": channel,
    })
    .to_string()
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[actix_web::test]
async fn contributor_can_subscribe_to_own_wallet() {
    let (addr, handle) = spawn_server().await;
    let mut session = connect_ws(addr, CONTRIBUTOR_TOKEN).await;
    prime_connection(&mut session).await;

    let channel = format!("wallet:{CONTRIBUTOR_WALLET}");
    session
        .send_text(&subscribe_message(&channel))
        .await
        .expect("subscribe message should send");
    assert_subscribed(&mut session, &channel).await;

    handle.stop(true).await;
}

#[actix_web::test]
async fn contributor_cannot_subscribe_to_other_wallet() {
    let (addr, handle) = spawn_server().await;
    let mut session = connect_ws(addr, CONTRIBUTOR_TOKEN).await;
    prime_connection(&mut session).await;

    let channel = format!("wallet:{OTHER_WALLET}");
    session
        .send_text(&subscribe_message(&channel))
        .await
        .expect("subscribe message should send");
    assert_forbidden(&mut session, &channel).await;

    handle.stop(true).await;
}

#[actix_web::test]
async fn reader_cannot_subscribe_to_any_private() {
    let (addr, handle) = spawn_server().await;
    let mut session = connect_ws(addr, READER_TOKEN).await;
    prime_connection(&mut session).await;

    for channel in [
        format!("wallet:{CONTRIBUTOR_WALLET}"),
        format!("rewards:{CONTRIBUTOR_ID}"),
    ] {
        session
            .send_text(&subscribe_message(&channel))
            .await
            .expect("subscribe message should send");
        assert_forbidden(&mut session, &channel).await;
    }

    handle.stop(true).await;
}
