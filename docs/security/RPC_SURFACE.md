# RPC and Node API Surface

This is the authoritative inventory for the native-chain `launch_api`. It reconciles the
historical CLI/backend route notes in `docs/ROUTE_MATRIX.md` and
`docs/MAIN_API_ROUTE_CONTRACT_CHECKLIST.md` with the routes registered in
`native-chain/src/bin/launch_api/routes/mod.rs` and the client messages accepted by
`native-chain/src/bin/launch_api/websocket.rs`.

## Scope and classification rule

- Inventory total: **156 methods** (153 HTTP, 3 WebSocket, 0 JSON-RPC).
- Classification totals: **30 public**, **80 authenticated**, **40 admin**, and **6 disabled-in-release**.
- The inventory is the union of the default routes and the `p2p`, `zk`, `execution`, and `privacy` feature-gated routes. A feature annotation in the handler column identifies conditional registration.
- `public` means no bearer token is required. `authenticated` means any non-operator role enforced by `require_role` or the authenticated WebSocket handshake. `admin` means `ApiRole::Operator` is enforced. Health/readiness remain public because anonymous callers receive deliberately reduced responses; localhost/operator callers receive detail.
- Applied mechanically: a method that can alter peers or partition state, modify validator/oracle/signing-key state, seed or purge node state, control bridge safety, mine blocks, submit rollup batches, or otherwise perform an operator mutation is `admin`. User-signed protocol transactions remain `authenticated`, except the deliberately permissionless signed submission route.
- `disabled-in-release` rows are one of two things: registered compatibility stubs that return a feature-disabled envelope rather than exposing the named capability, or **dev endpoints** that are compiled out of release builds behind the `dev-endpoints` cargo feature — annotated `(feature: dev-endpoints)` in the handler column (#2139, see *Dev endpoints* below).
- Every route is covered by the global per-IP read/write limiter. The three transaction ingress routes and execution runner also have the listed specialized limiter. WebSocket frames have authentication and subscription bounds, but no per-frame rate class.

## Authentication gap analysis

**Unauthenticated methods that should be admin: 0.** Every method matching the mechanical
administrative rule currently enforces `ApiRole::Operator`. The only unauthenticated write,
`POST /transactions/permissionless/submit`, is intentionally public and requires a signed,
replay-protected protocol transaction with its own signer-based rate limit. No follow-up
security issue was required by this inventory.

The `unbounded work risk` column marks handlers that enumerate growing state, construct
snapshots, run proof-heavy cryptography, or launch execution work and therefore deserve
explicit ceilings in the later query-cost/rate-limit stories. It is a risk flag, not a claim
that exploitation is currently demonstrated.

## Method inventory

| Method | Transport | Handler path:symbol | Classification | Mutates state? | Unbounded work risk | Rate-limit class |
| --- | --- | --- | --- | --- | --- | --- |
| `GET /ws` | `ws` | `native-chain/src/bin/launch_api/websocket.rs:crate::websocket::ws_index` | `authenticated` | no | no | global-read handshake; frames unmetered |
| `GET /health` | `http` | `native-chain/src/bin/launch_api/routes/chain.rs:chain::health` | `public` | no | no | global-read |
| `GET /version` | `http` | `native-chain/src/bin/launch_api/routes/chain.rs:chain::version` | `admin` | no | no | global-read |
| `GET /readyz` | `http` | `native-chain/src/bin/launch_api/routes/chain.rs:chain::readyz` | `public` | no | no | global-read |
| `GET /network/audit-log` | `http` | `native-chain/src/bin/launch_api/routes/network.rs:network::get_network_audit_log` | `admin` | no | no | global-read |
| `GET /network/info` | `http` | `native-chain/src/bin/launch_api/routes/network.rs:network::get_network_info` | `public` | no | no | global-read |
| `GET /network/scarcity` | `http` | `native-chain/src/bin/launch_api/routes/scarcity.rs:scarcity::get_scarcity` | `authenticated` | no | no | global-read |
| `GET /network/upgrade` | `http` | `native-chain/src/bin/launch_api/routes/network.rs:network::get_network_upgrade` | `public` | no | no | global-read |
| `GET /blockchain` | `http` | `native-chain/src/bin/launch_api/routes/chain.rs:chain::get_blockchain_compat` | `public` | no | no | global-read |
| `GET /chain/blocks` | `http` | `native-chain/src/bin/launch_api/routes/chain.rs:chain::get_chain_blocks` | `public` | no | no | global-read |
| `GET /chain/block/{height}` | `http` | `native-chain/src/bin/launch_api/routes/chain.rs:chain::get_chain_block_by_height` | `public` | no | no | global-read |
| `GET /addresses/{address}/transactions` | `http` | `native-chain/src/bin/launch_api/routes/chain.rs:chain::get_address_transactions` | `authenticated` | no | no | global-read |
| `GET /mempool/pending` | `http` | `native-chain/src/bin/launch_api/routes/chain.rs:chain::get_mempool_pending` | `authenticated` | no | no | global-read |
| `GET /chain/header/{height}` | `http` | `native-chain/src/bin/launch_api/routes/light.rs:light::get_chain_header` | `public` | no | no | global-read |
| `GET /da/chunk/{height}/{index}` | `http` | `native-chain/src/bin/launch_api/routes/light.rs:light::get_da_chunk` | `public` | no | no | global-read |
| `POST /transactions/submit` | `http` | `native-chain/src/bin/launch_api/routes/transactions.rs:transactions::submit_transaction_compat` | `authenticated` | yes | no | global-write + legacy-submit |
| `POST /transactions/permissionless/submit` | `http` | `native-chain/src/bin/launch_api/routes/transactions.rs:transactions::submit_permissionless_transaction` | `public` | yes | no | global-write + signer-submit |
| `GET /participation/policy` | `http` | `native-chain/src/bin/launch_api/routes/transactions.rs:transactions::get_participation_policy` | `public` | no | no | global-read |
| `GET /accounts/{address}/session-keys` | `http` | `native-chain/src/bin/launch_api/routes/session_keys.rs:session_keys::list_session_keys` | `authenticated` | no | no | global-read |
| `POST /session-keys/submit` | `http` | `native-chain/src/bin/launch_api/routes/session_keys.rs:session_keys::submit_session_key_action` | `authenticated` | yes | no | global-write + legacy-submit (failed authorization) |
| `GET /accounts/{address}/security-key` | `http` | `native-chain/src/bin/launch_api/routes/security_key.rs:security_key::get_security_key_status` | `public` | no | no | global-read |
| `GET /addresses/{address}` | `http` | `native-chain/src/bin/launch_api/routes/chain.rs:chain::get_address_compat` | `authenticated` | no | no | global-read |
| `GET /prometheus` | `http` | `native-chain/src/bin/launch_api/routes/chain.rs:chain::get_prometheus_metrics` | `authenticated` | no | no | global-read |
| `GET /metrics` | `http` | `native-chain/src/bin/launch_api/routes/chain.rs:chain::get_prometheus_metrics` | `authenticated` | no | no | global-read |
| `GET /heatmap/json` | `http` | `native-chain/src/bin/launch_api/routes/chain.rs:chain::get_heatmap_json_compat` | `public` | no | no | global-read |
| `GET /cross_chain` | `http` | `native-chain/src/bin/launch_api/routes/chain.rs:chain::cross_chain_feature_disabled` | `disabled-in-release` | no | no | global-read |
| `GET /cross_chain/status` | `http` | `native-chain/src/bin/launch_api/routes/chain.rs:chain::cross_chain_feature_disabled` | `disabled-in-release` | no | no | global-read |
| `GET /governance/proposals` | `http` | `native-chain/src/bin/launch_api/routes/governance.rs:governance::list_governance_proposals` | `public` | no | yes | global-read |
| `POST /governance/proposals/submit` | `http` | `native-chain/src/bin/launch_api/routes/governance.rs:governance::submit_governance_proposal` | `admin` | yes | no | global-write |
| `GET /governance/proposals/{id}` | `http` | `native-chain/src/bin/launch_api/routes/governance.rs:governance::get_governance_proposal` | `public` | no | no | global-read |
| `PUT /governance/proposals/{id}/activate` | `http` | `native-chain/src/bin/launch_api/routes/governance.rs:governance::activate_proposal` | `admin` | yes | no | global-write |
| `PUT /governance/proposals/{id}/finalize` | `http` | `native-chain/src/bin/launch_api/routes/governance.rs:governance::finalize_proposal` | `admin` | yes | no | global-write |
| `PUT /governance/proposals/{id}/execute` | `http` | `native-chain/src/bin/launch_api/routes/governance.rs:governance::execute_proposal` | `admin` | yes | no | global-write |
| `GET /governance/proposals/{id}/votes` | `http` | `native-chain/src/bin/launch_api/routes/governance.rs:governance::get_proposal_votes` | `public` | no | yes | global-read |
| `POST /governance/vote` | `http` | `native-chain/src/bin/launch_api/routes/governance.rs:governance::submit_governance_vote` | `authenticated` | yes | no | global-write |
| `POST /governance/delegate` | `http` | `native-chain/src/bin/launch_api/routes/governance.rs:governance::delegate_voting_power` | `authenticated` | yes | no | global-write |
| `GET /governance/laws` | `http` | `native-chain/src/bin/launch_api/routes/governance.rs:governance::list_governance_laws` | `public` | no | yes | global-read |
| `GET /governance/laws/{law_id}` | `http` | `native-chain/src/bin/launch_api/routes/governance.rs:governance::get_governance_law` | `public` | no | no | global-read |
| `POST /governance/quality/proposals/build` | `http` | `native-chain/src/bin/launch_api/routes/governance.rs:governance::build_quality_governance_proposal` | `authenticated` | no | no | global-write |
| `GET /governance/trust/weights` | `http` | `native-chain/src/bin/launch_api/routes/governance.rs:governance::get_trust_weights` | `public` | no | no | global-read |
| `POST /governance/trust/proposals/build` | `http` | `native-chain/src/bin/launch_api/routes/governance.rs:governance::build_trust_governance_proposal` | `authenticated` | no | no | global-write |
| `GET /governance/treasury` | `http` | `native-chain/src/bin/launch_api/routes/governance.rs:governance::get_treasury` | `public` | no | no | global-read |
| `GET /governance/treasury/transactions` | `http` | `native-chain/src/bin/launch_api/routes/governance.rs:governance::get_treasury_transactions` | `authenticated` | no | yes | global-read |
| `POST /governance/treasury/deposit` | `http` | `native-chain/src/bin/launch_api/routes/governance.rs:governance::treasury_deposit` | `admin` | yes | no | global-write |
| `POST /governance/treasury/withdraw` | `http` | `native-chain/src/bin/launch_api/routes/governance.rs:governance::treasury_withdraw` | `admin` | yes | no | global-write |
| `GET /governance/analytics/dashboard` | `http` | `native-chain/src/bin/launch_api/routes/governance.rs:governance::get_governance_analytics` | `authenticated` | no | yes | global-read |
| `GET /enhanced_security/status` | `http` | `native-chain/src/bin/launch_api/routes/chain.rs:chain::enhanced_security_feature_disabled` | `disabled-in-release` | no | no | global-read |
| `GET /layer2/status` | `http` | `native-chain/src/bin/launch_api/routes/chain.rs:chain::layer2_feature_disabled` | `disabled-in-release` | no | no | global-read |
| `GET /layer2/rollup-config` | `http` | `native-chain/src/bin/launch_api/routes/chain.rs:chain::get_layer2_rollup_config` | `public` | no | no | global-read |
| `POST /layer2/submit-batch` | `http` | `native-chain/src/bin/launch_api/routes/chain.rs:chain::submit_layer2_batch` | `admin` | yes | no | global-write |
| `GET /evm_compatibility/status` | `http` | `native-chain/src/bin/launch_api/routes/chain.rs:chain::evm_compatibility_feature_disabled` | `disabled-in-release` | no | no | global-read |
| `POST /transactions/contribution/commit` | `http` | `native-chain/src/bin/launch_api/routes/transactions.rs:transactions::submit_contribution_commit` | `authenticated` | yes | no | global-write + contributor-commit + sybil caps (#2148: stations/identity, commits/station/window) |
| `POST /transactions/contribution/reveal` | `http` | `native-chain/src/bin/launch_api/routes/transactions.rs:transactions::submit_contribution_reveal` | `authenticated` | yes | no | global-write |
| `POST /transactions/forecast/commit` | `http` | `native-chain/src/bin/launch_api/routes/transactions.rs:transactions::submit_forecast_commit` | `authenticated` | yes | no | global-write + contributor-commit |
| `POST /transactions/forecast/reveal` | `http` | `native-chain/src/bin/launch_api/routes/transactions.rs:transactions::submit_forecast_reveal` | `authenticated` | yes | yes | global-write |
| `GET /transactions/quality/events` | `http` | `native-chain/src/bin/launch_api/routes/quality.rs:quality::get_quality_events` | `authenticated` | no | yes | global-read |
| `GET /transactions/quality/summary` | `http` | `native-chain/src/bin/launch_api/routes/quality.rs:quality::get_quality_summary` | `authenticated` | no | no | global-read |
| `GET /transactions/rewards/trust/timeline` | `http` | `native-chain/src/bin/launch_api/routes/quality.rs:quality::get_trust_reward_impact_timeline` | `authenticated` | no | yes | global-read |
| `GET /transactions/rewards/trust/summary` | `http` | `native-chain/src/bin/launch_api/routes/quality.rs:quality::get_trust_reward_impact_summary` | `authenticated` | no | no | global-read |
| `GET /transactions/quality/participants/{address}/history` | `http` | `native-chain/src/bin/launch_api/routes/quality.rs:quality::get_quality_participant_history` | `authenticated` | no | yes | global-read |
| `POST /transactions/oracle/attestation` | `http` | `native-chain/src/bin/launch_api/routes/oracle.rs:oracle::submit_oracle_attestation` | `authenticated` | yes | no | global-write |
| `POST /transactions/oracle/registrations` | `http` | `native-chain/src/bin/launch_api/routes/oracle.rs:oracle::submit_oracle_registrations` | `admin` | yes | no | global-write |
| `POST /transactions/governance/schedule-extensions` | `http` | `native-chain/src/bin/launch_api/routes/schedule_extension.rs:schedule_extension::submit_schedule_extension` | `admin` | yes | no | global-write |
| `POST /transactions/oracle/operators` | `http` | `native-chain/src/bin/launch_api/routes/oracle.rs:oracle::register_oracle_operator` | `admin` | yes | no | global-write |
| `POST /transactions/station/registrations` | `http` | `native-chain/src/bin/launch_api/routes/station.rs:station::submit_station_registrations` | `admin` | yes | no | global-write |
| `GET /transactions/station/registrations` | `http` | `native-chain/src/bin/launch_api/routes/station.rs:station::list_station_registrations` | `admin` | no | no (O(1) registry ceiling, bounded page selection) | global-read |
| `GET /transactions/station/registrations/{station_id}` | `http` | `native-chain/src/bin/launch_api/routes/station.rs:station::get_station_registration` | `authenticated` | no | no | global-read |
| `GET /transactions/station/registrations/{station_id}/owner-ledger` | `http` | `native-chain/src/bin/launch_api/routes/station.rs:station::get_station_owner_ledger` | `authenticated` | no | no | global-read |
| `GET /accounts/{address}/send-allowlist` | `http` | `native-chain/src/bin/launch_api/routes/send_allowlist.rs:send_allowlist::show` | `authenticated` | no | no | global-read |
| `POST /transactions/station/owner-actions` | `http` | `native-chain/src/bin/launch_api/routes/station.rs:station::submit_station_owner_transactions` | `authenticated` | yes | no | global-write |
| `GET /transactions/contributions/status` | `http` | `native-chain/src/bin/launch_api/routes/transactions.rs:transactions::get_contribution_statuses` | `authenticated` | no | no | global-read |
| `GET /transactions/contributions/{station_id}/observations` | `http` | `native-chain/src/bin/launch_api/routes/transactions.rs:transactions::get_station_observations` | `authenticated` | no | yes | global-read |
| `GET /transactions/contributions/{commit_id}/reward-evidence` | `http` | `native-chain/src/bin/launch_api/routes/transactions.rs:transactions::get_contribution_reward_evidence` | `authenticated` | no | no | global-read (block search bounded by `WRFCOIN_REWARD_EVIDENCE_SCAN_BLOCKS`, chunked archive reads) |
| `GET /transactions/commit/{commit_id}/status` | `http` | `native-chain/src/bin/launch_api/routes/transactions.rs:transactions::get_commit_status` | `public` | no | no | global-read |
| `GET /transactions/stake/metrics` | `http` | `native-chain/src/bin/launch_api/routes/stake.rs:stake::get_stake_metrics` | `authenticated` | no | no | global-read |
| `GET /transactions/stake/{address}` | `http` | `native-chain/src/bin/launch_api/routes/stake.rs:stake::get_stake_status` | `authenticated` | no | no | global-read |
| `GET /transactions/oracle/config` | `http` | `native-chain/src/bin/launch_api/routes/oracle.rs:oracle::get_oracle_config` | `authenticated` | no | no | global-read |
| `GET /transactions/oracle/status` | `http` | `native-chain/src/bin/launch_api/routes/oracle.rs:oracle::get_oracle_status` | `authenticated` | no | no | global-read |
| `GET /transactions/{tx_id}/status` | `http` | `native-chain/src/bin/launch_api/routes/chain.rs:chain::get_transaction_status` | `authenticated` | no | no | global-read |
| `GET /transactions/oracle/pending` | `http` | `native-chain/src/bin/launch_api/routes/oracle.rs:oracle::get_pending_attestations` | `authenticated` | no | no | global-read |
| `GET /oracle/leaderboard` | `http` | `native-chain/src/bin/launch_api/routes/oracle.rs:oracle::get_oracle_leaderboard` | `authenticated` | no | no | global-read |
| `GET /oracle/late-quorum/unpaid` | `http` | `native-chain/src/bin/launch_api/routes/late_quorum.rs:late_quorum::get_late_quorum_unpaid` | `authenticated` | no | yes | global-read |
| `PUT /transactions/oracle/config` | `http` | `native-chain/src/bin/launch_api/routes/oracle.rs:oracle::update_oracle_config` | `admin` | yes | no | global-write |
| `POST /transactions/signing-keys/register` | `http` | `native-chain/src/bin/launch_api/routes/signing_keys.rs:signing_keys::register_signing_key_epoch` | `admin` | yes | no | global-write |
| `POST /transactions/signing-keys/revoke` | `http` | `native-chain/src/bin/launch_api/routes/signing_keys.rs:signing_keys::revoke_signing_key_epoch` | `admin` | yes | no | global-write |
| `GET /transactions/signing-keys/{role}` | `http` | `native-chain/src/bin/launch_api/routes/signing_keys.rs:signing_keys::get_signing_keys_by_role` | `admin` | no | no | global-read |
| `POST /network/signing-keys/register` | `http` | `native-chain/src/bin/launch_api/routes/signing_keys.rs:signing_keys::register_signing_key_epoch` | `admin` | yes | no | global-write |
| `POST /network/signing-keys/revoke` | `http` | `native-chain/src/bin/launch_api/routes/signing_keys.rs:signing_keys::revoke_signing_key_epoch` | `admin` | yes | no | global-write |
| `GET /network/signing-keys/{role}` | `http` | `native-chain/src/bin/launch_api/routes/signing_keys.rs:signing_keys::get_signing_keys_by_role` | `admin` | no | no | global-read |
| `GET /state/snapshot` | `http` | `native-chain/src/bin/launch_api/routes/snapshot.rs:snapshot::get_state_snapshot` | `authenticated` | no | yes | global-read |
| `GET /state/snapshot/entries` | `http` | `native-chain/src/bin/launch_api/routes/snapshot.rs:snapshot::get_state_snapshot_entries` | `authenticated` | no | yes | global-read |
| `GET /state/snapshot/proof/{key_hex}` | `http` | `native-chain/src/bin/launch_api/routes/snapshot.rs:snapshot::get_state_snapshot_proof` | `authenticated` | no | no | global-read |
| `POST /state/snapshot/verify-inclusion` | `http` | `native-chain/src/bin/launch_api/routes/snapshot.rs:snapshot::verify_state_snapshot_inclusion` | `authenticated` | no | no | global-write |
| `GET /state/snapshot/sync-payload` | `http` | `native-chain/src/bin/launch_api/routes/snapshot.rs:snapshot::get_snapshot_sync_payload` | `authenticated` | no | yes | global-read |
| `POST /state/snapshot/sync-payload/verify` | `http` | `native-chain/src/bin/launch_api/routes/snapshot.rs:snapshot::verify_snapshot_sync_payload` | `authenticated` | no | no | global-write |
| `POST /network/snapshot-sync/ingest` | `http` | `native-chain/src/bin/launch_api/routes/snapshot.rs:snapshot::ingest_snapshot_sync_payload` | `admin` | yes | yes | global-write |
| `GET /state/snapshot/export` | `http` | `native-chain/src/bin/launch_api/routes/snapshot.rs:snapshot::get_state_snapshot_export` | `authenticated` | no | yes | global-read |
| `POST /network/snapshot-sync/receipt` | `http` | `native-chain/src/bin/launch_api/routes/snapshot.rs:snapshot::post_snapshot_service_receipt` | `authenticated` | yes | no | global-write |
| `GET /network/snapshot-sync/receipts` | `http` | `native-chain/src/bin/launch_api/routes/snapshot.rs:snapshot::get_snapshot_service_receipts` | `admin` | no | no | global-read |
| `POST /blocks/mine` | `http` | `native-chain/src/bin/launch_api/routes/mining.rs:mining::mine_block` | `admin` | yes | no | global-write |
| `GET /mining/stats` | `http` | `native-chain/src/bin/launch_api/routes/mining.rs:mining::get_mining_stats` | `public` | no | no | global-read |
| `POST /rewards/payout` | `http` | `native-chain/src/bin/launch_api/routes/rewards.rs:rewards::payout` | `admin` | yes | no | global-write |
| `GET /node/sync/status` | `http` | `native-chain/src/bin/launch_api/routes/network.rs:network::get_node_sync_status` | `public` | no | no | global-read |
| `GET /node/network/failover` | `http` | `native-chain/src/bin/launch_api/routes/network.rs:network::get_node_network_failover` | `public` | no | no | global-read |
| `GET /node/resources` | `http` | `native-chain/src/bin/launch_api/routes/network.rs:network::get_node_resources` | `public` | no | no | global-read |
| `GET /node/capability` | `http` | `native-chain/src/bin/launch_api/routes/network.rs:network::get_node_capability` | `public` | no | no | global-read |
| `GET /node/metrics/host` | `http` | `native-chain/src/bin/launch_api/routes/host_metrics.rs:host_metrics::get_host_metrics` | `admin` | no | no | global-read |
| `GET /dashboard` | `http` | `native-chain/src/bin/launch_api/routes/status_page.rs:status_page::dashboard` | `public` | no | no | global-read |
| `POST /node/shutdown` | `http` | `native-chain/src/bin/launch_api/routes/node_shutdown.rs:node_shutdown::shutdown_node` | `admin` | yes | no | global-write |
| `GET /node/partition-status` | `http` | `native-chain/src/bin/launch_api/routes/network.rs:network::get_partition_status` | `public` | no | no | global-read |
| `POST /operator/partition-mode` | `http` | `native-chain/src/bin/launch_api/routes/network.rs:network::set_partition_mode` | `admin` | yes | no | global-write |
| `POST /debug/seed-account` | `http` | `native-chain/src/bin/launch_api/routes/debug.rs:debug::seed_account (feature: dev-endpoints)` | `disabled-in-release` | yes | no | global-write |
| `POST /operator/mempool/purge` | `http` | `native-chain/src/bin/launch_api/routes/operator.rs:operator::purge_mempool` | `admin` | yes | no | global-write |
| `POST /mempool/purge` | `http` | `native-chain/src/bin/launch_api/routes/operator.rs:operator::purge_mempool` | `admin` | yes | no | global-write |
| `POST /operator/mempool/clear` | `http` | `native-chain/src/bin/launch_api/routes/operator.rs:operator::clear_mempool` | `admin` | yes | no | global-write |
| `GET /bridge/safety/status` | `http` | `native-chain/src/bin/launch_api/routes/bridge.rs:bridge::get_bridge_safety_status` | `authenticated` | no | no | global-read |
| `POST /bridge/safety/halt` | `http` | `native-chain/src/bin/launch_api/routes/bridge.rs:bridge::halt_bridge` | `admin` | yes | no | global-write |
| `POST /bridge/safety/resume` | `http` | `native-chain/src/bin/launch_api/routes/bridge.rs:bridge::resume_bridge` | `admin` | yes | no | global-write |
| `POST /bridge/safety/challenge/open` | `http` | `native-chain/src/bin/launch_api/routes/bridge.rs:bridge::open_bridge_challenge` | `admin` | yes | no | global-write |
| `POST /bridge/safety/challenge/submit` | `http` | `native-chain/src/bin/launch_api/routes/bridge.rs:bridge::submit_bridge_challenge` | `authenticated` | yes | no | global-write |
| `POST /bridge/safety/challenge/resolve` | `http` | `native-chain/src/bin/launch_api/routes/bridge.rs:bridge::resolve_bridge_challenge` | `admin` | yes | no | global-write |
| `POST /bridge/safety/caps/update` | `http` | `native-chain/src/bin/launch_api/routes/bridge.rs:bridge::update_bridge_caps` | `admin` | yes | no | global-write |
| `GET /bridge/safety/exposure` | `http` | `native-chain/src/bin/launch_api/routes/bridge.rs:bridge::get_bridge_exposure` | `authenticated` | no | no | global-read |
| `GET /bridge/safety/audit-log` | `http` | `native-chain/src/bin/launch_api/routes/bridge.rs:bridge::get_bridge_audit_log` | `authenticated` | no | no | global-read |
| `POST /bridge/safety/scan-stuck` | `http` | `native-chain/src/bin/launch_api/routes/bridge.rs:bridge::scan_bridge_stuck` | `admin` | yes | no | global-write |
| `POST /nft/mint` | `http` | `native-chain/src/bin/launch_api/routes/nft.rs:nft::mint_nft` | `admin` | yes | no | global-write |
| `GET /nft/list` | `http` | `native-chain/src/bin/launch_api/routes/nft.rs:nft::list_nfts` | `authenticated` | no | no | global-read |
| `GET /nft/{id}` | `http` | `native-chain/src/bin/launch_api/routes/nft.rs:nft::get_nft` | `authenticated` | no | no | global-read |
| `POST /nft/{id}/verify` | `http` | `native-chain/src/bin/launch_api/routes/nft.rs:nft::verify_nft` | `authenticated` | yes | no | global-write |
| `POST /nft/{id}/transfer` | `http` | `native-chain/src/bin/launch_api/routes/nft.rs:nft::transfer_nft` | `admin` | yes | no | global-write |
| `GET /network/peers` | `http` | `native-chain/src/bin/launch_api/routes/network.rs:network::get_network_peers (feature: p2p)` | `public` | no | yes | global-read |
| `GET /network/peers/sync` | `http` | `native-chain/src/bin/launch_api/routes/network.rs:network::get_network_peers_sync (feature: p2p)` | `authenticated` | no | yes | global-read |
| `GET /network/stats` | `http` | `native-chain/src/bin/launch_api/routes/network.rs:network::get_network_stats (feature: p2p)` | `public` | no | no | global-read |
| `GET /network/peer_id` | `http` | `native-chain/src/bin/launch_api/routes/network.rs:network::get_network_peer_id (feature: p2p)` | `public` | no | no | global-read |
| `GET /network/dht/peers` | `http` | `native-chain/src/bin/launch_api/routes/network.rs:network::get_network_dht_peers (feature: p2p)` | `authenticated` | no | yes | global-read |
| `GET /network/peers/reputation` | `http` | `native-chain/src/bin/launch_api/routes/network.rs:network::get_network_peer_reputation (feature: p2p)` | `admin` | no | no | global-read |
| `POST /zk/range-proof/generate` | `http` | `native-chain/src/bin/launch_api/routes/zk.rs:zk_handlers::generate_range_proof (feature: zk)` | `authenticated` | no | yes | global-write |
| `POST /zk/range-proof/verify` | `http` | `native-chain/src/bin/launch_api/routes/zk.rs:zk_handlers::verify_range_proof (feature: zk)` | `authenticated` | no | no | global-write |
| `POST /zk/confidential-transfer/generate` | `http` | `native-chain/src/bin/launch_api/routes/zk.rs:zk_handlers::generate_confidential_transfer (feature: zk)` | `authenticated` | no | yes | global-write |
| `POST /zk/confidential-transfer/verify` | `http` | `native-chain/src/bin/launch_api/routes/zk.rs:zk_handlers::verify_confidential_transfer (feature: zk)` | `authenticated` | no | no | global-write |
| `POST /zk/selective-disclosure/generate` | `http` | `native-chain/src/bin/launch_api/routes/zk.rs:zk_handlers::generate_selective_disclosure (feature: zk)` | `authenticated` | no | yes | global-write |
| `POST /zk/selective-disclosure/verify` | `http` | `native-chain/src/bin/launch_api/routes/zk.rs:zk_handlers::verify_selective_disclosure (feature: zk)` | `authenticated` | no | no | global-write |
| `POST /zk/weather-model/generate` | `http` | `native-chain/src/bin/launch_api/routes/zk.rs:zk_handlers::generate_weather_model_proof (feature: zk)` | `authenticated` | no | yes | global-write |
| `POST /zk/weather-model/verify` | `http` | `native-chain/src/bin/launch_api/routes/zk.rs:zk_handlers::verify_weather_model_proof (feature: zk)` | `authenticated` | no | no | global-write |
| `GET /zk/stats` | `http` | `native-chain/src/bin/launch_api/routes/zk.rs:zk_handlers::get_zk_stats (feature: zk)` | `authenticated` | no | no | global-read |
| `GET /zk/circuits` | `http` | `native-chain/src/bin/launch_api/routes/zk.rs:zk_handlers::list_circuits (feature: zk)` | `authenticated` | no | no | global-read |
| `POST /execution/run` | `http` | `native-chain/src/bin/launch_api/routes/execution.rs:execution::submit_run (feature: execution)` | `authenticated` | yes | yes | global-write + token-quota |
| `GET /execution/models` | `http` | `native-chain/src/bin/launch_api/routes/execution.rs:execution::list_models (feature: execution)` | `authenticated` | no | no | global-read |
| `GET /execution/result/{run_id}` | `http` | `native-chain/src/bin/launch_api/routes/execution.rs:execution::get_result (feature: execution)` | `authenticated` | no | no | global-read |
| `POST /privacy/keys/register` | `http` | `native-chain/src/bin/launch_api/routes/privacy.rs:privacy_handlers::register_key (feature: privacy)` | `authenticated` | yes | no | global-write |
| `POST /privacy/stealth-keys/generate` | `http` | `native-chain/src/bin/launch_api/routes/privacy.rs:privacy_handlers::generate_stealth_keys (feature: privacy)` | `authenticated` | no | no | global-write |
| `POST /privacy/stealth-address/create` | `http` | `native-chain/src/bin/launch_api/routes/privacy.rs:privacy_handlers::create_stealth_address (feature: privacy)` | `authenticated` | no | no | global-write |
| `POST /privacy/stealth-address/check` | `http` | `native-chain/src/bin/launch_api/routes/privacy.rs:privacy_handlers::check_stealth_address (feature: privacy)` | `authenticated` | no | no | global-write |
| `POST /privacy/commitment/create` | `http` | `native-chain/src/bin/launch_api/routes/privacy.rs:privacy_handlers::create_commitment (feature: privacy)` | `authenticated` | no | no | global-write |
| `POST /privacy/commitment/verify` | `http` | `native-chain/src/bin/launch_api/routes/privacy.rs:privacy_handlers::verify_commitment (feature: privacy)` | `authenticated` | no | no | global-write |
| `POST /privacy/range-proof/create` | `http` | `native-chain/src/bin/launch_api/routes/privacy.rs:privacy_handlers::create_range_proof (feature: privacy)` | `authenticated` | no | yes | global-write |
| `POST /privacy/range-proof/verify` | `http` | `native-chain/src/bin/launch_api/routes/privacy.rs:privacy_handlers::verify_range_proof (feature: privacy)` | `authenticated` | no | no | global-write |
| `POST /privacy/ring-signature/create` | `http` | `native-chain/src/bin/launch_api/routes/privacy.rs:privacy_handlers::create_ring_signature (feature: privacy)` | `authenticated` | no | yes | global-write |
| `POST /privacy/ring-signature/verify` | `http` | `native-chain/src/bin/launch_api/routes/privacy.rs:privacy_handlers::verify_ring_signature (feature: privacy)` | `authenticated` | no | yes | global-write |
| `POST /privacy/payload/encrypt` | `http` | `native-chain/src/bin/launch_api/routes/privacy.rs:privacy_handlers::encrypt_payload (feature: privacy)` | `authenticated` | no | no | global-write |
| `POST /privacy/payload/decrypt` | `http` | `native-chain/src/bin/launch_api/routes/privacy.rs:privacy_handlers::decrypt_payload (feature: privacy)` | `authenticated` | no | no | global-write |
| `GET /privacy/key-images` | `http` | `native-chain/src/bin/launch_api/routes/privacy.rs:privacy_handlers::get_key_images (feature: privacy)` | `authenticated` | no | no | global-read |
| `SUBSCRIBE /ws` | `ws` | `native-chain/src/bin/launch_api/websocket.rs:ClientMessage::Subscribe` | `authenticated` | no (connection only) | no (32 subscriptions; 256-byte channel cap) | none after handshake |
| `UNSUBSCRIBE /ws` | `ws` | `native-chain/src/bin/launch_api/websocket.rs:ClientMessage::Unsubscribe` | `authenticated` | no (connection only) | no | none after handshake |

## Response-field note: `GET /transactions/oracle/pending` (#3240)

Each pending entry may carry `station_location: {latitude, longitude}`, the
station's registry location **rounded to 0.01° (~1.1 km)**, omitted when the
station has no registry entry. It exists so a service-side attester can pick the
nearest NWS reference station. The exact coordinates are already chain data:
they are in the `StationRegistration` transaction body in its block, and in the
`station_registry` state that peers exchange. This surface serves a deliberately
coarser value, and only to an authenticated Reader-or-higher principal. Note that
`GET /transactions/station/registrations/{station_id}` does **not** return
location, so this is the first dedicated HTTP read of it. That is why it is
coarsened rather than echoed. (`elevation_meters`, added to the same entries by
#3285 for the sea-level pressure recompute, is vertical only and carries no
horizontal position.) The field is unsigned and
read-only, and it is derived from existing state. It is not part of any
transaction, signing preimage or state root, and the method/path/handler triple
is unchanged.

No OpenAPI document in the tree describes this route: the legacy specs under
`docs/` and `sdks/generated/` predate launch_api, and the checked launch_api spec
(#3245, PR #3262) lists `station_location` here as a response-shape follow-up to
transcribe once both have landed.

## Station owner actions: `GET .../owner-ledger` and `POST /transactions/station/owner-actions` (#3345, #3347)

- `GET /transactions/station/registrations/{station_id}/owner-ledger` needs a Reader-or-higher bearer. It returns the station's owner-ledger entry (`owner_nonce`, freeze and owner-deactivation flags, any pending recovery with its notice window), whether each governing feature is active at the height the next block mines at, and the observed and settled height. The values are public chain state or derived from it; nothing is signed or secret. A station that is not registered is a 404.
- `POST /transactions/station/owner-actions` needs a Contributor-or-higher bearer. It accepts the owner-signed transactions of #3345 / #3347 (transfer, owner action, recovery file, cancel and execute), either as full transaction envelopes or as payloads that the node wraps itself. The payload signatures, not the bearer, authorize the action. Each transaction is checked at the next height against current state before it is admitted, a recovery execute is limited to one pending per station, and refusals return a stable `OWNER_TX_*` error code. It changes no consensus rule.
- The same owner transactions are not accepted from the peer-to-peer network; they reach a producer only through this route.

## Receipt admission and retention: snapshot sync (#3126, #2848)

`POST /network/snapshot-sync/receipt` requires Reader-or-higher authorization.
Its caller bucket identifies the authenticated bearer token (hashed in memory),
not a client or source IP. With one token configured per role, all holders of
`WRFCOIN_READER_TOKEN` share one caller's 32 admissions per 60-second monotonic
window and 64 retained receipts. One Reader holder can exhaust that share and
make other Reader holders receive 429. Distinct other-role tokens keep their
caller budgets, subject to the 4,096-receipt node cap and per-consumer-key limits
(eight admissions/minute and 16 retained receipts across bearers). The per-key
rate limit is the only per-consumer rate isolation inside a shared bearer; it
does not reserve capacity for another consumer.

Accepted evidence expires 15 minutes after admission, measured monotonically.
Admission and Operator reads remove expired receipts and bookkeeping without
evicting unexpired evidence. Operators must collect
`GET /network/snapshot-sync/receipts` before expiry and archive externally for
longer retention; reads do not renew the lifetime and restart loses the ledger.
Durable archival and settlement remain deferred to #2848 and need a durable sink.
See [snapshot join](../operator/snapshot-join.md) for the export/admission bounds.

## Response-field note: `GET /transactions/station/registrations` (#3342)

The station registry list returns each station's **exact** registered
`latitude`/`longitude` (plus `elevation_meters`), unlike the Reader-level
`GET /transactions/oracle/pending` note above, which rounds to 0.01°. It is
therefore classified `admin`: `ApiRole::Operator` only, because every
Contributor and Oracle bearer satisfies Reader. Its consumer is the node
operator's own backend, which syncs the rows into `weather_stations` and serves
anonymous readers only a 0.01° projection (backend#1237). The node API is never
proxied publicly (#658). The route is read-only over existing `station_registry`
state under one read lock. It returns a deterministic `station_id` order with a
`limit` (default 200, clamped to 500; `0` → 422) and a strictly-after `after`
keyset cursor. Per-request work is bounded (#3343 review): the registry size is
checked in O(1) against `STATION_LIST_MAX_REGISTRY_ENTRIES` (100,000) before
any enumeration (above it: 503 `STATION_LIST_REGISTRY_TOO_LARGE`), and the page
is selected with a max-heap of at most `limit + 1` entries, i.e. O(limit)
memory and O(n log limit) comparisons, never a collect-and-sort of the
registry. It changes no transaction, signing preimage or state root.

## Response-field note: `GET /network/upgrade` (#3277 S6, #3361 F4)

Besides the signed upgrade notice (#3095), the response carries two read-only
objects, both derived from the canonical chain state under one read lock and
the process-wide schedule the node already runs:

- `scheduled`: the **effective** feature schedule — the base schedule (entries
  with `source: "genesis"`, or `"bootstrap"` for a base entry the genesis
  schedule string does not carry, i.e. a per-chain `LEGACY_CHAIN_BOOTSTRAP`
  row) plus every applied `feature_schedule_extensions` entry
  (`source: "extension"` with `extension_sequence` and `carried_at_height`).
  Each entry says whether this binary implements the feature (`known`) and
  whether it is in force for the next block. `warnings` lists unknown entries
  inside the #3095 run-up window (the same window and WARN/ERROR severities as
  the notice run-up), and `halt_deadline` is the #3093 clean-halt deadline the
  node installed. `version_at_next` is the signing protocol version in force
  for the next block. Bounded: at most 256 entries and 256 warnings
  (`MAX_SCHEDULED_VIEW_ENTRIES`); beyond that the lowest activations are
  dropped, `truncated` is `true` and `total_entries` gives the full count.
- `faucet_sink`: `{address, source: "genesis" | "binding" | "none",
  active_from, at_height}` — the ADR-003 faucet sink the next block pays,
  resolved by the same function `State::effective_faucet_address` calls. The
  block-metadata key `faucet_rewards_minted_total` is a **per-block** value
  (the faucet slice minted in that block), not a running total, despite its
  name; the key is not renamed.

Neither object carries a secret: feature names, heights, sequence numbers and
a public faucet address only. Extension signer keys, release artifacts beyond
what the signed notice already publishes, and bearer tokens are never
included. The fields add no transaction, signing preimage or state-root input,
no environment variable, and the method/path/handler triple and its `public`
classification are unchanged.

## Host and Origin validation (#2136)

Every route in `configure_launch_routes` is registered inside one `web::scope("")` wrapped by
`origin::OriginGuard` (`native-chain/src/bin/launch_api/routes/origin.rs`; the rule is
`api_auth::check_origin`, pure and unit-tested). Before any handler runs:

| Check | Rule | Refusal |
| --- | --- | --- |
| `Host` | must name this node: `localhost`, `127.0.0.1`, `[::1]` plus `WRFCOIN_API_ALLOWED_HOSTS` (comma-separated names, port ignored). A request whose `Host` is not allowlisted is refused **even though it reached the socket** — that is the DNS-rebinding defence. `X-Forwarded-Host` is deliberately ignored. | 403 `HOST_NOT_ALLOWED` |
| `Origin` | when present, must be in the allowlist: `LAUNCH_API_CORS_ORIGINS` (the same list CORS is built from), or the two local dev origins when unset outside production, or nothing in production. Refused before the handler executes, not by withholding a CORS header. `null` is never allowed. One exception: when the browser asserts `Sec-Fetch-Site: same-origin` **and** the `Origin` is exactly `http(s)://` plus the `Host` header the browser sent (which must itself pass the `Host` check), the request comes from the node's own page — the `/dashboard` Stop button's `POST /node/shutdown` — and is judged as a same-origin request with no `Origin`; the handler's Operator bearer check still decides. | 403 `ORIGIN_NOT_ALLOWED` |
| No `Origin` | a **browser-originated** request (any `Sec-Fetch-*` header) to an `admin` method must carry an allowlisted `Origin` — unless the browser itself asserts `Sec-Fetch-Site: same-origin`, i.e. the page was served by this node (`GET /dashboard`, #3599) and the `Host` check has already passed; a same-origin GET never carries `Origin`, so such a request is judged like a non-browser client. Other requests without one proceed while `WRFCOIN_API_ALLOW_NO_ORIGIN` is `1` (the default — the CLI, curl and node-to-node calls send none). | 403 `ORIGIN_REQUIRED` |

Wildcards are unrepresentable: `*` in either allowlist, or an origin that is not
`http(s)://host[:port]`, refuses to build the policy and the node does not start. CORS
(`build_cors_from_env`) is an explicit `allowed_origin` list — it never reflects the request
origin, never `allow_any_origin`, never sets `Allow-Credentials` — and
`tests/host_origin_2136.rs` pins that source shape.

**Deployment note.** A node reached by a host name (`http://n5:8556`, a tailnet name) must list
it in `WRFCOIN_API_ALLOWED_HOSTS`, or every request answers 403 `HOST_NOT_ALLOWED`. Loopback
and IP-literal loopback need nothing.

## Error redaction (#2138)

A handler failure crosses the trust boundary in one shape only. `helpers::envelope_redacted`
takes an `InternalError { kind, detail }` and answers with the class's envelope code, the class's
**static** message (`&'static str` by type — `shared-protocol/src/security/error_redaction.rs`,
so a formatted internal value cannot be assigned to it) and a `correlation_id`, also sent as
`X-Correlation-Id`. The full detail is logged server-side against that id
(`handler error correlation_id=… kind=… detail=…`); support correlates, the client learns only
the class.

| Kind | Envelope code | HTTP | Public message |
| --- | --- | --- | --- |
| `Storage` | `STORAGE_ERROR` | 500 | The node could not read or write its local state |
| `Consensus` | `CONSENSUS_ERROR` | 500 | The node could not apply the requested consensus operation |
| `Crypto` | `CRYPTO_ERROR` | 500 | A cryptographic check failed |
| `Upstream` | `UPSTREAM_ERROR` | 502 | An upstream service did not respond correctly |
| `Internal` | `INTERNAL_SERVER_ERROR` | 500 | The node encountered an internal error |
| `Unavailable` | `SERVICE_UNAVAILABLE` | 503 | This capability is disabled or unavailable on this node |
| `BadRequest` … `RateLimited` | the existing 4xx codes | 400–429 | one static sentence each |

`redaction_findings` is the scanner `shared-protocol/tests/error_redaction.rs` runs over every
kind's serialised public shape: no filesystem path, no version string, no host name or address,
no store/SQL fragment, and none of the secret-name segments `scripts/check_secret_leak.py` uses.
`native-chain/tests/error_redaction_2138.rs` pins that no `INTERNAL_SERVER_ERROR` envelope in
`routes/` interpolates an internal error any more (the seed, bridge-audit and unclassified
block-production failures went through `envelope_redacted` in this story). 4xx messages that only
echo the caller's own malformed input (`invalid hex: …`) are unchanged: they carry nothing internal.

## WebSocket ceilings (#2135)

`GET /ws` lives under independent ceilings (`shared-protocol/src/security/ws_limits.rs`; wired in
`launch_api/websocket.rs`):

| Ceiling | Default (env) | Enforced | Refusal |
| --- | --- | --- | --- |
| connections, node-wide | 1024 (`WRFCOIN_WS_MAX_CONNECTIONS`) | at the handshake, before the upgrade | 503 `WS_TOO_MANY_CONNECTIONS` + `Retry-After` |
| connections per client IP | 16 (`WRFCOIN_WS_MAX_PER_IP`) | at the handshake; the IP is the rate limiter's trusted-proxy-aware identity | 503 `WS_TOO_MANY_FROM_IP` |
| subscriptions per connection | 32 (`WRFCOIN_WS_MAX_SUBSCRIPTIONS_PER_CONN`) | on `subscribe` | error frame `SUBSCRIPTION_LIMIT` |
| subscriptions, node-wide | 16 384 (`WRFCOIN_WS_MAX_SUBSCRIPTIONS_TOTAL`) | on `subscribe`, independently of the per-connection cap | error frame `WS_NODE_SUBSCRIPTION_LIMIT` |
| outbound queue | 256 frames (`WRFCOIN_WS_MAX_OUTBOUND_QUEUE`) | frames sent since the client's last pong | slow-consumer policy |
| slow-consumer policy | `disconnect` (`WRFCOIN_WS_SLOW_CONSUMER=disconnect\|drop-oldest`) | when the outbound queue is full | close `1008 slow consumer`, or keep the newest frames and log each drop |
| idle timeout | 300 s (`WRFCOIN_WS_IDLE_TIMEOUT_SECS`) | heartbeat ping every ≤30 s; no frame or pong for the timeout | close `1008 idle timeout` |

**Why disconnect is the default.** A client whose frames are silently dropped believes it has the
events it missed; it is quietly wrong, and nothing tells it so. An honest close lets it
reconnect and resync. `drop-oldest` is available for consumers that prefer liveness over
completeness and can tolerate gaps (every drop is logged with the connection's running count).

A closed or reaped connection releases its slot and its subscriptions through the admission
ticket's drop, whichever way the actor ends.

## Dev endpoints (compiled out of release builds, #2139)

Debug, trace and profiling routes are not turned off in a release binary — they are not in it.
Every such route is registered inside `#[cfg(any(test, feature = "dev-endpoints"))]` in
`routes/mod.rs` (and its handler module is declared under the same gate), so the handler is
absent from any build that does not opt in with `--features dev-endpoints`. `test` is part of
the gate only so the bin's own unit tests, which pin the #1811 runtime gate, keep compiling;
a release build never carries `cfg(test)`.

| Route | Feature | Runtime gate that also applies when compiled in |
| --- | --- | --- |
| `POST /debug/seed-account` | `dev-endpoints` | `WRFCOIN_ENABLE_DEBUG_SEED_API=1` + Operator token (#1811) |

The feature is **off by default** and not enabled by any release profile: `native-chain/Dockerfile`
builds with `--features "full-mining,p2p"`, `Dockerfile.producer-key-pop` with `--features hsm`,
and `release-signing.yml` with no features at all. The heap/async profilers (`dhat-heap`,
`tokio-console`) are likewise opt-in cargo features with no route surface.

Proof rather than policy: `python3 scripts/check_release_endpoints.py --binary <launch_api>`
scans a built binary for the path literal of every row annotated `(feature: dev-endpoints)`
and fails if any is present (a control route that every release registers must be present, so
the scan cannot pass on the wrong file). `--build-profiles` fails if any of the files above
ever names the feature. It runs against the real artifact in `release-signing.yml` and as the
`release-endpoints` job of `deploy-gate.yml`; `native-chain/tests/dev_endpoints_2139.rs` pins
the source-level shape.
## Query cost (#2137)

A query handler is charged for the work it performs, not the rows it returns
(`shared-protocol/src/security/query_cost.rs`: `QueryCost`, `CostMeter`, `HistoryRange`,
`page_size`, `Cursor`, `declared_inputs_within`; `routes/query_cost.rs` supplies the node's
limits, the refusals and the expensive pool).

| Limit | Default (env) | Where it bites | Refusal |
| --- | --- | --- | --- |
| page size | 500 (`WRFCOIN_QUERY_MAX_PAGE_SIZE`), per-route ceilings folded in | proposals, treasury transactions, quality events | 400 `PAGE_TOO_LARGE` — refused, never truncated |
| scan units | 50 000 (`WRFCOIN_QUERY_MAX_SCAN_UNITS`) | one unit per record examined; aborts mid-scan | 422 `QUERY_TOO_EXPENSIVE` |
| history range | 1 000 000 blocks (`WRFCOIN_QUERY_MAX_HISTORY_BLOCKS`) | `from_block`/`to_block` bounded before the first record is read | 400 `RANGE_TOO_WIDE` / `RANGE_INVERTED` |
| wall time | 2000 ms (`WRFCOIN_QUERY_MAX_WALL_MS`) | checked on every charge | 422 `QUERY_TOO_EXPENSIVE` |
| declared inputs | 64 ring decoys, 64 range-proof bits | checked before any allocation (`/privacy/ring-signature/create`, `/privacy/range-proof/create`) | 413 `TOO_MANY_INPUTS` |
| expensive pool | max(1, cores/2) permits (`WRFCOIN_EXPENSIVE_POOL_SIZE`) | proving-class work runs on a blocking thread under a permit; ordinary handlers never touch the pool | 503 `EXPENSIVE_POOL_SATURATED` + `Retry-After`, immediately |

**Cursors.** `GET /governance/proposals`, `GET /governance/treasury/transactions` and
`GET /transactions/quality/events` return `next_cursor`; passing it back as `?cursor=` resumes at
the recorded position, so page *k* costs what page 1 cost. `offset` is still accepted for
existing callers and is refused alongside a cursor (400). A cursor not issued by this node is
refused (400 `BAD_CURSOR`). The zk generate routes keep their own prover gate (#1832); the
expensive pool is the same discipline for the privacy proofs.

## Drift enforcement

`python3 scripts/check_release_endpoints.py` (no arguments) checks that every dev-endpoint row
is classified `disabled-in-release` and that no build profile enables the feature.

`python3 scripts/check_rpc_surface.py` extracts the registered Actix method/path/handler
triples and WebSocket client actions and compares them with this table in both directions.
The check is wired into the pull-request-triggered `ci-deploy-features.yml` workflow.

## Node-fraud report route (core4-archive#3589)

`POST /node/fraud/submit` (`routes/node_fraud.rs:node_fraud::submit`) is registered through a guarded
`web::resource(..)`, like the other `/node/{bonds,receipts}/submit` routes, which the table scanner
(`scripts/check_rpc_surface.py`, `.route(` only) does not see. Classification: `admin` (Operator
token), mutates state (mempool admission and gossip), no unbounded work (40 KiB body, 8 KiB wire cap),
per-token limit 30 requests/minute (429). It is **not mounted** (404) unless
`WRFCOIN_ENABLE_NODE_FRAUD_REPORT_API=1` (default off) and `NodeShareSettlement` is active. A non-forking
change: it admits a transaction type the mempool and `apply` already handle.

## Security-key wallet action route (`security_key_spend`)

`POST /accounts/security-key/actions` (`routes/security_key.rs:security_key::submit_security_key_action`) is
registered through a `web::resource(..)` (32 KiB JSON limit), which the table scanner (`.route(` only) does not
see. Classification: `authenticated` (Contributor token), mutates state (mempool admission of a
`SecurityKeyAction`), bounded work (one ed25519 and at most two P-256 verifications in a State dry-run at tip+1;
failed attempts charge the caller's legacy-submit budget). It answers 503 `FEATURE_NOT_ACTIVE` until
`security_key_spend` is active at the next block. Non-forking: it only admits a transaction the mempool and the
governance sink already handle, and consensus re-checks it at the inclusion height. The read
`GET /accounts/{address}/security-key` is in the table above. `/transactions/submit` and `/session-keys/submit`
also accept an optional `security_key_auth` assertion (no new route).
