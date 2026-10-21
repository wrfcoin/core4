//! #2137 — the launch_api side of bounded query cost.
//!
//! The rules are `security::query_cost` (pure, unit-tested in shared-protocol);
//! this module gives handlers the node's configured [`QueryCost`], the envelope
//! refusals for each limit, and the **expensive pool**: a process-wide, bounded
//! set of permits for proving-class work, so a burst of proof generation cannot
//! exhaust the blocking pool that ordinary requests also need (the #1832 lesson).
//! Ordinary async handlers never touch the pool, so its saturation is invisible
//! to them — `tests/query_cost_2137.rs` proves that.
//!
//! Configuration (all optional):
//!
//! | Variable | Default | Meaning |
//! |---|---|---|
//! | `WRFCOIN_QUERY_MAX_PAGE_SIZE` | 500 | larger pages are refused, not clamped |
//! | `WRFCOIN_QUERY_MAX_SCAN_UNITS` | 50 000 | records one query may examine |
//! | `WRFCOIN_QUERY_MAX_HISTORY_BLOCKS` | 1 000 000 | widest block range one query may address |
//! | `WRFCOIN_QUERY_MAX_WALL_MS` | 2000 | wall-clock deadline for one traversal |
//! | `WRFCOIN_EXPENSIVE_POOL_SIZE` | max(1, cores/2) | concurrent proving-class jobs |

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use actix_web::{HttpRequest, HttpResponse, http::StatusCode, web};
use tokio::sync::{Semaphore, TryAcquireError};
use wrfcoin_native_chain::security::query_cost::{
    CostExceeded, CursorError, InputsExceeded, PageError, QueryCost, RangeError,
};

use crate::helpers::{envelope_error, envelope_redacted};
use wrfcoin_native_chain::security::error_redaction::{ErrorKind, InternalError};

pub const MAX_PAGE_SIZE_ENV: &str = "WRFCOIN_QUERY_MAX_PAGE_SIZE";
pub const MAX_SCAN_UNITS_ENV: &str = "WRFCOIN_QUERY_MAX_SCAN_UNITS";
pub const MAX_HISTORY_BLOCKS_ENV: &str = "WRFCOIN_QUERY_MAX_HISTORY_BLOCKS";
pub const MAX_WALL_MS_ENV: &str = "WRFCOIN_QUERY_MAX_WALL_MS";
pub const EXPENSIVE_POOL_SIZE_ENV: &str = "WRFCOIN_EXPENSIVE_POOL_SIZE";
/// Seconds a caller is told to wait when the expensive pool is saturated.
pub const EXPENSIVE_POOL_RETRY_AFTER_SECS: u64 = 2;

fn env_u64(name: &str) -> Option<u64> {
    // determinism-ok: API query/worker admission limits only; these values never enter consensus state, a block, or a signed preimage.
    std::env::var(name).ok().and_then(|v| v.trim().parse::<u64>().ok()).filter(|v| *v > 0)
}

/// Build the limits from explicit values (tests) — `None` keeps the default.
pub fn query_cost_from_values(
    max_page_size: Option<u64>,
    max_scan_units: Option<u64>,
    max_history_blocks: Option<u64>,
    max_wall_ms: Option<u64>,
) -> QueryCost {
    let d = QueryCost::default();
    QueryCost {
        max_page_size: max_page_size.map(|v| v.min(u32::MAX as u64) as u32).unwrap_or(d.max_page_size),
        max_scan_units: max_scan_units.unwrap_or(d.max_scan_units),
        max_history_blocks: max_history_blocks.unwrap_or(d.max_history_blocks),
        max_wall_time: max_wall_ms.map(Duration::from_millis).unwrap_or(d.max_wall_time),
    }
}

/// The node's limits, read once from the environment.
pub fn query_cost() -> &'static QueryCost {
    static COST: OnceLock<QueryCost> = OnceLock::new();
    COST.get_or_init(|| {
        query_cost_from_values(
            env_u64(MAX_PAGE_SIZE_ENV),
            env_u64(MAX_SCAN_UNITS_ENV),
            env_u64(MAX_HISTORY_BLOCKS_ENV),
            env_u64(MAX_WALL_MS_ENV),
        )
    })
}

/// 422: the traversal exhausted its budget; the caller narrows the query.
pub fn cost_exceeded(req: &HttpRequest, e: CostExceeded) -> HttpResponse {
    envelope_error(req, StatusCode::UNPROCESSABLE_ENTITY, "QUERY_TOO_EXPENSIVE", e.to_string())
}

/// 400: the requested page is too large (never truncated) or zero.
pub fn page_rejected(req: &HttpRequest, e: PageError) -> HttpResponse {
    let code = match e {
        PageError::Zero => "BAD_REQUEST",
        PageError::TooLarge { .. } => "PAGE_TOO_LARGE",
    };
    envelope_error(req, StatusCode::BAD_REQUEST, code, e.to_string())
}

/// 400: the block range is inverted or wider than the node serves.
pub fn range_rejected(req: &HttpRequest, e: RangeError) -> HttpResponse {
    let code = match e {
        RangeError::Inverted { .. } => "RANGE_INVERTED",
        RangeError::TooWide { .. } => "RANGE_TOO_WIDE",
    };
    envelope_error(req, StatusCode::BAD_REQUEST, code, e.to_string())
}

/// 400: the continuation token was not issued by this node.
pub fn cursor_rejected(req: &HttpRequest, e: CursorError) -> HttpResponse {
    envelope_error(req, StatusCode::BAD_REQUEST, "BAD_CURSOR", e.to_string())
}

/// 413: the request declares more inputs than the node will allocate for.
pub fn inputs_rejected(req: &HttpRequest, e: InputsExceeded) -> HttpResponse {
    envelope_error(req, StatusCode::PAYLOAD_TOO_LARGE, "TOO_MANY_INPUTS", e.to_string())
}

fn default_pool_size() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get() / 2)
        .unwrap_or(1)
        .max(1)
}

/// The expensive pool: permits for proving-class work.
pub fn expensive_pool() -> &'static Arc<Semaphore> {
    static POOL: OnceLock<Arc<Semaphore>> = OnceLock::new();
    POOL.get_or_init(|| {
        let size = env_u64(EXPENSIVE_POOL_SIZE_ENV).map(|v| v as usize).unwrap_or_else(default_pool_size);
        Arc::new(Semaphore::new(size))
    })
}

/// 503 with `Retry-After`: every permit is in use. Refused immediately rather
/// than queued, so a saturated pool never holds a worker.
pub fn expensive_pool_saturated(req: &HttpRequest) -> HttpResponse {
    let mut response = envelope_error(
        req,
        StatusCode::SERVICE_UNAVAILABLE,
        "EXPENSIVE_POOL_SATURATED",
        "Proof-generation capacity is fully in use; retry later",
    );
    response.headers_mut().insert(
        actix_web::http::header::RETRY_AFTER,
        actix_web::http::header::HeaderValue::from_str(&EXPENSIVE_POOL_RETRY_AFTER_SECS.to_string())
            .expect("static header value"), // panic-ok: decimal formatting of a compile-time u64 constant is always a valid HTTP header value.
    );
    response
}

/// Run proving-class `work` on a blocking thread under one pool permit. When
/// the pool is saturated the caller gets the 503 without waiting; the permit is
/// held for the whole computation and released when it returns.
pub async fn run_expensive<T, F>(req: &HttpRequest, pool: &Arc<Semaphore>, work: F) -> Result<T, HttpResponse>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let permit = match pool.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(TryAcquireError::NoPermits) => return Err(expensive_pool_saturated(req)),
        Err(TryAcquireError::Closed) => {
            return Err(envelope_error(
                req,
                StatusCode::SERVICE_UNAVAILABLE,
                "EXPENSIVE_POOL_CLOSED",
                "Proof generation is unavailable on this node",
            ));
        }
    };
    match web::block(move || {
        let _permit = permit;
        work()
    })
    .await
    {
        Ok(value) => Ok(value),
        Err(e) => Err(envelope_redacted(
            req,
            InternalError::new(ErrorKind::Internal, format!("blocking worker failed: {e}")),
        )),
    }
}
