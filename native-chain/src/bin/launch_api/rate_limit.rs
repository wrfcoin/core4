//! Global per-IP HTTP rate limiting middleware for the launch_api server.
//!
//! Applies a sliding-window token bucket per client IP address with separate
//! limits for read operations (GET, HEAD, OPTIONS) and write operations
//! (POST, PUT, DELETE, PATCH).  Authenticated requests (carrying a valid
//! Bearer token found in the shared `token_store`) receive 10x the base
//! limit.
//!
//! Configuration via environment variables:
//!   - `RATE_LIMIT_READ_PER_MIN`  — max read  requests per minute per IP (default: 100)
//!   - `RATE_LIMIT_WRITE_PER_MIN` — max write requests per minute per IP (default: 20)
//!
//! When a client exceeds its budget the middleware returns **429 Too Many
//! Requests** with a `Retry-After` header (in seconds).
//!
//! Implements the actix-web `Transform` / `Service` traits so it can be
//! applied via `.wrap()` on the `App`.

use actix_web::{
    HttpResponse,
    body::EitherBody,
    dev::{Service, ServiceRequest, ServiceResponse, Transform},
    http::{Method, StatusCode, header},
};
use rustc_hash::FxHashMap;
use std::collections::VecDeque;
use std::future::{Future, Ready, ready};
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use wrfcoin_native_chain::security::api_auth::AuthPrincipal;

// Import envelope types from helpers module
use super::helpers::{LawBookEnvelope, LawBookEnvelopeError, LawBookEnvelopeMeta};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Default per-IP read budget (GET/HEAD/OPTIONS) per minute.
const DEFAULT_READ_PER_MIN: u32 = 100;
/// Default per-IP write budget (POST/PUT/DELETE/PATCH) per minute.
const DEFAULT_WRITE_PER_MIN: u32 = 20;
/// Multiplier applied to authenticated callers.
const AUTH_MULTIPLIER: u32 = 10;
/// Window duration in milliseconds (1 minute).
const WINDOW_MS: i64 = 60_000;
/// Maximum number of tracked IPs before we evict the oldest entries.
const MAX_TRACKED_IPS: usize = 50_000;
/// Seconds to report in the Retry-After header.
const RETRY_AFTER_SECS: u32 = 60;
/// Env var for comma-separated list of trusted reverse-proxy IP addresses.
const TRUSTED_PROXY_ENV_VAR: &str = "WRFCOIN_TRUSTED_PROXY";

fn read_per_min_from_env() -> u32 {
    std::env::var("RATE_LIMIT_READ_PER_MIN")
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|&v| v > 0)
        .unwrap_or(DEFAULT_READ_PER_MIN)
}

fn write_per_min_from_env() -> u32 {
    std::env::var("RATE_LIMIT_WRITE_PER_MIN")
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|&v| v > 0)
        .unwrap_or(DEFAULT_WRITE_PER_MIN)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// Parse `WRFCOIN_TRUSTED_PROXY` (comma-separated IP addresses) into a vec.
/// Invalid entries are silently skipped; absent env var yields an empty vec.
pub(crate) fn trusted_proxies_from_env() -> Vec<IpAddr> {
    std::env::var(TRUSTED_PROXY_ENV_VAR)
        .unwrap_or_default()
        .split(',')
        .filter_map(|s| s.trim().parse::<IpAddr>().ok())
        .collect()
}

/// Derive the logical client IP for rate-limiting purposes.
///
/// When `peer_ip` matches a configured trusted proxy, the first syntactically
/// valid IP from `X-Forwarded-For` (or `X-Real-IP`) is used instead.
/// If the peer is not a trusted proxy, forwarding headers are ignored entirely —
/// this prevents IP spoofing from untrusted callers.
pub(crate) fn resolve_client_ip(
    peer_ip: Option<IpAddr>,
    xff: Option<&str>,
    x_real_ip: Option<&str>,
    trusted_proxies: &[IpAddr],
) -> String {
    if let Some(peer) = peer_ip {
        if trusted_proxies.contains(&peer) {
            if let Some(ip) = xff
                .and_then(|h| h.split(',').next())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .and_then(|s| s.parse::<IpAddr>().ok())
            {
                return ip.to_string();
            }
            if let Some(ip) = x_real_ip
                .map(str::trim)
                .and_then(|s| s.parse::<IpAddr>().ok())
            {
                return ip.to_string();
            }
        }
        return peer.to_string();
    }
    "unknown".to_string()
}

fn extract_client_ip(req: &ServiceRequest, trusted_proxies: &[IpAddr]) -> String {
    let peer_ip = req.peer_addr().map(|a| a.ip());
    let xff = req.headers().get("X-Forwarded-For").and_then(|v| v.to_str().ok());
    let xri = req.headers().get("X-Real-IP").and_then(|v| v.to_str().ok());
    resolve_client_ip(peer_ip, xff, xri, trusted_proxies)
}

// ---------------------------------------------------------------------------
// Per-IP bucket
// ---------------------------------------------------------------------------

/// Sliding-window counter that tracks timestamps within a rolling window.
#[derive(Debug, Clone)]
struct IpBucket {
    read_timestamps: VecDeque<i64>,
    write_timestamps: VecDeque<i64>,
}

impl IpBucket {
    fn new() -> Self {
        Self {
            read_timestamps: VecDeque::new(),
            write_timestamps: VecDeque::new(),
        }
    }

    /// Prune timestamps older than the window from a queue.
    fn prune(queue: &mut VecDeque<i64>, now: i64) {
        let cutoff = now - WINDOW_MS;
        while queue.front().map_or(false, |&ts| ts <= cutoff) {
            queue.pop_front();
        }
    }

    /// Check whether this IP can make a request of the given class.
    /// Returns `true` if allowed (and records the timestamp), `false` if over limit.
    fn check_and_record(&mut self, is_write: bool, limit: u32, now: i64) -> bool {
        let queue = if is_write {
            &mut self.write_timestamps
        } else {
            &mut self.read_timestamps
        };
        Self::prune(queue, now);
        if queue.len() >= limit as usize {
            return false;
        }
        queue.push_back(now);
        true
    }

    /// Returns `true` when both queues are empty after pruning.
    fn is_empty_after_prune(&mut self, now: i64) -> bool {
        Self::prune(&mut self.read_timestamps, now);
        Self::prune(&mut self.write_timestamps, now);
        self.read_timestamps.is_empty() && self.write_timestamps.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

/// Thread-safe rate limiter state shared across all worker threads.
#[derive(Clone)] // secret-leak-ok: middleware copies share token_store, buckets, and trusted-proxy list through Arc; no bearer-token table is duplicated
pub struct RateLimiterState {
    buckets: Arc<RwLock<FxHashMap<String, IpBucket>>>,
    read_limit: u32,
    write_limit: u32,
    token_store: Arc<FxHashMap<String, AuthPrincipal>>,
    /// Trusted reverse-proxy IPs — forwarding headers are only read from these peers.
    trusted_proxies: Arc<[IpAddr]>,
}

impl RateLimiterState {
    pub fn new(token_store: Arc<FxHashMap<String, AuthPrincipal>>) -> Self {
        let read_limit = read_per_min_from_env();
        let write_limit = write_per_min_from_env();
        let trusted_proxies: Arc<[IpAddr]> = trusted_proxies_from_env().into();
        let proxy_list = if trusted_proxies.is_empty() {
            "none".to_string()
        } else {
            trusted_proxies.iter().map(|ip| ip.to_string()).collect::<Vec<_>>().join(", ")
        };
        log::info!(
            "HTTP rate limiter initialised: {read_limit} read/min, {write_limit} write/min \
             (auth tokens get {AUTH_MULTIPLIER}x); trusted proxies: {proxy_list}"
        );
        Self {
            buckets: Arc::new(RwLock::new(FxHashMap::default())),
            read_limit,
            write_limit,
            token_store,
            trusted_proxies,
        }
    }

    /// Returns the effective limit for the given request class, boosted for
    /// authenticated callers.
    fn effective_limit(&self, is_write: bool, is_authenticated: bool) -> u32 {
        let base = if is_write {
            self.write_limit
        } else {
            self.read_limit
        };
        if is_authenticated {
            base.saturating_mul(AUTH_MULTIPLIER)
        } else {
            base
        }
    }

    /// Attempt to pass the rate limiter for `ip`.  Returns `Ok(())` if
    /// allowed, or `Err(retry_after_secs)` if the caller should be throttled.
    async fn check(&self, ip: &str, is_write: bool, is_authenticated: bool) -> Result<(), u32> {
        let limit = self.effective_limit(is_write, is_authenticated);
        let now = now_ms();

        let mut guard = self.buckets.write().await;

        // Capacity check — evict stale entries when we have too many tracked IPs.
        if guard.len() > MAX_TRACKED_IPS {
            guard.retain(|_key, bucket| !bucket.is_empty_after_prune(now));
        }

        let bucket = guard.entry(ip.to_string()).or_insert_with(IpBucket::new);
        if bucket.check_and_record(is_write, limit, now) {
            Ok(())
        } else {
            Err(RETRY_AFTER_SECS)
        }
    }
}

// ---------------------------------------------------------------------------
// Middleware factory (`Transform`)
// ---------------------------------------------------------------------------

/// Actix-web middleware that enforces per-IP rate limits.
///
/// ```ignore
/// use rate_limit::{RateLimiter, RateLimiterState};
///
/// let rl_state = RateLimiterState::new(token_store.clone());
/// App::new()
///     .wrap(RateLimiter::new(rl_state))
///     .configure(configure_launch_routes)
/// ```
pub struct RateLimiter {
    state: RateLimiterState,
}

impl RateLimiter {
    pub fn new(state: RateLimiterState) -> Self {
        Self { state }
    }
}

impl<S, B> Transform<S, ServiceRequest> for RateLimiter
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error> + 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = actix_web::Error;
    type Transform = RateLimiterMiddleware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(RateLimiterMiddleware {
            service: Arc::new(service),
            state: self.state.clone(),
        }))
    }
}

// ---------------------------------------------------------------------------
// Middleware service
// ---------------------------------------------------------------------------

pub struct RateLimiterMiddleware<S> {
    // Arc so the service can be shared across the async boundary (actix-web
    // requires Service to be Clone-able in some configurations, and we need
    // to move it into the returned future).
    service: Arc<S>,
    state: RateLimiterState,
}

impl<S, B> Service<ServiceRequest> for RateLimiterMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error> + 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = actix_web::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    fn poll_ready(
        &self,
        ctx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.service.poll_ready(ctx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let service = self.service.clone();
        let state = self.state.clone();

        Box::pin(async move {
            // --- Extract client IP (proxy-aware, #1830) ---
            let ip = extract_client_ip(&req, &state.trusted_proxies);

            // --- Determine request class ---
            let is_write = !matches!(
                *req.method(),
                Method::GET | Method::HEAD | Method::OPTIONS
            );

            // --- Check authentication ---
            let is_authenticated = {
                let token = req
                    .headers()
                    .get("Authorization")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|h| h.strip_prefix("Bearer "))
                    .filter(|t| !t.is_empty());
                match token {
                    Some(t) => state.token_store.contains_key(t),
                    None => false,
                }
            };

            // --- Rate limit check ---
            match state.check(&ip, is_write, is_authenticated).await {
                Ok(()) => {
                    // Allowed — forward to inner service.
                    let res = service.call(req).await?;
                    Ok(res.map_into_left_body())
                }
                Err(retry_after) => {
                    // Throttled — return 429 with standard envelope.
                    let request_id = if let Some(header) = req.headers().get("x-request-id") {
                        if let Ok(s) = header.to_str() {
                            let trimmed = s.trim();
                            if !trimmed.is_empty() {
                                trimmed.to_string()
                            } else {
                                let now_nanos = SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_nanos();
                                format!("req_{now_nanos:x}")
                            }
                        } else {
                            let now_nanos = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_nanos();
                            format!("req_{now_nanos:x}")
                        }
                    } else {
                        let now_nanos = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_nanos();
                        format!("req_{now_nanos:x}")
                    };

                    let meta = LawBookEnvelopeMeta {
                        request_id: request_id.clone(),
                        timestamp: chrono::Utc::now()
                            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                        api_version: "v1",
                    };

                    let response = HttpResponse::build(StatusCode::TOO_MANY_REQUESTS)
                        .insert_header((header::RETRY_AFTER, retry_after.to_string()))
                        .insert_header(("X-Request-Id", request_id))
                        .json(LawBookEnvelope::<serde_json::Value> {
                            ok: false,
                            data: None,
                            error: Some(LawBookEnvelopeError {
                                code: "TOO_MANY_REQUESTS".to_string(),
                                message: "Rate limit exceeded. Please slow down.".to_string(),
                                correlation_id: None,
                            }),
                            meta,
                        });
                    Ok(req.into_response(response).map_into_right_body())
                }
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ip_bucket_allows_within_limit() {
        let mut bucket = IpBucket::new();
        let now = now_ms();
        for _ in 0..5 {
            assert!(bucket.check_and_record(false, 5, now));
        }
    }

    #[test]
    fn ip_bucket_blocks_over_limit() {
        let mut bucket = IpBucket::new();
        let now = now_ms();
        for _ in 0..3 {
            assert!(bucket.check_and_record(false, 3, now));
        }
        assert!(!bucket.check_and_record(false, 3, now));
    }

    #[test]
    fn ip_bucket_separates_read_and_write() {
        let mut bucket = IpBucket::new();
        let now = now_ms();
        // Fill up write budget
        for _ in 0..2 {
            assert!(bucket.check_and_record(true, 2, now));
        }
        assert!(!bucket.check_and_record(true, 2, now));
        // Read budget is independent
        assert!(bucket.check_and_record(false, 5, now));
    }

    #[test]
    fn ip_bucket_resets_after_window() {
        let mut bucket = IpBucket::new();
        let now = 100_000i64;
        assert!(bucket.check_and_record(false, 1, now));
        assert!(!bucket.check_and_record(false, 1, now));
        // After the window expires, the slot should be available again.
        let future = now + WINDOW_MS + 1;
        assert!(bucket.check_and_record(false, 1, future));
    }

    fn make_state(read_limit: u32, write_limit: u32) -> RateLimiterState {
        RateLimiterState {
            buckets: Arc::new(RwLock::new(FxHashMap::default())),
            read_limit,
            write_limit,
            token_store: Arc::new(FxHashMap::default()),
            trusted_proxies: Arc::from(vec![]),
        }
    }

    #[test]
    fn effective_limit_applies_auth_multiplier() {
        let state = make_state(100, 20);
        assert_eq!(state.effective_limit(false, false), 100);
        assert_eq!(state.effective_limit(false, true), 1000);
        assert_eq!(state.effective_limit(true, false), 20);
        assert_eq!(state.effective_limit(true, true), 200);
    }

    #[tokio::test]
    async fn rate_limiter_state_allows_and_then_blocks() {
        let state = make_state(3, 1);
        // Three reads should be fine.
        for _ in 0..3 {
            assert!(state.check("1.2.3.4", false, false).await.is_ok());
        }
        // Fourth read should be blocked.
        assert!(state.check("1.2.3.4", false, false).await.is_err());
        // A different IP is unaffected.
        assert!(state.check("5.6.7.8", false, false).await.is_ok());
    }

    #[tokio::test]
    async fn write_backpressure_does_not_block_read_probe_budget() {
        let state = make_state(1, 2);

        // Exhaust write capacity for this IP.
        assert!(state.check("9.9.9.9", true, false).await.is_ok());
        assert!(state.check("9.9.9.9", true, false).await.is_ok());
        assert!(state.check("9.9.9.9", true, false).await.is_err());

        // Read probe remains independently available.
        assert!(state.check("9.9.9.9", false, false).await.is_ok());
        assert!(state.check("9.9.9.9", false, false).await.is_err());
    }

    #[tokio::test]
    async fn authenticated_callers_get_higher_limits() {
        let state = make_state(2, 1);
        // Unauthenticated: 2 allowed, 3rd blocked.
        assert!(state.check("1.1.1.1", false, false).await.is_ok());
        assert!(state.check("1.1.1.1", false, false).await.is_ok());
        assert!(state.check("1.1.1.1", false, false).await.is_err());
        // Authenticated: limit is 2 * 10 = 20, so the 3rd should be fine.
        for _ in 0..20 {
            assert!(state.check("2.2.2.2", false, true).await.is_ok());
        }
        assert!(state.check("2.2.2.2", false, true).await.is_err());
    }

    // --- resolve_client_ip unit tests ---

    #[test]
    fn resolve_ip_no_proxies_uses_peer() {
        let peer: IpAddr = "1.2.3.4".parse().unwrap();
        let result = resolve_client_ip(Some(peer), Some("9.9.9.9"), Some("8.8.8.8"), &[]);
        assert_eq!(result, "1.2.3.4");
    }

    #[test]
    fn resolve_ip_trusted_proxy_prefers_xff_first_entry() {
        let proxy: IpAddr = "10.0.0.1".parse().unwrap();
        let trusted = vec![proxy];
        let result =
            resolve_client_ip(Some(proxy), Some("203.0.113.5, 10.0.0.1"), None, &trusted);
        assert_eq!(result, "203.0.113.5");
    }

    #[test]
    fn resolve_ip_trusted_proxy_falls_back_to_x_real_ip() {
        let proxy: IpAddr = "10.0.0.1".parse().unwrap();
        let trusted = vec![proxy];
        let result = resolve_client_ip(Some(proxy), None, Some("198.51.100.7"), &trusted);
        assert_eq!(result, "198.51.100.7");
    }

    #[test]
    fn resolve_ip_untrusted_peer_ignores_forwarding_headers() {
        let peer: IpAddr = "1.2.3.4".parse().unwrap();
        let proxy: IpAddr = "10.0.0.1".parse().unwrap();
        let trusted = vec![proxy];
        // peer is NOT in trusted list — spoofed headers must be ignored.
        let result =
            resolve_client_ip(Some(peer), Some("9.9.9.9"), Some("8.8.8.8"), &trusted);
        assert_eq!(result, "1.2.3.4");
    }

    #[test]
    fn resolve_ip_invalid_xff_falls_back_to_x_real_ip() {
        let proxy: IpAddr = "10.0.0.1".parse().unwrap();
        let trusted = vec![proxy];
        let result = resolve_client_ip(
            Some(proxy),
            Some("not-an-ip"),
            Some("203.0.113.1"),
            &trusted,
        );
        assert_eq!(result, "203.0.113.1");
    }

    #[test]
    fn resolve_ip_no_peer_addr_returns_unknown() {
        let result = resolve_client_ip(None, Some("9.9.9.9"), None, &[]);
        assert_eq!(result, "unknown");
    }
}
