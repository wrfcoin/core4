//! #2138 — the launch_api envelope redacts internal errors.
//!
//! `helpers::envelope_redacted` is the one path an internal failure takes to the
//! client. These tests drive it with a detail that carries every leak class and
//! assert the response body, headers and status expose only the class; and they
//! pin, at source level, that no `INTERNAL_SERVER_ERROR` envelope in `routes/`
//! interpolates an internal error into its message any more.

#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/helpers.rs"]
mod helpers;
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

use actix_web::{App, HttpRequest, HttpResponse, http::StatusCode, test as actix_test, web};
use wrfcoin_native_chain::security::error_redaction::{ErrorKind, InternalError, redaction_findings};

const LEAKY_DETAIL: &str = "redb table blocks at /home/example/wrfcoin/core4/wrfcoin.redb (wrfcoin-native-chain 0.1.0) \
    on gh-runner 192.0.2.10 private_key SELECT * FROM blocks WHERE h=1 storage.rs:1234";

async fn storage_failure(req: HttpRequest) -> HttpResponse {
    helpers::envelope_redacted(&req, InternalError::new(ErrorKind::Storage, LEAKY_DETAIL))
}

async fn bad_request_failure(req: HttpRequest) -> HttpResponse {
    helpers::envelope_redacted(&req, InternalError::new(ErrorKind::BadRequest, LEAKY_DETAIL))
}

#[actix_web::test]
async fn redacted_envelope_exposes_only_the_class_and_a_correlation_id() {
    let app = actix_test::init_service(
        App::new()
            .route("/storage", web::get().to(storage_failure))
            .route("/bad", web::get().to(bad_request_failure)),
    )
    .await;

    let resp = actix_test::call_service(&app, actix_test::TestRequest::get().uri("/storage").to_request()).await;
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let header_id = resp
        .headers()
        .get("X-Correlation-Id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .expect("X-Correlation-Id header");
    assert!(resp.headers().contains_key("X-Request-Id"));
    let body = actix_test::read_body(resp).await;
    let text = String::from_utf8(body.to_vec()).unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();

    assert_eq!(json["ok"], serde_json::json!(false));
    assert_eq!(json["error"]["code"], serde_json::json!("STORAGE_ERROR"));
    assert_eq!(json["error"]["message"], serde_json::json!(ErrorKind::Storage.public_message()));
    let body_id = json["error"]["correlation_id"].as_str().expect("correlation_id in body");
    assert_eq!(body_id, header_id);
    uuid::Uuid::parse_str(body_id).expect("correlation id is a uuid");
    let findings = redaction_findings(&text);
    assert!(findings.is_empty(), "{findings:?}\n{text}");
    for fragment in ["redb", "/home/", "gh-runner", "0.1.0", "SELECT", "private_key", "storage.rs"] {
        assert!(!text.contains(fragment), "{fragment} reached the client: {text}");
    }

    // A client-class kind maps to its 4xx status with the same discipline.
    let resp = actix_test::call_service(&app, actix_test::TestRequest::get().uri("/bad").to_request()).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let text = String::from_utf8(actix_test::read_body(resp).await.to_vec()).unwrap();
    assert!(redaction_findings(&text).is_empty(), "{text}");
    assert!(text.contains("\"code\":\"BAD_REQUEST\""));
}

#[actix_web::test]
async fn plain_envelope_error_still_serialises_without_a_correlation_field() {
    async fn plain(req: HttpRequest) -> HttpResponse {
        helpers::envelope_error(&req, StatusCode::NOT_FOUND, "NOT_FOUND", "Not found")
    }
    let app = actix_test::init_service(App::new().route("/x", web::get().to(plain))).await;
    let resp = actix_test::call_service(&app, actix_test::TestRequest::get().uri("/x").to_request()).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let text = String::from_utf8(actix_test::read_body(resp).await.to_vec()).unwrap();
    assert!(!text.contains("correlation_id"), "unchanged wire shape for static messages: {text}");
    assert!(text.contains("\"code\":\"NOT_FOUND\""));
}

/// Source pin: no 5xx envelope in `routes/` formats an internal error into the
/// message. A `format!("…{e}…")` within a few lines of `INTERNAL_SERVER_ERROR`
/// is the leak shape this story removed; the redacting helper is the way through.
#[actix_web::test]
async fn no_internal_server_error_envelope_in_routes_interpolates_an_internal_error() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bin/launch_api/routes");
    let mut scanned = 0;
    let mut offenders = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = source.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if !line.contains("INTERNAL_SERVER_ERROR") || line.trim_start().starts_with("//") {
                continue;
            }
            scanned += 1;
            let window = lines[i.saturating_sub(4)..(i + 5).min(lines.len())].join("\n");
            if window.contains("format!(") && (window.contains("{e}") || window.contains("{e:") || window.contains("{err}") || window.contains("{error}")) {
                offenders.push(format!("{}:{}", path.display(), i + 1));
            }
        }
    }
    assert!(scanned >= 5, "expected to scan the 5xx sites, saw {scanned}");
    assert!(offenders.is_empty(), "5xx envelopes interpolating an internal error: {offenders:?}");
}
