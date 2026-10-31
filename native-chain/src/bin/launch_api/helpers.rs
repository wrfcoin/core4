use actix_web::{HttpRequest, HttpResponse, http::StatusCode, web};
use serde::Serialize;
use wrfcoin_native_chain::security::error_redaction::{InternalError, to_public};
use std::time::{SystemTime, UNIX_EPOCH};

/// THE process-environment lock for this test binary (#2290/#2398).
///
/// Process env is global to the whole test binary and `cargo test` runs tests
/// concurrently, so **every** test that mutates *any* environment variable must
/// serialize on this ONE lock. Two independent mutexes do not exclude each
/// other: this binary previously had `genesis_producers`' lock guarding the
/// producer-auth variables while `preflight_tests` cleared
/// `WRFCOIN_NETWORK_ID` under a *different* mutex. That let a preflight test
/// clear the network selector after a producer-auth test set it but before
/// `resolve()` read it — and the observed failure was not merely a flake, it
/// was the security test seeing an **empty, unenforced** producer set.
///
/// Do not add another env mutex. Add a caller to this one.
///
/// **Audited callers** (whole-binary sweep, #2398 review): `preflight_tests`,
/// `genesis_producers::tests::with_env`, `main.rs` token-store tests (via
/// `token_store_env_lock`, which now delegates here), the three CORS tests
/// that set `LAUNCH_API_CORS_ORIGINS`, the two `p2p_events` parity tests that
/// install `GAMIFICATION_OPERATOR_PUBKEYS_ENV` /
/// `ORACLE_GOVERNANCE_PUBKEYS_ENV`, and
/// `transition_extractor::{gamification_test_operator, oracle_governance_test_key}`
/// which install those same two variables.
///
/// **There is NO "same value" exception.** An earlier revision of this comment
/// claimed that writes of a byte-identical value (from a fixed RNG seed) were
/// safe without the lock. That reasoning is wrong: `setenv` mutates the
/// `environ` array itself and may reallocate and free it, so two threads
/// writing the *same* value can still corrupt memory or crash a concurrent
/// reader. The race is on the array, not on the value — which is why Rust 2024
/// marks `std::env::set_var` `unsafe`. Every write takes this lock, full stop.
///
/// **There are no known remaining offenders.** Every `std::env::set_var` /
/// `remove_var` in this test binary takes this lock. The last two —
/// `transition_extractor::{gamification_test_operator, oracle_governance_test_key}`
/// — were split out of #2398 to keep a consensus-breaking PR small and closed
/// separately; they are in the audited list above.
///
/// If you are adding an env-mutating test: take this lock, and add yourself to
/// the list above. If you believe your write is exempt, re-read the paragraph
/// above — the "same value" argument is the one that has already been wrong
/// once.
#[cfg(test)]
#[path = "test_env.rs"]
pub(crate) mod test_env;

use crate::state::{
    AppState, CONTRIBUTION_COMMIT_RATE_LIMIT_MAX_KEYS, CONTRIBUTION_COMMIT_RATE_LIMIT_MAX_REQUESTS,
    CONTRIBUTION_COMMIT_RATE_LIMIT_WINDOW_SECS, LEGACY_SUBMIT_RATE_LIMIT_MAX_KEYS,
    LEGACY_SUBMIT_RATE_LIMIT_MAX_REQUESTS, LEGACY_SUBMIT_RATE_LIMIT_WINDOW_SECS,
    LEGACY_SUBMIT_RETRY_AFTER_SECS, LEGACY_SUBMIT_TOKEN_RATE_LIMIT_MAX_REQUESTS, MAX_NFT_ID_LEN,
    MAX_NONCE_LEN, MAX_PARTICIPANT_ID_LEN, MAX_STATION_ID_LEN, PERMISSIONLESS_REPLAY_TTL_SECS,
    PERMISSIONLESS_SUBMIT_MAX_REQUESTS, PERMISSIONLESS_SUBMIT_RATE_LIMIT_MAX_KEYS,
    PERMISSIONLESS_SUBMIT_WINDOW_SECS, SlidingWindowCounter,
};
use actix_web::http::header::{HeaderValue, RETRY_AFTER};
use rustc_hash::FxHashMap;
use wrfcoin_native_chain::crypto::signature::PublicKey;
use wrfcoin_native_chain::security::api_auth::ApiRole;
use wrfcoin_native_chain::security::key_registry::SigningRole;

/// Allow one full target block interval before surfacing stale/syncing state.
/// The node host testnet archive shows healthy cadence slightly above 5 minutes,
/// so a 300-second cutoff flaps between healthy blocks.
pub const NODE_SYNC_STALE_AFTER_SECS: i64 = 600;

// ---------------------------------------------------------------------------
// API error / envelope types
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ApiErrorResponse {
    pub error: String,
    pub code: u16,
}

#[derive(Debug, Serialize)]
pub struct LawBookEnvelopeMeta {
    pub request_id: String,
    pub timestamp: String,
    pub api_version: &'static str,
}

#[derive(Debug, Serialize)]
pub struct LawBookEnvelopeError {
    pub code: String,
    pub message: String,
    /// #2138: present when the error went through `envelope_redacted`. The
    /// full internal detail is logged server-side under this id; the client
    /// receives only the class and a static message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LawBookEnvelope<T: Serialize> {
    pub ok: bool,
    pub data: Option<T>,
    pub error: Option<LawBookEnvelopeError>,
    pub meta: LawBookEnvelopeMeta,
}

// ---------------------------------------------------------------------------
// Error / envelope helpers
// ---------------------------------------------------------------------------

#[deprecated(note = "Use envelope_error() instead")]
#[allow(dead_code)]
pub fn api_error(status: StatusCode, message: impl Into<String>) -> HttpResponse {
    let body = ApiErrorResponse {
        error: message.into(),
        code: status.as_u16(),
    };
    HttpResponse::build(status).json(body)
}

pub fn envelope_meta(request: &HttpRequest) -> LawBookEnvelopeMeta {
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            let now_nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            format!("req_{now_nanos:x}")
        });

    LawBookEnvelopeMeta {
        request_id,
        timestamp: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        api_version: "v1",
    }
}

pub fn envelope_success<T: Serialize>(
    request: &HttpRequest,
    status: StatusCode,
    data: T,
) -> HttpResponse {
    let meta = envelope_meta(request);
    let request_id = meta.request_id.clone();
    HttpResponse::build(status)
        .insert_header(("X-Request-Id", request_id))
        .json(LawBookEnvelope {
            ok: true,
            data: Some(data),
            error: None,
            meta,
        })
}

pub fn envelope_error(
    request: &HttpRequest,
    status: StatusCode,
    code: &'static str,
    message: impl Into<String>,
) -> HttpResponse {
    let meta = envelope_meta(request);
    let request_id = meta.request_id.clone();
    HttpResponse::build(status)
        .insert_header(("X-Request-Id", request_id))
        .json(LawBookEnvelope::<serde_json::Value> {
            ok: false,
            data: None,
            error: Some(LawBookEnvelopeError {
                code: code.to_string(),
                message: message.into(),
                correlation_id: None,
            }),
            meta,
        })
}

/// #2138: the only way an internal failure crosses the trust boundary. The
/// client receives the class's envelope code, its **static** message and a
/// correlation id (also sent as `X-Correlation-Id`); the full detail — store
/// names, paths, dependency messages — is logged here against that id, so
/// support can correlate without the client learning anything. Use this for
/// every 5xx and for any 4xx whose detail would echo something internal;
/// `envelope_error` remains for messages that are deliberately static or that
/// only echo the caller's own malformed input.
pub fn envelope_redacted(request: &HttpRequest, internal: InternalError) -> HttpResponse {
    let redacted = to_public(&internal);
    let meta = envelope_meta(request);
    let request_id = meta.request_id.clone();
    if internal.kind.is_server_fault() {
        log::error!("request_id={request_id} {}", redacted.record);
    } else {
        log::warn!("request_id={request_id} {}", redacted.record);
    }
    let status = StatusCode::from_u16(internal.kind.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let correlation_id = redacted.public.correlation_id.to_string();
    HttpResponse::build(status)
        .insert_header(("X-Request-Id", request_id))
        .insert_header(("X-Correlation-Id", correlation_id.clone()))
        .json(LawBookEnvelope::<serde_json::Value> {
            ok: false,
            data: None,
            error: Some(LawBookEnvelopeError {
                code: internal.kind.envelope_code().to_string(),
                message: redacted.public.message.to_string(),
                correlation_id: Some(correlation_id),
            }),
            meta,
        })
}

/// `503` response for the node-local oracle/signing-key admin routes when they
/// are disabled (the default, #1775). These routes fold `oracle_set` /
/// `oracle_quorum` / Oracle-role `signing_keys` into the consensus `state_root`
/// out of band (no gossiped tx, never reconstructed by `extract_transitions`),
/// so enabling them on a multi-node network forks this node from its peers —
/// same class as the NFT mutation API (#1764). Gated by
/// `AppState::oracle_admin_api_enabled` / `WRFCOIN_ENABLE_ORACLE_ADMIN_API=1`.
pub fn oracle_admin_api_disabled(request: &HttpRequest) -> HttpResponse {
    envelope_error(
        request,
        StatusCode::SERVICE_UNAVAILABLE,
        "ORACLE_ADMIN_API_DISABLED",
        "Node-local oracle/signing-key admin endpoints are disabled on this \
         node. They mutate oracle_set / oracle_quorum / Oracle-role signing \
         keys — all part of the consensus state root — out of band (not \
         gossiped, not reconstructed by a syncing peer), so enabling them would \
         fork this node away from its peers (core4#1775). Seed oracle/signing-key \
         config deterministically at genesis instead. Set \
         WRFCOIN_ENABLE_ORACLE_ADMIN_API=1 to re-enable on single-node or \
         trusted-operator deployments only.",
    )
}

/// `409` response when a node-local state mutation is attempted on a live
/// network (height > 0). These endpoints bypass block production and gossip —
/// applying them after genesis forks this node's `state_root` away from peers
/// that never saw the same call (core4#1856).
pub fn live_network_mutation_rejected(request: &HttpRequest, height: u64) -> HttpResponse {
    envelope_error(
        request,
        StatusCode::CONFLICT,
        "LIVE_NETWORK_MUTATION_REJECTED",
        format!(
            "This endpoint mutates consensus state_root-hashed fields out of band \
             (no block, no gossip path) and is only safe at genesis (height 0). \
             The chain is at height {height} — applying this mutation would fork \
             this node's state root away from peers that did not receive the same \
             call. Use a governance proposal or the oracle bootstrap ceremony to \
             change oracle / NFT / signing-key state on a live network (core4#1856)."
        ),
    )
}

/// `503` response for the node-local debug seeding route (`POST
/// /debug/seed-account`) when it is disabled (the default, #1811). The route
/// mutates `accounts` (via `ApplyReward`) and `stake_locked` /
/// `spendable_balance` — all part of the consensus `state_root` — out of band
/// (no gossiped tx, never reconstructed by `extract_transitions`), so a single
/// call forks this node from its peers; same class as the NFT mutation API
/// (#1764) and oracle/signing-key admin API (#1775). Gated by
/// `AppState::debug_seed_api_enabled` / `WRFCOIN_ENABLE_DEBUG_SEED_API=1`.
#[cfg(any(test, feature = "dev-endpoints"))]
pub fn debug_seed_api_disabled(request: &HttpRequest) -> HttpResponse {
    envelope_error(
        request,
        StatusCode::SERVICE_UNAVAILABLE,
        "DEBUG_SEED_API_DISABLED",
        "The debug account-seeding endpoint is disabled on this node. It \
         mutates account balances and locked stake — all part of the consensus \
         state root — out of band (not gossiped, not reconstructed by a syncing \
         peer), so a single call would fork this node away from its peers \
         (core4#1811). Set WRFCOIN_ENABLE_DEBUG_SEED_API=1 to re-enable on \
         single-node or trusted-operator deployments only.",
    )
}

pub fn block_age_is_stale(height: u64, age_secs: i64) -> bool {
    height > 0 && age_secs > NODE_SYNC_STALE_AFTER_SECS
}

pub fn sync_status_from_block_age(height: u64, age_secs: i64) -> &'static str {
    if height == 0 {
        "genesis"
    } else if block_age_is_stale(height, age_secs) {
        "syncing"
    } else {
        "synced"
    }
}

pub fn law_book_ok<T: Serialize>(request: &HttpRequest, data: T) -> HttpResponse {
    envelope_success(request, StatusCode::OK, data)
}

pub fn law_book_error(
    request: &HttpRequest,
    status: StatusCode,
    code: &'static str,
    message: impl Into<String>,
) -> HttpResponse {
    envelope_error(request, status, code, message)
}

pub fn feature_disabled_envelope(request: &HttpRequest, feature: &str) -> HttpResponse {
    envelope_error(
        request,
        StatusCode::SERVICE_UNAVAILABLE,
        "FEATURE_DISABLED",
        format!("{feature} is disabled on this node"),
    )
}

pub fn feature_disabled_envelope_with_hint(
    request: &HttpRequest,
    feature: &str,
    hint: &str,
) -> HttpResponse {
    envelope_error(
        request,
        StatusCode::SERVICE_UNAVAILABLE,
        "FEATURE_DISABLED",
        format!("{feature} is disabled on this node; {hint}"),
    )
}

// ---------------------------------------------------------------------------
// Auth helpers
// ---------------------------------------------------------------------------

pub fn bearer_token_from_request(request: &HttpRequest) -> Option<&str> {
    request
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .filter(|token| !token.is_empty())
}

pub fn require_role(
    req: &HttpRequest,
    state: &AppState,
    required: ApiRole,
) -> Result<ApiRole, HttpResponse> {
    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok());

    let token = match auth_header.and_then(|h| h.strip_prefix("Bearer ")) {
        Some(t) if !t.is_empty() => t,
        _ => {
            return Err(envelope_error(
                req,
                StatusCode::UNAUTHORIZED,
                "UNAUTHORIZED",
                "Missing or malformed Authorization header",
            ));
        },
    };

    let caller_principal = match state.token_store.get(token) {
        Some(principal) => principal.clone(),
        None => {
            return Err(envelope_error(
                req,
                StatusCode::FORBIDDEN,
                "FORBIDDEN",
                "Invalid API token",
            ));
        },
    };

    if caller_principal.satisfies_role(required) {
        Ok(caller_principal.role)
    } else {
        Err(envelope_error(
            req,
            StatusCode::FORBIDDEN,
            "FORBIDDEN",
            format!("{required} role required"),
        ))
    }
}

/// #3070 item 1 — forecast submissions are wallet-scoped.
///
/// The bearer grants the Contributor *role*; it does not say whom the caller
/// may submit for. Without this check one Contributor bearer could commit and
/// reveal for any `forecaster` address — collecting another party's skill
/// reward, or spreading commits across arbitrary payees to sidestep the
/// per-forecaster commit rate limit. The rule:
///
/// - an Operator bearer may submit for any forecaster (it acts for the node);
/// - any other bearer must carry a wallet claim
///   (`WRFCOIN_CONTRIBUTOR_WALLET_ADDRESS` on the token) and `forecaster` must
///   equal it, compared case-insensitively exactly like the `wallet:` websocket
///   channel;
/// - a bearer with no wallet claim cannot submit for anyone — refusing is the
///   only reading that never widens who a token can pay.
///
/// Call after `require_role(.., ApiRole::Contributor)`, which already refused
/// missing and unknown tokens.
/// On success returns the **canonical forecaster**: the one value the caller
/// must use for the rate-limit key, the transaction sender and the reveal.
/// For a wallet-bound bearer it is the bearer's (trimmed) wallet claim, so case
/// or whitespace variants of the wallet cannot mint fresh rate-limit buckets or
/// distinct sender strings (review of core4#3114). For an operator bearer, which
/// is exempt from the binding, it is the trimmed request value.
pub fn require_forecaster_bound_to_bearer(
    req: &HttpRequest,
    state: &AppState,
    forecaster: &str,
) -> Result<String, HttpResponse> {
    let principal = bearer_token_from_request(req)
        .and_then(|token| state.token_store.get(token))
        .ok_or_else(|| envelope_error(req, StatusCode::FORBIDDEN, "FORBIDDEN", "Invalid API token"))?;
    if principal.role == ApiRole::Operator {
        return Ok(forecaster.trim().to_string());
    }
    match principal
        .identity
        .as_ref()
        .and_then(|identity| identity.wallet_address.as_deref())
        .map(str::trim)
        .filter(|wallet| !wallet.is_empty())
    {
        Some(wallet) if wallet.eq_ignore_ascii_case(forecaster.trim()) => Ok(wallet.to_string()),
        Some(_) => Err(envelope_error(
            req,
            StatusCode::FORBIDDEN,
            "FORECASTER_NOT_BOUND_TO_BEARER",
            "forecaster must be the wallet address bound to this bearer",
        )),
        None => Err(envelope_error(
            req,
            StatusCode::FORBIDDEN,
            "FORECASTER_BEARER_UNBOUND",
            "forecast submissions are wallet-scoped: this bearer carries no wallet claim \
             (bind WRFCOIN_CONTRIBUTOR_WALLET_ADDRESS to the token, or use an operator bearer)",
        )),
    }
}

/// Returns `true` if the request comes from localhost OR carries a valid Operator bearer token.
/// Used by health/readyz endpoints to decide between minimal and full responses.
pub fn is_operator_or_localhost(req: &HttpRequest, state: &AppState) -> bool {
    // Check TCP peer is localhost (not spoofable via headers).
    if let Some(addr) = req.peer_addr() {
        if addr.ip().is_loopback() {
            return true;
        }
    }
    // Check for valid Operator bearer token.
    if let Some(token) = bearer_token_from_request(req) {
        if let Some(principal) = state.token_store.get(token) {
            return principal.satisfies_role(ApiRole::Operator);
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Rate limiting / replay protection
// ---------------------------------------------------------------------------

/// Legacy `/transactions/submit` budget for requests that FAILED
/// authorization (and for missing authorization), keyed by the caller's
/// bearer token (#3258 R1): every failed attempt costs an ed25519
/// verification, so a token can drive only
/// `LEGACY_SUBMIT_RATE_LIMIT_MAX_REQUESTS` of them per window. It is a budget
/// of its own (#3337): authenticated transfers are charged to
/// [`enforce_legacy_authenticated_submit_rate_limit`] instead, so one relayed
/// user's failures never starve another user's valid sends on the same token.
pub async fn enforce_legacy_submit_rate_limit(
    req: &HttpRequest,
    state: &AppState,
) -> Result<(), HttpResponse> {
    let token = bearer_token_from_request(req).unwrap_or("__missing__");
    charge_legacy_submit_buckets(
        req,
        state,
        vec![(
            token.to_string(),
            LEGACY_SUBMIT_RATE_LIMIT_MAX_REQUESTS,
            "Too many transaction submissions. Please retry later.",
        )],
    )
    .await
}

/// Resolve the per-token authenticated-transfer ceiling, honouring
/// `LEGACY_SUBMIT_TOKEN_RATE_LIMIT_PER_MIN` (#3337). `0` or an unparseable
/// value falls back to the compiled default, never to "unlimited".
pub fn legacy_submit_token_max_requests() -> usize {
    std::env::var("LEGACY_SUBMIT_TOKEN_RATE_LIMIT_PER_MIN") // determinism-ok: HTTP admission budget, never consensus state
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(LEGACY_SUBMIT_TOKEN_RATE_LIMIT_MAX_REQUESTS)
}

/// Legacy `/transactions/submit` budget for a transfer whose `from`-bound
/// authorization VERIFIED (#3337). Two buckets, both must admit:
///
/// - per signed `from`: `LEGACY_SUBMIT_RATE_LIMIT_MAX_REQUESTS` per window,
///   the same per-sender budget the endpoint always meant to give, now keyed
///   on the account that signed rather than on the (possibly shared relay)
///   bearer;
/// - per bearer token, across all signers:
///   [`legacy_submit_token_max_requests`], so a token holder cannot admit
///   without bound by minting fresh signers.
///
/// Keys carry a `\u{1}` prefix: a bearer token comes from a header value,
/// which can never contain a control byte, so these keys cannot collide with
/// the bearer-keyed failed-authorization bucket.
pub async fn enforce_legacy_authenticated_submit_rate_limit(
    req: &HttpRequest,
    state: &AppState,
    from: &str,
) -> Result<(), HttpResponse> {
    let token = bearer_token_from_request(req).unwrap_or("__missing__");
    // One bucket per ACCOUNT, whatever the spelling (`0x`, case): key on the
    // parsed address. The handler has already refused an unparseable `from`.
    let account = from
        .parse::<wrfcoin_native_chain::common::types::Address>()
        .map(|address| address.to_hex())
        .unwrap_or_else(|_| from.to_ascii_lowercase());
    charge_legacy_submit_buckets(
        req,
        state,
        vec![
            (
                format!("\u{1}token:{token}"), // secret-leak-ok: in-memory limiter key, never output
                legacy_submit_token_max_requests(),
                "Too many transaction submissions on this token. Please retry later.",
            ),
            (
                format!("\u{1}from:{account}"),
                LEGACY_SUBMIT_RATE_LIMIT_MAX_REQUESTS,
                "Too many transaction submissions. Please retry later.",
            ),
        ],
    )
    .await
}

/// Charge `(key, max_requests, message)` buckets under one lock, all or
/// nothing: every bucket is checked first, and the request is recorded into
/// all of them only when every one admits. The first bucket that refuses
/// answers 429 and NOTHING is recorded, so a signer that is over its own
/// per-account budget never spends the shared per-token ceiling that other
/// signers on the same relay token depend on (#3337 review).
async fn charge_legacy_submit_buckets(
    req: &HttpRequest,
    state: &AppState,
    buckets: Vec<(String, usize, &'static str)>,
) -> Result<(), HttpResponse> {
    let now = chrono::Utc::now().timestamp_millis();
    let mut guard = state.legacy_submit_rate_limit.write().await;
    sweep_legacy_submit_rate_limit(req, &mut guard, now)?;
    for (key, max_requests, message) in &buckets {
        let counter = guard.entry(key.clone()).or_insert_with(|| {
            SlidingWindowCounter::new(LEGACY_SUBMIT_RATE_LIMIT_WINDOW_SECS * 1_000, *max_requests)
        });
        if !counter.has_room(now) {
            return Err(legacy_rate_limit_response(
                req,
                StatusCode::TOO_MANY_REQUESTS,
                "RATE_LIMITED",
                message,
            ));
        }
    }
    for (key, _, _) in &buckets {
        if let Some(counter) = guard.get_mut(key) {
            counter.check_and_record(now);
        }
    }
    Ok(())
}

fn legacy_rate_limit_response(
    req: &HttpRequest,
    status: StatusCode,
    code: &'static str,
    message: &str,
) -> HttpResponse {
    let mut response = envelope_error(req, status, code, message.to_string());
    response.headers_mut().insert(
        RETRY_AFTER,
        HeaderValue::from_str(&LEGACY_SUBMIT_RETRY_AFTER_SECS.to_string())
            .unwrap_or_else(|_| HeaderValue::from_static("60")),
    );
    response
}

/// Capacity check: if the map exceeds the hard cap, sweep all expired
/// entries; if it is still over, reject to bound memory.
fn sweep_legacy_submit_rate_limit(
    req: &HttpRequest,
    guard: &mut FxHashMap<String, SlidingWindowCounter>,
    now: i64,
) -> Result<(), HttpResponse> {
    if guard.len() > LEGACY_SUBMIT_RATE_LIMIT_MAX_KEYS {
        guard.retain(|_key, counter| !counter.is_empty_after_prune(now));
        if guard.len() > LEGACY_SUBMIT_RATE_LIMIT_MAX_KEYS {
            return Err(legacy_rate_limit_response(
                req,
                StatusCode::SERVICE_UNAVAILABLE,
                "RATE_LIMITER_CAPACITY",
                "Rate limiter capacity exceeded. Please retry later.",
            ));
        }
    }
    Ok(())
}

pub async fn enforce_permissionless_submit_rate_limit(
    req: &HttpRequest,
    state: &AppState,
    signer_fingerprint: &str,
) -> Result<(), HttpResponse> {
    let now = chrono::Utc::now().timestamp_millis();
    let mut guard = state.permissionless_submit_rate_limit.write().await;

    // Capacity check: if the map exceeds the hard cap, sweep all expired entries.
    //
    // This endpoint is the only fully-anonymous write path (no bearer token,
    // no session) — an attacker can mint a fresh signer fingerprint per
    // request by rotating keypairs, so without this cap the map grows without
    // bound and the node OOMs (#2077).
    if guard.len() > PERMISSIONLESS_SUBMIT_RATE_LIMIT_MAX_KEYS {
        guard.retain(|_key, counter| !counter.is_empty_after_prune(now));
        // If still over limit after full sweep, reject to bound memory.
        if guard.len() > PERMISSIONLESS_SUBMIT_RATE_LIMIT_MAX_KEYS {
            let mut response = envelope_error(
                req,
                StatusCode::SERVICE_UNAVAILABLE,
                "RATE_LIMITER_CAPACITY",
                "Permissionless rate limiter capacity exceeded. Please retry later.",
            );
            response.headers_mut().insert(
                RETRY_AFTER,
                HeaderValue::from_str(&LEGACY_SUBMIT_RETRY_AFTER_SECS.to_string())
                    .unwrap_or_else(|_| HeaderValue::from_static("60")),
            );
            return Err(response);
        }
    }

    let counter = guard
        .entry(signer_fingerprint.to_string())
        .or_insert_with(|| {
            SlidingWindowCounter::new(
                PERMISSIONLESS_SUBMIT_WINDOW_SECS * 1_000,
                PERMISSIONLESS_SUBMIT_MAX_REQUESTS,
            )
        });
    if !counter.check_and_record(now) {
        let mut response = envelope_error(
            req,
            StatusCode::TOO_MANY_REQUESTS,
            "RATE_LIMITED",
            "Permissionless submission throttle triggered by behavior/risk controls.",
        );
        response.headers_mut().insert(
            RETRY_AFTER,
            HeaderValue::from_str(&LEGACY_SUBMIT_RETRY_AFTER_SECS.to_string())
                .unwrap_or_else(|_| HeaderValue::from_static("60")),
        );
        return Err(response);
    }
    Ok(())
}

/// Resolve the per-contributor commit budget, honouring the
/// `CONTRIBUTION_COMMIT_RATE_LIMIT_PER_MIN` override (#1609). A value of `0`
/// or an unparseable string falls back to the compiled default so the endpoint
/// is never accidentally left unthrottled.
pub fn contribution_commit_max_requests() -> usize {
    std::env::var("CONTRIBUTION_COMMIT_RATE_LIMIT_PER_MIN")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(CONTRIBUTION_COMMIT_RATE_LIMIT_MAX_REQUESTS)
}

/// Per-contributor backpressure for `POST /transactions/contribution/commit`
/// (#1609).
///
/// Keyed by the contributor id so a single misconfigured collector or retry
/// storm cannot flood the commit endpoint, grow the mempool unboundedly, and
/// stall the block-production scheduler — while honest contributors keep their
/// own independent budgets. Mirrors [`enforce_legacy_submit_rate_limit`]:
/// sliding window, capacity sweep to bound memory, and a `429 Too Many
/// Requests` with a `Retry-After` header when the budget is exhausted.
pub async fn enforce_contribution_commit_rate_limit(
    req: &HttpRequest,
    state: &AppState,
    contributor: &str,
) -> Result<(), HttpResponse> {
    let now = chrono::Utc::now().timestamp_millis();
    let max_requests = contribution_commit_max_requests();
    let mut guard = state.contribution_commit_rate_limit.write().await;

    // Capacity check: if the map exceeds the hard cap, sweep expired entries.
    if guard.len() > CONTRIBUTION_COMMIT_RATE_LIMIT_MAX_KEYS {
        guard.retain(|_key, counter| !counter.is_empty_after_prune(now));
        // If still over limit after a full sweep, reject to bound memory.
        if guard.len() > CONTRIBUTION_COMMIT_RATE_LIMIT_MAX_KEYS {
            let mut response = envelope_error(
                req,
                StatusCode::SERVICE_UNAVAILABLE,
                "RATE_LIMITER_CAPACITY",
                "Contribution commit rate limiter capacity exceeded. Please retry later.",
            );
            response.headers_mut().insert(
                RETRY_AFTER,
                HeaderValue::from_str(&LEGACY_SUBMIT_RETRY_AFTER_SECS.to_string())
                    .unwrap_or_else(|_| HeaderValue::from_static("60")),
            );
            return Err(response);
        }
    }

    let counter = guard.entry(contributor.to_string()).or_insert_with(|| {
        SlidingWindowCounter::new(
            CONTRIBUTION_COMMIT_RATE_LIMIT_WINDOW_SECS * 1_000,
            max_requests,
        )
    });
    if !counter.check_and_record(now) {
        let mut response = envelope_error(
            req,
            StatusCode::TOO_MANY_REQUESTS,
            "RATE_LIMITED",
            "Too many contribution commits for this contributor. Please slow down.",
        );
        response.headers_mut().insert(
            RETRY_AFTER,
            HeaderValue::from_str(&LEGACY_SUBMIT_RETRY_AFTER_SECS.to_string())
                .unwrap_or_else(|_| HeaderValue::from_static("60")),
        );
        return Err(response);
    }
    Ok(())
}

/// Reserve `replay_key` for this request, or refuse it as a replay (409).
///
/// A reservation is atomic under the map's write lock, so two concurrent
/// submissions of the same key can never both proceed. Returns the
/// reservation's timestamp: a caller that reserves a key and then does NOT
/// admit the request (mempool refusal) must hand it back with
/// [`release_permissionless_replay_key`] and that timestamp, or the
/// signer's only valid next request is refused for the full TTL (#3337).
pub async fn enforce_permissionless_replay_protection(
    req: &HttpRequest,
    state: &AppState,
    replay_key: &str,
) -> Result<i64, HttpResponse> {
    let now = chrono::Utc::now().timestamp();
    let mut guard = state.permissionless_replay_seen.write().await;
    guard.retain(|_, seen_at| *seen_at > now - PERMISSIONLESS_REPLAY_TTL_SECS);
    if guard.contains_key(replay_key) {
        return Err(envelope_error(
            req,
            StatusCode::CONFLICT,
            "REPLAY_REJECTED",
            "Replay rejected: nonce has already been observed for this signer.",
        ));
    }
    guard.insert(replay_key.to_string(), now);
    persist_permissionless_replay(state, &guard).await;
    Ok(now)
}

/// Read-only replay check: refuse `replay_key` (409) if it was already
/// observed within the TTL, WITHOUT reserving it or writing storage. Lets a
/// caller refuse an authenticated replay before charging any budget (#1846)
/// while reserving — a durable write — only after its rate limit admits the
/// request, so a rate-limited retry loop costs no storage writes (#3337
/// review). The reservation itself stays atomic in
/// [`enforce_permissionless_replay_protection`].
pub async fn refuse_seen_permissionless_replay_key(
    req: &HttpRequest,
    state: &AppState,
    replay_key: &str,
) -> Result<(), HttpResponse> {
    let now = chrono::Utc::now().timestamp(); // determinism-ok: HTTP replay-window admission, never consensus state
    let guard = state.permissionless_replay_seen.read().await;
    match guard.get(replay_key) {
        Some(seen_at) if *seen_at > now - PERMISSIONLESS_REPLAY_TTL_SECS => Err(envelope_error(
            req,
            StatusCode::CONFLICT,
            "REPLAY_REJECTED",
            "Replay rejected: nonce has already been observed for this signer.",
        )),
        _ => Ok(()),
    }
}

/// Hand back a key reserved by [`enforce_permissionless_replay_protection`]
/// for a request that was then NOT admitted (#3337): a mempool-refused
/// transfer never entered the mempool, its nonce did not advance, and the
/// signer must be able to retry it.
///
/// Removes the entry only if it is still THIS reservation (`reserved_at`,
/// the timestamp the reservation returned). A handler that stalled past the
/// TTL may find its entry expired and the key re-reserved by a later,
/// legitimate submission; that reservation is left alone. The timestamp
/// identifies the reservation uniquely: a key cannot be reserved twice
/// while its entry is live, and an expired entry is at least a full TTL
/// older than any re-reservation.
pub async fn release_permissionless_replay_key(
    state: &AppState,
    replay_key: &str,
    reserved_at: i64,
) {
    let mut guard = state.permissionless_replay_seen.write().await;
    if guard.get(replay_key) == Some(&reserved_at) {
        guard.remove(replay_key);
        persist_permissionless_replay(state, &guard).await;
    }
}

/// Persist the replay map so replay protection survives node restart (#1845).
async fn persist_permissionless_replay(state: &AppState, guard: &FxHashMap<String, i64>) {
    let snapshot: std::collections::HashMap<String, i64> =
        guard.iter().map(|(k, v)| (k.clone(), *v)).collect(); // determinism-ok: collected into a map; order never observed (moved from #1845)
    let storage_guard = state.storage.read().await;
    if let Err(e) = storage_guard.save_permissionless_replay(&snapshot) {
        tracing::warn!("replay-seen persist failed (non-fatal): {e}");
    }
}

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

pub fn validate_participant_id(
    req: &HttpRequest,
    value: &str,
    field: &str,
) -> Result<(), HttpResponse> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(envelope_error(
            req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            format!("{field} is required"),
        ));
    }
    if trimmed.len() > MAX_PARTICIPANT_ID_LEN {
        return Err(envelope_error(
            req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            format!("{field} exceeds max length {MAX_PARTICIPANT_ID_LEN}"),
        ));
    }
    Ok(())
}

pub fn validate_station_id(
    req: &HttpRequest,
    value: &str,
    field: &str,
) -> Result<(), HttpResponse> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(envelope_error(
            req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            format!("{field} is required"),
        ));
    }
    if trimmed.len() > MAX_STATION_ID_LEN {
        return Err(envelope_error(
            req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            format!("{field} exceeds max length {MAX_STATION_ID_LEN}"),
        ));
    }
    Ok(())
}

pub fn validate_commit_hash(req: &HttpRequest, commit_hash: &str) -> Result<(), HttpResponse> {
    let normalized = commit_hash.trim();
    let valid = normalized.len() == 64 && normalized.chars().all(|c| c.is_ascii_hexdigit());
    if valid {
        Ok(())
    } else {
        Err(envelope_error(
            req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "commit_hash must be a 64-character hex string",
        ))
    }
}

pub fn validate_nonce(req: &HttpRequest, nonce: &str) -> Result<(), HttpResponse> {
    let trimmed = nonce.trim();
    if trimmed.is_empty() {
        return Err(envelope_error(
            req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "nonce is required",
        ));
    }
    if trimmed.len() > MAX_NONCE_LEN {
        return Err(envelope_error(
            req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            format!("nonce exceeds max length {MAX_NONCE_LEN}"),
        ));
    }
    Ok(())
}

pub fn validate_timestamp(
    req: &HttpRequest,
    timestamp: i64,
    field: &str,
) -> Result<(), HttpResponse> {
    if timestamp <= 0 {
        return Err(envelope_error(
            req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            format!("{field} must be a positive unix timestamp"),
        ));
    }
    let now = chrono::Utc::now().timestamp();
    let max_future_skew = 24 * 60 * 60; // 24h
    let max_past_skew = 365 * 24 * 60 * 60; // 365d
    if timestamp > now + max_future_skew || timestamp < now - max_past_skew {
        return Err(envelope_error(
            req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            format!("{field} must be within a sane launch window"),
        ));
    }
    Ok(())
}

pub fn parse_oracle_public_key(
    req: &HttpRequest,
    oracle_id: &str,
) -> Result<PublicKey, HttpResponse> {
    PublicKey::from_hex(oracle_id).map_err(|_| {
        envelope_error(
            req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "oracle_id must be an ed25519 public key hex string",
        )
    })
}

pub fn validate_nft_id(req: &HttpRequest, id: &str) -> Result<(), HttpResponse> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        return Err(envelope_error(
            req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "nft_id is required",
        ));
    }
    if trimmed.len() > MAX_NFT_ID_LEN {
        return Err(envelope_error(
            req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            format!("nft_id exceeds max length {MAX_NFT_ID_LEN}"),
        ));
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(envelope_error(
            req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "nft_id must be alphanumeric with hyphens/underscores",
        ));
    }
    Ok(())
}

pub fn parse_signing_role(raw: &str) -> Option<SigningRole> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "oracle" => Some(SigningRole::Oracle),
        "calibrator" => Some(SigningRole::Calibrator),
        "bridge_operator" | "bridge-operator" | "bridgeoperator" => {
            Some(SigningRole::BridgeOperator)
        },
        "governance_operator" | "governance-operator" | "governanceoperator" => {
            Some(SigningRole::GovernanceOperator)
        },
        "validator" => Some(SigningRole::Validator),
        _ => None,
    }
}

pub fn signing_role_label(role: SigningRole) -> &'static str {
    match role {
        SigningRole::Oracle => "oracle",
        SigningRole::Calibrator => "calibrator",
        SigningRole::BridgeOperator => "bridge_operator",
        SigningRole::GovernanceOperator => "governance_operator",
        SigningRole::Validator => "validator",
    }
}

// ---------------------------------------------------------------------------
// Transaction helpers
// ---------------------------------------------------------------------------

pub fn canonical_hash_hex(material: &str) -> String {
    use wrfcoin_native_chain::crypto::hash::hash_bytes;
    hash_bytes(material.as_bytes())
        .to_string()
        .trim_start_matches("0x")
        .to_string()
}

#[cfg(test)]
pub fn build_default_confidential_transfer_payload(
    from: &str,
    to: &str,
    amount: u64,
) -> wrfcoin_native_chain::blockchain::transaction::ConfidentialTransferPayload {
    use wrfcoin_native_chain::blockchain::transaction::{
        ConfidentialTransferPayload, PrivacyLevel,
    };

    let seed = format!(
        "privacy-default:{}:{}:{}:{}",
        from,
        to,
        amount,
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );
    let derive = |label: &str| canonical_hash_hex(&format!("{seed}:{label}"));

    ConfidentialTransferPayload {
        output_commitments: vec![derive("commitment:0"), derive("commitment:1")],
        range_proof: Some(derive("range-proof")),
        stealth_data: Some(derive("stealth-data")),
        ring_signature: Some(derive("ring-signature")),
        key_images: vec![derive("key-image:0")],
        encrypted_payload: Some(derive("encrypted-payload")),
        privacy_level: PrivacyLevel::High,
        selective_disclosures: vec![],
        signed_message: None,
    }
}

pub fn build_transparent_opt_out_payload(
    from: &str,
    to: &str,
    amount: u64,
    reason: Option<&str>,
    auth_pubkey: Option<String>,
    auth_signature: Option<String>,
) -> wrfcoin_native_chain::blockchain::transaction::TransparentTransferPayload {
    use wrfcoin_native_chain::blockchain::transaction::TransparentTransferPayload;

    let reason = reason
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("explicit_user_opt_out");
    let audit_tag = canonical_hash_hex(&format!(
        "transparent-opt-out:{from}:{to}:{amount}:{reason}"
    ));

    TransparentTransferPayload {
        from: from.to_string(),
        to: to.to_string(),
        amount,
        transparent_opt_out: true,
        opt_out_audit_tag: Some(audit_tag),
        // #1727: the `from`-bound authorization is supplied by the client; the
        // server never forges one.
        auth_pubkey,
        auth_signature,
        // #1727: governance/system-transfer provenance is never minted by this
        // user-facing opt-out helper.
        authority: None,
        authority_signature: None,
        passkey_auth: None,
        sponsorship: None,
        session_key_auth: None,
    }
}

pub async fn enqueue_protocol_transaction(
    state: web::Data<AppState>,
    tx_type: wrfcoin_native_chain::blockchain::transaction::TransactionType,
    sender_label: &str,
    payload: serde_json::Value,
) -> Result<TxAcceptedResponse, String> {
    enqueue_protocol_transaction_with_nonce(state, tx_type, sender_label, payload, 0).await
}

pub async fn enqueue_protocol_transaction_with_nonce(
    state: web::Data<AppState>,
    tx_type: wrfcoin_native_chain::blockchain::transaction::TransactionType,
    sender_label: &str,
    payload: serde_json::Value,
    nonce: u64,
) -> Result<TxAcceptedResponse, String> {
    enqueue_protocol_transaction_with_nonce_and_fee(state, tx_type, sender_label, payload, nonce, 0)
        .await
}

/// [`enqueue_protocol_transaction_with_nonce`] with a declared `tx.fee`
/// (the burned transfer fee, `transfer_fee_burn`). `fee = 0` builds exactly
/// the envelope the fee-less helper builds.
pub async fn enqueue_protocol_transaction_with_nonce_and_fee(
    state: web::Data<AppState>,
    tx_type: wrfcoin_native_chain::blockchain::transaction::TransactionType,
    sender_label: &str,
    payload: serde_json::Value,
    nonce: u64,
    fee: u64,
) -> Result<TxAcceptedResponse, String> {
    use wrfcoin_native_chain::blockchain::transaction::{
        Transaction, TransactionInput, TransactionOutput,
    };
    use wrfcoin_native_chain::crypto::hash::hash_bytes;

    let seed = format!(
        "{}:{}:{}",
        sender_label,
        tx_type,
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );
    let inputs = vec![TransactionInput {
        txid: hash_bytes(seed.as_bytes()),
        vout: 0,
        signature: None,
        public_key: Some(sender_label.to_string()),
        coinbase_data: None,
    }];

    let outputs = vec![TransactionOutput {
        address: sender_label.to_string(),
        amount: 0,
        script: "OP_TRUE".to_string(),
        nft_data: None,
    }];

    let mut tx = Transaction::new(inputs, outputs);
    tx.tx_type = tx_type;
    tx.nonce = nonce;
    tx.fee = fee;
    tx.data = Some(payload.to_string());
    if let Ok(addr) = sender_label.parse::<wrfcoin_native_chain::common::types::Address>() {
        tx.sender_address = addr;
    }
    tx.set_id();

    let signing_keys = if tx_type
        == wrfcoin_native_chain::blockchain::transaction::TransactionType::OracleAttestation
    {
        let blockchain = state.blockchain.read().await;
        Some(blockchain.state.signing_keys.clone())
    } else {
        None
    };

    {
        let canonical_height = state.blockchain.read().await.height;
        let mut mempool = state.mempool.write().await;
        mempool
            .add_transaction_with_signing_keys_at_height(tx.clone(), signing_keys.as_ref(), Some(canonical_height))
            .map_err(|err| {
                state
                    .failed_transactions_total
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                format!("Failed to add transaction: {err}")
            })?;
    }

    #[cfg(feature = "p2p")]
    let propagation = if let Some(ref p2p) = state.p2p {
        queue_transaction_gossip(
            &p2p.command_tx,
            bincode::serialize(&tx).map_err(|error| error.to_string()),
            &state.tx_gossip_backpressure_streak,
        )
    } else {
        GossipPropagation::disabled()
    };

    #[cfg(not(feature = "p2p"))]
    let propagation = GossipPropagation::disabled();

    let id = tx
        .id
        .as_ref()
        .map(|h| h.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    Ok(TxAcceptedResponse {
        id,
        tx_type: tx.tx_type.to_string(),
        local_acceptance: "accepted".to_string(),
        propagation: propagation.status.to_string(),
        propagation_error: propagation.error.map(str::to_string),
    })
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct GossipPropagation {
    status: &'static str,
    error: Option<&'static str>,
}

impl GossipPropagation {
    fn disabled() -> Self {
        Self {
            status: "disabled",
            error: None,
        }
    }
}

#[cfg(feature = "p2p")]
pub(crate) fn queue_transaction_gossip(
    command_tx: &tokio::sync::mpsc::Sender<wrfcoin_p2p::P2pCommand>,
    serialized: Result<Vec<u8>, String>,
    streak: &std::sync::Mutex<u64>,
) -> GossipPropagation {
    use tokio::sync::mpsc::error::TrySendError;
    use wrfcoin_shared_protocol::monitoring::CORE_METRICS;

    // Ordering the send outcome with the streak update prevents a delayed
    // failure from overwriting a concurrent successful reset.
    let mut consecutive = streak
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let failure = match serialized {
        Err(_) => Some("serialize"),
        Ok(bytes) => match command_tx.try_send(
            wrfcoin_p2p::P2pCommand::BroadcastTransactions(vec![bytes]),
        ) {
            Ok(()) => None,
            Err(TrySendError::Full(_)) => Some("full"),
            Err(TrySendError::Closed(_)) => Some("closed"),
        },
    };

    if let Some(reason) = failure {
        *consecutive = (*consecutive).saturating_add(1);
        let current = *consecutive;
        CORE_METRICS
            .tx_gossip_enqueue_failures_total
            .with_label_values(&[reason])
            .inc();
        CORE_METRICS
            .tx_gossip_backpressure_streak
            .set(current.min(i64::MAX as u64) as i64);
        log::warn!(
            "TX_GOSSIP_ENQUEUE_FAILED: reason={reason} consecutive={current}; transaction remains accepted in the local mempool"
        );
        GossipPropagation {
            status: "failed",
            error: Some(reason),
        }
    } else {
        *consecutive = 0;
        CORE_METRICS.tx_gossip_backpressure_streak.set(0);
        GossipPropagation {
            status: "queued",
            error: None,
        }
    }
}

#[cfg(feature = "p2p")]
pub(crate) fn reconcile_tx_gossip_backpressure(
    command_tx: &tokio::sync::mpsc::Sender<wrfcoin_p2p::P2pCommand>,
    streak: &std::sync::Mutex<u64>,
) -> u64 {
    use wrfcoin_shared_protocol::monitoring::CORE_METRICS;

    let mut consecutive = streak
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if *consecutive > 0 && !command_tx.is_closed() && command_tx.capacity() > 0 {
        log::info!(
            "TX_GOSSIP_BACKPRESSURE_RECOVERED: command channel has capacity; clearing consecutive failure streak"
        );
        *consecutive = 0;
        CORE_METRICS.tx_gossip_backpressure_streak.set(0);
    }
    *consecutive
}

#[cfg(feature = "p2p")]
pub(crate) fn readyz_tx_gossip_check(streak: u64) -> (serde_json::Value, bool) {
    const SUSTAINED_GOSSIP_FAILURES: u64 = 3;
    let blocks = streak >= SUSTAINED_GOSSIP_FAILURES;
    (
        serde_json::json!({
            "status": if blocks { "backpressured" } else { "ok" },
            "consecutive_enqueue_failures": streak,
            "failure_threshold": SUSTAINED_GOSSIP_FAILURES,
            "readiness_gate": blocks,
            "hint": if blocks {
                Some("Transaction propagation is persistently failing. Inspect TX_GOSSIP_ENQUEUE_FAILED logs and the P2P command consumer.")
            } else {
                None
            }
        }),
        blocks,
    )
}

pub async fn submit_protocol_transaction(
    req: &HttpRequest,
    state: web::Data<AppState>,
    tx_type: wrfcoin_native_chain::blockchain::transaction::TransactionType,
    sender_label: &str,
    payload: serde_json::Value,
) -> HttpResponse {
    match enqueue_protocol_transaction(state, tx_type, sender_label, payload).await {
        Ok(accepted) => HttpResponse::Created().json(accepted),
        Err(err) => envelope_error(req, StatusCode::BAD_REQUEST, "BAD_REQUEST", err),
    }
}

#[cfg(feature = "p2p")]
fn p2p_chain_state(
    block: &wrfcoin_native_chain::blockchain::block::Block,
    state_root: [u8; 32],
) -> (u64, [u8; 32], [u8; 32]) {
    let block_hash = block.hash.unwrap_or_else(|| block.calculate_hash());
    (block.height, block_hash.into(), state_root)
}

/// Update the P2P sync manager's local view without publishing a status
/// message. Historical blocks imported during IBD use this path: the sync
/// manager must observe every accepted height so it can plan the next range,
/// but broadcasting one `ChainStatus` per imported block can exceed an honest
/// peer's inbound rate limit. The P2P task publishes the latest local status
/// periodically (currently every 300 seconds below height 25,000), and the
/// sync-completion event adds one immediate final flush.
#[cfg(feature = "p2p")]
pub fn update_p2p_chain_state(
    p2p: &wrfcoin_p2p::P2pClient,
    block: &wrfcoin_native_chain::blockchain::block::Block,
    state_root: [u8; 32],
) {
    let (height, tip_hash, state_root) = p2p_chain_state(block, state_root);

    let _ = p2p
        .command_tx
        .try_send(wrfcoin_p2p::P2pCommand::UpdateLocalChainState {
            height,
            tip_hash,
            state_root,
        });
}

/// Publish the supplied chain tip without changing the sync manager's local
/// height. Used for the single end-of-IBD flush after all synced blocks have
/// already sent their per-block `UpdateLocalChainState` commands.
#[cfg(feature = "p2p")]
pub fn broadcast_p2p_chain_status(
    p2p: &wrfcoin_p2p::P2pClient,
    block: &wrfcoin_native_chain::blockchain::block::Block,
    state_root: [u8; 32],
) {
    let (height, tip_hash, state_root) = p2p_chain_state(block, state_root);
    let _ = p2p
        .command_tx
        .try_send(wrfcoin_p2p::P2pCommand::BroadcastChainStatus {
            height,
            tip_hash,
            state_root,
        });
}

#[cfg(feature = "p2p")]
pub fn notify_p2p_chain_advanced(
    p2p: &wrfcoin_p2p::P2pClient,
    block: &wrfcoin_native_chain::blockchain::block::Block,
    broadcast_block: bool,
    state_root: [u8; 32],
) {
    if broadcast_block {
        if let Ok(block_bytes) = bincode::serialize(block) {
            let _ = p2p
                .command_tx
                .try_send(wrfcoin_p2p::P2pCommand::BroadcastBlock(block_bytes));
        }
    }
    update_p2p_chain_state(p2p, block, state_root);
    broadcast_p2p_chain_status(p2p, block, state_root);
}

// ---------------------------------------------------------------------------
// Pre-flight diagnostics
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Clone)]
pub struct CheckResult {
    pub name: &'static str,
    pub status: CheckStatus,
    pub message: String,
}

#[derive(Debug)]
pub struct PreflightReport {
    pub checks: Vec<CheckResult>,
}

impl PreflightReport {
    pub fn has_failures(&self) -> bool {
        self.checks.iter().any(|c| c.status == CheckStatus::Fail)
    }
}

#[derive(Debug, Clone)]
pub struct PreflightConfig {
    pub api_host: String,
    pub api_port: u16,
    pub data_dir: String,
    pub network_id: String,
    pub chain_id: String,
    pub p2p_enabled: bool,
    /// The P2P TCP listen host, read exactly as startup reads it
    /// (`WRFCOIN_P2P_HOST`, default `0.0.0.0`). The preflight port check
    /// binds THIS host: binding `0.0.0.0` for a node configured on
    /// `127.0.0.1` made Windows raise a firewall prompt for a sync-only node.
    pub p2p_host: String,
    pub p2p_port: u16,
    pub p2p_quic_enabled: bool,
    pub p2p_quic_port: u16,
    pub p2p_bootstrap: Option<String>,
    pub operator_token_set: bool,
}

impl PreflightConfig {
    pub fn from_env() -> Self {
        let p2p_port = std::env::var("WRFCOIN_P2P_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8555);
        Self {
            api_host: std::env::var("WRFCOIN_API_HOST").unwrap_or_else(|_| "127.0.0.1".to_string()),
            api_port: std::env::var("WRFCOIN_API_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(8556),
            data_dir: std::env::var("WRFCOIN_DATA_DIR").unwrap_or_else(|_| "./data".to_string()),
            network_id: std::env::var("WRFCOIN_NETWORK_ID")
                .unwrap_or_else(|_| "unknown".to_string()),
            chain_id: std::env::var("WRFCOIN_CHAIN_ID").unwrap_or_else(|_| "777".to_string()),
            p2p_enabled: std::env::var("WRFCOIN_P2P_ENABLED")
                .unwrap_or_default()
                .eq_ignore_ascii_case("true"),
            // determinism-ok: startup-only preflight probe of the transport bind address; never enters state
            p2p_host: std::env::var("WRFCOIN_P2P_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            p2p_port,
            p2p_quic_enabled: std::env::var("WRFCOIN_P2P_QUIC_ENABLED")
                .unwrap_or_default()
                .eq_ignore_ascii_case("true"),
            p2p_quic_port: std::env::var("WRFCOIN_P2P_QUIC_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(p2p_port),
            p2p_bootstrap: std::env::var("WRFCOIN_P2P_BOOTSTRAP")
                .ok()
                .filter(|s| !s.is_empty()),
            operator_token_set: std::env::var("WRFCOIN_OPERATOR_TOKEN")
                .ok()
                .filter(|s| !s.is_empty())
                .is_some(),
        }
    }
}

pub fn print_startup_banner(config: &PreflightConfig) {
    let version = env!("CARGO_PKG_VERSION");
    println!();
    println!("======================================");
    println!("  WRFCoin Node v{}", version);
    println!(
        "  Network: {} (chain_id={})",
        config.network_id, config.chain_id
    );
    println!("  API:     {}:{}", config.api_host, config.api_port);
    println!("  Data:    {}", config.data_dir);
    if config.p2p_enabled {
        if config.p2p_quic_enabled {
            println!(
                "  P2P:     tcp/{} + udp/{}/quic-v1",
                config.p2p_port, config.p2p_quic_port
            );
        } else {
            println!("  P2P:     tcp/{}", config.p2p_port);
        }
    }
    println!("======================================");
}

/// Print a supplementary banner after P2P networking starts, showing the
/// local peer ID and a ready-to-share multiaddress.
#[cfg(feature = "p2p")]
pub fn print_p2p_ready(peer_id: &str, p2p_port: u16, quic_port: Option<u16>) {
    println!();
    println!("  P2P Ready:");
    println!("    Peer ID:    {peer_id}");
    println!("    TCP addr:   /ip4/<YOUR_IP>/tcp/{p2p_port}/p2p/{peer_id}");
    if let Some(quic_port) = quic_port {
        println!("    QUIC addr:  /ip4/<YOUR_IP>/udp/{quic_port}/quic-v1/p2p/{peer_id}");
    }
    println!("======================================");
}

/// Write the peer ID to `<data_dir>/peer_id.txt` for easy operator access.
#[cfg(feature = "p2p")]
pub fn write_peer_id_file(data_dir: &str, peer_id: &str) {
    let path = std::path::Path::new(data_dir).join("peer_id.txt");
    if let Err(e) = std::fs::write(&path, peer_id) {
        log::warn!("Failed to write peer_id.txt to {}: {e}", path.display());
    } else {
        log::info!("Peer ID written to {}", path.display());
    }
}

/// Post-startup readiness summary printed after the HTTP server binds.
pub fn print_post_startup_summary(
    host: &str,
    port: u16,
    chain_height: u64,
    p2p_info: Option<(&str, u16, Option<u16>, usize)>, // (peer_id, tcp_port, quic_port, peers)
) {
    let api_url = format!("http://{host}:{port}");
    println!();
    println!("======================================");
    println!("  Node is ready!");
    println!();
    println!("  API:        {api_url}");
    println!("  Health:     {api_url}/readyz");
    if chain_height == 0 {
        println!("  Chain:      height=0, genesis block");
    } else {
        println!("  Chain:      height={chain_height}");
    }
    if let Some((peer_id, p2p_port, quic_port, peers)) = p2p_info {
        if let Some(quic_port) = quic_port {
            println!("  P2P:        tcp/{p2p_port} + udp/{quic_port}/quic-v1 ({peers} peers)");
        } else {
            println!("  P2P:        listening on tcp/{p2p_port} ({peers} peers)");
        }
        println!("  Peer ID:    {peer_id}");
    } else {
        println!("  P2P:        disabled");
    }
    println!();
    println!("  Quick checks:");
    println!("    curl {api_url}/readyz");
    println!("    curl {api_url}/chain/info");
    println!("======================================");
}

pub fn print_preflight_report(report: &PreflightReport) {
    println!("  Pre-flight checks:");
    for check in &report.checks {
        let tag = match check.status {
            CheckStatus::Ok => "[OK]  ",
            CheckStatus::Warn => "[WARN]",
            CheckStatus::Fail => "[FAIL]",
        };
        println!("    {} {:<18} {}", tag, check.name, check.message);
    }
    if report.has_failures() {
        println!("======================================");
    } else {
        println!("  All checks passed. Starting node...");
        println!("======================================");
    }
}

pub fn run_preflight_checks(config: &PreflightConfig) -> PreflightReport {
    let mut checks = Vec::new();

    // 1. API port
    checks.push(check_port_available(
        &config.api_host,
        config.api_port,
        "api_port",
    ));

    // 2. P2P port (if enabled)
    if config.p2p_enabled {
        checks.push(check_p2p_port(config));
    }

    // 3. Data directory
    checks.push(check_data_dir(&config.data_dir));

    // 4. Storage integrity — use file-level check (avoids importing Storage)
    checks.push(check_storage_file(&config.data_dir));

    // 5. NTP
    let ntp = check_ntp_sync();
    checks.push(if ntp.synchronized {
        CheckResult {
            name: "ntp",
            status: CheckStatus::Ok,
            message: format!("Synchronized (source: {})", ntp.source),
        }
    } else {
        CheckResult {
            name: "ntp",
            status: CheckStatus::Warn,
            message: format!("Not synchronized (source: {})", ntp.source),
        }
    });

    // 6. Bootstrap peers (if P2P enabled)
    if config.p2p_enabled {
        checks.push(check_bootstrap_peers(&config.p2p_bootstrap));
    }

    // 7. Operator token
    // NOTE: preflight runs independently of load_token_store(), so the Warn branch
    // below is reachable even when WRFCOIN_ALLOW_NO_AUTH is not set.  The actual
    // fail-fast exit happens later in load_token_store(); this check is purely
    // advisory.
    checks.push(if config.operator_token_set {
        CheckResult {
            name: "operator_token",
            status: CheckStatus::Ok,
            message: "WRFCOIN_OPERATOR_TOKEN set".to_string(),
        }
    } else {
        CheckResult {
            name: "operator_token",
            status: CheckStatus::Warn,
            message: "WRFCOIN_OPERATOR_TOKEN not set — auth fail-fast will trigger unless WRFCOIN_ALLOW_NO_AUTH=1".to_string(),
        }
    });

    PreflightReport { checks }
}

/// Bind-test the P2P TCP port on the host the node will actually listen on,
/// not on the `0.0.0.0` wildcard.
fn check_p2p_port(config: &PreflightConfig) -> CheckResult {
    check_port_available(&config.p2p_host, config.p2p_port, "p2p_port")
}

fn check_port_available(host: &str, port: u16, name: &'static str) -> CheckResult {
    match std::net::TcpListener::bind((host, port)) {
        Ok(_listener) => CheckResult {
            name,
            status: CheckStatus::Ok,
            message: format!("Port {} available on {}", port, host),
        },
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => CheckResult {
            name,
            status: CheckStatus::Fail,
            message: format!("Port {} already in use on {}", port, host),
        },
        Err(e) => CheckResult {
            name,
            status: CheckStatus::Warn,
            message: format!("Port {} check on {} inconclusive: {}", port, host, e),
        },
    }
}

fn check_data_dir(data_dir: &str) -> CheckResult {
    let path = std::path::Path::new(data_dir);
    if let Err(e) = std::fs::create_dir_all(path) {
        return CheckResult {
            name: "data_dir",
            status: CheckStatus::Fail,
            message: format!("Cannot create {}: {}", data_dir, e),
        };
    }
    let probe = path.join(".wrfcoin_preflight_probe");
    match std::fs::write(&probe, b"ok") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            CheckResult {
                name: "data_dir",
                status: CheckStatus::Ok,
                message: format!("{} exists and writable", data_dir),
            }
        },
        Err(e) => CheckResult {
            name: "data_dir",
            status: CheckStatus::Fail,
            message: format!("{} not writable: {}", data_dir, e),
        },
    }
}

fn check_storage_file(data_dir: &str) -> CheckResult {
    let db_path = std::path::Path::new(data_dir).join("wrfcoin.redb");
    if !db_path.exists() {
        return CheckResult {
            name: "storage",
            status: CheckStatus::Ok,
            message: "Fresh start (no existing database)".to_string(),
        };
    }
    // Verify the file is readable
    match std::fs::metadata(&db_path) {
        Ok(meta) => {
            let size_mb = meta.len() as f64 / (1024.0 * 1024.0);
            CheckResult {
                name: "storage",
                status: CheckStatus::Ok,
                message: format!("Database found ({:.1} MB)", size_mb),
            }
        },
        Err(e) => CheckResult {
            name: "storage",
            status: CheckStatus::Fail,
            message: format!("Cannot read database: {}", e),
        },
    }
}

/// Lightweight multiaddr format check (does not require libp2p).
/// Returns `true` if the string looks like a valid multiaddr
/// (starts with `/ip4/`, `/ip6/`, `/dns4/`, or `/dns6/`).
fn looks_like_multiaddr(s: &str) -> bool {
    s.starts_with("/ip4/")
        || s.starts_with("/ip6/")
        || s.starts_with("/dns4/")
        || s.starts_with("/dns6/")
}

fn check_bootstrap_peers(bootstrap: &Option<String>) -> CheckResult {
    match bootstrap {
        Some(peers) => {
            let entries: Vec<&str> = peers
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect();
            let total = entries.len();
            let valid = entries.iter().filter(|s| looks_like_multiaddr(s)).count();
            let invalid = total - valid;
            if invalid > 0 {
                CheckResult {
                    name: "bootstrap_peers",
                    status: CheckStatus::Warn,
                    message: format!("{total} configured, {valid} valid, {invalid} invalid"),
                }
            } else {
                CheckResult {
                    name: "bootstrap_peers",
                    status: CheckStatus::Ok,
                    message: format!("{total} bootstrap peer(s) configured"),
                }
            }
        },
        None => CheckResult {
            name: "bootstrap_peers",
            status: CheckStatus::Warn,
            message: "No bootstrap peers — node will run isolated".to_string(),
        },
    }
}

// ---------------------------------------------------------------------------
// System health checks
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct NtpCheckResult {
    pub synchronized: bool,
    pub source: &'static str,
}

/// Check whether the system clock is NTP-synchronized.
///
/// On Linux, tries `/run/systemd/timesync/synchronized` (instant file check),
/// then falls back to `timedatectl show --property=NTPSynchronized --value`.
/// On non-Linux platforms, returns a "skipped" result.
pub fn check_ntp_sync() -> NtpCheckResult {
    #[cfg(target_os = "linux")]
    {
        check_ntp_sync_linux()
    }
    #[cfg(not(target_os = "linux"))]
    {
        NtpCheckResult {
            synchronized: true,
            source: "skipped",
        }
    }
}

#[cfg(target_os = "linux")]
fn check_ntp_sync_linux() -> NtpCheckResult {
    // Fast path: systemd-timesyncd writes this file when synchronized.
    if std::path::Path::new("/run/systemd/timesync/synchronized").exists() {
        return NtpCheckResult {
            synchronized: true,
            source: "timesync-file",
        };
    }

    // Fallback: ask timedatectl (works with chrony, ntpd, etc.)
    match std::process::Command::new("timedatectl")
        .args(["show", "--property=NTPSynchronized", "--value"])
        .output()
    {
        Ok(output) if output.status.success() => {
            let value = String::from_utf8_lossy(&output.stdout);
            NtpCheckResult {
                synchronized: value.trim().eq_ignore_ascii_case("yes"),
                source: "timedatectl",
            }
        },
        _ => NtpCheckResult {
            synchronized: false,
            source: "unavailable",
        },
    }
}

// ---------------------------------------------------------------------------
// Shared response types used across multiple route modules
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct TxAcceptedResponse {
    pub id: String,
    pub tx_type: String,
    /// Local mempool result. HTTP 201 means this field is always `accepted`.
    pub local_acceptance: String,
    /// P2P propagation handoff: `queued`, `failed`, or `disabled`.
    pub propagation: String,
    /// Bounded failure reason: `serialize`, `full`, or `closed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub propagation_error: Option<String>,
}

#[cfg(test)]
mod preflight_tests {
    use super::*;

    #[cfg(feature = "p2p")]
    #[tokio::test]
    async fn transaction_gossip_reports_full_closed_serialization_and_recovery_3225() {
        use wrfcoin_p2p::P2pCommand;

        let streak = std::sync::Mutex::new(0);
        let (command_tx, mut command_rx) = tokio::sync::mpsc::channel(1);
        command_tx
            .try_send(P2pCommand::BroadcastTransactions(vec![vec![0]]))
            .expect("fixture fills the real bounded command channel");

        for expected_streak in 1..=3 {
            let full = queue_transaction_gossip(&command_tx, Ok(vec![1]), &streak);
            assert_eq!(
                full,
                GossipPropagation {
                    status: "failed",
                    error: Some("full"),
                }
            );
            assert_eq!(*streak.lock().unwrap(), expected_streak);
        }
        let (readiness, blocks) = readyz_tx_gossip_check(*streak.lock().unwrap());
        assert!(blocks, "sustained full-channel failures must make readiness red");
        assert_eq!(readiness["status"], "backpressured");

        command_rx.try_recv().expect("drain the saturated fixture channel");
        assert_eq!(
            reconcile_tx_gossip_backpressure(&command_tx, &streak),
            0,
            "readiness observation must recover after the channel drains, without requiring new traffic"
        );
        assert!(!readyz_tx_gossip_check(0).1);

        let recovered = queue_transaction_gossip(&command_tx, Ok(vec![2]), &streak);
        assert_eq!(recovered.status, "queued");

        command_rx.try_recv().expect("observe the recovered production command");
        drop(command_rx);
        let closed = queue_transaction_gossip(&command_tx, Ok(vec![3]), &streak);
        assert_eq!(closed.error, Some("closed"));

        let serialize = queue_transaction_gossip(
            &command_tx,
            Err("injected serialization failure".to_string()),
            &streak,
        );
        assert_eq!(serialize.error, Some("serialize"));

        let (recovery_tx, _recovery_rx) = tokio::sync::mpsc::channel(1);
        assert_eq!(
            queue_transaction_gossip(&recovery_tx, Ok(vec![4]), &streak).status,
            "queued"
        );
        assert_eq!(*streak.lock().unwrap(), 0);
    }

    // Was a second, independent `ENV_MUTEX`. It guarded `WRFCOIN_NETWORK_ID`
    // while the producer-auth tests guarded the same variable under a
    // different lock, so the two never excluded each other (#2398 review).
    // Both now serialize on the single process-env lock.
    use crate::helpers::test_env;

    #[test]
    fn preflight_config_defaults() {
        let _guard = test_env::lock();
        std::env::remove_var("WRFCOIN_API_HOST");
        std::env::remove_var("WRFCOIN_API_PORT");
        std::env::remove_var("WRFCOIN_DATA_DIR");
        std::env::remove_var("WRFCOIN_NETWORK_ID");
        std::env::remove_var("WRFCOIN_P2P_ENABLED");
        std::env::remove_var("WRFCOIN_P2P_HOST");
        std::env::remove_var("WRFCOIN_P2P_PORT");
        std::env::remove_var("WRFCOIN_P2P_QUIC_ENABLED");
        std::env::remove_var("WRFCOIN_P2P_QUIC_PORT");
        std::env::remove_var("WRFCOIN_P2P_BOOTSTRAP");
        std::env::remove_var("WRFCOIN_CHAIN_ID");
        std::env::remove_var("WRFCOIN_OPERATOR_TOKEN");

        let config = PreflightConfig::from_env();
        assert_eq!(config.api_host, "127.0.0.1");
        assert_eq!(config.api_port, 8556);
        assert_eq!(config.data_dir, "./data");
        assert_eq!(config.network_id, "unknown");
        assert_eq!(config.chain_id, "777");
        assert!(!config.p2p_enabled);
        assert_eq!(config.p2p_host, "0.0.0.0");
        assert_eq!(config.p2p_port, 8555);
        assert!(!config.p2p_quic_enabled);
        assert_eq!(config.p2p_quic_port, 8555);
        assert!(config.p2p_bootstrap.is_none());
        assert!(!config.operator_token_set);
    }

    #[test]
    fn preflight_config_quic_env_override_uses_tcp_port_fallback() {
        let _guard = test_env::lock();
        std::env::set_var("WRFCOIN_P2P_PORT", "9444");
        std::env::set_var("WRFCOIN_P2P_QUIC_ENABLED", "true");
        std::env::remove_var("WRFCOIN_P2P_QUIC_PORT");

        let config = PreflightConfig::from_env();

        assert!(config.p2p_quic_enabled);
        assert_eq!(config.p2p_port, 9444);
        assert_eq!(config.p2p_quic_port, 9444);

        std::env::remove_var("WRFCOIN_P2P_PORT");
        std::env::remove_var("WRFCOIN_P2P_QUIC_ENABLED");
    }

    fn loopback_p2p_config(port: u16) -> PreflightConfig {
        PreflightConfig {
            api_host: "127.0.0.1".into(),
            api_port: 0,
            data_dir: "./data".into(),
            network_id: "unknown".into(),
            chain_id: "777".into(),
            p2p_enabled: true,
            p2p_host: "127.0.0.1".into(),
            p2p_port: port,
            p2p_quic_enabled: false,
            p2p_quic_port: port,
            p2p_bootstrap: None,
            operator_token_set: false,
        }
    }

    /// The P2P preflight must bind-test the configured host. Binding 0.0.0.0
    /// for a node configured on 127.0.0.1 pops the Windows firewall prompt.
    #[test]
    fn p2p_port_check_binds_the_configured_host_not_the_wildcard() {
        let _guard = test_env::lock();
        std::env::set_var("WRFCOIN_P2P_HOST", "127.0.0.1");
        let from_env = PreflightConfig::from_env();
        std::env::remove_var("WRFCOIN_P2P_HOST");
        assert_eq!(from_env.p2p_host, "127.0.0.1");

        // Free port: the check passes and says which host it tested.
        let free = check_p2p_port(&loopback_p2p_config(0));
        assert_eq!(free.status, CheckStatus::Ok, "{}", free.message);
        assert!(free.message.contains("127.0.0.1"), "{}", free.message);
        assert!(!free.message.contains("0.0.0.0"), "{}", free.message);

        // Occupied on the configured host: the check sees it.
        let held = match std::net::TcpListener::bind(("127.0.0.1", 0)) {
            Ok(listener) => listener,
            Err(e) => panic!("bind an ephemeral loopback port: {e}"),
        };
        let port = match held.local_addr() {
            Ok(addr) => addr.port(),
            Err(e) => panic!("read the held port: {e}"),
        };
        let busy = check_p2p_port(&loopback_p2p_config(port));
        assert_eq!(busy.status, CheckStatus::Fail, "{}", busy.message);
        assert!(busy.message.contains("127.0.0.1"), "{}", busy.message);
    }

    #[test]
    fn report_has_failures_detects_fail() {
        let report = PreflightReport {
            checks: vec![
                CheckResult {
                    name: "a",
                    status: CheckStatus::Ok,
                    message: "ok".into(),
                },
                CheckResult {
                    name: "b",
                    status: CheckStatus::Fail,
                    message: "fail".into(),
                },
            ],
        };
        assert!(report.has_failures());
    }

    #[test]
    fn report_no_failures_with_ok_and_warn() {
        let report = PreflightReport {
            checks: vec![
                CheckResult {
                    name: "a",
                    status: CheckStatus::Ok,
                    message: "ok".into(),
                },
                CheckResult {
                    name: "b",
                    status: CheckStatus::Warn,
                    message: "warn".into(),
                },
            ],
        };
        assert!(!report.has_failures());
    }

    #[test]
    fn check_port_free() {
        let result = check_port_available("127.0.0.1", 0, "test");
        assert_eq!(result.status, CheckStatus::Ok);
    }

    #[test]
    fn check_port_occupied() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let result = check_port_available("127.0.0.1", port, "test");
        assert_eq!(result.status, CheckStatus::Fail);
        drop(listener);
    }

    #[test]
    fn check_data_dir_temp() {
        let dir = std::env::temp_dir().join("wrfcoin_preflight_test");
        let _ = std::fs::remove_dir_all(&dir);
        let result = check_data_dir(dir.to_str().unwrap());
        assert_eq!(result.status, CheckStatus::Ok);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_storage_file_missing() {
        let dir = std::env::temp_dir().join("wrfcoin_storage_test_missing");
        let _ = std::fs::create_dir_all(&dir);
        let result = check_storage_file(dir.to_str().unwrap());
        assert_eq!(result.status, CheckStatus::Ok);
        assert!(result.message.contains("Fresh start"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_bootstrap_peers_none() {
        let result = check_bootstrap_peers(&None);
        assert_eq!(result.status, CheckStatus::Warn);
    }

    #[test]
    fn check_bootstrap_peers_present() {
        let result = check_bootstrap_peers(&Some("/ip4/1.2.3.4/tcp/9000".to_string()));
        assert_eq!(result.status, CheckStatus::Ok);
    }

    #[test]
    fn check_bootstrap_peers_accepts_quic_multiaddrs() {
        let result = check_bootstrap_peers(&Some(
            "/ip4/1.2.3.4/udp/9000/quic-v1/p2p/12D3KooWQm4YvKk2p2g4hQ8f9n7s6r5t4u3v2w1x"
                .to_string(),
        ));
        assert_eq!(result.status, CheckStatus::Ok);
    }

    #[test]
    fn check_bootstrap_peers_mixed_valid_and_invalid() {
        let result = check_bootstrap_peers(&Some(
            "/ip4/1.2.3.4/tcp/9000,ip4/5.6.7.8/tcp/9000".to_string(),
        ));
        assert_eq!(result.status, CheckStatus::Warn);
        assert!(result.message.contains("1 valid"));
        assert!(result.message.contains("1 invalid"));
    }

    #[test]
    fn check_bootstrap_peers_all_invalid() {
        let result = check_bootstrap_peers(&Some("garbage,also-garbage".to_string()));
        assert_eq!(result.status, CheckStatus::Warn);
        assert!(result.message.contains("0 valid"));
    }

    #[test]
    fn looks_like_multiaddr_positive() {
        assert!(looks_like_multiaddr("/ip4/1.2.3.4/tcp/9000"));
        assert!(looks_like_multiaddr("/ip6/::1/tcp/9000"));
        assert!(looks_like_multiaddr("/dns4/node.example.com/tcp/9000"));
        assert!(looks_like_multiaddr(
            "/ip4/1.2.3.4/udp/9000/quic-v1/p2p/12D3KooWAbCdEf"
        ));
    }

    #[test]
    fn looks_like_multiaddr_negative() {
        assert!(!looks_like_multiaddr("ip4/1.2.3.4/tcp/9000"));
        assert!(!looks_like_multiaddr("1.2.3.4:9000"));
        assert!(!looks_like_multiaddr(""));
    }
}

/// Resolve this chain's genesis (height-1) block hash for chain identity.
///
/// Two sources, in order (#1714/#2797):
///  1. the lowest in-memory block, when it is the height-1 genesis anchor;
///  2. the lowest persisted block in the durable archive.
///
/// Lives here (not main.rs) so both the boot path and the `/network/info`
/// route can call it — the route modules are also compiled by the test
/// harness under a different crate root, where main.rs items do not exist.
///
/// Returns `None` only when neither source has a block (an empty,
/// never-initialized chain), in which case `/network/info` reports `"unknown"`.
pub(crate) fn resolve_genesis_hash(
    blockchain: &wrfcoin_native_chain::blockchain::chain::Blockchain,
    storage: &wrfcoin_native_chain::storage::Storage,
) -> Option<String> {
    let format_hash = |block: &wrfcoin_shared_protocol::blockchain::block::Block| {
        let hash = block.hash.unwrap_or_else(|| block.calculate_hash());
        format!("0x{}", hash.to_hex())
    };

    // 1. Genesis still in the in-memory window: the lowest in-memory block,
    //    confirmed to be the height-1 genesis anchor (not just any window edge).
    if let Some(block) = blockchain.blocks().values().next() {
        if block.height == 1
            && block.prev_block_hash == wrfcoin_shared_protocol::crypto::hash::Hash::default()
        {
            return Some(format_hash(block));
        }
    }

    // 2. Genesis evicted from the window: read the lowest persisted block from
    //    the durable archive (height 1 on this chain).
    resolve_genesis_from_archive(storage)
}

/// Archive half of [`resolve_genesis_hash`], callable off-guard on the
/// blocking pool with a cloned [`Storage`] handle (#2797 review).
///
/// The lowest persisted block is only reported as genesis when it actually IS
/// the height-1 anchor (`height == 1`, zero previous hash). A truncated or
/// malformed archive whose lowest block is some later height must yield
/// `None`/"unknown" — naming an arbitrary first-row hash `genesis_hash` would
/// convert a corrupt datadir into a plausible wrong chain identity.
pub(crate) fn resolve_genesis_from_archive(
    storage: &wrfcoin_native_chain::storage::Storage,
) -> Option<String> {
    match storage.load_lowest_block() {
        Ok(Some(block)) => {
            if block.height == 1
                && block.prev_block_hash == wrfcoin_shared_protocol::crypto::hash::Hash::default()
            {
                let hash = block.hash.unwrap_or_else(|| block.calculate_hash());
                Some(format!("0x{}", hash.to_hex()))
            } else {
                log::warn!(
                    "lowest persisted block (height {}) is not the height-1 genesis anchor; \
                     refusing to report it as genesis_hash",
                    block.height
                );
                None
            }
        },
        Ok(None) => None,
        Err(e) => {
            log::warn!("Failed to load genesis block from storage for genesis_hash: {e}");
            None
        },
    }
}
