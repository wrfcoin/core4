//! Operator-gated submission of a `NodeFraudReport` (core4-archive#3589; design `docs/design/fraud-report-route.md`).
//!
//! Non-forking: this route only admits a transaction the mempool and `apply` already understand.
//! The blame dry-run below is ADVISORY at the edge. Consensus (`node_fraud::apply`) stays the only
//! authority, and the mempool is deliberately not made to run `responsible()`.
use crate::{
    helpers::{envelope_error, require_role},
    state::AppState,
};
use actix_web::{HttpRequest, HttpResponse, Responder, http::StatusCode, web};
use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
    time::{Duration, Instant},
};
use wrfcoin_native_chain::security::api_auth::ApiRole;
use wrfcoin_shared_protocol::{
    blockchain::transaction::{Transaction, TransactionType},
    consensus::{
        node_bond_registry::epoch_close,
        node_fraud::{self, NodeFraudReport},
        node_share_contracts::node_challenge_blocks,
    },
    crypto::{chain_binding::ChainIdentity, hash::hash_bytes},
};

pub const ENABLE_ENV: &str = "WRFCOIN_ENABLE_NODE_FRAUD_REPORT_API";
pub const MAX_NODE_FRAUD_JSON_BYTES: usize = 40 * 1024;
/// Owner-settled 2026-10: 30 requests per minute per Operator token.
pub const RATE_LIMIT_PER_MIN: usize = 30;
const WINDOW: Duration = Duration::from_secs(60);
/// Bound on distinct tokens tracked; the token store is small, so this is a belt-and-braces cap.
const MAX_TRACKED_TOKENS: usize = 1024;

static LIMITER: Mutex<Option<HashMap<[u8; 32], VecDeque<Instant>>>> = Mutex::new(None);

/// Default off: the route is not mounted (404) unless the switch is `1`/`true`.
pub fn api_enabled() -> bool {
    // determinism-ok: API-edge route switch (WRFCOIN_ENABLE_*_API convention); decides only whether this HTTP route is mounted and never enters block validity, a signing preimage or the state root
    std::env::var(ENABLE_ENV)
        .ok()
        .is_some_and(|v| v.trim() == "1" || v.trim().eq_ignore_ascii_case("true"))
}

/// Guard used by the route table: 404 unless the switch is on AND `NodeShareSettlement` is active.
pub fn route_enabled(_: &actix_web::guard::GuardContext<'_>) -> bool {
    use wrfcoin_shared_protocol::crypto::chain_binding::{ConsensusFeature, feature_active};
    api_enabled() && feature_active(ConsensusFeature::NodeShareSettlement, u64::MAX)
}

/// Sliding-window limiter keyed by a hash of the bearer token. Returns false when over budget.
fn allow(token: &str, now: Instant) -> bool {
    let key = *hash_bytes(token.as_bytes()).as_bytes();
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

#[cfg(test)]
pub fn reset_limiter_for_test() {
    *LIMITER.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

fn identity() -> Option<ChainIdentity> {
    let c =
        wrfcoin_shared_protocol::governance::schedule_extension::schedule_extension_authority()?
            .context();
    Some(ChainIdentity::new(c.chain_id, c.network_id))
}

fn err(req: &HttpRequest, status: StatusCode, code: &'static str, msg: &'static str) -> HttpResponse {
    envelope_error(req, status, code, msg)
}

pub async fn submit(
    req: HttpRequest,
    state: web::Data<AppState>,
    body: web::Json<Transaction>,
) -> impl Responder {
    if let Err(response) = require_role(&req, &state, ApiRole::Operator) {
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
        let mut resp = err(
            &req,
            StatusCode::TOO_MANY_REQUESTS,
            "RATE_LIMITED",
            "fraud report rate limit exceeded",
        );
        resp.headers_mut().insert(
            actix_web::http::header::RETRY_AFTER,
            actix_web::http::header::HeaderValue::from_static("60"),
        );
        return resp;
    }
    let tx = body.into_inner();
    // Refuse oversized/underpriced packets before taking the chain lock or verifying signatures.
    if state
        .mempool
        .read()
        .await
        .check_transaction_admission_cost(&tx)
        .is_err()
    {
        return err(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "NODE_FRAUD_REFUSED",
            "fraud report admission refused",
        );
    }
    let payload: Option<NodeFraudReport> = if tx.tx_type == TransactionType::NodeFraudReport {
        node_fraud::parse(&tx)
    } else {
        None
    };
    let Some(payload) = payload else {
        return err(
            &req,
            StatusCode::BAD_REQUEST,
            "NODE_FRAUD_PAYLOAD",
            "malformed or oversized fraud report",
        );
    };
    let Some(identity) = identity() else {
        return err(
            &req,
            StatusCode::SERVICE_UNAVAILABLE,
            "NODE_SERVICE_INACTIVE",
            "Node service inactive",
        );
    };
    let chain = state.blockchain.read().await;
    let Some(registry) = chain.state.node_bond_registry.as_ref() else {
        return err(
            &req,
            StatusCode::SERVICE_UNAVAILABLE,
            "NODE_SERVICE_INACTIVE",
            "Node service inactive",
        );
    };
    if node_fraud::verify_authorization(&payload, &identity, &registry.genesis_hash).is_err() {
        return err(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "NODE_FRAUD_AUTHORIZATION",
            "reporter authorization invalid",
        );
    }
    let Some(carrying) = chain.height.checked_add(1) else {
        return err(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "NODE_FRAUD_REFUSED",
            "no valid carrying height",
        );
    };
    // Advisory deadline: the first claim's epoch decides it, exactly as `binding` does.
    let first_epoch = payload.batch.claims.first().map(|c| c.ticket.context.epoch);
    let deadline = first_epoch
        .and_then(|e| epoch_close(registry.activation_height, e))
        .and_then(|h| {
            h.checked_add(node_challenge_blocks(chain.state.genesis_emission_chain_id()))
        });
    if deadline.is_some_and(|d| carrying > d) {
        return err(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "NODE_FRAUD_EXPIRED",
            "challenge window closed",
        );
    }
    // Dry-run the CONSENSUS blame function (read only, no copy of its rules).
    let responsible = match node_fraud::responsible(&chain.state, &payload, carrying, &identity) {
        Ok(ids) => ids,
        Err(_) => {
            return err(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "NODE_FRAUD_UNPROVABLE",
                "report is not provable against current state",
            );
        },
    };
    let epoch = first_epoch.unwrap_or_default();
    if let Some(fraud) = chain.state.node_fraud.as_ref() {
        if !responsible.is_empty()
            && responsible
                .iter()
                .all(|id| fraud.jailed.contains(id) || fraud.terminals.contains_key(&(epoch, *id)))
        {
            return err(
                &req,
                StatusCode::CONFLICT,
                "NODE_FRAUD_ALREADY_SANCTIONED",
                "every responsible deployment is already sanctioned",
            );
        }
    }
    let height = chain.height;
    drop(chain);

    // Archive the exact signed transaction (retryable, independent of inclusion).
    let id = tx.id.map(|hash| hash.to_hex());
    let archived = serde_json::to_vec(&tx).ok().map(|bytes| {
        let key = *hash_bytes(&bytes).as_bytes();
        (key, bytes)
    });
    let Some((key, bytes)) = archived else {
        return err(
            &req,
            StatusCode::SERVICE_UNAVAILABLE,
            "NODE_FRAUD_ARCHIVE_UNAVAILABLE",
            "fraud report archive unavailable",
        );
    };
    if state
        .storage
        .read()
        .await
        .archive_node_receipt(&key, &bytes)
        .is_err()
    {
        return err(
            &req,
            StatusCode::SERVICE_UNAVAILABLE,
            "NODE_FRAUD_ARCHIVE_UNAVAILABLE",
            "fraud report archive failed; no admission attempted",
        );
    }
    // Serialize before the tx moves into the pool; gossip only after admission.
    #[cfg(feature = "p2p")]
    let serialized = bincode::serialize(&tx).map_err(|error| error.to_string());
    if state
        .mempool
        .write()
        .await
        .add_transaction_with_signing_keys_at_height(tx, None, Some(height))
        .is_err()
    {
        return err(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "NODE_FRAUD_REFUSED",
            "fraud report admission refused",
        );
    }
    #[cfg(feature = "p2p")]
    if let Some(ref p2p) = state.p2p {
        crate::helpers::queue_transaction_gossip(
            &p2p.command_tx,
            serialized,
            &state.tx_gossip_backpressure_streak,
        );
    }
    // At most the two accused deployments of one report: bounded, built without collect or with_capacity.
    let mut ids: Vec<String> = Vec::new();
    for id in &responsible {
        ids.push(hex::encode(id));
    }
    HttpResponse::Accepted().json(serde_json::json!({
        "tx_id": id,
        "admitted": true,
        "included": false,
        "responsible": ids,
        "deadline_height": deadline,
    }))
}
