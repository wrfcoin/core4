//! #2137 — bounded query cost on the launch_api side.
//!
//! The rules (`CostMeter`, `HistoryRange`, `page_size`, `Cursor`,
//! `declared_inputs_within`) are unit-tested in shared-protocol. These tests prove
//! the launch_api pieces: the refusal shapes each limit produces, that the
//! expensive pool refuses at once when saturated **without blocking ordinary
//! requests**, and — at source level — that the paged, ranged and proving
//! handlers actually call the bounds before doing the work.

#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/helpers.rs"]
mod helpers;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/routes/query_cost.rs"]
mod query_cost;
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

use std::time::{Duration, Instant};

use actix_web::{App, HttpRequest, HttpResponse, http::StatusCode, test as actix_test, web};
use tokio::sync::Semaphore;
use wrfcoin_native_chain::security::query_cost::{
    CostExceeded, CursorError, InputsExceeded, PageError, RangeError,
};

async fn body_json(resp: actix_web::dev::ServiceResponse<impl actix_web::body::MessageBody>) -> serde_json::Value {
    let bytes = actix_test::read_body(resp).await;
    serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::String(String::from_utf8_lossy(&bytes).into_owned()))
}

#[actix_web::test]
async fn blocking_worker_failure_is_redacted_and_correlated() {
    let pool = std::sync::Arc::new(Semaphore::new(1));
    let request = actix_test::TestRequest::default().to_http_request();
    let response = query_cost::run_expensive(&request, &pool, || -> () {
        panic!("synthetic-worker-private-detail-2137");
    })
    .await
    .unwrap_err();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let correlation = response.headers().get("X-Correlation-Id").unwrap().to_str().unwrap().to_owned();
    assert!(response.headers().contains_key("X-Request-Id"));
    let bytes = actix_web::body::to_bytes(response.into_body()).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["error"]["code"], "INTERNAL_SERVER_ERROR");
    assert_eq!(body["error"]["correlation_id"], correlation);
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(!text.contains("blocking worker"), "{text}");
    assert!(!text.contains("synthetic-worker-private-detail-2137"), "{text}");
    assert_eq!(pool.available_permits(), 1, "panic releases the pool permit");
}

#[actix_web::test]
async fn each_limit_has_its_own_refusal_shape() {
    async fn page(req: HttpRequest) -> HttpResponse {
        query_cost::page_rejected(&req, PageError::TooLarge { requested: 501, max: 500 })
    }
    async fn range(req: HttpRequest) -> HttpResponse {
        query_cost::range_rejected(&req, RangeError::TooWide { span: 2_000_000, max: 1_000_000 })
    }
    async fn inverted(req: HttpRequest) -> HttpResponse {
        query_cost::range_rejected(&req, RangeError::Inverted { from: 9, to: 1 })
    }
    async fn cost(req: HttpRequest) -> HttpResponse {
        query_cost::cost_exceeded(&req, CostExceeded::ScanUnits { spent: 50_001, max: 50_000 })
    }
    async fn cursor(req: HttpRequest) -> HttpResponse {
        query_cost::cursor_rejected(&req, CursorError("junk".into()))
    }
    async fn inputs(req: HttpRequest) -> HttpResponse {
        query_cost::inputs_rejected(&req, InputsExceeded { declared: 65, max: 64 })
    }
    let app = actix_test::init_service(
        App::new()
            .route("/page", web::get().to(page))
            .route("/range", web::get().to(range))
            .route("/inverted", web::get().to(inverted))
            .route("/cost", web::get().to(cost))
            .route("/cursor", web::get().to(cursor))
            .route("/inputs", web::get().to(inputs)),
    )
    .await;
    for (path, status, code) in [
        ("/page", StatusCode::BAD_REQUEST, "PAGE_TOO_LARGE"),
        ("/range", StatusCode::BAD_REQUEST, "RANGE_TOO_WIDE"),
        ("/inverted", StatusCode::BAD_REQUEST, "RANGE_INVERTED"),
        ("/cost", StatusCode::UNPROCESSABLE_ENTITY, "QUERY_TOO_EXPENSIVE"),
        ("/cursor", StatusCode::BAD_REQUEST, "BAD_CURSOR"),
        ("/inputs", StatusCode::PAYLOAD_TOO_LARGE, "TOO_MANY_INPUTS"),
    ] {
        let resp = actix_test::call_service(&app, actix_test::TestRequest::get().uri(path).to_request()).await;
        assert_eq!(resp.status(), status, "{path}");
        let json = body_json(resp).await;
        assert_eq!(json["ok"], serde_json::json!(false), "{path}");
        assert_eq!(json["error"]["code"], serde_json::json!(code), "{path}: {json}");
    }
}

#[actix_web::test]
async fn a_saturated_expensive_pool_refuses_at_once_and_never_blocks_ordinary_requests() {
    // A pool of one permit, held by the test for the whole scenario.
    let pool: Arc<Semaphore> = Arc::new(Semaphore::new(1));
    let held = pool.clone().try_acquire_owned().expect("first permit");

    let pool_for_route = pool.clone();
    async fn ordinary() -> HttpResponse {
        HttpResponse::Ok().body("ordinary")
    }
    let app = actix_test::init_service(
        App::new()
            .app_data(web::Data::new(pool_for_route))
            .route(
                "/expensive",
                web::get().to(|req: HttpRequest, pool: web::Data<Arc<Semaphore>>| async move {
                    match query_cost::run_expensive(&req, pool.get_ref(), || {
                        std::thread::sleep(Duration::from_millis(200));
                        42u64
                    })
                    .await
                    {
                        Ok(v) => HttpResponse::Ok().body(v.to_string()),
                        Err(resp) => resp,
                    }
                }),
            )
            .route("/health", web::get().to(ordinary)),
    )
    .await;

    // Saturated: the expensive route answers 503 + Retry-After immediately.
    let started = Instant::now();
    let resp = actix_test::call_service(&app, actix_test::TestRequest::get().uri("/expensive").to_request()).await;
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        resp.headers().get("Retry-After").and_then(|v| v.to_str().ok()),
        Some(query_cost::EXPENSIVE_POOL_RETRY_AFTER_SECS.to_string().as_str())
    );
    let json = body_json(resp).await;
    assert_eq!(json["error"]["code"], serde_json::json!("EXPENSIVE_POOL_SATURATED"));
    assert!(started.elapsed() < Duration::from_millis(150), "refusal must not queue: {:?}", started.elapsed());

    // Ordinary requests are unaffected while the pool is saturated.
    for _ in 0..5 {
        let started = Instant::now();
        let resp = actix_test::call_service(&app, actix_test::TestRequest::get().uri("/health").to_request()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(started.elapsed() < Duration::from_millis(150));
    }

    // Release the permit: the expensive route runs its work under it.
    drop(held);
    let resp = actix_test::call_service(&app, actix_test::TestRequest::get().uri("/expensive").to_request()).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(actix_test::read_body(resp).await, "42");
    assert_eq!(pool.available_permits(), 1, "permit released after the work");
}

#[actix_web::test]
async fn node_limits_come_from_values_with_defaults_kept() {
    let d = query_cost::query_cost_from_values(None, None, None, None);
    assert_eq!(d.max_page_size, 500);
    assert_eq!(d.max_scan_units, 50_000);
    assert_eq!(d.max_history_blocks, 1_000_000);
    assert_eq!(d.max_wall_time, Duration::from_secs(2));
    let c = query_cost::query_cost_from_values(Some(10), Some(20), Some(30), Some(40));
    assert_eq!((c.max_page_size, c.max_scan_units, c.max_history_blocks), (10, 20, 30));
    assert_eq!(c.max_wall_time, Duration::from_millis(40));
    let node = query_cost::query_cost();
    assert!(node.max_page_size >= 1 && node.max_scan_units >= 1);
}

/// Source pins: the handlers this story bounds call the bound before the work.
#[actix_web::test]
async fn handlers_apply_the_bounds_before_the_work() {
    let routes = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bin/launch_api/routes");
    let read = |f: &str| std::fs::read_to_string(routes.join(f)).unwrap();

    let governance = read("governance.rs");
    for handler in ["list_governance_proposals", "get_treasury_transactions"] {
        let start = governance.find(&format!("pub async fn {handler}(")).unwrap_or_else(|| panic!("{handler}"));
        let body = &governance[start..start + 3000];
        assert!(body.contains("page_size("), "{handler} must resolve its page through page_size");
        assert!(body.contains("start_position("), "{handler} must accept a cursor");
        assert!(body.contains("meter.charge("), "{handler} must charge its scan");
        assert!(body.contains("next_cursor"), "{handler} must return next_cursor");
        assert!(!body.contains(".clamp(1, MAX_") && !body.contains(".min(MAX_"), "{handler} must refuse, not clamp");
    }

    let quality = read("quality.rs");
    let start = quality.find("pub async fn get_quality_events(").unwrap();
    let body = &quality[start..start + 6000];
    let bound = body.find("HistoryRange::bounded(").expect("range bounded");
    let scan = body.find("while position > scan_floor").expect("metered scan loop");
    assert!(bound < scan, "the range must be bounded before the scan starts");
    assert!(body.contains("meter.charge(1)"), "one unit per record examined");
    assert!(body.contains("query_cost::cost_exceeded("), "the scan aborts on budget");
    assert!(body.contains("page_size("));

    let privacy = read("privacy.rs");
    let ring = privacy.find("pub async fn create_ring_signature(").unwrap();
    let ring_body = &privacy[ring..ring + 4000];
    let check = ring_body.find("declared_inputs_within(body.decoy_public_keys.len()").expect("ring size declared check");
    let alloc = ring_body.find("Vec::with_capacity(body.decoy_public_keys.len())").expect("ring allocation");
    assert!(check < alloc, "declared ring size is checked before allocating for it");
    assert!(ring_body.contains("run_expensive("), "ring signing runs on the expensive pool");
    let range = privacy.find("pub async fn create_range_proof(").unwrap();
    let range_body = &privacy[range..range + 3000];
    let check = range_body.find("declared_inputs_within(body.n_bits").expect("bits declared check");
    let prove = range_body.find("RangeProof::prove(").expect("proving");
    assert!(check < prove);
    assert!(range_body.contains("run_expensive("), "range proving runs on the expensive pool");
}
