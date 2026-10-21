//! #3599 — the operator status page, served by the node itself.
//!
//! `GET /dashboard` returns one self-contained HTML document
//! (`status_page.html`, embedded at compile time) that renders this node's own
//! routes in a browser: a chain panel, a host panel and a node panel, refreshed
//! every 10 s. The document carries no data and no secret, so the route needs
//! no token; the operator-only route it reads (`/node/metrics/host`) takes the
//! bearer token the page keeps in the tab's session storage.
//!
//! Operators may open it on a laptop whose only network is the tailnet, so the
//! document makes no external request of any kind: inline CSS and JS, no CDN,
//! no fonts, no images. The tests below pin that, and that every path the page
//! fetches is registered in `routes/mod.rs`.

use actix_web::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use actix_web::{HttpResponse, Responder};

/// The page, embedded so the binary is the only thing an operator deploys.
pub const STATUS_PAGE_HTML: &str = include_str!("status_page.html");

/// Content-Security-Policy for the page: same-origin fetches and the inline
/// code the document itself carries, nothing else.
pub const STATUS_PAGE_CSP: &str = "default-src 'none'; script-src 'unsafe-inline'; \
                                   style-src 'unsafe-inline'; connect-src 'self'; img-src data:";

/// `GET /dashboard`
pub async fn dashboard() -> impl Responder {
    HttpResponse::Ok()
        .insert_header((CONTENT_TYPE, "text/html; charset=utf-8"))
        .insert_header((CACHE_CONTROL, "no-store"))
        .insert_header(("Content-Security-Policy", STATUS_PAGE_CSP))
        .insert_header(("X-Content-Type-Options", "nosniff"))
        .insert_header(("X-Frame-Options", "DENY"))
        .insert_header(("Referrer-Policy", "no-referrer"))
        .body(STATUS_PAGE_HTML)
}

#[cfg(test)]
mod tests {
    use actix_web::http::StatusCode;
    use actix_web::{App, test, web};

    use super::*;

    /// The registration site, read at compile time like the other source pins
    /// in this binary (`main.rs`, `p2p_events.rs`).
    const MOD_RS: &str = include_str!("mod.rs");

    /// Every path the page may request: the string literals of its single
    /// `const ROUTES = { ... }` object.
    fn page_routes() -> Vec<String> {
        let start = STATUS_PAGE_HTML
            .find("const ROUTES = {")
            .expect("the page declares its routes in one ROUTES object");
        let block = &STATUS_PAGE_HTML[start..];
        let end = block.find("};").expect("the ROUTES object closes");
        block[..end]
            .split('\'')
            .skip(1)
            .step_by(2)
            .map(str::to_string)
            .collect()
    }

    #[actix_web::test]
    async fn dashboard_route_returns_the_embedded_page_with_its_headers() {
        let app =
            test::init_service(App::new().route("/dashboard", web::get().to(dashboard))).await;
        let resp = test::call_service(&app, test::TestRequest::get().uri("/dashboard").to_request())
            .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let header = |name: &str| {
            resp.headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_else(|| panic!("{name} header missing"))
                .to_string()
        };
        assert_eq!(header("content-type"), "text/html; charset=utf-8");
        assert_eq!(header("cache-control"), "no-store");
        assert_eq!(header("content-security-policy"), STATUS_PAGE_CSP);
        assert_eq!(header("x-frame-options"), "DENY");
        let body = test::read_body(resp).await;
        let body = std::str::from_utf8(&body).expect("utf-8 page");
        assert!(body.contains("<title>"), "the page carries a <title>");
        assert_eq!(body, STATUS_PAGE_HTML, "the body is the embedded document, unchanged");
    }

    #[::core::prelude::v1::test]
    fn the_route_is_registered_in_mod_rs() {
        assert!(
            MOD_RS.contains(".route(\"/dashboard\", web::get().to(status_page::dashboard))"),
            "GET /dashboard must be registered in routes/mod.rs"
        );
    }

    #[::core::prelude::v1::test]
    fn every_path_the_page_fetches_is_a_registered_route() {
        let paths = page_routes();
        assert!(paths.len() >= 7, "ROUTES lists the page's data sources, found {paths:?}");
        for path in &paths {
            assert!(path.starts_with('/'), "route literal {path:?} must be an absolute path");
            assert!(
                MOD_RS.contains(&format!("\"{path}\"")),
                "the page fetches {path}, which routes/mod.rs does not register"
            );
        }
        for required in ["/network/info", "/node/sync/status", "/node/metrics/host"] {
            assert!(paths.iter().any(|p| p == required), "the page must read {required}");
        }
        // The ROUTES object is the only place a path may come from: one fetch
        // call, taking its argument from that table.
        assert_eq!(
            STATUS_PAGE_HTML.matches("fetch(").count(),
            1,
            "the page must issue requests through its single fetch helper"
        );
        assert!(STATUS_PAGE_HTML.contains("fetch(path, {"), "the fetch helper takes a ROUTES path");
    }

    #[::core::prelude::v1::test]
    fn the_page_makes_no_external_request() {
        for forbidden in [
            "http://",
            "https://",
            "//cdn",
            "<link rel=\"stylesheet\" href=",
            "<script src=",
            "@import",
            "url(",
            "<img ",
            "<iframe",
        ] {
            assert!(
                !STATUS_PAGE_HTML.contains(forbidden),
                "the page must not contain {forbidden:?}: it has to work with no network \
                 beyond the node itself"
            );
        }
    }

    #[::core::prelude::v1::test]
    fn the_page_names_no_environment() {
        for forbidden in ["192.168.", "10.0.", "172.16.", ".ts.net", "/home/", "wrfcoin.com"] {
            assert!(
                !STATUS_PAGE_HTML.contains(forbidden),
                "the page must not carry environment-specific text {forbidden:?}"
            );
        }
    }

    /// A 403 from the #2136 origin guard (`ORIGIN_REQUIRED`, `ORIGIN_NOT_ALLOWED`,
    /// `HOST_NOT_ALLOWED`) is the node refusing the browser, not a wrong token:
    /// the page must say so and must not tell the operator to re-enter the
    /// token. Only `require_role`'s codes mean a rejected token.
    #[::core::prelude::v1::test]
    fn the_page_tells_an_origin_guard_refusal_from_a_rejected_token() {
        for code in ["ORIGIN_REQUIRED", "ORIGIN_NOT_ALLOWED", "HOST_NOT_ALLOWED"] {
            assert!(
                STATUS_PAGE_HTML.contains(&format!("'{code}'")),
                "the page must recognise the guard code {code}"
            );
        }
        for code in ["UNAUTHORIZED", "FORBIDDEN"] {
            assert!(
                STATUS_PAGE_HTML.contains(&format!("'{code}'")),
                "the page must recognise the require_role code {code}"
            );
        }
        assert!(
            STATUS_PAGE_HTML.contains("GUARD_CODES.includes(code)")
                && STATUS_PAGE_HTML.contains("origin guard refuses browser reads"),
            "a guard refusal is reported as the guard, with its code"
        );
        assert!(
            STATUS_PAGE_HTML.contains("locked.hidden = !tokenRejected"),
            "only a rejected token re-shows the token field"
        );
    }

    #[::core::prelude::v1::test]
    fn the_token_never_reaches_the_url_or_persistent_storage() {
        assert!(STATUS_PAGE_HTML.contains("sessionStorage"), "the token lives in sessionStorage");
        assert!(!STATUS_PAGE_HTML.contains("localStorage"), "never localStorage");
        assert!(!STATUS_PAGE_HTML.contains("document.cookie"), "never a cookie");
        assert!(!STATUS_PAGE_HTML.contains("console.log"), "the page logs nothing");
        assert!(
            STATUS_PAGE_HTML.contains("headers['Authorization'] = 'Bearer ' + token"),
            "the token is sent as the bearer header helpers.rs expects"
        );
        assert!(!STATUS_PAGE_HTML.contains("token="), "never a token query parameter");
    }
}
