pub mod node_receipts;
pub mod node_bonds;
pub mod bridge;
pub mod chain;
// Dev endpoints (#2139): absent from any build without `--features dev-endpoints`.
// `test` keeps the bin's own unit tests (which pin the #1811 runtime gate) compiling.
#[cfg(any(test, feature = "dev-endpoints"))]
pub mod debug;
pub mod decimal;
pub mod operator;
#[cfg(feature = "execution")]
pub mod execution;
pub mod governance;
pub mod host_metrics;
pub mod late_quorum;
pub mod light;
pub mod mining;
pub mod network;
pub mod nft;
pub mod oracle;
pub mod origin;
pub mod station;
pub mod privacy;
pub mod quality;
pub mod query_cost;
pub mod rewards;
pub mod schedule_extension;
pub mod signing_keys;
pub mod snapshot;
pub mod sybil;
pub mod stake;
pub mod status_page;
pub mod transactions;
#[cfg(feature = "zk")]
pub mod zk;

use actix_web::web;

// Refuse experimental Node resources before JSON extraction when the feature
// is absent from the process schedule. Handlers additionally bind active height.
fn node_receipts_configured(_: &actix_web::guard::GuardContext<'_>) -> bool {
    use wrfcoin_shared_protocol::crypto::chain_binding::{ConsensusFeature, feature_active};
    feature_active(ConsensusFeature::NodeShareSettlement, u64::MAX)
}

// True only when `main` registered the ReceiptConsumer marker (#3586).
fn receipt_consumer_ingest_configured(ctx: &actix_web::guard::GuardContext<'_>) -> bool {
    ctx.app_data::<node_receipts::ReceiptConsumerIngestLimit>()
        .is_some_and(|m| m.0 == node_receipts::MAX_NODE_RECEIVER_SNAPSHOT_INGEST_BYTES)
}

pub fn configure_launch_routes(cfg: &mut web::ServiceConfig) {
    // #2136: every route is registered inside ONE scope wrapped by `OriginGuard`,
    // so a request whose `Host` is not this node (DNS rebinding) or whose
    // `Origin` is outside the allowlist is refused before any handler runs.
    // Registering a route on `cfg` directly would bypass it —
    // `tests/host_origin_2136.rs` pins that nothing does.
    let scope = web::scope("")
        .wrap(origin::OriginGuard::from_env())
        .route("/ws", web::get().to(crate::websocket::ws_index))
        .route("/health", web::get().to(chain::health))
        .route("/version", web::get().to(chain::version))
        .route("/readyz", web::get().to(chain::readyz))
        .route("/network/audit-log", web::get().to(network::get_network_audit_log))
        .route("/network/info", web::get().to(network::get_network_info))
        // #3095: unauthenticated read, like /network/info — the notice it
        // returns is governance-signed, so a caller verifies the artifact
        // rather than trusting this node's answer.
        .route("/network/upgrade", web::get().to(network::get_network_upgrade))
        // Legacy API compatibility routes retained for smoke-envelope validation.
        .route("/blockchain", web::get().to(chain::get_blockchain_compat))
        .route("/chain/blocks", web::get().to(chain::get_chain_blocks))
        .route("/chain/block/{height}", web::get().to(chain::get_chain_block_by_height))
        // #2188 light-node serving: header projection + DA chunk proofs.
        .route("/chain/header/{height}", web::get().to(light::get_chain_header))
        .route("/da/chunk/{height}/{index}", web::get().to(light::get_da_chunk))
        .route(
            "/addresses/{address}/transactions",
            web::get().to(chain::get_address_transactions),
        )
        .route("/mempool/pending", web::get().to(chain::get_mempool_pending))
        .route(
            "/transactions/submit",
            web::post().to(transactions::submit_transaction_compat),
        )
        .route(
            "/transactions/permissionless/submit",
            web::post().to(transactions::submit_permissionless_transaction),
        )
        .route("/participation/policy", web::get().to(transactions::get_participation_policy))
        .route("/addresses/{address}", web::get().to(chain::get_address_compat))
        .route("/prometheus", web::get().to(chain::get_prometheus_metrics))
        .route("/metrics", web::get().to(chain::get_prometheus_metrics))
        .route("/heatmap/json", web::get().to(chain::get_heatmap_json_compat))
        // Explicit fallback handlers used when feature families are disabled.
        .route("/cross_chain", web::get().to(chain::cross_chain_feature_disabled))
        .route("/cross_chain/status", web::get().to(chain::cross_chain_feature_disabled))
        .service(
            web::scope("/governance")
                .route("/proposals", web::get().to(governance::list_governance_proposals))
                .route("/proposals/submit", web::post().to(governance::submit_governance_proposal))
                .route("/proposals/{id}", web::get().to(governance::get_governance_proposal))
                .route("/proposals/{id}/activate", web::put().to(governance::activate_proposal))
                .route("/proposals/{id}/finalize", web::put().to(governance::finalize_proposal))
                .route("/proposals/{id}/execute", web::put().to(governance::execute_proposal))
                .route("/proposals/{id}/votes", web::get().to(governance::get_proposal_votes))
                .route("/vote", web::post().to(governance::submit_governance_vote))
                .route("/delegate", web::post().to(governance::delegate_voting_power))
                .route("/laws", web::get().to(governance::list_governance_laws))
                .route("/laws/{law_id}", web::get().to(governance::get_governance_law))
                .route("/quality/proposals/build", web::post().to(governance::build_quality_governance_proposal))
                .route("/trust/weights", web::get().to(governance::get_trust_weights))
                .route("/trust/proposals/build", web::post().to(governance::build_trust_governance_proposal))
                .route("/treasury", web::get().to(governance::get_treasury))
                .route("/treasury/transactions", web::get().to(governance::get_treasury_transactions))
                .route("/treasury/deposit", web::post().to(governance::treasury_deposit))
                .route("/treasury/withdraw", web::post().to(governance::treasury_withdraw))
                .route("/analytics/dashboard", web::get().to(governance::get_governance_analytics))
        )
        .route("/enhanced_security/status", web::get().to(chain::enhanced_security_feature_disabled))
        .route("/layer2/status", web::get().to(chain::layer2_feature_disabled))
        .route("/layer2/rollup-config", web::get().to(chain::get_layer2_rollup_config))
        .route("/layer2/submit-batch", web::post().to(chain::submit_layer2_batch))
        .route(
            "/evm_compatibility/status",
            web::get().to(chain::evm_compatibility_feature_disabled),
        )
        .route(
            "/transactions/contribution/commit",
            web::post().to(transactions::submit_contribution_commit),
        )
        .route(
            "/transactions/contribution/reveal",
            web::post().to(transactions::submit_contribution_reveal),
        )
        .route(
            "/transactions/forecast/commit",
            web::post().to(transactions::submit_forecast_commit),
        )
        .route(
            "/transactions/forecast/reveal",
            web::post().to(transactions::submit_forecast_reveal),
        )
        .route(
            "/transactions/quality/events",
            web::get().to(quality::get_quality_events),
        )
        .route(
            "/transactions/quality/summary",
            web::get().to(quality::get_quality_summary),
        )
        .route(
            "/transactions/rewards/trust/timeline",
            web::get().to(quality::get_trust_reward_impact_timeline),
        )
        .route(
            "/transactions/rewards/trust/summary",
            web::get().to(quality::get_trust_reward_impact_summary),
        )
        .route(
            "/transactions/quality/participants/{address}/history",
            web::get().to(quality::get_quality_participant_history),
        )
        .route(
            "/transactions/oracle/attestation",
            web::post().to(oracle::submit_oracle_attestation),
        )
        .route(
            "/transactions/oracle/operators",
            web::post().to(oracle::register_oracle_operator),
        )
        .route(
            "/transactions/station/registrations",
            web::post().to(station::submit_station_registrations),
        )
        .route(
            "/transactions/station/registrations",
            web::get().to(station::list_station_registrations),
        )
        .route(
            "/transactions/station/registrations/{station_id}",
            web::get().to(station::get_station_registration),
        )
        .route(
            "/transactions/station/registrations/{station_id}/owner-ledger",
            web::get().to(station::get_station_owner_ledger),
        )
        .route(
            "/transactions/station/owner-actions",
            web::post().to(station::submit_station_owner_transactions),
        )
        .route(
            "/transactions/oracle/registrations",
            web::post().to(oracle::submit_oracle_registrations),
        )
        .route(
            "/transactions/governance/schedule-extensions",
            web::post().to(schedule_extension::submit_schedule_extension),
        )
        .route(
            "/transactions/contributions/{station_id}/observations",
            web::get().to(transactions::get_station_observations),
        )
        .route(
            "/transactions/contributions/status",
            web::get().to(transactions::get_contribution_statuses),
        )
        .route(
            "/transactions/contributions/{commit_id}/reward-evidence",
            web::get().to(transactions::get_contribution_reward_evidence),
        )
        .route(
            "/transactions/commit/{commit_id}/status",
            web::get().to(transactions::get_commit_status),
        )
        .route(
            "/transactions/stake/metrics",
            web::get().to(stake::get_stake_metrics),
        )
        .route(
            "/transactions/stake/{address}",
            web::get().to(stake::get_stake_status),
        )
        .route(
            "/transactions/oracle/config",
            web::get().to(oracle::get_oracle_config),
        )
        .route(
            "/transactions/oracle/status",
            web::get().to(oracle::get_oracle_status),
        )
        // Keep the dynamic status lookup after every literal
        // `/transactions/*/status` route so Actix cannot capture those paths.
        .route(
            "/transactions/{tx_id}/status",
            web::get().to(chain::get_transaction_status),
        )
        // #2464: the attester read surface — reveal preimages awaiting
        // attestation, from the mempool (prompt path) and from mined blocks
        // short of quorum (deferred late-quorum path). Read-only.
        .route(
            "/transactions/oracle/pending",
            web::get().to(oracle::get_pending_attestations),
        )
        .route(
            "/oracle/leaderboard",
            web::get().to(oracle::get_oracle_leaderboard),
        )
        // #2464: durable reconciliation of contributions that reached oracle
        // quorum after their reward decision had closed. Read-only.
        .route(
            "/oracle/late-quorum/unpaid",
            web::get().to(late_quorum::get_late_quorum_unpaid),
        )
        .route(
            "/transactions/oracle/config",
            web::put().to(oracle::update_oracle_config),
        )
        .route(
            "/transactions/signing-keys/register",
            web::post().to(signing_keys::register_signing_key_epoch),
        )
        .route(
            "/transactions/signing-keys/revoke",
            web::post().to(signing_keys::revoke_signing_key_epoch),
        )
        .route(
            "/transactions/signing-keys/{role}",
            web::get().to(signing_keys::get_signing_keys_by_role),
        )
        .route(
            "/network/signing-keys/register",
            web::post().to(signing_keys::register_signing_key_epoch),
        )
        .route(
            "/network/signing-keys/revoke",
            web::post().to(signing_keys::revoke_signing_key_epoch),
        )
        .route(
            "/network/signing-keys/{role}",
            web::get().to(signing_keys::get_signing_keys_by_role),
        )
        .route("/state/snapshot", web::get().to(snapshot::get_state_snapshot))
        .route(
            "/state/snapshot/entries",
            web::get().to(snapshot::get_state_snapshot_entries),
        )
        .route(
            "/state/snapshot/proof/{key_hex}",
            web::get().to(snapshot::get_state_snapshot_proof),
        )
        .route(
            "/state/snapshot/verify-inclusion",
            web::post().to(snapshot::verify_state_snapshot_inclusion),
        )
        .route(
            "/state/snapshot/sync-payload",
            web::get().to(snapshot::get_snapshot_sync_payload),
        )
        .route(
            "/state/snapshot/sync-payload/verify",
            web::post().to(snapshot::verify_snapshot_sync_payload),
        )
        .route("/network/snapshot-sync/ingest", web::post().to(snapshot::ingest_snapshot_sync_payload))
        .service(web::resource("/network/snapshot-sync/bonded-ingest").guard(actix_web::guard::fn_guard(node_receipts_configured)).app_data(web::JsonConfig::default().limit(node_receipts::MAX_NODE_BONDED_SNAPSHOT_INGEST_BYTES)).route(web::post().to(node_receipts::ingest)))
        // #3586: the isolated ReceiptConsumer process (marker registered in main) gets the
        // larger receiver-only budget; this guarded resource is tried first, every other
        // role falls through to the 2 MiB resource below.
        .service(web::resource("/network/snapshot-sync/receiver-ingest").guard(actix_web::guard::fn_guard(receipt_consumer_ingest_configured)).app_data(web::JsonConfig::default().limit(node_receipts::MAX_NODE_RECEIVER_SNAPSHOT_INGEST_BYTES)).route(web::post().to(node_receipts::ingest)))
        .service(web::resource("/network/snapshot-sync/receiver-ingest").app_data(web::JsonConfig::default().limit(node_receipts::MAX_NODE_BONDED_SNAPSHOT_INGEST_BYTES)).route(web::post().to(node_receipts::ingest)))
        .service(web::resource("/network/snapshot-sync/bonded-ticket").guard(actix_web::guard::fn_guard(node_receipts_configured)).app_data(web::JsonConfig::default().limit(16 * 1024)).route(web::post().to(node_receipts::ticket)))
        .service(web::resource("/network/snapshot-sync/bonded-endorsement").guard(actix_web::guard::fn_guard(node_receipts_configured)).app_data(web::JsonConfig::default().limit(4096)).route(web::post().to(node_receipts::endorse)))
        .service(web::resource("/node/bonds/submit").guard(actix_web::guard::fn_guard(node_receipts_configured)).app_data(web::JsonConfig::default().limit(40 * 1024)).route(web::post().to(node_bonds::submit)))
        .service(web::resource("/node/bonds/{id}").guard(actix_web::guard::fn_guard(node_receipts_configured)).route(web::get().to(node_bonds::inspect)))
        .service(web::resource("/node/settlement/{epoch}").guard(actix_web::guard::fn_guard(node_receipts_configured)).route(web::get().to(node_receipts::settlement)))
        .service(web::resource("/node/receipts/submit").guard(actix_web::guard::fn_guard(node_receipts_configured)).app_data(web::JsonConfig::default().limit(40 * 1024)).route(web::post().to(node_receipts::submit)))
        .service(web::resource("/node/receipts/{id}").guard(actix_web::guard::fn_guard(node_receipts_configured)).route(web::get().to(node_bonds::receipt)))
        .route("/state/snapshot/export", web::get().to(snapshot::get_state_snapshot_export))
        .route(
            "/network/snapshot-sync/receipt",
            web::post().to(snapshot::post_snapshot_service_receipt),
        )
        .route(
            "/network/snapshot-sync/receipts",
            web::get().to(snapshot::get_snapshot_service_receipts),
        )
        .route("/blocks/mine", web::post().to(mining::mine_block))
        .route("/mining/stats", web::get().to(mining::get_mining_stats))
        // Authorized on-chain gamification payout (mobile#215)
        .route("/rewards/payout", web::post().to(rewards::payout))
        .route("/node/sync/status", web::get().to(network::get_node_sync_status))
        .route(
            "/node/network/failover",
            web::get().to(network::get_node_network_failover),
        )
        .route("/node/resources", web::get().to(network::get_node_resources))
        .route("/node/capability", web::get().to(network::get_node_capability))
        .route("/node/metrics/host", web::get().to(host_metrics::get_host_metrics))
        // #3599: the operator status page — one embedded HTML document that
        // renders the routes above in a browser. Carries no data; no token.
        .route("/dashboard", web::get().to(status_page::dashboard))
        // Partition-mode status + operator push (#1217)
        .route(
            "/node/partition-status",
            web::get().to(network::get_partition_status),
        )
        .route(
            "/operator/partition-mode",
            web::post().to(network::set_partition_mode),
        )
        // Operator mempool management routes (#1179)
        .route(
            "/operator/mempool/purge",
            web::post().to(operator::purge_mempool),
        )
        .route(
            "/mempool/purge",
            web::post().to(operator::purge_mempool),
        )
        .route(
            "/operator/mempool/clear",
            web::post().to(operator::clear_mempool),
        )
        // Bridge safety routes
        .route(
            "/bridge/safety/status",
            web::get().to(bridge::get_bridge_safety_status),
        )
        .route("/bridge/safety/halt", web::post().to(bridge::halt_bridge))
        .route("/bridge/safety/resume", web::post().to(bridge::resume_bridge))
        .route(
            "/bridge/safety/challenge/open",
            web::post().to(bridge::open_bridge_challenge),
        )
        .route(
            "/bridge/safety/challenge/submit",
            web::post().to(bridge::submit_bridge_challenge),
        )
        .route(
            "/bridge/safety/challenge/resolve",
            web::post().to(bridge::resolve_bridge_challenge),
        )
        .route(
            "/bridge/safety/caps/update",
            web::post().to(bridge::update_bridge_caps),
        )
        .route(
            "/bridge/safety/exposure",
            web::get().to(bridge::get_bridge_exposure),
        )
        .route(
            "/bridge/safety/audit-log",
            web::get().to(bridge::get_bridge_audit_log),
        )
        .route(
            "/bridge/safety/scan-stuck",
            web::post().to(bridge::scan_bridge_stuck),
        )
        // NFT routes
        .route("/nft/mint", web::post().to(nft::mint_nft))
        .route("/nft/list", web::get().to(nft::list_nfts))
        .route("/nft/{id}", web::get().to(nft::get_nft))
        .route("/nft/{id}/verify", web::post().to(nft::verify_nft))
        .route("/nft/{id}/transfer", web::post().to(nft::transfer_nft));

    // Dev endpoints (#2139): compiled OUT of every binary that does not opt in
    // with `--features dev-endpoints`, so a release binary has no handler to
    // misconfigure on. `test` is included so the bin's own unit tests still see
    // the route (they pin the #1811 runtime gate); a release build never has
    // cfg(test). The route remains inside the #2136 OriginGuard scope. Absence
    // from the shipped binary is proven, not assumed:
    // `scripts/check_release_endpoints.py --binary <launch_api>` scans it for
    // every route RPC_SURFACE.md marks `(feature: dev-endpoints)`.
    #[cfg(any(test, feature = "dev-endpoints"))]
    let scope = scope.route("/debug/seed-account", web::post().to(debug::seed_account));

    // P2P network introspection routes (only compiled with --features p2p)
    #[cfg(feature = "p2p")]
    let scope = scope
        .route("/network/peers", web::get().to(network::get_network_peers))
            .route("/network/stats", web::get().to(network::get_network_stats))
            .route("/network/peer_id", web::get().to(network::get_network_peer_id))
            .route("/network/dht/peers", web::get().to(network::get_network_dht_peers))
            .route("/network/peers/sync", web::get().to(network::get_network_peers_sync))
            .route(
                "/network/peers/reputation",
                web::get().to(network::get_network_peer_reputation),
            );

    // ZK proof endpoints (only compiled with --features zk)
    #[cfg(feature = "zk")]
    let scope = {
        use zk::zk_handlers;
        scope.route(
            "/zk/range-proof/generate",
            web::post().to(zk_handlers::generate_range_proof),
        )
        .route(
            "/zk/range-proof/verify",
            web::post().to(zk_handlers::verify_range_proof),
        )
        .route(
            "/zk/confidential-transfer/generate",
            web::post().to(zk_handlers::generate_confidential_transfer),
        )
        .route(
            "/zk/confidential-transfer/verify",
            web::post().to(zk_handlers::verify_confidential_transfer),
        )
        .route(
            "/zk/selective-disclosure/generate",
            web::post().to(zk_handlers::generate_selective_disclosure),
        )
        .route(
            "/zk/selective-disclosure/verify",
            web::post().to(zk_handlers::verify_selective_disclosure),
        )
        .route(
            "/zk/weather-model/generate",
            web::post().to(zk_handlers::generate_weather_model_proof),
        )
        .route(
            "/zk/weather-model/verify",
            web::post().to(zk_handlers::verify_weather_model_proof),
        )
        .route("/zk/stats", web::get().to(zk_handlers::get_zk_stats))
        .route("/zk/circuits", web::get().to(zk_handlers::list_circuits))
    };

    // Execution endpoints (only compiled with --features execution)
    #[cfg(feature = "execution")]
    let scope = scope
        .route("/execution/run", web::post().to(execution::submit_run))
            .route("/execution/models", web::get().to(execution::list_models))
            .route(
                "/execution/result/{run_id}",
                web::get().to(execution::get_result),
            );

    // Privacy endpoints (only compiled with --features privacy)
    // All privacy endpoints require authentication (issue #452).
    #[cfg(feature = "privacy")]
    let scope = {
        use privacy::privacy_handlers;
        scope.route(
            "/privacy/keys/register",
            web::post().to(privacy_handlers::register_key),
        )
        .route(
            "/privacy/stealth-keys/generate",
            web::post().to(privacy_handlers::generate_stealth_keys),
        )
        .route(
            "/privacy/stealth-address/create",
            web::post().to(privacy_handlers::create_stealth_address),
        )
        .route(
            "/privacy/stealth-address/check",
            web::post().to(privacy_handlers::check_stealth_address),
        )
        .route(
            "/privacy/commitment/create",
            web::post().to(privacy_handlers::create_commitment),
        )
        .route(
            "/privacy/commitment/verify",
            web::post().to(privacy_handlers::verify_commitment),
        )
        .route(
            "/privacy/range-proof/create",
            web::post().to(privacy_handlers::create_range_proof),
        )
        .route(
            "/privacy/range-proof/verify",
            web::post().to(privacy_handlers::verify_range_proof),
        )
        .route(
            "/privacy/ring-signature/create",
            web::post().to(privacy_handlers::create_ring_signature),
        )
        .route(
            "/privacy/ring-signature/verify",
            web::post().to(privacy_handlers::verify_ring_signature),
        )
        .route(
            "/privacy/payload/encrypt",
            web::post().to(privacy_handlers::encrypt_payload),
        )
        .route(
            "/privacy/payload/decrypt",
            web::post().to(privacy_handlers::decrypt_payload),
        )
        .route(
            "/privacy/key-images",
            web::get().to(privacy_handlers::get_key_images),
        )
    };

    cfg.service(scope);
}
