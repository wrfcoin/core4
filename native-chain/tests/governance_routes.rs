#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/routes/query_cost.rs"]
mod query_cost;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/routes/governance.rs"]
mod governance;
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

use actix_web::{App, http::StatusCode, test, web};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, de::DeserializeOwned};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};
use tokio::sync::RwLock;
use wrfcoin_consensus_engines::{ConsensusFactory, ConsensusType, TypedConsensusManager};
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::blockchain::state::{
    GovernanceParameterUpdate, GovernanceProposalClass, GovernanceProposalRecord,
    GovernanceProposalStatus, GovernanceProposalVoteRecord, LawAmendmentRecord,
    LawEnforcementLevel, LawRecord, LawStatus, TreasuryTransactionRecord,
};
use wrfcoin_native_chain::bridge::safety_manager::BridgeSafetyManager;
use wrfcoin_native_chain::bridge::types::BridgeSafetyConfig;
use wrfcoin_native_chain::common::types::Address;
use wrfcoin_native_chain::config::settings::Config;
use wrfcoin_native_chain::crypto::signature::PrivateKey;
use wrfcoin_native_chain::mempool::pool::Mempool;
use wrfcoin_native_chain::security::api_auth::{ApiRole, AuthPrincipal};
use wrfcoin_native_chain::storage::{DEFAULT_CHECKPOINT_INTERVAL, Storage};
use wrfcoin_shared_protocol::crypto::signer::{BlockSigner, EnvKeySigner};

const TEST_OPERATOR_TOKEN: &str = "test-operator-token";
const TEST_CONTRIBUTOR_TOKEN: &str = "test-contributor-token";

#[derive(Debug, Deserialize)]
struct AcceptedTransaction {
    id: String,
    tx_type: String,
}

#[derive(Debug, Deserialize)]
struct VoteHistoryResponse {
    proposal_id: String,
    yes_votes: u64,
    no_votes: u64,
    votes: Vec<VoteRecord>,
}

#[derive(Debug, Deserialize)]
struct VoteRecord {
    voter: String,
    approve: bool,
    voting_power: u64,
    cast_at_block: u64,
}

#[derive(Debug, Deserialize)]
struct LawListResponse {
    items: Vec<LawListItem>,
    next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LawListItem {
    law_id: String,
}

#[derive(Debug, Deserialize)]
struct LawDetailResponse {
    law_id: String,
    version: u32,
}

#[derive(Debug, Deserialize)]
struct TreasurySummaryResponse {
    balance: String,
    total_deposits: String,
    total_withdrawals: String,
    transaction_count: usize,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GovernanceAnalyticsResponse {
    total_proposals: usize,
    proposals_scanned: usize,
    active_proposals: usize,
    executed_proposals: usize,
    rejected_proposals: usize,
    failed_quorum_proposals: usize,
    failed_execution_proposals: usize,
    total_votes_cast: usize,
    unique_voters: usize,
    total_delegations: usize,
    treasury_balance: String,
    law_count: usize,
}

#[derive(Debug, Deserialize)]
struct ProposalListResponse {
    total: usize,
    offset: usize,
    limit: usize,
    proposals: Vec<GovernanceProposalRecord>,
}

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
    // Keep this scope aligned with native-chain/src/bin/launch_api/routes/mod.rs.
    App::new().app_data(state).service(
        web::scope("/governance")
            .route(
                "/proposals",
                web::get().to(governance::list_governance_proposals),
            )
            .route(
                "/proposals/submit",
                web::post().to(governance::submit_governance_proposal),
            )
            .route(
                "/proposals/{id}",
                web::get().to(governance::get_governance_proposal),
            )
            .route(
                "/proposals/{id}/activate",
                web::put().to(governance::activate_proposal),
            )
            .route(
                "/proposals/{id}/finalize",
                web::put().to(governance::finalize_proposal),
            )
            .route(
                "/proposals/{id}/execute",
                web::put().to(governance::execute_proposal),
            )
            .route(
                "/proposals/{id}/votes",
                web::get().to(governance::get_proposal_votes),
            )
            .route("/vote", web::post().to(governance::submit_governance_vote))
            .route(
                "/delegate",
                web::post().to(governance::delegate_voting_power),
            )
            .route("/laws", web::get().to(governance::list_governance_laws))
            .route(
                "/laws/{law_id}",
                web::get().to(governance::get_governance_law),
            )
            .route(
                "/quality/proposals/build",
                web::post().to(governance::build_quality_governance_proposal),
            )
            .route(
                "/trust/weights",
                web::get().to(governance::get_trust_weights),
            )
            .route(
                "/trust/proposals/build",
                web::post().to(governance::build_trust_governance_proposal),
            )
            .route("/treasury", web::get().to(governance::get_treasury))
            .route(
                "/treasury/transactions",
                web::get().to(governance::get_treasury_transactions),
            )
            .route(
                "/treasury/deposit",
                web::post().to(governance::treasury_deposit),
            )
            .route(
                "/treasury/withdraw",
                web::post().to(governance::treasury_withdraw),
            )
            .route(
                "/analytics/dashboard",
                web::get().to(governance::get_governance_analytics),
            ),
    )
}

fn test_address(byte: u8) -> Address {
    Address([byte; 20])
}

fn envelope_data<T: DeserializeOwned>(body: &serde_json::Value) -> serde_json::Result<T> {
    serde_json::from_value(body.get("data").cloned().unwrap_or(serde_json::Value::Null))
}

async fn seed_proposal(
    state: &web::Data<state::AppState>,
    proposal_id: &str,
    status: GovernanceProposalStatus,
) {
    let mut votes_by_voter = FxHashMap::default();
    votes_by_voter.insert(
        "contributor".to_string(),
        GovernanceProposalVoteRecord {
            approve: true,
            voting_power: 7,
            cast_at_block: 12,
        },
    );

    let mut eligible_voting_power_by_voter = FxHashMap::default();
    eligible_voting_power_by_voter.insert("contributor".to_string(), 7);

    let mut blockchain = state.blockchain.write().await;
    blockchain.state.governance.proposals.insert(
        proposal_id.to_string(),
        GovernanceProposalRecord {
            id: proposal_id.to_string(),
            proposer: test_address(0x11),
            update: GovernanceParameterUpdate::EmissionRate(77),
            proposal_class: GovernanceProposalClass::Standard,
            yes_votes: 7,
            no_votes: 1,
            quorum_percent: 60,
            approval_percent: 50,
            eligible_voting_power: 7,
            eligible_voting_power_by_voter,
            deposit: wrfcoin_native_chain::common::types::Balance(1_000),
            voting_deadline: 1_713_566_400,
            status,
            finalized_at_block: None,
            execution_error: None,
            votes_by_voter,
        },
    );
}

async fn seed_law(state: &web::Data<state::AppState>, law_id: &str) {
    let mut blockchain = state.blockchain.write().await;
    blockchain.state.governance.law_book.laws.insert(
        law_id.to_string(),
        LawRecord {
            law_id: law_id.to_string(),
            title: "METAR inline privilege".to_string(),
            category: "data_submission".to_string(),
            plain_language_summary: "Calibrated inline reveals must include plausible METAR."
                .to_string(),
            enforcement_level: LawEnforcementLevel::Consensus,
            status: LawStatus::Active,
            version: 2,
            amendments: vec![
                LawAmendmentRecord {
                    proposal_id: "law-create".to_string(),
                    updated_at_block: 5,
                    summary: "law created".to_string(),
                    version: 1,
                },
                LawAmendmentRecord {
                    proposal_id: "law-amend".to_string(),
                    updated_at_block: 9,
                    summary: "summary amended".to_string(),
                    version: 2,
                },
            ],
        },
    );
}

async fn seed_treasury(state: &web::Data<state::AppState>) {
    let mut blockchain = state.blockchain.write().await;
    blockchain.state.governance.treasury_balance = 700;
    blockchain
        .state
        .governance
        .delegations
        .insert("delegator-1".to_string(), "delegatee-1".to_string());
    blockchain.state.governance.treasury_transactions = vec![
        TreasuryTransactionRecord {
            tx_type: "deposit".to_string(),
            amount: 1_000,
            address: "admin".to_string(),
            block_height: 4,
            memo: Some("bootstrap".to_string()),
            proposal_id: None,
        },
        TreasuryTransactionRecord {
            tx_type: "withdraw".to_string(),
            amount: 300,
            address: "recipient-1".to_string(),
            block_height: 9,
            memo: None,
            proposal_id: Some("prop-001".to_string()),
        },
    ];
}

#[actix_web::test]
async fn submit_proposal_requires_operator_on_production_submit_route() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/governance/proposals/submit")
        .set_json(serde_json::json!({
            "proposal_id": "prop-001",
            "parameter": "VOTING_PERIOD",
            "value": "604800"
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert!(serde_json::from_value::<AcceptedTransaction>(body.clone()).is_err());
    assert_eq!(body["error"]["code"], "UNAUTHORIZED");
}

#[actix_web::test]
async fn submit_proposal_is_fenced_on_production_submit_route_2024() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/governance/proposals/submit")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
        .set_json(serde_json::json!({
            "proposal_id": "prop-001",
            "parameter": "VOTING_PERIOD",
            "value": "604800",
            "voting_deadline": 1713566400,
            "deposit": 1000000
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    // #2024: the GovernanceProposal surface is fenced at mempool admission, so
    // even an OPERATOR-authenticated submission is refused. This test previously
    // asserted CREATED; it is re-targeted rather than deleted, because "the
    // production route no longer accepts this type" is exactly the property the
    // fence exists to guarantee, and it must fail loudly if the fence is lifted
    // without a from-bound proposer authorization replacing it.
    //
    // Operator-token gating is NOT the authorization model here: the token gates
    // this HTTP route, while the same transaction arriving over P2P never sees
    // it. That asymmetry is why the fence is at the consensus boundary instead.
    assert_ne!(
        resp.status(),
        StatusCode::CREATED,
        "an unauthenticated-proposer surface must not be accepted (#2024)"
    );
}

#[actix_web::test]
async fn list_proposals_uses_production_collection_route() {
    let state = test_state(None);
    seed_proposal(&state, "prop-001", GovernanceProposalStatus::Active).await;
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::get()
        .uri("/governance/proposals")
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: ProposalListResponse = test::read_body_json(resp).await;
    assert_eq!(body.total, 1);
    assert_eq!(body.proposals.len(), 1);
    assert_eq!(body.proposals[0].id, "prop-001");
}

#[actix_web::test]
async fn list_proposals_returns_stable_order_and_pagination() {
    let state = test_state(None);
    seed_proposal(&state, "prop-charlie", GovernanceProposalStatus::Active).await;
    seed_proposal(&state, "prop-alpha", GovernanceProposalStatus::Active).await;
    seed_proposal(&state, "prop-bravo", GovernanceProposalStatus::Active).await;
    let app = test::init_service(test_app(state)).await;

    // Page 1: first two in lexicographic id order.
    let req = test::TestRequest::get()
        .uri("/governance/proposals?limit=2&offset=0")
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: ProposalListResponse = test::read_body_json(resp).await;
    assert_eq!(body.total, 3);
    assert_eq!(body.limit, 2);
    assert_eq!(body.offset, 0);
    assert_eq!(body.proposals.len(), 2);
    assert_eq!(body.proposals[0].id, "prop-alpha");
    assert_eq!(body.proposals[1].id, "prop-bravo");

    // Page 2: third entry.
    let req2 = test::TestRequest::get()
        .uri("/governance/proposals?limit=2&offset=2")
        .to_request();
    let resp2 = test::call_service(&app, req2).await;
    assert_eq!(resp2.status(), StatusCode::OK);
    let body2: ProposalListResponse = test::read_body_json(resp2).await;
    assert_eq!(body2.total, 3);
    assert_eq!(body2.proposals.len(), 1);
    assert_eq!(body2.proposals[0].id, "prop-charlie");
}

#[actix_web::test]
async fn get_proposal_returns_seeded_record_at_production_detail_route() {
    let state = test_state(None);
    seed_proposal(&state, "prop-001", GovernanceProposalStatus::Active).await;
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::get()
        .uri("/governance/proposals/prop-001")
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: GovernanceProposalRecord = test::read_body_json(resp).await;
    assert_eq!(body.id, "prop-001");
    assert_eq!(body.status, GovernanceProposalStatus::Active);
}

#[actix_web::test]
async fn activate_proposal_requires_operator_on_production_route() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::put()
        .uri("/governance/proposals/prop-001/activate")
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[actix_web::test]
async fn finalize_proposal_requires_operator_on_production_route() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::put()
        .uri("/governance/proposals/prop-001/finalize")
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[actix_web::test]
async fn execute_proposal_requires_operator_on_production_route() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::put()
        .uri("/governance/proposals/prop-001/execute")
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[actix_web::test]
async fn get_proposal_votes_returns_enveloped_history_at_production_route() {
    let state = test_state(None);
    seed_proposal(&state, "prop-001", GovernanceProposalStatus::Active).await;
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::get()
        .uri("/governance/proposals/prop-001/votes")
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let data: VoteHistoryResponse = envelope_data(&body).expect("vote history should deserialize");
    assert_eq!(data.proposal_id, "prop-001");
    assert_eq!(data.yes_votes, 7);
    assert_eq!(data.no_votes, 1);
    assert_eq!(data.votes.len(), 1);
    assert_eq!(data.votes[0].voter, "contributor");
    assert!(data.votes[0].approve);
    assert_eq!(data.votes[0].voting_power, 7);
    assert_eq!(data.votes[0].cast_at_block, 12);
}

#[actix_web::test]
async fn submit_vote_requires_contributor_on_production_route() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/governance/vote")
        .set_json(serde_json::json!({
            "proposal_id": "prop-001",
            "approve": true
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert!(serde_json::from_value::<AcceptedTransaction>(body.clone()).is_err());
    assert_eq!(body["error"]["code"], "UNAUTHORIZED");
}

#[actix_web::test]
async fn delegate_requires_contributor_on_production_route() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/governance/delegate")
        .set_json(serde_json::json!({
            "delegate_to": "delegatee-address"
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert!(serde_json::from_value::<AcceptedTransaction>(body.clone()).is_err());
    assert_eq!(body["error"]["code"], "UNAUTHORIZED");
}

#[actix_web::test]
async fn list_laws_uses_production_collection_route() {
    let state = test_state(None);
    seed_law(&state, "LAW-002-METAR-INLINE-PRIVILEGE").await;
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::get()
        .uri("/governance/laws?status=active")
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let data: LawListResponse = envelope_data(&body).expect("law list should deserialize");
    assert_eq!(data.items.len(), 1);
    assert_eq!(data.items[0].law_id, "LAW-002-METAR-INLINE-PRIVILEGE");
    assert!(data.next_cursor.is_none());
}

#[actix_web::test]
async fn get_law_returns_seeded_detail_at_production_route() {
    let state = test_state(None);
    seed_law(&state, "LAW-002-METAR-INLINE-PRIVILEGE").await;
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::get()
        .uri("/governance/laws/LAW-002-METAR-INLINE-PRIVILEGE")
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let data: LawDetailResponse = envelope_data(&body).expect("law detail should deserialize");
    assert_eq!(data.law_id, "LAW-002-METAR-INLINE-PRIVILEGE");
    assert_eq!(data.version, 2);
}

#[actix_web::test]
async fn build_quality_proposal_uses_production_build_route() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/governance/quality/proposals/build")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
        .set_json(serde_json::json!({
            "target": "high_threshold",
            "value": 0.75
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: governance::QualityGovernanceProposalBuildResponse = test::read_body_json(resp).await;
    assert!(!body.proposal_payload.parameter.is_empty());
    assert_eq!(body.proposal_payload.value, "0.75");
}

#[actix_web::test]
async fn build_quality_proposal_requires_contributor_auth() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/governance/quality/proposals/build")
        .set_json(serde_json::json!({
            "target": "high_threshold",
            "value": 0.75
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["error"]["code"], "UNAUTHORIZED");
}

#[actix_web::test]
async fn get_trust_weights_uses_production_route() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::get()
        .uri("/governance/trust/weights")
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: governance::TrustWeightsResponse = test::read_body_json(resp).await;
    assert!(body.bounds.max >= body.bounds.min);
    assert_eq!(body.ordering_hints.len(), 2);
}

#[actix_web::test]
async fn build_trust_proposal_uses_production_build_route() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/governance/trust/proposals/build")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
        .set_json(serde_json::json!({
            "target": "unverified_weight",
            "value": 0.5
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: governance::TrustGovernanceProposalBuildResponse = test::read_body_json(resp).await;
    assert!(!body.proposal_payload.parameter.is_empty());
    assert_eq!(body.proposal_payload.value, "0.5");
}

#[actix_web::test]
async fn build_trust_proposal_requires_contributor_auth() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/governance/trust/proposals/build")
        .set_json(serde_json::json!({
            "target": "unverified_weight",
            "value": 0.5
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["error"]["code"], "UNAUTHORIZED");
}

#[actix_web::test]
async fn get_treasury_uses_production_route() {
    let state = test_state(None);
    seed_treasury(&state).await;
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::get()
        .uri("/governance/treasury")
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let data: TreasurySummaryResponse = envelope_data(&body).expect("treasury should deserialize");
    assert_eq!(data.balance, "700");
    assert_eq!(data.total_deposits, "1000");
    assert_eq!(data.total_withdrawals, "300");
    assert_eq!(data.transaction_count, 2);
}

#[actix_web::test]
async fn get_treasury_transactions_uses_production_route() {
    let state = test_state(None);
    seed_treasury(&state).await;
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::get()
        .uri("/governance/treasury/transactions")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let data = body.get("data").expect("envelope data field");
    assert_eq!(data["total"], 2);
    assert_eq!(data["offset"], 0);
    let txs = data["transactions"].as_array().expect("transactions array");
    assert_eq!(txs.len(), 2);
    assert_eq!(txs[0]["tx_type"], "deposit");
    assert_eq!(txs[1]["tx_type"], "withdraw");
}

#[actix_web::test]
async fn get_treasury_transactions_requires_reader_auth() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::get()
        .uri("/governance/treasury/transactions")
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["error"]["code"], "UNAUTHORIZED");
}

#[actix_web::test]
async fn get_treasury_transactions_pagination_limit1_offset0() {
    let state = test_state(None);
    seed_treasury(&state).await;
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::get()
        .uri("/governance/treasury/transactions?limit=1&offset=0")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let data = body.get("data").expect("envelope data field");
    assert_eq!(data["total"], 2, "total reflects full collection");
    assert_eq!(data["offset"], 0);
    assert_eq!(data["limit"], 1);
    let txs = data["transactions"].as_array().expect("transactions array");
    assert_eq!(txs.len(), 1, "only one item on the page");
    assert_eq!(txs[0]["tx_type"], "deposit");
}

#[actix_web::test]
async fn get_treasury_transactions_pagination_limit1_offset1() {
    let state = test_state(None);
    seed_treasury(&state).await;
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::get()
        .uri("/governance/treasury/transactions?limit=1&offset=1")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let data = body.get("data").expect("envelope data field");
    assert_eq!(data["total"], 2, "total reflects full collection");
    let txs = data["transactions"].as_array().expect("transactions array");
    assert_eq!(txs.len(), 1, "second page has one item");
    assert_eq!(txs[0]["tx_type"], "withdraw");
}

#[actix_web::test]
async fn treasury_deposit_requires_operator_on_production_route() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/governance/treasury/deposit")
        .set_json(serde_json::json!({
            "amount": 1000
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert!(serde_json::from_value::<AcceptedTransaction>(body.clone()).is_err());
    assert_eq!(body["error"]["code"], "UNAUTHORIZED");
}

#[actix_web::test]
async fn treasury_withdraw_reports_missing_authorizing_proposal_on_production_route() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::post()
        .uri("/governance/treasury/withdraw")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
        .set_json(serde_json::json!({
            "amount": 500,
            "recipient": "recipient-addr",
            "proposal_id": "missing-proposal"
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert!(serde_json::from_value::<AcceptedTransaction>(body.clone()).is_err());
    assert_eq!(body["error"]["code"], "NOT_FOUND");
}

#[actix_web::test]
async fn analytics_dashboard_uses_production_route() {
    let state = test_state(None);
    seed_proposal(&state, "prop-001", GovernanceProposalStatus::Active).await;
    seed_law(&state, "LAW-002-METAR-INLINE-PRIVILEGE").await;
    seed_treasury(&state).await;
    let app = test::init_service(test_app(state)).await;
    let req = test::TestRequest::get()
        .uri("/governance/analytics/dashboard")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let data: GovernanceAnalyticsResponse =
        envelope_data(&body).expect("analytics should deserialize");
    assert_eq!(data.total_proposals, 1);
    assert_eq!(data.proposals_scanned, 1, "proposals_scanned must equal total when under the cap");
    assert_eq!(data.active_proposals, 1);
    assert_eq!(data.total_votes_cast, 1);
    assert_eq!(data.unique_voters, 1);
    assert_eq!(data.total_delegations, 1);
    assert_eq!(data.treasury_balance, "700");
    assert_eq!(data.law_count, 1);
}

// ── #1835 regression tests ────────────────────────────────────────────────

#[actix_web::test]
async fn analytics_dashboard_requires_reader_auth() {
    // Issue #1835: the analytics endpoint must reject unauthenticated callers.
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;

    // No Authorization header.
    let resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/governance/analytics/dashboard")
            .to_request(),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "analytics dashboard must require auth (#1835)"
    );
}

#[actix_web::test]
async fn analytics_dashboard_proposals_scanned_bounded_by_cap() {
    // Issue #1835: the analytics scan must be capped so an auth'd caller
    // cannot trigger an arbitrarily large O(proposals × votes) scan.
    // We seed 3 proposals and verify proposals_scanned == min(3, cap) == 3.
    let state = test_state(None);
    seed_proposal(&state, "prop-cap-001", GovernanceProposalStatus::Active).await;
    seed_proposal(&state, "prop-cap-002", GovernanceProposalStatus::Executed).await;
    seed_proposal(&state, "prop-cap-003", GovernanceProposalStatus::Rejected).await;
    let app = test::init_service(test_app(state)).await;

    let resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/governance/analytics/dashboard")
            .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let data: GovernanceAnalyticsResponse =
        envelope_data(&body).expect("analytics should deserialize");

    assert_eq!(data.total_proposals, 3, "total_proposals must reflect the full set");
    assert!(
        data.proposals_scanned <= data.total_proposals,
        "proposals_scanned ({}) must not exceed total_proposals ({})",
        data.proposals_scanned,
        data.total_proposals
    );
    assert!(
        data.proposals_scanned <= governance::MAX_ANALYTICS_PROPOSALS_SCAN,
        "proposals_scanned ({}) must not exceed the hard cap ({})",
        data.proposals_scanned,
        governance::MAX_ANALYTICS_PROPOSALS_SCAN
    );
    // With only 3 proposals all statuses must be reflected.
    assert_eq!(data.active_proposals, 1, "active count must be correct");
    assert_eq!(data.executed_proposals, 1, "executed count must be correct");
    assert_eq!(data.rejected_proposals, 1, "rejected count must be correct");
}
