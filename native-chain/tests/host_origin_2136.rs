//! #2136 — DNS-rebinding, `Host` and `Origin` validation before any handler runs.
//!
//! The rule is `api_auth::check_origin` (pure, unit-tested in shared-protocol). These
//! tests prove the actix guard applies it from the real headers with the real
//! `admin` set of the surface document, that a refused request never reaches its
//! handler, that a wildcard cannot be configured, that CORS never reflects, and that
//! `configure_launch_routes` registers nothing outside the guarded scope.

#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/helpers.rs"]
mod helpers;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/routes/origin.rs"]
mod origin;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/state.rs"]
mod state;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/rate_limit.rs"]
mod rate_limit;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/websocket.rs"]
mod websocket;

include!("support/launch_api_test_support.inc.rs");

use std::sync::atomic::{AtomicUsize, Ordering};

use actix_web::{App, HttpResponse, http::StatusCode, test, web};
use origin::{AdminMethodSet, OriginGuard, policy_from_values};
use wrfcoin_native_chain::security::api_auth::{OriginPolicy, OriginPolicyError};

/// Dev defaults: loopback hosts, the two local dev origins, non-browser clients allowed.
fn dev_policy() -> OriginPolicy {
    policy_from_values(None, None, false, None).expect("dev defaults build")
}

/// One counter per app so parallel tests cannot see each other's handler hits.
fn guarded_app(
    policy: OriginPolicy,
    reached: Arc<AtomicUsize>,
) -> App<
    impl actix_web::dev::ServiceFactory<
        actix_web::dev::ServiceRequest,
        Config = (),
        Response = actix_web::dev::ServiceResponse<impl actix_web::body::MessageBody>,
        Error = actix_web::Error,
        InitError = (),
    >,
> {
    let admin_counter = reached.clone();
    let admin_get_counter = reached.clone();
    let public_counter = reached;
    App::new().app_data(test_state(None)).service(
        web::scope("")
            .wrap(OriginGuard::with_policy(policy))
            .route(
                "/blocks/mine",
                web::post().to(move || {
                    let c = admin_counter.clone();
                    async move {
                        c.fetch_add(1, Ordering::SeqCst);
                        HttpResponse::Ok().body("handler reached")
                    }
                }),
            )
            .route(
                "/health",
                web::get().to(move || {
                    let c = public_counter.clone();
                    async move {
                        c.fetch_add(1, Ordering::SeqCst);
                        HttpResponse::Ok().body("handler reached")
                    }
                }),
            )
            // An `admin` GET (surface class `admin`): what the node's own status
            // page reads with a bearer token (#3599).
            .route(
                "/node/metrics/host",
                web::get().to(move || {
                    let c = admin_get_counter.clone();
                    async move {
                        c.fetch_add(1, Ordering::SeqCst);
                        HttpResponse::Ok().body("handler reached")
                    }
                }),
            ),
    )
}

/// Issue `method path` with the given headers; returns (status, JSON body or raw text).
macro_rules! call {
    ($app:expr, $method:expr, $path:expr, $headers:expr) => {{
        let headers: &[(&str, &str)] = $headers;
        let mut req = match $method {
            "POST" => test::TestRequest::post(),
            _ => test::TestRequest::get(),
        }
        .uri($path);
        for (name, value) in headers {
            req = req.insert_header((*name, *value));
        }
        let resp = test::call_service(&$app, req.to_request()).await;
        let status = resp.status();
        let body = test::read_body(resp).await;
        let json = serde_json::from_slice::<serde_json::Value>(&body)
            .unwrap_or(serde_json::Value::String(String::from_utf8_lossy(&body).into_owned()));
        (status, json)
    }};
}

fn error_code(json: &serde_json::Value) -> &str {
    json["error"]["code"].as_str().unwrap_or("")
}

#[actix_web::test]
async fn rebinding_host_is_refused_before_the_handler_even_though_it_reached_the_socket() {
    let reached = Arc::new(AtomicUsize::new(0));
    let app = test::init_service(guarded_app(dev_policy(), reached.clone())).await;

    let (status, json) = call!(app, "GET", "/health", &[("Host", "attacker.example:8556")]);
    assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
    assert_eq!(error_code(&json), "HOST_NOT_ALLOWED");
    assert_eq!(reached.load(Ordering::SeqCst), 0, "handler must not run on a rebinding Host");

    for host in ["127.0.0.1:8556", "localhost", "LOCALHOST:8556", "[::1]:8556"] {
        let (status, json) = call!(app, "GET", "/health", &[("Host", host)]);
        assert_eq!(status, StatusCode::OK, "{host}: {json}");
    }
    // No Host header at all (no browser ever omits it): judged by the server's own host.
    let (status, _) = call!(app, "GET", "/health", &[]);
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reached.load(Ordering::SeqCst), 5);
}

#[actix_web::test]
async fn x_forwarded_host_cannot_launder_a_rebinding_host() {
    let reached = Arc::new(AtomicUsize::new(0));
    let app = test::init_service(guarded_app(dev_policy(), reached.clone())).await;
    let (status, json) = call!(
        app,
        "GET",
        "/health",
        &[("Host", "attacker.example"), ("X-Forwarded-Host", "localhost"), ("Forwarded", "host=localhost")]
    );
    assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
    assert_eq!(error_code(&json), "HOST_NOT_ALLOWED");
    assert_eq!(reached.load(Ordering::SeqCst), 0);
}

#[actix_web::test]
async fn disallowed_origin_is_refused_before_the_handler_executes() {
    let reached = Arc::new(AtomicUsize::new(0));
    let app = test::init_service(guarded_app(dev_policy(), reached.clone())).await;

    let (status, json) = call!(app, "GET", "/health", &[("Host", "localhost:8556"), ("Origin", "https://evil.example")]);
    assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
    assert_eq!(error_code(&json), "ORIGIN_NOT_ALLOWED");
    let (status, json) = call!(app, "GET", "/health", &[("Host", "localhost:8556"), ("Origin", "null")]);
    assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
    assert_eq!(reached.load(Ordering::SeqCst), 0, "a refused Origin must never execute the handler");

    let (status, json) = call!(app, "GET", "/health", &[("Host", "localhost:8556"), ("Origin", "http://localhost:3000")]);
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(reached.load(Ordering::SeqCst), 1);
}

#[actix_web::test]
async fn admin_method_from_a_browser_needs_an_allowlisted_origin_but_the_cli_does_not() {
    let reached = Arc::new(AtomicUsize::new(0));
    let app = test::init_service(guarded_app(dev_policy(), reached.clone())).await;
    let host = ("Host", "127.0.0.1:8556");

    // Browser shape without an Origin (Sec-Fetch-* is set by every browser fetch
    // and cannot be forged or removed by a page): refused on an admin method.
    let (status, json) = call!(app, "POST", "/blocks/mine", &[host, ("Sec-Fetch-Mode", "cors"), ("Sec-Fetch-Site", "cross-site")]);
    assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
    assert_eq!(error_code(&json), "ORIGIN_REQUIRED");
    assert_eq!(reached.load(Ordering::SeqCst), 0);

    // `same-site` (another port on this host) and `none` (typed URL, bookmark)
    // are not same-origin: an admin GET with those is refused the same way.
    for site in ["same-site", "none"] {
        let (status, json) = call!(app, "GET", "/node/metrics/host", &[host, ("Sec-Fetch-Mode", "cors"), ("Sec-Fetch-Site", site)]);
        assert_eq!(status, StatusCode::FORBIDDEN, "Sec-Fetch-Site: {site}: {json}");
        assert_eq!(error_code(&json), "ORIGIN_REQUIRED");
    }
    assert_eq!(reached.load(Ordering::SeqCst), 0);

    // Same browser shape on a public method proceeds (a same-origin navigation).
    let (status, _) = call!(app, "GET", "/health", &[host, ("Sec-Fetch-Mode", "navigate")]);
    assert_eq!(status, StatusCode::OK);

    // Browser with an allowlisted Origin: proceeds (authentication is #2133's job).
    let (status, json) = call!(app, "POST", "/blocks/mine", &[host, ("Origin", "http://localhost:5173"), ("Sec-Fetch-Mode", "cors")]);
    assert_eq!(status, StatusCode::OK, "{json}");

    // CLI / curl / node-to-node: no Origin, no Sec-Fetch-*.
    let (status, json) = call!(app, "POST", "/blocks/mine", &[host]);
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(reached.load(Ordering::SeqCst), 3);

    // The node's own page (#3599): the browser asserts `Sec-Fetch-Site: same-origin`
    // and, for a GET, sends no Origin at all. The admin GET reaches its handler,
    // whose bearer check (#2133) still decides; the Host check above ran first.
    let (status, json) = call!(app, "GET", "/node/metrics/host", &[host, ("Sec-Fetch-Mode", "cors"), ("Sec-Fetch-Site", "same-origin"), ("Sec-Fetch-Dest", "empty")]);
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(reached.load(Ordering::SeqCst), 4);

    // Same-origin does not launder a rebinding Host: the host allowlist runs first.
    let (status, json) = call!(app, "GET", "/node/metrics/host", &[("Host", "attacker.example:8556"), ("Sec-Fetch-Mode", "cors"), ("Sec-Fetch-Site", "same-origin")]);
    assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
    assert_eq!(error_code(&json), "HOST_NOT_ALLOWED");
    assert_eq!(reached.load(Ordering::SeqCst), 4);
}

#[actix_web::test]
async fn allow_no_origin_off_refuses_every_originless_request() {
    let reached = Arc::new(AtomicUsize::new(0));
    let strict = policy_from_values(None, None, false, Some("0")).unwrap();
    let app = test::init_service(guarded_app(strict, reached.clone())).await;
    let (status, json) = call!(app, "GET", "/health", &[("Host", "localhost:8556")]);
    assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
    assert_eq!(error_code(&json), "ORIGIN_REQUIRED");
    // The node's own page is no exception when the operator turned the switch off.
    let (status, json) = call!(app, "GET", "/health", &[("Host", "localhost:8556"), ("Sec-Fetch-Site", "same-origin")]);
    assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
    assert_eq!(error_code(&json), "ORIGIN_REQUIRED");
    assert_eq!(reached.load(Ordering::SeqCst), 0);
}

#[actix_web::test]
async fn wildcard_cors_is_not_producible_from_config() {
    assert!(matches!(
        policy_from_values(None, Some("*"), false, None),
        Err(OriginPolicyError::Wildcard { field: "allowed_origins", .. })
    ));
    assert!(matches!(
        policy_from_values(None, Some("https://app.example, *"), true, None),
        Err(OriginPolicyError::Wildcard { .. })
    ));
    assert!(matches!(
        policy_from_values(Some("*"), None, false, None),
        Err(OriginPolicyError::Wildcard { field: "allowed_hosts", .. })
    ));
    assert!(matches!(
        policy_from_values(None, Some("https://*.example"), false, None),
        Err(OriginPolicyError::Wildcard { .. })
    ));
    // Production with nothing configured: no browser origin is allowed at all
    // (the CORS side answers the same with its sentinel origin).
    let prod = policy_from_values(None, None, true, None).unwrap();
    assert!(prod.allowed_origins.is_empty());
    assert!(!prod.origin_allowed("http://localhost:3000"));
    // Configured names are added to the loopback defaults, never replacing them.
    let named = policy_from_values(Some("n5, node.example:8556"), Some("https://app.example"), true, None).unwrap();
    assert!(named.host_allowed("n5:8556"));
    assert!(named.host_allowed("node.example"));
    assert!(named.host_allowed("127.0.0.1"));
    assert!(named.origin_allowed("https://APP.example/"));
}

#[actix_web::test]
async fn the_admin_set_is_the_surface_document() {
    let admin = AdminMethodSet::from_surface_document();
    assert!(admin.len() >= 30, "{}", admin.len());
    assert!(admin.contains("POST", "/blocks/mine"));
    assert!(admin.contains("post", "/blocks/mine?x=1"));
    assert!(admin.contains("PUT", "/governance/proposals/abc/activate"));
    assert!(admin.contains("GET", "/version"));
    assert!(!admin.contains("GET", "/health"));
    assert!(!admin.contains("GET", "/blocks/mine"));
    assert!(!admin.contains("POST", "/not/registered"));
}

#[actix_web::test]
async fn cors_is_an_explicit_allowlist_that_never_reflects_or_wildcards() {
    // `build_cors_from_env` lives in the boot path this story must not edit; pin
    // the properties the story requires of it from the source instead.
    let main = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bin/launch_api/main.rs")).unwrap();
    let start = main.find("fn build_cors_from_env(").expect("build_cors_from_env exists");
    let body = &main[start..start + 3000];
    assert!(body.contains(".allowed_origin("), "CORS must be an explicit allowed_origin list");
    for forbidden in ["allow_any_origin", "allowed_origin_fn", "supports_credentials", "send_wildcard"] {
        assert!(!body.contains(forbidden), "CORS must never use {forbidden}");
    }
}

#[actix_web::test]
async fn configure_launch_routes_registers_every_route_inside_the_guarded_scope() {
    let source = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bin/launch_api/routes/mod.rs")).unwrap();
    assert!(
        source.contains("web::scope(\"\")\n        .wrap(origin::OriginGuard::from_env())"),
        "the route scope must be wrapped by the #2136 guard"
    );
    assert!(!source.contains("cfg.route("), "a route registered directly on the ServiceConfig bypasses the guard");
    assert!(source.contains("cfg.service(scope);"), "the guarded scope must be mounted");
}
