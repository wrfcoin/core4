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
//! no fonts, no images. Its only off-node URLs are two navigation links in the
//! "What's next" section (opened by the user, never fetched by the page). The
//! tests below pin that, and that every path the page fetches is registered in
//! `routes/mod.rs`.
//!
//! The page can also stop the node (`POST /node/shutdown`, Operator) with the
//! same token. The Windows Start-menu entry hands that token over in the URL
//! fragment (`/dashboard#operator=<token>`), which browsers never send to the
//! server; the page moves it to session storage and clears the address bar.

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

    /// The only off-node URLs the page may carry: navigation links in the
    /// "What's next" ladder, each an `<a href>` opened in a new tab without a
    /// referrer. The page never fetches them.
    const NAVIGATION_LINKS: &[&str] = &[
        "https://wrfcoin.com/downloads",
        "https://github.com/wrfcoin/infra/blob/main/testnet/JOIN_TESTNET.md",
    ];

    /// The page with its allowlisted navigation links removed, after checking
    /// each one appears only as such a link.
    fn page_without_navigation_links() -> String {
        let mut page = STATUS_PAGE_HTML.to_string();
        for link in NAVIGATION_LINKS {
            let anchor =
                format!("<a href=\"{link}\" target=\"_blank\" rel=\"noopener noreferrer\">");
            assert_eq!(
                STATUS_PAGE_HTML.matches(link).count(),
                STATUS_PAGE_HTML.matches(&anchor).count(),
                "{link} may appear only as a new-tab, no-referrer navigation link"
            );
            page = page.replace(&anchor, "");
        }
        page
    }

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
        for required in ["/network/info", "/node/sync/status", "/node/metrics/host", "/node/shutdown"] {
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
        let page = page_without_navigation_links();
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
                !page.contains(forbidden),
                "the page must not contain {forbidden:?}: it has to work with no network \
                 beyond the node itself"
            );
        }
    }

    #[::core::prelude::v1::test]
    fn the_page_names_no_environment() {
        let page = page_without_navigation_links();
        for forbidden in ["192.168.", "10.0.", "172.16.", ".ts.net", "/home/", "wrfcoin.com"] {
            assert!(
                !page.contains(forbidden),
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

    /// The Windows package opens this page for people who have never run a
    /// node: it must say what it is, give sync as a sentence and a bar, and
    /// make no claim of earnings (a sync-only node earns nothing).
    #[::core::prelude::v1::test]
    fn the_page_greets_a_newcomer_and_claims_no_earnings() {
        for text in [
            "WRFCoin node running on this computer",
            "'Syncing: block ' + fmtInt(height) + ' of ' + fmtInt(target)",
            "'Up to date'",
            "Can't reach your node",
            "role=\"progressbar\"",
            "It does not earn coins or rewards, and this page shows no balances.",
        ] {
            assert!(STATUS_PAGE_HTML.contains(text), "the newcomer summary must carry {text:?}");
        }
        let lower = STATUS_PAGE_HTML.to_ascii_lowercase();
        for forbidden in ["earned", "earning", "/balance", "/rewards", "payout"] {
            assert!(
                !lower.contains(forbidden),
                "the page must not show or fetch earnings ({forbidden:?})"
            );
        }
    }

    /// The owner's ladder: four rungs, the newcomer's own marked, what is not
    /// open yet marked "Coming", the climbing principle stated, and no promise
    /// of income — a sync-only node is said to be unpaid.
    #[::core::prelude::v1::test]
    fn the_page_shows_the_participation_ladder_and_promises_no_income() {
        let next = STATUS_PAGE_HTML
            .find("id=\"whats-next\"")
            .map(|start| &STATUS_PAGE_HTML[start..])
            .and_then(|rest| rest.find("</section>").map(|end| &rest[..end]))
            .expect("the page has a What's next section");
        let rungs = [
            "Sync node <span class=\"pill ok\">You are here</span>",
            "Bonded node <span class=\"pill info\">Preview</span>",
            "Attester (oracle) <span class=\"pill\">Coming</span>",
            "Block producer <span class=\"pill\">Coming</span>",
        ];
        let mut at = 0;
        for rung in rungs {
            let found = next[at..].find(rung).unwrap_or_else(|| panic!("ladder rung {rung:?} in order"));
            at += found + rung.len();
        }
        for text in [
            "You climb by reliable time on the rung below, not by computer power.",
            "250.36 WRF bond",
            "faucet bond grant",
            "visible peer for 24 hours",
            "serves chain snapshots to new nodes",
            "about 11 hours",
            "P2P address other nodes can reach",
            "Seated by governance",
            "A sync node is not paid.",
            "this page promises no income",
            "https://wrfcoin.com/downloads",
            "testnet/JOIN_TESTNET.md",
        ] {
            assert!(next.contains(text), "What's next must say {text:?}");
        }
        let lower = next.to_ascii_lowercase();
        for forbidden in ["guarantee", "apy", "profit", "per day", "per month", "you will earn", "you'll earn"] {
            assert!(!lower.contains(forbidden), "the ladder must make no income claim ({forbidden:?})");
        }
    }

    /// The Stop button: Operator token required, plain-words confirm, the
    /// stopped message once the node no longer answers, and the Start-menu
    /// entry still named.
    #[::core::prelude::v1::test]
    fn the_page_offers_a_guarded_stop_button() {
        for text in [
            "id=\"stop-btn\" disabled>Stop node</button>",
            "btn.disabled = !token || stopState !== null || unreachable;",
            "Stop your node? It stops syncing until you start it again from the Start menu.",
            "if (!window.confirm(STOP_CONFIRM)) return;",
            "request(ROUTES.shutdown, 'POST', true)",
            "Node stopped. Start it again from Start menu \\u2192 WRFCoin Node.",
            "if (r.status === 0) {\n      stopState = 'stopped';",
            "Stop WRFCoin Node",
        ] {
            assert!(STATUS_PAGE_HTML.contains(text), "the Stop control must carry {text:?}");
        }
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
        // The Windows launcher hands the token over in the fragment: read once,
        // removed from the address bar, never written back into a URL.
        assert!(STATUS_PAGE_HTML.contains("hash.indexOf('#operator=') !== 0"));
        assert!(
            STATUS_PAGE_HTML
                .contains("history.replaceState(null, '', location.pathname + location.search)"),
            "the fragment token is cleared from the address bar"
        );
        let take = STATUS_PAGE_HTML.find("function takeTokenFromFragment").expect("fragment intake");
        let clear = STATUS_PAGE_HTML[take..].find("history.replaceState").expect("clears") + take;
        let store = STATUS_PAGE_HTML[take..].find("token = value;").expect("stores") + take;
        assert!(clear < store, "the address bar is cleared before the token is used");
        for forbidden in ["location.hash =", "location.href =", "location.search =", "pushState("] {
            assert!(!STATUS_PAGE_HTML.contains(forbidden), "the page must not write {forbidden:?}");
        }
    }
}
