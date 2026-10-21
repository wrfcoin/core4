use actix_web::{HttpRequest, HttpResponse, Responder, web};
use serde::{Deserialize, Serialize};

use crate::helpers::{envelope_error, require_role};
use crate::state::AppState;
use wrfcoin_native_chain::security::api_auth::ApiRole;

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct PurgeQuery {
    /// Maximum age in seconds; transactions older than this are removed.
    pub max_age_secs: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct MempoolPurgeResponse {
    pub removed: usize,
    pub remaining: usize,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// POST /operator/mempool/purge?max_age_secs=N
///
/// Removes transactions that have been in the mempool longer than `max_age_secs`
/// seconds. Defaults to 3600s (1 hour) if the query parameter is omitted.
/// Requires Operator auth.
pub async fn purge_mempool(
    req: HttpRequest,
    state: web::Data<AppState>,
    query: web::Query<PurgeQuery>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }

    let max_age_secs = query.max_age_secs.unwrap_or(3600);
    if max_age_secs == 0 {
        return envelope_error(
            &req,
            actix_web::http::StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "max_age_secs must be greater than 0",
        );
    }

    let (removed, remaining) = {
        let mut mempool = state.mempool.write().await;
        let removed = mempool.purge_older_than_secs(max_age_secs);
        let remaining = mempool.size();
        (removed, remaining)
    };

    log::info!(
        "Operator mempool purge: max_age_secs={} removed={} remaining={}",
        max_age_secs,
        removed,
        remaining,
    );

    HttpResponse::Ok().json(MempoolPurgeResponse { removed, remaining })
}

/// POST /operator/mempool/clear
///
/// Removes all pending transactions from the mempool (nuclear option).
/// Requires Operator auth.
pub async fn clear_mempool(
    req: HttpRequest,
    state: web::Data<AppState>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }

    let (removed, remaining) = {
        let mut mempool = state.mempool.write().await;
        let removed = mempool.clear_all();
        let remaining = mempool.size();
        (removed, remaining)
    };

    log::info!(
        "Operator mempool clear: removed={} remaining={}",
        removed,
        remaining,
    );

    HttpResponse::Ok().json(MempoolPurgeResponse { removed, remaining })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn purge_query_defaults() {
        let q = PurgeQuery { max_age_secs: None };
        assert_eq!(q.max_age_secs.unwrap_or(3600), 3600);
    }

    #[test]
    fn purge_query_custom() {
        let q = PurgeQuery { max_age_secs: Some(600) };
        assert_eq!(q.max_age_secs.unwrap_or(3600), 600);
    }
}
