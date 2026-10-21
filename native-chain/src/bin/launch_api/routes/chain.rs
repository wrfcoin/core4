// Hot-path performance bundle (#1376/#1377/#1378/#1392): this module hosts
// `/chain/blocks`, `/chain/metrics`, and the Prometheus monitoring snapshot.
// Read paths here are scrape-frequency (every ~15s) so any per-request scan
// or JSON re-parse compounds into steady-state CPU pressure on the launch
// API. Caching guidelines: any cached value MUST return identical output to
// the un-cached version; invalidate on block apply, not on a timer.
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::OnceLock;
use std::time::Instant;

use actix_web::{HttpRequest, HttpResponse, Responder, http::StatusCode, web};
use serde::Deserialize;
use serde_json::json;

use crate::helpers::{
    block_age_is_stale, envelope_error, envelope_success, feature_disabled_envelope, require_role,
};
use crate::routes::quality::{
    QualityRewardMultiplierResponse, TrustClassCountersResponse,
    build_trust_reward_cache_entry, reward_multipliers_from_policy,
    should_include_trust_impact_block,
};
use crate::state::{AppState, TrustRewardCacheEntry};
use wrfcoin_native_chain::blockchain::state::TrustWeightingPolicy;
use wrfcoin_native_chain::blockchain::block::Block;
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::blockchain::transaction::{
    Layer2BatchSubmitPayload, Transaction, TransactionType,
};

fn contribution_rewards_response(
    rewards: wrfcoin_native_chain::blockchain::state::ContributionRewardAggregate,
) -> serde_json::Value {
    json!({
        "real_paid": rewards.real_paid.into_inner().to_string(),
        "real_pending": rewards.real_pending.into_inner().to_string(),
        "synthetic_paid": rewards.synthetic_paid.into_inner().to_string(),
        "unclassified_paid": rewards.unclassified_paid.into_inner().to_string(),
        "real_decision_count": rewards.real_decision_count,
        "synthetic_decision_count": rewards.synthetic_decision_count,
        "unclassified_decision_count": rewards.unclassified_decision_count,
    })
}
use crate::helpers::submit_protocol_transaction;
use wrfcoin_native_chain::common::types::Address;
use wrfcoin_native_chain::crypto::hash::Hash;
use wrfcoin_shared_protocol::security::api_auth::ApiRole;
use wrfcoin_native_chain::monitoring::{
    MonitoringInputs, MonitoringSnapshot, NetworkTelemetry, build_snapshot,
};

#[cfg(feature = "p2p")]
pub(crate) fn readyz_p2p_consumer_check(
    health: &crate::state::P2pConsumerHealth,
) -> (serde_json::Value, bool) {
    let status = health.status();
    let blocks = health.blocks_readiness();
    (
        json!({
            "status": status.as_str(),
            "readiness_gate": blocks,
            "policy": "fail_stop",
            // Raw JoinError/panic detail remains server-side; /readyz is public.
            "detail": if blocks {
                Some("inbound P2P consumer is unavailable")
            } else {
                None
            },
            "hint": if blocks {
                Some("Inbound P2P processing is not live. Inspect P2P_EVENT_CONSUMER_FAILED logs and restart the node after correcting the cause.")
            } else {
                None
            }
        }),
        blocks,
    )
}

pub async fn get_blockchain_compat(req: HttpRequest, state: web::Data<AppState>) -> impl Responder {
    let blockchain = state.blockchain.read().await;
    let mempool = state.mempool.read().await;
    envelope_success(
        &req,
        StatusCode::OK,
        json!({
            "height": blockchain.height,
            "tip": blockchain.tip.to_string(),
            "accounts": blockchain.state.accounts.len(),
            "nfts": blockchain.state.nfts.len(),
            "pending_transactions": mempool.size(),
        }),
    )
}

pub async fn get_address_compat(
    req: HttpRequest,
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    let address_raw = path.into_inner();
    let address: Address = match address_raw.parse() {
        Ok(address) => address,
        Err(_) => {
            return envelope_error(
                &req,
                StatusCode::BAD_REQUEST,
                "VALIDATION_ERROR",
                "address must be a valid hex address",
            );
        },
    };

    let blockchain = state.blockchain.read().await;
    let Some(account) = blockchain.state.get_account(&address) else {
        return envelope_error(
            &req,
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "address not found",
        );
    };

    envelope_success(
        &req,
        StatusCode::OK,
        json!({
            "address": address.to_string(),
            "balance": account.balance.into_inner().to_string(),
            "nonce": account.nonce.to_string(),
            "stake_required": account.stake_required.into_inner().to_string(),
            "stake_locked": account.stake_locked.into_inner().to_string(),
            "spendable_balance": account.spendable_balance.into_inner().to_string(),
            "earned_balance_total": account.earned_balance_total.into_inner().to_string(),
            "contribution_rewards": contribution_rewards_response(blockchain.state.contribution_reward_aggregate(&address)),
        }),
    )
}

/// Returns aggregated trust/quality metrics for `/chain/metrics` (#1392, #1476).
///
/// Per-block JSON metadata is parsed once and stored in `cache`; subsequent
/// calls skip the re-parse and read cached struct fields directly.  The cache
/// is pruned to the current in-memory block window on each call so it cannot
/// grow beyond `Blockchain::blocks()`.  Identical memoization contract to
/// `count_observed_privacy_proofs` (#1469).
///
/// Returns `(trust_class_distribution, trust_weighted_sum, trust_weighted_denominator,
///           quality_weighted_sum, quality_weighted_denominator, blocks_observed,
///           latest_block_height)`.
fn collect_trust_reward_metrics_cached(
    blockchain: &wrfcoin_native_chain::blockchain::chain::Blockchain,
    cache: &std::sync::Mutex<rustc_hash::FxHashMap<u64, TrustRewardCacheEntry>>,
    current_trust_policy: &TrustWeightingPolicy,
    default_quality_multipliers: &QualityRewardMultiplierResponse,
) -> (TrustClassCountersResponse, f64, f64, f64, f64, usize, Option<u64>) {
    let blocks = blockchain.blocks();
    let mut cache = cache.lock().expect("trust_reward_metrics_cache poisoned");
    cache.retain(|height, _| blocks.contains_key(height));

    let mut trust_class_distribution = TrustClassCountersResponse::default();
    let mut trust_weighted_sum = 0.0f64;
    let mut trust_weighted_denominator = 0.0f64;
    let mut quality_weighted_sum = 0.0f64;
    let mut quality_weighted_denominator = 0.0f64;
    let mut blocks_observed = 0usize;
    let mut latest_height: Option<u64> = None;

    for (height, block) in blocks {
        if !should_include_trust_impact_block(block) {
            continue;
        }
        let entry = cache
            .entry(*height)
            .or_insert_with(|| build_trust_reward_cache_entry(block));

        trust_class_distribution.unverified = trust_class_distribution
            .unverified
            .saturating_add(entry.trust_unverified);
        trust_class_distribution.verified_station = trust_class_distribution
            .verified_station
            .saturating_add(entry.trust_verified_station);
        trust_class_distribution.trusted_provider = trust_class_distribution
            .trusted_provider
            .saturating_add(entry.trust_trusted_provider);

        let (uw, vsw, tpw) = entry
            .trust_weights
            .as_ref()
            .map(|tw| {
                (
                    tw.unverified_weight,
                    tw.verified_station_weight,
                    tw.trusted_provider_weight,
                )
            })
            .unwrap_or((
                current_trust_policy.unverified_weight,
                current_trust_policy.verified_station_weight,
                current_trust_policy.trusted_provider_weight,
            ));
        trust_weighted_sum += entry.trust_unverified as f64 * uw;
        trust_weighted_sum += entry.trust_verified_station as f64 * vsw;
        trust_weighted_sum += entry.trust_trusted_provider as f64 * tpw;
        trust_weighted_denominator += (entry.trust_unverified
            + entry.trust_verified_station
            + entry.trust_trusted_provider) as f64;

        let [qh, qb, qw, ql] = entry.quality_multipliers.unwrap_or([
            default_quality_multipliers.high,
            default_quality_multipliers.borderline,
            default_quality_multipliers.warning_only,
            default_quality_multipliers.low,
        ]);
        quality_weighted_sum += entry.quality_high as f64 * qh;
        quality_weighted_sum += entry.quality_borderline as f64 * qb;
        quality_weighted_sum += entry.quality_warning_only as f64 * qw;
        quality_weighted_sum += entry.quality_low as f64 * ql;
        quality_weighted_denominator += (entry.quality_high
            + entry.quality_borderline
            + entry.quality_warning_only
            + entry.quality_low) as f64;

        blocks_observed += 1;
        // BTreeMap iterates ascending; overwrite unconditionally to get the max.
        latest_height = Some(*height);
    }

    (
        trust_class_distribution,
        trust_weighted_sum,
        trust_weighted_denominator,
        quality_weighted_sum,
        quality_weighted_denominator,
        blocks_observed,
        latest_height,
    )
}

#[allow(dead_code)]
pub async fn get_metrics_compat(req: HttpRequest, state: web::Data<AppState>) -> impl Responder {
    let blockchain = state.blockchain.read().await;
    let mempool = state.mempool.read().await;
    let pending_transactions = mempool.size();
    let current_trust_policy = blockchain.state.governance.params.trust_weighting_policy.clone();
    let current_quality_policy = blockchain.state.governance.params.data_quality_policy.clone();
    let default_quality_multipliers = reward_multipliers_from_policy(&current_quality_policy);

    // #1392 + #1476: use cached per-block data to skip JSON re-parse on every scrape.
    let (
        trust_class_distribution,
        trust_weighted_sum,
        trust_weighted_denominator,
        quality_weighted_sum,
        quality_weighted_denominator,
        trust_impact_blocks_observed,
        latest_trust_impact_height,
    ) = collect_trust_reward_metrics_cached(
        &blockchain,
        &state.trust_reward_metrics_cache,
        &current_trust_policy,
        &default_quality_multipliers,
    );

    let estimated_weighted_trust_multiplier = if trust_weighted_denominator > f64::EPSILON {
        trust_weighted_sum / trust_weighted_denominator
    } else {
        0.0
    };
    let estimated_weighted_quality_multiplier = if quality_weighted_denominator > f64::EPSILON {
        quality_weighted_sum / quality_weighted_denominator
    } else {
        0.0
    };
    let monitoring_snapshot = build_runtime_monitoring_snapshot(&state, &blockchain, pending_transactions);

    envelope_success(
        &req,
        StatusCode::OK,
        json!({
            "chain_height": blockchain.height,
            "pending_transactions": pending_transactions,
            "quality_event_count": blockchain.state.quality_policy_events().len(),
            "account_count": blockchain.state.accounts.len(),
            "trust_impact_blocks_observed": trust_impact_blocks_observed,
            "trust_class_distribution_total": trust_class_distribution,
            "trust_weighted_multiplier_estimate": estimated_weighted_trust_multiplier,
            "quality_weighted_multiplier_estimate": estimated_weighted_quality_multiplier,
            "effective_weighted_multiplier_estimate": estimated_weighted_trust_multiplier * estimated_weighted_quality_multiplier,
            "current_trust_weights": current_trust_policy,
            "latest_trust_impact_block_height": latest_trust_impact_height,
            "monitoring_snapshot": monitoring_snapshot,
        }),
    )
}

pub async fn get_heatmap_json_compat(req: HttpRequest, state: web::Data<AppState>) -> impl Responder {
    let blockchain = state.blockchain.read().await;
    let mempool = state.mempool.read().await;
    let snapshot =
        build_runtime_monitoring_snapshot(&state, &blockchain, mempool.size());
    let heatmap_value = match serde_json::to_value(snapshot.heatmap) {
        Ok(v) => v,
        Err(err) => {
            log::error!("heatmap serialization failed: {err}");
            return envelope_error(
                &req,
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_SERVER_ERROR",
                "Failed to serialize heatmap",
            );
        }
    };
    envelope_success(&req, StatusCode::OK, heatmap_value)
}

pub async fn cross_chain_feature_disabled(req: HttpRequest) -> impl Responder {
    envelope_error(
        &req,
        StatusCode::SERVICE_UNAVAILABLE,
        "NOT_AVAILABLE",
        "cross_chain is not available in this release",
    )
}

#[allow(dead_code)]
pub async fn governance_feature_disabled(req: HttpRequest) -> impl Responder {
    feature_disabled_envelope(&req, "governance")
}

pub async fn enhanced_security_feature_disabled(req: HttpRequest) -> impl Responder {
    feature_disabled_envelope(&req, "enhanced_security")
}

pub async fn layer2_feature_disabled(req: HttpRequest) -> impl Responder {
    feature_disabled_envelope(&req, "layer2")
}

pub async fn get_layer2_rollup_config(req: HttpRequest) -> impl Responder {
    let config = wrfcoin_shared_protocol::layer2::RollupConfig::default();
    envelope_success(&req, StatusCode::OK, serde_json::json!({
        "status": "types_migrated",
        "rollup_type": format!("{:?}", config.rollup_type),
        "batch_size": config.batch_size,
    }))
}

/// POST /layer2/submit-batch
///
/// Submit a layer2 rollup batch for on-chain finalization.  The ZK proof is
/// verified by `ZkVerifier::verify_batch_proof` when the transaction is
/// applied to a block via `apply_protocol_transaction`; nodes compiled
/// without the `zk` feature reject every batch via `RejectAllZkVerifier`.
pub async fn submit_layer2_batch(
    req: HttpRequest,
    state: web::Data<AppState>,
    body: web::Json<Layer2BatchSubmitPayload>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }

    let payload_value = serde_json::to_value(&*body).unwrap_or(serde_json::Value::Null);
    submit_protocol_transaction(
        &req,
        state,
        TransactionType::Layer2BatchSubmit,
        &body.operator,
        payload_value,
    )
    .await
}

pub async fn evm_compatibility_feature_disabled(req: HttpRequest) -> impl Responder {
    feature_disabled_envelope(&req, "evm_compatibility")
}

// ---------------------------------------------------------------------------
// Block introspection (#985)
// ---------------------------------------------------------------------------

const DEFAULT_BLOCK_LIST_LIMIT: usize = 20;
const MAX_BLOCK_LIST_LIMIT: usize = 100;

#[derive(Debug, Deserialize)]
pub struct BlockListQuery {
    pub limit: Option<usize>,
}

fn chain_block_summary(block: &Block) -> serde_json::Value {
    json!({
        "height": block.height,
        "timestamp": block.timestamp.to_rfc3339(),
        "hash": block.hash.as_ref().map(|h| h.to_string()).unwrap_or_else(|| "pending".into()),
        "prev_block_hash": block.prev_block_hash.to_string(),
        "tx_count": block.transactions.len(),
    })
}

/// Canonical projection outcome for one committed transaction (#2787).
enum CanonicalProofPayload {
    /// Not one of the two proof-relevant variants: the response shape is
    /// unchanged for this transaction.
    NotApplicable,
    /// Proof-relevant and decoded from the committed payload.
    Decoded(serde_json::Value),
    /// Proof-relevant, but the committed payload does not decode into its
    /// canonical type.
    Undecodable,
}

/// Project the canonical payload fields a quorum verifier needs from a
/// committed transaction (#2787).
///
/// Decoding goes through the transaction's own canonical typed parsers, which
/// read the committed `data` field of the block's transaction — never logs,
/// mempool state, status counters, `sender_address`, or a second mutable
/// lookup. `sender_address` in particular is a legacy compatibility field and
/// is *not* the attesting oracle identity; the oracle identity is only
/// authoritative inside the signed payload.
///
/// Scope is deliberately the two variants the reveal-after-quorum proof needs
/// and only the fields it needs. Everything projected here is already
/// committed to the public block; no signature material, token, key, or
/// environment value is exposed.
///
/// A proof-relevant transaction whose payload will not decode is reported
/// explicitly rather than silently omitted, so a verifier cannot read absence
/// of a payload as absence of an attestation. An undecodable transaction of
/// any other type is `NotApplicable` and therefore cannot take the block
/// endpoint down.
fn canonical_transaction_proof_payload(tx: &Transaction) -> CanonicalProofPayload {
    match tx.tx_type {
        TransactionType::OracleAttestation => match tx.parse_oracle_attestation_payload() {
            Some(payload) => CanonicalProofPayload::Decoded(json!({
                "commit_id": payload.commit_id,
                "oracle_id": payload.oracle_id,
                "approved": payload.approved,
                "nonce": payload.nonce,
            })),
            None => CanonicalProofPayload::Undecodable,
        },
        TransactionType::ContributionReveal => match tx.parse_reveal_payload() {
            Some(payload) => CanonicalProofPayload::Decoded(json!({
                "commit_id": payload.commit_id,
            })),
            None => CanonicalProofPayload::Undecodable,
        },
        _ => CanonicalProofPayload::NotApplicable,
    }
}

fn chain_block_detail(block: &Block) -> serde_json::Value {
    let transactions: Vec<serde_json::Value> = block
        .transactions
        .iter()
        .map(|tx| {
            let mut entry = json!({
                "type": format!("{:?}", tx.tx_type),
                "sender": tx.sender_address.to_string(),
            });
            match canonical_transaction_proof_payload(tx) {
                CanonicalProofPayload::NotApplicable => {},
                CanonicalProofPayload::Decoded(payload) => {
                    entry["payload"] = payload;
                },
                CanonicalProofPayload::Undecodable => {
                    entry["payload_error"] = json!("undecodable_canonical_payload");
                },
            }
            entry
        })
        .collect();

    json!({
        "height": block.height,
        "timestamp": block.timestamp.to_rfc3339(),
        "hash": block.hash.as_ref().map(|h| h.to_string()).unwrap_or_else(|| "pending".into()),
        "prev_block_hash": block.prev_block_hash.to_string(),
        "tx_count": block.transactions.len(),
        "difficulty": block.difficulty,
        "version": block.version,
        "nonce": block.nonce,
        "metadata": block.metadata,
        // Producer authentication is consensus-visible block data, not secret
        // key material. Expose it here so the cut runbook can distinguish a
        // signed block from an unsigned one at the actual API seam.
        "producer_pubkey": block.producer_pubkey,
        "producer_sig": block.producer_sig,
        "transactions": transactions,
    })
}

/// GET /chain/blocks?limit=N — return the most recent N block summaries.
pub async fn get_chain_blocks(
    req: HttpRequest,
    state: web::Data<AppState>,
    query: web::Query<BlockListQuery>,
) -> impl Responder {
    let limit = query
        .limit
        .unwrap_or(DEFAULT_BLOCK_LIST_LIMIT)
        .clamp(1, MAX_BLOCK_LIST_LIMIT);

    // #1376: read the in-memory chain inside the lock and map directly to
    // JSON summaries — never clone the full `BTreeMap<u64, Block>`. Persisted
    // blocks (only needed when start_height < first_in_memory_height) are
    // loaded after the read lock is released; the resulting set is small
    // (bounded by `limit`, default 20).
    let (chain_height, first_in_memory_height) = {
        let blockchain = state.blockchain.read().await;
        (blockchain.height, blockchain.blocks().keys().next().copied())
    };

    if chain_height == 0 {
        return envelope_success(
            &req,
            StatusCode::OK,
            json!({
                "blocks": [],
                "chain_height": 0,
                "returned": 0,
            }),
        );
    }

    let start_height = chain_height.saturating_sub(limit as u64 - 1).max(1);

    // Load persisted blocks first (small range, bounded by limit) so the
    // blockchain read lock is acquired only once below for the in-memory
    // tail.
    let persisted_summaries: Vec<(u64, serde_json::Value)> =
        match first_in_memory_height {
            Some(first_in_memory_height) if start_height < first_in_memory_height => {
                let storage_end = first_in_memory_height.saturating_sub(1).min(chain_height);
                let storage = state.storage.read().await;
                match storage.load_blocks_range(start_height, storage_end) {
                    Ok(blocks) => blocks
                        .into_iter()
                        .map(|block| (block.height, chain_block_summary(&block)))
                        .collect(),
                    Err(err) => {
                        log::error!(
                            "Failed to load persisted blocks {}..={} for /chain/blocks: {}",
                            start_height,
                            storage_end,
                            err
                        );
                        return envelope_error(
                            &req,
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "INTERNAL_ERROR",
                            "failed to load persisted blocks",
                        );
                    },
                }
            },
            _ => Vec::new(),
        };

    // In-memory tail: range over `blockchain.blocks()` while holding the
    // read lock and map summaries directly. No clone of the block map.
    let in_memory_summaries: Vec<(u64, serde_json::Value)> = {
        let blockchain = state.blockchain.read().await;
        let in_memory_start = first_in_memory_height
            .map(|f| start_height.max(f))
            .unwrap_or(start_height);
        blockchain
            .blocks()
            .range(in_memory_start..=chain_height)
            .map(|(h, block)| (*h, chain_block_summary(block)))
            .collect()
    };

    // Merge in height order: persisted segment is strictly below the
    // in-memory segment by construction.
    let blocks: Vec<serde_json::Value> = persisted_summaries
        .into_iter()
        .chain(in_memory_summaries.into_iter())
        .map(|(_, v)| v)
        .collect();

    envelope_success(
        &req,
        StatusCode::OK,
        json!({
            "blocks": blocks,
            "chain_height": chain_height,
            "returned": blocks.len(),
        }),
    )
}

/// GET /chain/block/{height} — return a single block by height.
pub async fn get_chain_block_by_height(
    req: HttpRequest,
    state: web::Data<AppState>,
    path: web::Path<u64>,
) -> impl Responder {
    let height = path.into_inner();
    let in_memory_block = {
        let blockchain = state.blockchain.read().await;
        blockchain.block_at_height(height).cloned()
    };

    let block = match in_memory_block {
        Some(block) => block,
        None => {
            let storage = state.storage.read().await;
            match storage.load_blocks_range(height, height) {
                Ok(mut blocks) => match blocks.pop() {
                    Some(block) => block,
                    None => {
                        return envelope_error(
                            &req,
                            StatusCode::NOT_FOUND,
                            "NOT_FOUND",
                            "block not found at requested height (may have been evicted from in-memory window)",
                        );
                    },
                },
                Err(err) => {
                    log::error!(
                        "Failed to load persisted block {} for /chain/block/{{height}}: {}",
                        height,
                        err
                    );
                    return envelope_error(
                        &req,
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "INTERNAL_ERROR",
                        "failed to load persisted block",
                    );
                },
            }
        },
    };

    envelope_success(&req, StatusCode::OK, chain_block_detail(&block))
}

const DEFAULT_ADDRESS_TX_LIMIT: usize = 20;
const MAX_ADDRESS_TX_LIMIT: usize = 100;
const DEFAULT_PENDING_TX_LIMIT: usize = 50;
const MAX_PENDING_TX_LIMIT: usize = 200;

#[derive(Debug, Deserialize)]
pub struct AddressTransactionsQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub credit_limit: Option<usize>,
    pub credit_after: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PendingTransactionsQuery {
    pub limit: Option<usize>,
}

fn transaction_read_summary(tx: &Transaction, block_height: Option<u64>) -> serde_json::Value {
    json!({
        "tx_id": tx.hash().to_string(),
        "tx_type": format!("{:?}", tx.tx_type),
        "sender": tx.sender_address.to_string(),
        "block_height": block_height,
    })
}

/// Address-specific read projection. The compatibility sender field is not
/// authoritative for transparent transfers; their canonical payload is.
fn address_transaction_read_summary(
    tx: &Transaction,
    height: u64,
    address: Address,
) -> Option<serde_json::Value> {
    let transfer = tx.parse_transparent_transfer_payload().and_then(|payload| {
        let from = payload.from.parse::<Address>().ok()?;
        let to = payload.to.parse::<Address>().ok()?;
        (payload.transparent_opt_out && payload.amount > 0).then_some((from, to, payload.amount))
    });
    let mut summary = transaction_read_summary(tx, Some(height));
    summary["event_id"] = json!(format!("transaction:{}", tx.hash()));
    summary["direction"] = json!("unknown");
    summary["from"] = serde_json::Value::Null;
    summary["to"] = serde_json::Value::Null;
    summary["counterparty"] = serde_json::Value::Null;
    summary["amount_base_units"] = serde_json::Value::Null;
    summary["kind"] = serde_json::Value::Null;
    summary["settlement"] = json!("included");
    if let Some((from, to, amount)) = transfer {
        if from != address && to != address {
            return None;
        }
        let (direction, counterparty) = if from == address && to == address {
            ("self", address)
        } else if to == address {
            ("received", from)
        } else {
            ("sent", to)
        };
        summary["direction"] = json!(direction);
        summary["from"] = json!(from.to_string());
        summary["to"] = json!(to.to_string());
        summary["counterparty"] = json!(counterparty.to_string());
        summary["amount_base_units"] = json!(amount.to_string());
        summary["kind"] = json!("transfer");
        Some(summary)
    } else if tx.sender_address == address {
        // Keep unsupported sent-operation coverage without guessing a debit,
        // credit or amount (notably for included but unapplied reward payouts).
        Some(summary)
    } else {
        None
    }
}

/// Canonical state is the payment receipt. Block metadata can hold a nominal
/// amount, a deferred decision or repeated observations, so it is never used
/// to infer a paid amount or settlement height here.
fn paid_credit_page(
    chain: &Blockchain,
    address: Address,
    limit: usize,
    after: Option<&str>,
) -> (
    usize,
    Vec<wrfcoin_native_chain::blockchain::state::ContributionRewardRecord>,
    Option<String>,
) {
    use std::ops::Bound::{Excluded, Unbounded};
    use wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass;
    // Bound examined records BEFORE filtering. Sparse beneficiary matches
    // must not turn a page into a full-state scan. Empty pages still advance.
    let mut page: Vec<_> = chain
        .state
        .contribution_reward_records
        .range::<str, _>((after.map_or(Unbounded, Excluded), Unbounded))
        .take(limit + 1)
        .collect();
    let examined = page.len();
    let next = if page.len() > limit {
        page.pop();
        page.last().map(|(id, _)| (*id).clone())
    } else {
        None
    };
    let records = page
        .into_iter()
        .filter(|(id, record)| {
            record.address == address
                && record.provenance_class == ContributionProvenanceClass::Real
                && record.amount.into_inner() > 0
                && record.commit_id() == Some(id.as_str())
                && chain.state.finalized_contribution_rewards.contains(*id)
        })
        .map(|(_, record)| record.clone())
        .collect();
    (examined, records, next)
}

fn paid_credit_summary(
    record: wrfcoin_native_chain::blockchain::state::ContributionRewardRecord,
) -> serde_json::Value {
    json!({
        "event_id": record.reason,
        "tx_id": null,
        "tx_type": null,
        "sender": null,
        "from": null,
        "to": record.address.to_string(),
        "direction": "received",
        "counterparty": null,
        "amount_base_units": record.amount.into_inner().to_string(),
        "kind": if record.reason.starts_with("forecast:") { "forecast_reward" } else { "weather_reward" },
        "settlement": "paid",
        "block_height": null,
        "timestamp": null,
    })
}

/// GET /transactions/{tx_id}/status — status within the node's bounded window.
pub async fn get_transaction_status(
    req: HttpRequest,
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    let raw_id = path.into_inner();
    let tx_id: Hash = match raw_id.parse() {
        Ok(id) => id,
        Err(_) => {
            return envelope_error(
                &req,
                StatusCode::BAD_REQUEST,
                "VALIDATION_ERROR",
                "tx_id must be a 32-byte hexadecimal transaction hash",
            );
        },
    };

    if let Some(tx) = state.mempool.read().await.get_transaction(&tx_id) {
        return envelope_success(
            &req,
            StatusCode::OK,
            json!({
                "tx_id": tx_id.to_string(),
                "status": "pending",
                "tx_type": format!("{:?}", tx.tx_type),
                "window_limited": true,
            }),
        );
    }

    let included = {
        let blockchain = state.blockchain.read().await;
        blockchain.blocks().iter().find_map(|(height, block)| {
            block
                .transactions
                .iter()
                .find(|tx| tx.hash() == tx_id)
                .map(|tx| (*height, format!("{:?}", tx.tx_type)))
        })
    };
    match included {
        Some((height, tx_type)) => envelope_success(
            &req,
            StatusCode::OK,
            json!({
                "tx_id": tx_id.to_string(),
                "status": "included",
                "block_height": height,
                "tx_type": tx_type,
                "window_limited": true,
            }),
        ),
        None => envelope_success(
            &req,
            StatusCode::OK,
            json!({
                "tx_id": tx_id.to_string(),
                "status": "unknown",
                "window_limited": true,
            }),
        ),
    }
}

/// GET /addresses/{address}/transactions — retained transactions plus paid
/// state-backed credits, with independent pagination and unknown credit times.
pub async fn get_address_transactions(
    req: HttpRequest,
    state: web::Data<AppState>,
    path: web::Path<String>,
    query: web::Query<AddressTransactionsQuery>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    let address: Address = match path.into_inner().parse() {
        Ok(address) => address,
        Err(_) => {
            return envelope_error(
                &req,
                StatusCode::BAD_REQUEST,
                "VALIDATION_ERROR",
                "address must be a valid hex address",
            );
        },
    };
    let address_text = address.to_string();
    let limit = query
        .limit
        .unwrap_or(DEFAULT_ADDRESS_TX_LIMIT)
        .clamp(1, MAX_ADDRESS_TX_LIMIT);
    let offset = query.offset.unwrap_or(0);
    let credit_limit = query
        .credit_limit
        .unwrap_or(limit)
        .clamp(1, MAX_ADDRESS_TX_LIMIT);

    let (
        window_start_height,
        total,
        transactions,
        credit_records_examined,
        credit_records,
        credit_next_cursor,
    ) = {
        let blockchain = state.blockchain.read().await;
        let window_start_height = blockchain.blocks().keys().next().copied();
        let mut matches = Vec::new();
        for (height, block) in blockchain.blocks().iter().rev() {
            for tx in block.transactions.iter().rev() {
                if let Some(summary) = address_transaction_read_summary(tx, *height, address) {
                    matches.push(summary);
                }
            }
        }
        let total = matches.len();
        let page: Vec<serde_json::Value> = matches.into_iter().skip(offset).take(limit).collect();
        let (credit_records_examined, credit_records, credit_next_cursor) = paid_credit_page(
            &blockchain,
            address,
            credit_limit,
            query.credit_after.as_deref(),
        );
        (
            window_start_height,
            total,
            page,
            credit_records_examined,
            credit_records,
            credit_next_cursor,
        )
    };
    // Allocate credit JSON after dropping the chain read guard.
    let credit_events: Vec<_> = credit_records
        .into_iter()
        .map(paid_credit_summary)
        .collect();

    envelope_success(
        &req,
        StatusCode::OK,
        json!({
            "address": address_text,
            "transactions": transactions,
            "limit": limit,
            "offset": offset,
            "total_in_window": total,
            "window_start_height": window_start_height,
            "window_limited": true,
            "credit_events": credit_events,
            "credit_records_examined": credit_records_examined,
            "credit_scan_limit": credit_limit + 1,
            "credit_limit": credit_limit,
            "credit_next_cursor": credit_next_cursor,
            "credit_order": "commit_id_ascending",
        }),
    )
}

/// GET /mempool/pending — bounded metadata only; payloads are never returned.
pub async fn get_mempool_pending(
    req: HttpRequest,
    state: web::Data<AppState>,
    query: web::Query<PendingTransactionsQuery>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    let limit = query
        .limit
        .unwrap_or(DEFAULT_PENDING_TX_LIMIT)
        .clamp(1, MAX_PENDING_TX_LIMIT);
    let (total, transactions) = {
        let mempool = state.mempool.read().await;
        let total = mempool.size();
        let mut summaries: Vec<_> = mempool
            .iter_transactions()
            .map(|tx| transaction_read_summary(tx, None))
            .collect();
        summaries.sort_by(|left, right| left["tx_id"].as_str().cmp(&right["tx_id"].as_str()));
        summaries.truncate(limit);
        (total, summaries)
    };
    envelope_success(
        &req,
        StatusCode::OK,
        json!({
            "transactions": transactions,
            "limit": limit,
            "total_pending": total,
            "window_limited": true,
        }),
    )
}

static PROCESS_START: OnceLock<Instant> = OnceLock::new();

/// Record the process start. `main` calls this before the server binds so
/// `/health`'s `uptime_seconds` counts from boot; without that call the
/// `OnceLock` is first set by whichever request arrives first, and the
/// value reads 0 for the life of that request (#3599).
pub fn record_process_start() {
    // determinism-ok: boot-time monotonic clock for the operator API's uptime_seconds; read once at startup, never a consensus input (block validity, signing preimage or state root)
    PROCESS_START.get_or_init(Instant::now);
}

fn process_uptime_secs() -> u64 {
    PROCESS_START.get_or_init(Instant::now).elapsed().as_secs()
}

pub async fn health(req: HttpRequest, state: web::Data<AppState>) -> impl Responder {
    if crate::helpers::is_operator_or_localhost(&req, &state) {
        HttpResponse::Ok().json(json!({
            "status": "ok",
            "service": "wrfcoin-native-launch-api",
            "version": env!("CARGO_PKG_VERSION"),
            "uptime_seconds": process_uptime_secs()
        }))
    } else {
        HttpResponse::Ok().json(json!({ "status": "ok" }))
    }
}

pub async fn version(req: HttpRequest, state: web::Data<AppState>) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }
    HttpResponse::Ok().json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "service": "wrfcoin-native-chain",
        "network": std::env::var("WRFCOIN_NETWORK_ID").unwrap_or_else(|_| "unknown".to_string())
    }))
}

pub async fn readyz(req: HttpRequest, state: web::Data<AppState>) -> impl Responder {
    let mut checks = serde_json::Map::new();
    let mut all_ok = true;

    let state_diverged = state.state_diverged.load(Ordering::SeqCst);
    checks.insert(
        "state_divergence".into(),
        json!({
            "status": if state_diverged { "diverged" } else { "ok" },
            "readiness_gate": true
        }),
    );
    if state_diverged {
        all_ok = false;
    }

    // 1. Storage: can we read blockchain height?
    let blockchain = state.blockchain.read().await;
    let height = blockchain.height;
    checks.insert("storage".into(), json!({"status": "ok", "height": height}));

    // 2. Block age: time since last block
    if let Some(last_block) = blockchain.last_block() {
        let age_secs = chrono::Utc::now()
            .signed_duration_since(last_block.timestamp)
            .num_seconds()
            .max(0);
        let stale = block_age_is_stale(height, age_secs);
        let status = if stale { "stale" } else { "ok" };
        if stale { all_ok = false; }
        let mut bp = json!({"status": status, "block_age_seconds": age_secs, "height": height});
        if stale {
            bp["hint"] = json!(
                "No new blocks in 10+ minutes. If height=0, mining may not be started. \
                 Check consensus engine logs and ensure at least one miner is active."
            );
        }
        checks.insert("block_production".into(), bp);
    } else {
        checks.insert("block_production".into(), json!({"status": "ok", "height": 0}));
    }
    drop(blockchain);

    // The boot-time producer check is immutable: it records the set resolved
    // for the next block using the restored on-chain history and the exact
    // signer (if any) used by the production adapter. A pure follower remains
    // ready, but its non-producing posture is visible to operators. A
    // configured signer that is not authorized is a hard readiness failure
    // unless startup already refused it under WRFCOIN_REQUIRE_PRODUCER_AUTH.
    let producer_check = &state.producer_authority_boot;
    if producer_check.readiness_gate {
        all_ok = false;
    }
    checks.insert(
        "next_block_producer_authority".into(),
        json!({
            "status": producer_check.status,
            "next_height": producer_check.next_height,
            "node_key": producer_check.node_key,
            "authorized_set": producer_check.authorized_set,
            "message": producer_check.message,
            "readiness_gate": producer_check.readiness_gate,
        }),
    );

    // Boot-time P2P posture is explicit even when no client exists. An
    // `Option<P2pClient>` alone cannot distinguish intentionally disabled from
    // failed initialization, which previously let an isolated node report ready.
    let p2p_startup = &state.p2p_startup;
    let mut p2p_state = p2p_startup.state;
    #[cfg(feature = "p2p")]
    if p2p_state == "running" && state.p2p.is_none() {
        p2p_state = "failed";
    }
    let p2p_blocks = p2p_state == "failed"
        || (p2p_startup.required && p2p_state != "running");
    if p2p_blocks {
        all_ok = false;
    }
    checks.insert(
        "p2p_startup".into(),
        json!({
            "status": p2p_state,
            "profile": p2p_startup.profile,
            "required": p2p_startup.required,
            "identity": p2p_startup.identity,
            "proof_class": if p2p_startup.simulated { "SIMULATED" } else { "RUNTIME" },
            "detail": p2p_startup.detail,
            "readiness_gate": p2p_startup.required || p2p_state == "failed",
        }),
    );

    // 3. P2P peers (only if P2P feature is compiled in and enabled).
    //    Peer count is diagnostic-only by default (single-operator/dev, where
    //    0 peers is expected — #812). On a multi-node/public network an operator
    //    sets WRFCOIN_MIN_READY_PEERS>0 to turn it into a hard readiness gate, so
    //    an eclipsed/partitioned node (0 peers, or < min) reports not_ready
    //    instead of silently serving a stale/forked view (#2042). A start-up
    //    grace window (WRFCOIN_READY_PEERS_GRACE_SECS, default 120s) suppresses
    //    the gate during initial peer discovery to avoid boot-time flapping.
    #[cfg(feature = "p2p")]
    if let Some(ref client) = state.p2p {
        let gossip_streak = crate::helpers::reconcile_tx_gossip_backpressure(
            &client.command_tx,
            &state.tx_gossip_backpressure_streak,
        );
        let (gossip_check, gossip_blocks) =
            crate::helpers::readyz_tx_gossip_check(gossip_streak);
        if gossip_blocks {
            all_ok = false;
        }
        checks.insert("transaction_gossip".into(), gossip_check);
        let (consumer_check, consumer_blocks) =
            readyz_p2p_consumer_check(&state.p2p_consumer_health);
        if consumer_blocks {
            all_ok = false;
        }
        checks.insert("p2p_event_consumer".into(), consumer_check);

        let stats = client.stats();
        let min_ready_peers = ready_min_peers();
        let within_grace = process_uptime_secs() < ready_peers_grace_secs();
        let (peer_check, peer_blocks) =
            readyz_peer_check(stats.connected_peers, min_ready_peers, within_grace);
        if peer_blocks {
            all_ok = false;
        }
        checks.insert("peers".into(), peer_check);

        let blocks_mesh_peers = stats
            .mesh_peers_by_topic
            .get("blocks")
            .copied()
            .unwrap_or(0);
        // Mesh peer count is diagnostic only on single-operator testnets.
        // Zero mesh peers is surfaced as a hint, but does not block readiness.
        checks.insert("p2p_mesh".into(), readyz_mesh_check(blocks_mesh_peers));
        checks.insert(
            "p2p_outbound_diversity".into(),
            readyz_outbound_diversity_check(
                stats.outbound_peers,
                stats.outbound_distinct_subnets,
                stats.outbound_low_diversity,
                stats.outbound_low_diversity_events,
            ),
        );

        // Block download (#1614 follow-up to #1606, #2806): a joiner whose IBD
        // has halted must say so here, not only in a log line. `stalled` is the
        // #1482 orphan-seed halt (the height-1 parent every peer serves does not
        // chain onto an empty local chain); `validation_halted` is the #2807
        // corroborated rule halt (canonical blocks this binary rejects — the
        // 11,016 wedge). Both are terminal without operator action, so both
        // gate readiness: a node that will never reach the tip must not be
        // routed to as if it had.
        let (sync_check, sync_blocks) = readyz_sync_check(
            &stats.sync_state,
            stats.sync_local_height,
            stats.sync_target_height,
        );
        if sync_blocks {
            all_ok = false;
        }
        checks.insert("block_sync".into(), sync_check);
    }

    // 4. WAN failover (#1798): diagnostic by default. When configured as a hard
    //    gate (WRFCOIN_FAILOVER_HARD_GATE=1), a Disconnected WAN — primary and
    //    LTE/5G backup both down — blocks readiness so load balancers route away
    //    from a node that cannot reach the network. `metered` (on cellular
    //    backup) is surfaced but never gates on its own.
    {
        let (wan_state, metered) = {
            let monitor = state.failover_monitor.read().await;
            (monitor.state(), monitor.is_metered())
        };
        let hard_gate = std::env::var("WRFCOIN_FAILOVER_HARD_GATE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let disconnected =
            wan_state == wrfcoin_native_chain::network_failover::WanState::Disconnected;
        let gate_blocks = hard_gate && disconnected;
        if gate_blocks {
            all_ok = false;
        }
        let mut wp = json!({
            "status": if disconnected { "disconnected" } else { "ok" },
            "wan_state": wan_state.to_string(),
            "metered": metered,
            "readiness_gate": hard_gate,
        });
        if gate_blocks {
            wp["hint"] = json!(
                "WAN is Disconnected (primary and LTE/5G backup both down) and \
                 WRFCOIN_FAILOVER_HARD_GATE is enabled, so this node reports not_ready."
            );
        }
        checks.insert("wan_failover".into(), wp);
    }

    // Export the readiness verdict so Prometheus can alert on it (#2057):
    // `wrfcoin_node_ready == 0 for: 2m`. Set from the same evaluation that
    // drives the HTTP status below, so the gauge and the 200/503 never disagree.
    wrfcoin_shared_protocol::monitoring::CORE_METRICS.set_node_ready(all_ok);

    let status_code = if all_ok { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };

    if crate::helpers::is_operator_or_localhost(&req, &state) {
        HttpResponse::build(status_code).json(json!({
            "status": if all_ok { "ready" } else { "not_ready" },
            "uptime_seconds": process_uptime_secs(),
            "checks": checks
        }))
    } else {
        HttpResponse::build(status_code).json(json!({
            "status": if all_ok { "ready" } else { "not_ready" }
        }))
    }
}

/// Default grace period (seconds) after process start before the peer-count
/// readiness gate is enforced, avoiding a flap to not_ready during the initial
/// peer-discovery window. Overridable via `WRFCOIN_READY_PEERS_GRACE_SECS`.
#[cfg(feature = "p2p")]
const READY_PEERS_GRACE_SECS_DEFAULT: u64 = 120;

/// Operator-configured minimum connected-peer count for `/readyz` to report
/// ready. `0` (the default) keeps peer count diagnostic-only — the historical
/// single-operator behavior (#812). `>0` turns it into a hard gate for
/// multi-node/public networks so an eclipsed node fails readiness (#2042).
#[cfg(feature = "p2p")]
fn ready_min_peers() -> usize {
    std::env::var("WRFCOIN_MIN_READY_PEERS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(0)
}

/// Grace window (seconds) after start-up during which the peer gate is
/// suppressed. Overridable via `WRFCOIN_READY_PEERS_GRACE_SECS`.
#[cfg(feature = "p2p")]
fn ready_peers_grace_secs() -> u64 {
    std::env::var("WRFCOIN_READY_PEERS_GRACE_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(READY_PEERS_GRACE_SECS_DEFAULT)
}

/// Build the `peers` readiness check. Returns the JSON diagnostic and whether
/// the peer count should block readiness (drive a 503).
///
/// The gate is active only when `min_ready_peers > 0` (operator opt-in for
/// multi-node nets). When active and `connected_peers < min_ready_peers`, the
/// node blocks readiness — unless still `within_grace` (start-up window), in
/// which case the shortfall is surfaced as a hint but does not block. With
/// `min_ready_peers == 0` the check is diagnostic-only, matching the historical
/// single-operator behavior (#812, #2042).
#[cfg(feature = "p2p")]
fn readyz_peer_check(
    connected_peers: usize,
    min_ready_peers: usize,
    within_grace: bool,
) -> (serde_json::Value, bool) {
    let gate_active = min_ready_peers > 0;
    let below_min = connected_peers < min_ready_peers;
    let blocks = gate_active && below_min && !within_grace;

    let peer_status = if connected_peers == 0 {
        "no_peers"
    } else if gate_active && below_min {
        "below_min"
    } else {
        "ok"
    };

    let mut check = json!({
        "status": peer_status,
        "connected": connected_peers,
        "min_ready_peers": min_ready_peers,
        "readiness_gate": gate_active,
        "grace_active": gate_active && below_min && within_grace,
    });

    if blocks {
        check["hint"] = json!(
            "Connected peer count is below WRFCOIN_MIN_READY_PEERS on a multi-node \
             network — the node may be eclipsed or partitioned, so it reports \
             not_ready and load balancers should route away. Check peer \
             connectivity, BOOTSTRAP_PEERS, and that inbound P2P (port 8555) is \
             reachable."
        );
    } else if connected_peers == 0 {
        check["hint"] = json!(
            "No peers connected. Check: (1) WRFCOIN_P2P_ENABLED=true, \
             (2) BOOTSTRAP_PEERS contains valid multiaddresses, \
             (3) port 8555 is reachable."
        );
    }
    (check, blocks)
}

/// Readiness verdict for the block downloader (#1614, #2806).
///
/// Pure over the published `NetworkStats` sync observation so it is testable
/// without a swarm. `stalled` and `validation_halted` are the two halts the
/// downloader declares terminal (`SyncState::Stalled` / `SyncState::ValidationHalted`);
/// each gates readiness and carries the operator action. `downloading` is
/// diagnostic (`syncing`) — a node in IBD is behind, not broken — and every
/// other label is `ok`.
#[cfg(feature = "p2p")]
fn readyz_sync_check(
    sync_state: &str,
    local_height: u64,
    target_height: u64,
) -> (serde_json::Value, bool) {
    let (status, gate, hint) = match sync_state {
        "validation_halted" => (
            "validation_halted",
            true,
            Some(format!(
                "Block download halted: canonical block {} (served by multiple distinct peers) \
                 fails this node's authoritative validation, so re-downloading cannot cure it \
                 and sync will never reach the tip (#2806/#2807). Either this binary is behind \
                 the release every peer runs — upgrade it — or the history below the halt is \
                 sealed for this binary: join from a signed state snapshot above height {} \
                 (GET /state/snapshot/sync-payload on a trusted node, POST \
                 /network/snapshot-sync/ingest here).",
                local_height.saturating_add(1),
                local_height
            )),
        ),
        "stalled" => (
            "stalled",
            true,
            Some(
                "Block download halted at genesis: every height-1 block peers serve is an \
                 orphan seed that does not chain onto an empty local chain, so the required \
                 height-1 parent is missing from the seed's storage (#1482). The missing parent \
                 hash is in this node's log (`IBD: halted after orphan seed drop`); the fix is on \
                 the seed (recover its storage), not here."
                    .to_string(),
            ),
        ),
        "downloading" => ("syncing", false, None),
        _ => ("ok", false, None),
    };
    let mut check = json!({
        "status": status,
        "sync_state": sync_state,
        "local_height": local_height,
        "target_height": target_height,
        "readiness_gate": gate,
    });
    if let Some(hint) = hint {
        check["hint"] = json!(hint);
    }
    (check, gate)
}

#[cfg(feature = "p2p")]
fn readyz_mesh_check(mesh_peers: usize) -> serde_json::Value {
    let mesh_status = if mesh_peers >= 1 { "ok" } else { "empty_mesh" };
    let mut check = json!({
        "status": mesh_status,
        "topic": "blocks",
        "mesh_peers": mesh_peers,
        "readiness_gate": false
    });
    if mesh_peers < 1 {
        check["hint"] = json!(
            "Blocks topic has no mesh peers yet. Check that peers have joined \
             the gossipsub mesh and that FLOOD_PUBLISH or mesh_n_low are set \
             appropriately for small testnets."
        );
    }
    check
}

/// Outbound peer diversity as a readiness check (#2114).
///
/// **Diagnostic, not a gate.** It reports and never blocks readiness, for the
/// same reason #2042 left the peer count diagnostic: a node that is eclipsed is
/// still serving, and routing traffic away from it does not un-eclipse it —
/// it just removes the operator's visibility into a node that is still
/// answering queries with an attacker's view of the chain. The value here is
/// that the condition becomes *observable* on the surface operators already
/// watch.
///
/// The event counter is included deliberately: a node that was eclipsed and has
/// since recovered shows `status: "ok"` with a non-zero `low_diversity_events`,
/// which is a materially different situation from one that was never eclipsed
/// and reads identically without it.
#[cfg(feature = "p2p")]
fn readyz_outbound_diversity_check(
    outbound_peers: usize,
    distinct_subnets: usize,
    low_diversity: bool,
    low_diversity_events: u64,
) -> serde_json::Value {
    let status = if outbound_peers == 0 {
        // No self-chosen peers yet. Not a diversity verdict — see
        // `OutboundPeerSet::is_low_diversity`.
        "no_outbound_peers"
    } else if low_diversity {
        "low_diversity"
    } else {
        "ok"
    };

    let mut check = json!({
        "status": status,
        "outbound_peers": outbound_peers,
        "distinct_subnets": distinct_subnets,
        "low_diversity_events": low_diversity_events,
        "readiness_gate": false,
    });

    if low_diversity {
        check["hint"] = json!(
            "The outbound peer set spans too few distinct /24 subnets. This is the \
             observable signature of an eclipse in progress: every peer this node \
             chose to dial may be controlled by one party, so its view of the chain \
             cannot be trusted. Check BOOTSTRAP_PEERS for diversity and whether the \
             Kademlia routing table has been poisoned."
        );
    }
    check
}

pub async fn get_prometheus_metrics(
    req: HttpRequest,
    state: web::Data<AppState>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }

    use prometheus::Encoder;
    use prometheus::TextEncoder;
    use wrfcoin_shared_protocol::monitoring::CORE_METRICS;

    let blockchain = state.blockchain.read().await;
    let mempool = state.mempool.read().await;
    let _ = build_runtime_monitoring_snapshot(&state, &blockchain, mempool.size());
    // #2464: read from the DURABLE block archive, not the in-memory window.
    // The window is a bounded ring (default 200) that evicts the oldest block
    // on every apply, so a window-scoped total silently decays toward zero as
    // the blocks that recorded unpaid contributions age out — indistinguishable
    // from the backlog having been paid. See `routes::late_quorum`.
    //
    // On a storage read failure the previous gauge value is deliberately left
    // in place: a scrape that cannot read the archive must not publish a 0 that
    // would read as "backlog cleared".
    {
        let storage = state.storage.read().await;
        match crate::routes::late_quorum::unpaid_total(&blockchain, &storage) {
            Ok(total) => CORE_METRICS.set_oracle_late_quorum_unpaid(total),
            Err(e) => log::error!(
                "late-quorum unpaid gauge not updated; durable archive read failed: {e}"
            ),
        }
    }
    #[cfg(feature = "p2p")]
    if let Some(client) = state.p2p.as_ref() {
        let stats = client.stats();
        CORE_METRICS.set_p2p_sync(
            stats.sync_local_height,
            stats.sync_target_height,
            &stats.sync_state,
            stats.sync_pending_ranges,
            stats.sync_in_flight_ranges,
            stats.sync_outstanding_ranges,
        );
    }

    let encoder = TextEncoder::new();
    let metric_families = CORE_METRICS.registry.gather();
    let mut buffer = Vec::new();
    match encoder.encode(&metric_families, &mut buffer) {
        Ok(()) => HttpResponse::Ok()
            .content_type(encoder.format_type())
            .body(buffer),
        Err(e) => {
            log::error!("Failed to encode prometheus metrics: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "metrics encoding failed"}))
        }
    }
}

fn build_runtime_monitoring_snapshot(
    state: &AppState,
    blockchain: &Blockchain,
    pending_transactions: usize,
) -> MonitoringSnapshot {
    let state_diverged = state.state_diverged.load(Ordering::SeqCst);
    // Finalized height: prefer the BFT-finalized height (#1216) when a BFT
    // source has advanced it. Fall back to the probabilistic
    // "tip - MAX_REORG_DEPTH" floor when the BFT guard is still inert
    // (e.g. single-node dev deployments or nodes running PoP without an
    // attached BFT gadget), so operator alerting (#1148) keeps working.
    //
    // The max() is load-bearing: once a BFT source has advanced
    // `blockchain.finalized_height()`, even briefly, it is monotonic and
    // will never roll back. The probabilistic floor can lag during
    // catch-up but never exceeds the BFT value on a healthy node.
    let probabilistic_floor = blockchain
        .height
        .saturating_sub(wrfcoin_shared_protocol::blockchain::chain::MAX_REORG_DEPTH);
    let bft_finalized = blockchain.finalized_height();
    let chain_finalized_height = probabilistic_floor.max(bft_finalized);
    build_snapshot(MonitoringInputs {
        node_id: monitoring_node_id(state),
        node_name: "launch-api-node".to_string(),
        chain_height: blockchain.height,
        tip_hash: blockchain.tip.to_string(),
        account_count: blockchain.state.accounts.len(),
        nft_count: blockchain.state.nfts.len(),
        pending_transactions,
        quality_event_count: blockchain.state.quality_policy_events().len(),
        slashing_events_by_reason: slashing_events_by_reason(blockchain),
        transactions_processed_total: blockchain
            .blocks()
            .values()
            .map(|block| block.transactions.len() as u64)
            .sum(),
        transactions_failed_total: state
            .failed_transactions_total
            .load(std::sync::atomic::Ordering::Relaxed),
        zk_proofs_observed_total: count_observed_privacy_proofs(
            blockchain,
            &state.zk_proofs_per_block_cache,
        ),
        state_diverged,
        chain_finalized_height,
        state_pruning_last_height: blockchain.state.last_state_pruning_height,
        network: monitoring_network_telemetry(state),
    })
}

fn slashing_events_by_reason(blockchain: &Blockchain) -> HashMap<String, u64> {
    blockchain.state.slashing_events_by_reason()
}

/// Sum privacy-proof structures observed across all in-memory blocks (#3250).
///
/// Per-block proof counts are memoized in `cache` keyed by block height; the
/// payload-parsing work in `count_privacy_proofs_in_transaction` runs at most once
/// per block instead of on every Prometheus scrape (default ~15s). The cache
/// is pruned to the current in-memory window on each call so it cannot grow
/// beyond `Blockchain::blocks()`.
///
/// The returned value is identical to the un-memoized variant: it is the sum
/// of proof counts over the blocks currently held in `Blockchain::blocks()`,
/// which honours the in-memory eviction window.
fn count_observed_privacy_proofs(
    blockchain: &Blockchain,
    cache: &std::sync::Mutex<rustc_hash::FxHashMap<u64, u64>>,
) -> u64 {
    let blocks = blockchain.blocks();
    let mut cache = cache.lock().unwrap_or_else(|p| p.into_inner());
    cache.retain(|height, _| blocks.contains_key(height));
    let mut total: u64 = 0;
    for (height, block) in blocks {
        let count = *cache.entry(*height).or_insert_with(|| {
            block
                .transactions
                .iter()
                .map(count_privacy_proofs_in_transaction)
                .sum::<u64>()
        });
        total = total.saturating_add(count);
    }
    total
}

fn count_privacy_proofs_in_transaction(tx: &Transaction) -> u64 {
    let mut proofs = 0u64;

    if let Some(payload) = tx.parse_confidential_transfer_payload() {
        if payload.range_proof.is_some() {
            proofs = proofs.saturating_add(1);
        }
        // Structural check only — full ed25519 verification requires a
        // verifier public key which is not available in this metrics context.
        // Cryptographic verification remains unavailable here; see #3250 and #2080.
        proofs = proofs.saturating_add(
            payload
                .selective_disclosures
                .iter()
                .filter(|p| {
                    hex::decode(&p.verifier_attestation)
                        .map_or(false, |b| b.len() == 64)
                })
                .count() as u64,
        );
    }

    if tx.parse_shielded_deposit_payload().is_some() {
        proofs = proofs.saturating_add(1);
    }

    if let Some(payload) = tx.parse_shielded_withdrawal_payload() {
        // range_proof and authorization_proof are always present (non-optional)
        proofs = proofs.saturating_add(2);
        // Structural presence only — cryptographic verification remains #2080.
        if payload
            .selective_disclosure
            .as_ref()
            .is_some_and(|p| {
                hex::decode(&p.verifier_attestation)
                    .map_or(false, |b| b.len() == 64)
            })
        {
            proofs = proofs.saturating_add(1);
        }
    }

    proofs
}

#[cfg(feature = "p2p")]
fn monitoring_node_id(state: &AppState) -> String {
    state
        .p2p
        .as_ref()
        .map(|client| client.local_peer_id.to_string())
        .unwrap_or_else(|| "launch-api-node".to_string())
}

#[cfg(not(feature = "p2p"))]
fn monitoring_node_id(_state: &AppState) -> String {
    "launch-api-node".to_string()
}

#[cfg(feature = "p2p")]
fn monitoring_network_telemetry(state: &AppState) -> Option<NetworkTelemetry> {
    // Read capability snapshot non-blockingly; fall back to defaults if contended.
    let (ap_mode_active, is_metered) = state
        .node_capability
        .try_read()
        .map(|s| (s.ap_mode_enabled, s.is_metered))
        .unwrap_or((false, false));
    state.p2p.as_ref().map(|client| {
        let stats = client.stats();
        NetworkTelemetry {
            connected_peers: stats.connected_peers,
            mesh_peers_blocks: stats.mesh_peers_by_topic.get("blocks").copied().unwrap_or(0),
            mesh_peers_transactions: stats
                .mesh_peers_by_topic
                .get("transactions")
                .copied()
                .unwrap_or(0),
            mesh_peers_weather: stats.mesh_peers_by_topic.get("weather").copied().unwrap_or(0),
            mesh_peers_forecast: stats.mesh_peers_by_topic.get("forecast").copied().unwrap_or(0),
            blocks_received: stats.blocks_received,
            blocks_published: stats.blocks_published,
            txs_received: stats.txs_received,
            txs_published: stats.txs_published,
            weather_msgs_received: stats.weather_msgs_received,
            weather_msgs_published: stats.weather_msgs_published,
            forecast_tasks_received: stats.forecast_tasks_received,
            forecast_tasks_published: stats.forecast_tasks_published,
            bytes_inbound: stats.bytes_inbound,
            bytes_outbound: stats.bytes_outbound,
            bytes_inbound_blocks: stats.inbound_blocks.bytes,
            bytes_inbound_txs: stats.inbound_txs.bytes,
            bytes_inbound_weather: stats.inbound_weather.bytes,
            bytes_inbound_forecast: stats.inbound_forecast.bytes,
            bytes_outbound_blocks: stats.outbound_blocks.bytes,
            bytes_outbound_txs: stats.outbound_txs.bytes,
            bytes_outbound_weather: stats.outbound_weather.bytes,
            bytes_outbound_forecast: stats.outbound_forecast.bytes,
            uptime_secs: stats.uptime_secs,
            ap_mode_active,
            is_metered,
        }
    })
}

#[cfg(not(feature = "p2p"))]
fn monitoring_network_telemetry(state: &AppState) -> Option<NetworkTelemetry> {
    // Without P2P, still propagate WAN-failover capability state into the telemetry
    // so monitoring snapshots reflect AP/LTE status regardless of P2P build (#1802).
    state
        .node_capability
        .try_read()
        .map(|s| NetworkTelemetry {
            ap_mode_active: s.ap_mode_enabled,
            is_metered: s.is_metered,
            ..NetworkTelemetry::default()
        })
        .ok()
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    // health/version/readyz integration tests live in main.rs (require AppState).

    // #2464 late-quorum coverage moved to `routes::late_quorum` when the total
    // stopped being window-scoped; the equivalent absent-key / zero-count /
    // clean-chain cases live there alongside the eviction test the
    // window-scoped version could not express.

    #[cfg(feature = "p2p")]
    #[cfg(feature = "p2p")]
    #[test]
    fn readyz_reports_low_outbound_diversity_without_gating() {
        let (check, ) = (super::readyz_outbound_diversity_check(4, 1, true, 2), );
        assert_eq!(check["status"], "low_diversity");
        assert_eq!(check["distinct_subnets"], 1);
        assert_eq!(check["low_diversity_events"], 2);
        assert_eq!(
            check["readiness_gate"], false,
            "the eclipse signal is diagnostic — gating would remove operator \
             visibility into a node that is still answering with a poisoned view"
        );
        assert!(check["hint"].is_string(), "a low-diversity report must say what to do");
    }

    #[cfg(feature = "p2p")]
    #[test]
    fn readyz_distinguishes_no_outbound_peers_from_low_diversity() {
        // A node that has dialed nobody is starting up, not eclipsed. If these
        // read the same, the signal is useless during every boot.
        let starting = super::readyz_outbound_diversity_check(0, 0, false, 0);
        assert_eq!(starting["status"], "no_outbound_peers");
        assert!(starting["hint"].is_null());

        let eclipsed = super::readyz_outbound_diversity_check(3, 1, true, 1);
        assert_eq!(eclipsed["status"], "low_diversity");
    }

    #[cfg(feature = "p2p")]
    #[test]
    fn readyz_keeps_the_event_count_after_recovery() {
        // Recovered-from-eclipse must not be indistinguishable from
        // never-eclipsed.
        let recovered = super::readyz_outbound_diversity_check(5, 3, false, 4);
        assert_eq!(recovered["status"], "ok");
        assert_eq!(
            recovered["low_diversity_events"], 4,
            "history must survive recovery or the incident becomes invisible"
        );
    }

    // `readyz_peer_check` became `#[cfg(feature = "p2p")]` in #2541; this
    // pre-existing test calls it and was not gated with it, so default-features
    // builds broke. The three tests added alongside it in that PR were gated
    // correctly — this is the one that predates them.
    /// #1614 DoD item 4 (deferred from #1606): the two terminal downloader
    /// halts gate readiness and name the operator action; IBD in progress is
    /// diagnostic; a healthy downloader is `ok`.
    #[cfg(feature = "p2p")]
    #[::core::prelude::v1::test]
    fn readyz_sync_check_gates_the_two_terminal_halts_1614() {
        let (check, gates) = readyz_sync_check("validation_halted", 11_016, 11_900);
        assert!(gates, "a corroborated rule halt must gate readiness");
        assert_eq!(check["status"], "validation_halted");
        assert_eq!(check["readiness_gate"], true);
        assert_eq!(check["local_height"], 11_016);
        let hint = check["hint"].as_str().expect("halt carries an operator hint");
        assert!(hint.contains("11017"), "hint names the halted height: {hint}");
        assert!(hint.contains("/network/snapshot-sync/ingest"), "hint names the snapshot path: {hint}");

        let (check, gates) = readyz_sync_check("stalled", 0, 3_600);
        assert!(gates, "the orphan-seed halt must gate readiness");
        assert_eq!(check["status"], "stalled");
        assert!(check["hint"].as_str().unwrap().contains("#1482"));

        let (check, gates) = readyz_sync_check("downloading", 40, 3_600);
        assert!(!gates, "IBD in progress is behind, not broken");
        assert_eq!(check["status"], "syncing");
        assert!(check["hint"].is_null());

        for healthy in ["idle", "discovering", "complete"] {
            let (check, gates) = readyz_sync_check(healthy, 3_600, 3_600);
            assert!(!gates, "{healthy} must not gate");
            assert_eq!(check["status"], "ok");
            assert_eq!(check["sync_state"], healthy);
        }
    }

    #[cfg(feature = "p2p")]
    #[actix_web::test]
    async fn readyz_peer_check_treats_zero_peers_as_diagnostic() {
        // Default gate disabled (min_ready_peers=0): 0 peers is diagnostic, not a gate.
        let (check, blocks) = readyz_peer_check(0, 0, false);
        assert!(!blocks, "peer count must not block readiness when gate is disabled");
        assert_eq!(check["status"], "no_peers");
        assert_eq!(check["connected"], 0);
        assert_eq!(check["readiness_gate"], false);
        // Hint field must be present when no peers connected (#959).
        assert!(check["hint"].is_string(), "hint field should be present for zero peers");
        assert!(check["hint"].as_str().unwrap().contains("WRFCOIN_P2P_ENABLED"));
    }

    #[cfg(feature = "p2p")]
    #[actix_web::test]
    async fn readyz_peer_check_no_hint_when_peers_connected() {
        let (check, blocks) = readyz_peer_check(3, 0, false);
        assert!(!blocks);
        assert_eq!(check["status"], "ok");
        assert_eq!(check["connected"], 3);
        // Hint field must NOT be present when peers are connected (backward-compatible).
        assert!(check["hint"].is_null(), "hint field should be absent when peers are connected");
    }

    #[cfg(feature = "p2p")]
    #[actix_web::test]
    async fn readyz_peer_check_gates_zero_peers_when_min_set_after_grace() {
        // Multi-node gate active (min=1), grace elapsed: 0 peers => not ready (#2042).
        let (check, blocks) = readyz_peer_check(0, 1, false);
        assert!(blocks, "0 peers with an active gate past grace must block readiness");
        assert_eq!(check["status"], "no_peers");
        assert_eq!(check["readiness_gate"], true);
        assert_eq!(check["min_ready_peers"], 1);
        assert_eq!(check["grace_active"], false);
        assert!(
            check["hint"].as_str().unwrap().contains("WRFCOIN_MIN_READY_PEERS"),
            "eclipse hint should name the gating env var"
        );
    }

    #[cfg(feature = "p2p")]
    #[actix_web::test]
    async fn readyz_peer_check_does_not_gate_during_grace() {
        // Same shortfall but still inside the start-up grace window: no block.
        let (check, blocks) = readyz_peer_check(0, 1, true);
        assert!(!blocks, "gate must be suppressed during the start-up grace window");
        assert_eq!(check["readiness_gate"], true);
        assert_eq!(check["grace_active"], true);
    }

    #[cfg(feature = "p2p")]
    #[actix_web::test]
    async fn readyz_peer_check_ready_when_min_met() {
        let (check, blocks) = readyz_peer_check(2, 1, false);
        assert!(!blocks, "meeting the peer minimum must be ready");
        assert_eq!(check["status"], "ok");
        assert!(check["hint"].is_null());
    }

    #[cfg(feature = "p2p")]
    #[actix_web::test]
    async fn readyz_peer_check_gates_below_min_nonzero() {
        // 1 peer connected but 3 required: below_min, blocks past grace.
        let (check, blocks) = readyz_peer_check(1, 3, false);
        assert!(blocks, "connected peers below the configured minimum must block");
        assert_eq!(check["status"], "below_min");
        assert_eq!(check["connected"], 1);
        assert_eq!(check["min_ready_peers"], 3);
    }

    #[cfg(feature = "p2p")]
    #[actix_web::test]
    async fn readyz_mesh_check_treats_empty_blocks_mesh_as_diagnostic() {
        let check = readyz_mesh_check(0);
        assert_eq!(check["status"], "empty_mesh");
        assert_eq!(check["topic"], "blocks");
        assert_eq!(check["mesh_peers"], 0);
        assert_eq!(check["readiness_gate"], false);
        assert!(check["hint"].is_string(), "empty mesh should carry a hint");
    }

    #[cfg(feature = "p2p")]
    #[actix_web::test]
    async fn readyz_mesh_check_no_hint_when_blocks_mesh_present() {
        let check = readyz_mesh_check(2);
        assert_eq!(check["status"], "ok");
        assert_eq!(check["mesh_peers"], 2);
        assert!(check["hint"].is_null(), "healthy mesh should not carry a hint");
    }

    #[test]
    fn length_valid_garbage_attestation_is_observed_not_verified() {
        use wrfcoin_native_chain::blockchain::transaction::{
            ConfidentialTransferPayload, PrivacyLevel, SelectiveDisclosureProofPayload,
        };

        let payload = ConfidentialTransferPayload {
            output_commitments: vec![],
            range_proof: Some("structurally-present".to_string()),
            stealth_data: None,
            ring_signature: None,
            key_images: vec![],
            encrypted_payload: None,
            privacy_level: PrivacyLevel::High,
            selective_disclosures: vec![SelectiveDisclosureProofPayload {
                disclosure_id: "garbage-attestation".to_string(),
                scope: vec!["amount".to_string()],
                expires_at_block: u64::MAX,
                disclosed_fields_hash: "not-a-real-hash".to_string(),
                verifier_attestation: "00".repeat(64),
            }],
            signed_message: None,
        };
        let mut tx = Transaction::new(vec![], vec![]);
        tx.tx_type = TransactionType::ConfidentialTransfer;
        tx.data = Some(serde_json::to_string(&payload).unwrap());

        assert_eq!(
            count_privacy_proofs_in_transaction(&tx),
            2,
            "range proof and length-valid garbage attestation are observations only"
        );
    }

    #[test]
    fn heatmap_serialization_maps_non_finite_floats_to_null() {
        // Documents serde_json's actual behavior for non-finite f64: `to_value`
        // does NOT error — it maps NaN/Infinity to JSON `null` (via
        // Number::from_f64 returning None). So get_heatmap_json_compat never
        // panics on non-finite heatmap values; they serialize to null and the
        // Err arm remains as defensive coverage for genuinely-unserializable
        // payloads rather than a path NaN can reach.
        assert_eq!(serde_json::to_value(f64::NAN).unwrap(), serde_json::Value::Null);
        assert_eq!(
            serde_json::to_value(f64::INFINITY).unwrap(),
            serde_json::Value::Null
        );
        assert_eq!(
            serde_json::to_value(f64::NEG_INFINITY).unwrap(),
            serde_json::Value::Null
        );
    }
}

#[cfg(test)]
mod process_start_tests {
    use super::*;

    /// Both uptime clocks are started by `main` before the HTTP server is
    /// built, not by the first request (#3599).
    #[::core::prelude::v1::test]
    fn main_records_process_start_before_binding_the_server() {
        let main_src = include_str!("../main.rs");
        let main_fn = main_src.find("async fn main()").expect("main exists");
        let body = &main_src[main_fn..];
        let server = body.find("HttpServer::new(").expect("main builds the server");
        for call in [
            "routes::chain::record_process_start();",
            "routes::network::record_process_start();",
        ] {
            let at = body.find(call).unwrap_or_else(|| panic!("main must call {call}"));
            assert!(at < server, "{call} must run before HttpServer::new");
        }
    }

    #[::core::prelude::v1::test]
    fn uptime_counts_from_the_recorded_start() {
        record_process_start();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        assert!(process_uptime_secs() >= 1, "uptime must advance once the start is recorded");
    }
}
