//! Forecast late score node API (`consensus::forecast_late_score`).
//!
//! - `POST /forecast/late-score/submit` takes a `TransactionType::ForecastScore`
//!   transaction (built offline by `wrfcoin forecast score build`), admits it
//!   to the mempool with the chain's signing-key registry, and gossips it.
//!   Anyone with a contributor token may relay a score; it pays the relay
//!   nothing. Refused with 503 `FEATURE_NOT_ACTIVE` until `forecast_late_score`
//!   is in force at tip + 1. Before admission the node checks, read-only at
//!   the tip, that every signer is in the oracle set and that every signature
//!   verifies through the signing-key registry; a forged or outside signature
//!   is refused. A score with fewer signatures than the tip's quorum is
//!   ADMITTED and flagged `below_quorum_at_tip`, like the mempool (and the P2P
//!   path) admit it: the quorum can change in the block that includes it.
//!   Apply is the only authority (quorum, timing, bucket and cited readings);
//!   a score that does not apply is dropped at assembly and removed from that
//!   producer's mempool. Each token may submit at most
//!   [`RATE_LIMIT_PER_MIN`] scores a minute (429 `RATE_LIMITED`).
//! - `GET /forecast/late-score` says whether the lane is in force, the chain
//!   and network id a signer binds, the oracle set and quorum, the limits and
//!   the state counts.
//! - `GET /forecast/late-score/buckets/{id}` is one open bucket with its
//!   forecasts; `GET /forecast/late-score/records/{commit_id}` is one recorded
//!   forecast with its bucket id.

use crate::{
    helpers::{envelope_error, envelope_success, require_role},
    state::AppState,
};
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use actix_web::{HttpRequest, Responder, http::StatusCode, web};
use wrfcoin_native_chain::security::api_auth::ApiRole;
use wrfcoin_shared_protocol::{
    blockchain::transaction::{Transaction, TransactionType},
    consensus::forecast_late_score::{self as late, ForecastScorePayload},
    crypto::chain_binding::{ChainIdentity, forecast_late_score_active},
};

/// Largest accepted JSON body. The payload is capped at 64 KiB, and it sits
/// in the transaction as an escaped JSON string, so allow for escaping.
pub const MAX_FORECAST_SCORE_JSON_BYTES: usize = 4 * late::MAX_FORECAST_SCORE_PAYLOAD_BYTES;

/// Score submissions one bearer token may make per minute.
pub const RATE_LIMIT_PER_MIN: usize = 60;
const WINDOW: Duration = Duration::from_secs(60);
const MAX_TRACKED_TOKENS: usize = 1024;
static LIMITER: Mutex<Option<HashMap<[u8; 32], VecDeque<Instant>>>> = Mutex::new(None);

/// Sliding-window limiter keyed by a hash of the bearer token (the same shape
/// as the node-fraud route's). False when over budget.
fn allow(token: &str, now: Instant) -> bool {
    let key = *wrfcoin_shared_protocol::crypto::hash::hash_bytes(token.as_bytes()).as_bytes();
    let mut guard = LIMITER.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    if map.len() >= MAX_TRACKED_TOKENS && !map.contains_key(&key) {
        map.retain(|_, q| q.back().is_some_and(|t| now.duration_since(*t) < WINDOW));
        if map.len() >= MAX_TRACKED_TOKENS {
            return false;
        }
    }
    let q = map.entry(key).or_default();
    while q.front().is_some_and(|t| now.duration_since(*t) >= WINDOW) {
        q.pop_front();
    }
    if q.len() >= RATE_LIMIT_PER_MIN {
        return false;
    }
    q.push_back(now);
    true
}

fn limits() -> serde_json::Value {
    serde_json::json!({
        "lead_hours": late::LEAD_HOURS,
        "score_opens_after_secs": late::SCORE_OPENS_AFTER_SECS,
        "score_window_secs": late::SCORE_WINDOW_SECS,
        "max_forecasts_per_score_tx": late::MAX_FORECASTS_PER_SCORE_TX,
        "max_score_txs_per_block": late::MAX_FORECAST_SCORE_TXS_PER_BLOCK,
        "max_window_readings": late::MAX_WINDOW_READINGS,
        "max_observation_signatures": late::MAX_OBSERVATION_SIGNATURES,
        "max_payload_bytes": late::MAX_FORECAST_SCORE_PAYLOAD_BYTES,
        "paid_per_bucket": late::PAID_PER_BUCKET,
        "bucket_cap": late::BUCKET_CAP.to_string(),
        "skill_unit": late::SKILL_UNIT.to_string(),
    })
}

pub async fn submit(req: HttpRequest, state: web::Data<AppState>, body: web::Json<Transaction>) -> impl Responder {
    if let Err(response) = require_role(&req, &state, ApiRole::Contributor) {
        return response;
    }
    // require_role succeeded, so the header is a well-formed known token.
    let token = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .unwrap_or_default();
    // determinism-ok: API-edge per-token request rate limiter; wall-clock only decides whether this HTTP request is served and never feeds consensus, state or a signing preimage
    if !allow(token, Instant::now()) {
        let mut response =
            envelope_error(&req, StatusCode::TOO_MANY_REQUESTS, "RATE_LIMITED", "forecast score rate limit exceeded");
        response.headers_mut().insert(
            actix_web::http::header::RETRY_AFTER,
            actix_web::http::header::HeaderValue::from_static("60"),
        );
        return response;
    }
    let tx = body.into_inner();
    let tip = state.blockchain.read().await.height;
    if !forecast_late_score_active(tip.saturating_add(1)) {
        return envelope_error(
            &req,
            StatusCode::SERVICE_UNAVAILABLE,
            "FEATURE_NOT_ACTIVE",
            "forecast_late_score is not in force at the next height",
        );
    }
    if tx.tx_type != TransactionType::ForecastScore {
        return envelope_error(&req, StatusCode::BAD_REQUEST, "FORECAST_SCORE_PAYLOAD", "not a ForecastScore transaction");
    }
    let Some(payload) = ForecastScorePayload::parse(&tx) else {
        return envelope_error(
            &req,
            StatusCode::BAD_REQUEST,
            "FORECAST_SCORE_PAYLOAD",
            "data is not a forecast score payload (or is over 64 KiB)",
        );
    };
    if let Err(error) = payload.validate() {
        return envelope_error(&req, StatusCode::BAD_REQUEST, "FORECAST_SCORE_PAYLOAD", error.to_string());
    }
    // Cheap refusals before any signature work, the same order as mempool
    // admission (occupancy caps first): a resent score or a full pool costs
    // no signature check here.
    {
        let pool = state.mempool.read().await;
        let Some(tx_id) = tx.id.as_ref() else {
            return envelope_error(&req, StatusCode::BAD_REQUEST, "FORECAST_SCORE_PAYLOAD", "transaction has no id");
        };
        if pool.contains(tx_id) {
            return envelope_error(&req, StatusCode::CONFLICT, "ALREADY_PENDING", "this score is already in the mempool");
        }
        if pool.size() >= wrfcoin_shared_protocol::common::constants::MEMPOOL_CAPACITY {
            return envelope_error(&req, StatusCode::SERVICE_UNAVAILABLE, "MEMPOOL_FULL", "the mempool is at capacity");
        }
        if pool.check_transaction_admission_cost(&tx).is_err() {
            return envelope_error(&req, StatusCode::UNPROCESSABLE_ENTITY, "FORECAST_SCORE_REFUSED", "admission cost refused");
        }
    }
    // Read-only signer checks at the tip, before the pool is touched: every
    // signer in the oracle set, every signature verifying through the
    // registry. The quorum count is NOT a refusal here (see the module doc).
    let (signing_keys, below_quorum_at_tip) = {
        let chain = state.blockchain.read().await;
        if !payload.signatures.iter().all(|s| chain.state.oracle_set.contains(&s.oracle)) {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "FORECAST_SCORE_REFUSED",
                late::ForecastLateScoreError::UnauthorizedOracle.to_string(),
            );
        }
        let identity = ChainIdentity::active();
        if let Err(error) = payload.verify_registered_signers(
            &identity.chain_id,
            &identity.network_id,
            &chain.state.signing_keys,
            tx.timestamp.timestamp(),
        ) {
            return envelope_error(&req, StatusCode::UNPROCESSABLE_ENTITY, "FORECAST_SCORE_REFUSED", error.to_string());
        }
        (chain.state.signing_keys.clone(), payload.signatures.len() < chain.state.oracle_quorum.max(1))
    };
    let id = tx.id.map(|id| id.to_hex());
    #[cfg(feature = "p2p")]
    let serialized = bincode::serialize(&tx).map_err(|error| error.to_string());
    if let Err(error) =
        state.mempool.write().await.add_transaction_with_signing_keys_at_height(tx, Some(&signing_keys), Some(tip))
    {
        return envelope_error(&req, StatusCode::UNPROCESSABLE_ENTITY, "MEMPOOL_REJECTED", error.to_string());
    }
    #[cfg(feature = "p2p")]
    if let Some(ref p2p) = state.p2p {
        crate::helpers::queue_transaction_gossip(&p2p.command_tx, serialized, &state.tx_gossip_backpressure_streak);
    }
    // "Submitted" is not "scored": a client shows it as pending until the
    // forecasts carry a skill in GET /forecast/late-score/records/{id}.
    envelope_success(
        &req,
        StatusCode::ACCEPTED,
        serde_json::json!({
            "tx_id": id,
            "forecasts": payload.forecast_commit_ids,
            "admitted": true,
            "included": false,
            // The tip's quorum is above the signatures carried: the score
            // applies only if the including block lowers it first.
            "below_quorum_at_tip": below_quorum_at_tip,
        }),
    )
}

pub async fn status(req: HttpRequest, state: web::Data<AppState>) -> impl Responder {
    let chain = state.blockchain.read().await;
    let lane = &chain.state.forecast_late_score;
    let identity = ChainIdentity::active();
    let mut oracle_set: Vec<&String> = chain.state.oracle_set.iter().collect();
    oracle_set.sort();
    envelope_success(
        &req,
        StatusCode::OK,
        serde_json::json!({
            "in_force_next_height": forecast_late_score_active(chain.height.saturating_add(1)),
            "observed_at_height": chain.height,
            // What a signer must bind (`--chain-id` / `--network-id` in the CLI).
            "chain_id": identity.chain_id,
            "network_id": identity.network_id,
            "oracle_set": oracle_set,
            "oracle_quorum": chain.state.oracle_quorum,
            "pending_forecasts": lane.pending.len(),
            "records": lane.records.len(),
            "open_buckets": lane.buckets.len(),
            "limits": limits(),
        }),
    )
}

pub async fn bucket(req: HttpRequest, state: web::Data<AppState>, id: web::Path<String>) -> impl Responder {
    let id = id.into_inner();
    let chain = state.blockchain.read().await;
    let lane = &chain.state.forecast_late_score;
    let Some(bucket) = lane.buckets.get(id.trim()) else {
        return envelope_error(&req, StatusCode::NOT_FOUND, "BUCKET_NOT_FOUND", "no open bucket with this id");
    };
    let forecasts: Vec<serde_json::Value> = bucket
        .commit_ids
        .iter()
        .map(|commit_id| serde_json::json!({"commit_id": commit_id, "record": lane.records.get(commit_id)}))
        .collect();
    envelope_success(
        &req,
        StatusCode::OK,
        serde_json::json!({
            "observed_at_height": chain.height,
            "bucket_id": id.trim(),
            "station_id": bucket.station_id,
            "variable": bucket.variable,
            "lead_hours": bucket.lead_hours,
            "hour": bucket.hour,
            "closes_at": late::bucket_closes_at(bucket.hour),
            "observations": bucket.observations,
            "forecasts": forecasts,
        }),
    )
}

pub async fn record(req: HttpRequest, state: web::Data<AppState>, commit_id: web::Path<String>) -> impl Responder {
    let commit_id = commit_id.into_inner();
    let chain = state.blockchain.read().await;
    let Some(record) = chain.state.forecast_late_score.records.get(commit_id.trim()) else {
        return envelope_error(&req, StatusCode::NOT_FOUND, "RECORD_NOT_FOUND", "no recorded forecast with this commit id");
    };
    let bucket_id = late::bucket_hour(record.target)
        .and_then(|hour| late::bucket_id(&record.station_id, record.variable, record.lead_hours, hour));
    envelope_success(
        &req,
        StatusCode::OK,
        serde_json::json!({
            "observed_at_height": chain.height,
            "commit_id": commit_id.trim(),
            "bucket_id": bucket_id,
            "scored": record.skill.is_some(),
            "record": record,
        }),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn json_body_cap_covers_an_escaped_payload() {
        assert!(super::MAX_FORECAST_SCORE_JSON_BYTES >= 2 * super::late::MAX_FORECAST_SCORE_PAYLOAD_BYTES);
    }
}
