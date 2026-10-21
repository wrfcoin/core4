//! #2136 — DNS-rebinding, `Host` and `Origin` validation.
//!
//! Every launch_api route is registered inside a single scope wrapped by
//! [`OriginGuard`], so a request whose `Host` does not name this node, or whose
//! `Origin` is outside the allowlist, is refused before any handler runs. The rule
//! itself is `api_auth::check_origin` (pure, unit-tested in shared-protocol); this
//! module only reads the headers, the environment and the surface document.
//!
//! * **Host allowlist** — `localhost`, `127.0.0.1`, `[::1]` plus
//!   `WRFCOIN_API_ALLOWED_HOSTS` (comma-separated names; port ignored). A page that
//!   rebinds its own name to this node's address sends *its* name in `Host`, which
//!   is not here, so the request is refused even though it reached the socket.
//!   `X-Forwarded-Host` is deliberately ignored: it is what a proxy claims, not
//!   what the browser sent.
//! * **Origin allowlist** — `LAUNCH_API_CORS_ORIGINS`, the same list CORS is built
//!   from (`build_cors_from_env`): the two local dev origins when it is unset
//!   outside production, nothing when it is unset in production. A disallowed
//!   `Origin` is refused here rather than by withholding a CORS header, since the
//!   latter only stops the browser reading the response, not the request running.
//! * **No Origin** — a browser-originated request (any `Sec-Fetch-*` header, which
//!   a page can neither forge nor remove) to an `admin` method must carry an
//!   allowlisted `Origin` — unless the browser asserts `Sec-Fetch-Site:
//!   same-origin`, i.e. the page was served by this node (`/dashboard`, #3599)
//!   and the Host check has passed; a same-origin GET never carries `Origin`, so
//!   it is judged like a non-browser client. Other requests without one proceed while
//!   `WRFCOIN_API_ALLOW_NO_ORIGIN` is `1` (default): the CLI, curl and
//!   node-to-node calls never send one, and requiring it of them would prove
//!   nothing — a non-browser client sets any header it likes.
//!
//! Wildcards are unrepresentable (`OriginPolicy::build` refuses `*`), so a
//! permissive policy cannot be configured; a malformed one stops the node.

use std::future::{Future, Ready, ready};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use actix_web::{
    HttpRequest, HttpResponse,
    body::EitherBody,
    dev::{Service, ServiceRequest, ServiceResponse, Transform},
    http::{StatusCode, header},
};
use wrfcoin_native_chain::security::api_auth::{
    OriginError, OriginPolicy, OriginPolicyError, RequestOrigin, check_origin,
};

use crate::helpers::envelope_error;

/// Extra host names this node may be addressed by (comma-separated, port ignored).
pub const ALLOWED_HOSTS_ENV: &str = "WRFCOIN_API_ALLOWED_HOSTS";
/// `0` refuses every request that carries no `Origin`; default `1`.
pub const ALLOW_NO_ORIGIN_ENV: &str = "WRFCOIN_API_ALLOW_NO_ORIGIN";
/// The browser-origin allowlist, shared with CORS.
pub const CORS_ORIGINS_ENV: &str = "LAUNCH_API_CORS_ORIGINS";
/// What CORS allows when `LAUNCH_API_CORS_ORIGINS` is unset outside production.
const DEV_ORIGINS: &[&str] = &["http://localhost:3000", "http://localhost:5173"];

/// The surface document: the `admin` rows decide which methods a browser may
/// only reach with an allowlisted `Origin`. Parsed here rather than shared so
/// this story depends on nothing unmerged.
const SURFACE_DOCUMENT: &str = include_str!("../../../../../docs/security/RPC_SURFACE.md");

/// Build the policy the way `build_cors_from_env` builds CORS, from the same
/// inputs, so the two can never disagree about which origins are allowed.
pub fn policy_from_values(
    allowed_hosts: Option<&str>,
    cors_origins: Option<&str>,
    is_production: bool,
    allow_no_origin: Option<&str>,
) -> Result<OriginPolicy, OriginPolicyError> {
    let hosts: Vec<&str> = allowed_hosts
        .map(|v| v.split(',').map(str::trim).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    let origins: Vec<&str> = match (cors_origins, is_production) {
        (Some(val), _) => val.split(',').map(str::trim).filter(|s| !s.is_empty()).collect(),
        (None, true) => Vec::new(),
        (None, false) => DEV_ORIGINS.to_vec(),
    };
    let allow_no_origin = allow_no_origin
        .map(|v| !matches!(v.trim(), "0" | "false" | "no"))
        .unwrap_or(true);
    OriginPolicy::build(hosts, origins, allow_no_origin)
}

pub fn policy_from_env() -> Result<OriginPolicy, OriginPolicyError> {
    // determinism-ok: startup-only HTTP admission config (#2136); Host/Origin allowlists gate which requests reach a handler and never enter block validity, a signing preimage or the state root
    let hosts = std::env::var(ALLOWED_HOSTS_ENV).ok();
    // determinism-ok: startup-only HTTP admission config (#2136); same policy input as the line above, never a consensus input
    let origins = std::env::var(CORS_ORIGINS_ENV).ok();
    // determinism-ok: startup-only HTTP admission config (#2136); selects the production default for the no-Origin rule, never a consensus input
    let is_production = std::env::var("NODE_ENV")
        .map(|v| v.eq_ignore_ascii_case("production"))
        .unwrap_or(false);
    // determinism-ok: startup-only HTTP admission config (#2136); the no-Origin admission switch, never a consensus input
    let allow_no_origin = std::env::var(ALLOW_NO_ORIGIN_ENV).ok();
    policy_from_values(hosts.as_deref(), origins.as_deref(), is_production, allow_no_origin.as_deref())
}

/// The `admin`-classified methods of the surface document: verb plus path
/// segments, `{param}` matching any one segment.
#[derive(Debug, Clone, Default)]
pub struct AdminMethodSet {
    entries: Vec<(String, Vec<String>)>,
}

impl AdminMethodSet {
    pub fn from_markdown(doc: &str) -> Self {
        let mut entries = Vec::new();
        for line in doc.lines() {
            if !line.starts_with("| `") {
                continue;
            }
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            // Inventory rows have seven columns (nine cells with the empty ends).
            if cells.len() < 9 || cells[4].trim_matches('`') != "admin" {
                continue;
            }
            let method = cells[1].trim_matches('`');
            let Some((verb, path)) = method.split_once(' ') else { continue };
            let segments: Vec<String> = path.split('/').filter(|s| !s.is_empty()).map(str::to_string).collect();
            entries.push((verb.to_ascii_uppercase(), segments));
        }
        Self { entries }
    }

    pub fn from_surface_document() -> Self {
        Self::from_markdown(SURFACE_DOCUMENT)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn contains(&self, verb: &str, path: &str) -> bool {
        let verb = verb.to_ascii_uppercase();
        let path = path.split('?').next().unwrap_or(path);
        let actual: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        self.entries.iter().any(|(v, pattern)| {
            *v == verb
                && pattern.len() == actual.len()
                && pattern.iter().zip(&actual).all(|(p, a)| {
                    (p.starts_with('{') && p.ends_with('}') && !a.is_empty()) || p == a
                })
        })
    }
}

struct Inner {
    policy: OriginPolicy,
    admin: AdminMethodSet,
}

/// The middleware. Cheap to clone; one per worker.
#[derive(Clone)]
pub struct OriginGuard {
    inner: Arc<Inner>,
}

static LOGGED: AtomicBool = AtomicBool::new(false);

impl OriginGuard {
    /// Production constructor. A policy that cannot be built (a wildcard, a
    /// malformed origin) panics here — in the server factory — so the node does
    /// not come up permissive.
    pub fn from_env() -> Self {
        let policy = policy_from_env()
            .unwrap_or_else(|e| panic!("refusing to start with an invalid Host/Origin policy (#2136): {e}"));
        let guard = Self::with_policy(policy);
        if !LOGGED.swap(true, Ordering::Relaxed) {
            let p = &guard.inner.policy;
            log::info!(
                "Host/Origin policy (#2136): hosts={:?} origins={:?} allow_no_origin={} admin_methods={}",
                p.allowed_hosts,
                p.allowed_origins,
                p.allow_no_origin,
                guard.inner.admin.len()
            );
        }
        guard
    }

    pub fn with_policy(policy: OriginPolicy) -> Self {
        Self::with_policy_and_admin(policy, AdminMethodSet::from_surface_document())
    }

    pub fn with_policy_and_admin(policy: OriginPolicy, admin: AdminMethodSet) -> Self {
        Self {
            inner: Arc::new(Inner { policy, admin }),
        }
    }

    pub fn policy(&self) -> &OriginPolicy {
        &self.inner.policy
    }
}

impl<S, B> Transform<S, ServiceRequest> for OriginGuard
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error> + 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = actix_web::Error;
    type Transform = OriginMiddleware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(OriginMiddleware {
            service: Arc::new(service),
            inner: self.inner.clone(),
        }))
    }
}

pub struct OriginMiddleware<S> {
    service: Arc<S>,
    inner: Arc<Inner>,
}

fn refusal(request: &HttpRequest, error: &OriginError) -> HttpResponse {
    let message = match error {
        OriginError::MissingHost => "Request carries no Host header".to_string(),
        OriginError::HostNotAllowed(host) => {
            format!("Host {host:?} does not name this node; allowlist it with {ALLOWED_HOSTS_ENV} if it should")
        }
        OriginError::OriginNotAllowed(origin) => {
            format!("Origin {origin:?} is not allowlisted for this node ({CORS_ORIGINS_ENV})")
        }
        OriginError::OriginRequired => "This request must carry an allowlisted Origin".to_string(),
    };
    envelope_error(request, StatusCode::FORBIDDEN, error.code(), message)
}

impl<S, B> Service<ServiceRequest> for OriginMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error> + 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = actix_web::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    fn poll_ready(&self, ctx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), Self::Error>> {
        self.service.poll_ready(ctx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let service = self.service.clone();
        let inner = self.inner.clone();

        Box::pin(async move {
            let verdict = {
                let headers = req.headers();
                // The raw Host header — never `connection_info().host()`, which
                // honours X-Forwarded-Host. A client that sent none (no browser
                // ever does) is judged by the server's own configured host.
                let host_header = headers.get(header::HOST).and_then(|v| v.to_str().ok());
                let configured_host = req.app_config().host();
                let host = host_header.or(Some(configured_host));
                let origin = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok());
                // `Sec-Fetch-Site: same-origin` is the browser's own statement (a
                // forbidden header name: a page can neither set nor strip it) that
                // the requesting page was served from this very origin — by this
                // node, once the Host allowlist inside `check_origin` has passed.
                // A same-origin GET never carries `Origin` (the Fetch standard adds
                // it only to cross-origin or non-GET requests), so without this the
                // node's own `/dashboard` (#3599) could never read an `admin`
                // method. Such a request is judged like a non-browser client:
                // admitted while `allow_no_origin` holds, the handler's bearer check
                // still ahead of it. DNS rebinding is unaffected: a rebound page's
                // `Host` names the attacker's site and fails the host allowlist
                // first.
                let same_origin_page = headers
                    .get("sec-fetch-site")
                    .and_then(|v| v.to_str().ok())
                    .is_some_and(|v| v.trim().eq_ignore_ascii_case("same-origin"));
                let browser_originated = !same_origin_page
                    && (origin.is_some()
                        || headers.iter().any(|(name, _)| name.as_str().starts_with("sec-fetch-")));
                let admin_method = inner.admin.contains(req.method().as_str(), req.path());
                check_origin(
                    &RequestOrigin {
                        host,
                        origin,
                        browser_originated,
                        admin_method,
                    },
                    &inner.policy,
                )
            };

            match verdict {
                Ok(()) => {
                    let res = service.call(req).await?;
                    Ok(res.map_into_left_body())
                }
                Err(error) => {
                    let response = refusal(req.request(), &error);
                    Ok(req.into_response(response).map_into_right_body())
                }
            }
        })
    }
}
