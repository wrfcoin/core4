//! #2148 — contribution-side Sybil caps on the ingress edge.
//!
//! One process-wide [`SybilLimiter`] guards `POST /transactions/contribution/commit`:
//! an identity may commit for at most `WRFCOIN_SYBIL_MAX_STATIONS_PER_IDENTITY`
//! stations, and a station may make at most `WRFCOIN_SYBIL_MAX_COMMITS_PER_STATION`
//! commits per `WRFCOIN_SYBIL_WINDOW_SECS`. The identity is the bearer's
//! wallet claim when a non-operator token carries one (compared
//! case-insensitively) and the request's `contributor` id otherwise, including
//! for the shared operator token, which acts for many contributors.
//!
//! These caps bound rate per key, not identity. The `contributor` id is
//! self-asserted, and `station_id` is only shape-validated: this edge, mempool
//! admission and `RegisterCommit` never consult the station registry. A bearer
//! can therefore rotate `station_id` and `contributor` for fresh budgets, and
//! those commits can land on-chain; a commit for an unregistered station is
//! `Unclassified` and earns nothing (#2460 gate). Enforcing registration here
//! is an open owner decision (Refs #2148), not something this module provides.
//!
//! Refusals map to stable codes: `SYBIL_STATION_LIMIT` (403),
//! `SYBIL_STATION_COMMIT_CAP` (429 + Retry-After), `SYBIL_LIMITER_CAPACITY`
//! (503 + Retry-After). A refused commit records nothing.

use std::sync::{Mutex, OnceLock};

use actix_web::http::header::{HeaderValue, RETRY_AFTER};
use actix_web::{HttpRequest, HttpResponse, http::StatusCode};
use wrfcoin_native_chain::security::api_auth::ApiRole;
use wrfcoin_native_chain::security::sybil::{ContributionSybilPolicy, SybilLimiter, SybilRefusal};

use crate::helpers::{bearer_token_from_request, envelope_error};
use crate::state::AppState;

pub const MAX_STATIONS_ENV: &str = "WRFCOIN_SYBIL_MAX_STATIONS_PER_IDENTITY";
pub const MAX_COMMITS_ENV: &str = "WRFCOIN_SYBIL_MAX_COMMITS_PER_STATION";
pub const WINDOW_ENV: &str = "WRFCOIN_SYBIL_WINDOW_SECS";
pub const MAX_TRACKED_ENV: &str = "WRFCOIN_SYBIL_MAX_TRACKED_ENTRIES";
/// Retry-After for a limiter at capacity: one sweep interval's worth.
pub const CAPACITY_RETRY_AFTER_SECS: u64 = 60;

/// Build a policy from optional values; anything absent keeps the default,
/// and `normalised` keeps every value sane.
pub fn policy_from_values(
    max_stations: Option<u32>,
    max_commits: Option<u32>,
    window_secs: Option<u64>,
    max_tracked: Option<usize>,
) -> ContributionSybilPolicy {
    let d = ContributionSybilPolicy::default();
    ContributionSybilPolicy {
        max_stations_per_identity: max_stations.unwrap_or(d.max_stations_per_identity),
        max_commits_per_station: max_commits.unwrap_or(d.max_commits_per_station),
        window_secs: window_secs.unwrap_or(d.window_secs),
        max_tracked_entries: max_tracked.unwrap_or(d.max_tracked_entries),
    }
    .normalised()
}

fn env_number<T: std::str::FromStr>(name: &str) -> Option<T> {
    // determinism-ok: node-local API admission limiter; never enters consensus state
    std::env::var(name).ok().and_then(|v| v.trim().parse::<T>().ok())
}

pub fn policy_from_env() -> ContributionSybilPolicy {
    policy_from_values(
        env_number(MAX_STATIONS_ENV),
        env_number(MAX_COMMITS_ENV),
        env_number(WINDOW_ENV),
        env_number(MAX_TRACKED_ENV),
    )
}

/// The node-wide limiter, built once from the environment.
pub fn sybil_limiter() -> &'static Mutex<SybilLimiter> {
    static LIMITER: OnceLock<Mutex<SybilLimiter>> = OnceLock::new();
    LIMITER.get_or_init(|| Mutex::new(SybilLimiter::new(policy_from_env())))
}

/// The identity a commit is attributed to: the bearer's wallet claim when a
/// non-operator token carries one, else the self-asserted contributor id.
/// Lower-cased so a case-varied address cannot mint fresh identities.
///
/// The shared operator token acts for many contributors, so its wallet claim
/// (`WRFCOIN_OPERATOR_WALLET_ADDRESS`) is never the identity: keying on it
/// would collapse every station the node submits for into one identity capped
/// at `max_stations_per_identity` (#2148 review F3).
pub fn contribution_identity(req: &HttpRequest, state: &AppState, contributor: &str) -> String {
    bearer_token_from_request(req)
        .and_then(|token| state.token_store.get(token))
        .filter(|principal| principal.role != ApiRole::Operator)
        .and_then(|principal| principal.identity.as_ref())
        .and_then(|identity| identity.wallet_address.as_deref())
        .map(str::trim)
        .filter(|wallet| !wallet.is_empty())
        .map(|wallet| format!("wallet:{}", wallet.to_ascii_lowercase()))
        .unwrap_or_else(|| format!("contributor:{}", contributor.trim().to_ascii_lowercase()))
}

fn refusal_response(req: &HttpRequest, refusal: SybilRefusal) -> HttpResponse {
    let (status, retry_after) = match &refusal {
        SybilRefusal::TooManyStations { .. } => (StatusCode::FORBIDDEN, None),
        SybilRefusal::StationCommitCapExceeded { retry_after_secs, .. } => {
            (StatusCode::TOO_MANY_REQUESTS, Some(*retry_after_secs))
        },
        SybilRefusal::LimiterCapacity { .. } => {
            (StatusCode::SERVICE_UNAVAILABLE, Some(CAPACITY_RETRY_AFTER_SECS))
        },
    };
    let mut response = envelope_error(req, status, refusal.code(), refusal.to_string());
    if let Some(secs) = retry_after {
        response.headers_mut().insert(
            RETRY_AFTER,
            HeaderValue::from_str(&secs.to_string()).unwrap_or_else(|_| HeaderValue::from_static("60")),
        );
    }
    response
}

/// Enforce the caps against a specific limiter (tests pass their own).
pub fn enforce_with(
    limiter: &Mutex<SybilLimiter>,
    req: &HttpRequest,
    state: &AppState,
    contributor: &str,
    station_id: &str,
    now_secs: i64,
) -> Result<(), HttpResponse> {
    let identity = contribution_identity(req, state, contributor);
    let verdict = limiter
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .admit_commit(&identity, station_id.trim(), now_secs);
    verdict.map_err(|refusal| refusal_response(req, refusal))
}

/// Enforce the caps against the node-wide limiter.
pub fn enforce_contribution_sybil_policy(
    req: &HttpRequest,
    state: &AppState,
    contributor: &str,
    station_id: &str,
) -> Result<(), HttpResponse> {
    enforce_with(
        sybil_limiter(),
        req,
        state,
        contributor,
        station_id,
        // determinism-ok: node-local API admission limiter; never enters consensus state
        chrono::Utc::now().timestamp(),
    )
}
