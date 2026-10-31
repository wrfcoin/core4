//! `POST /node/shutdown` — stop this node from its own status page.
//!
//! The Windows package has no console for Ctrl-C, and its Start-menu "Stop
//! WRFCoin Node" entry is easy to miss. This route lets the operator stop the
//! node from `/dashboard`: it wakes the SAME graceful-shutdown task in `main`
//! that SIGINT/SIGTERM wake (P2P disconnect -> storage checkpoint -> HTTP stop),
//! so the node saves its place exactly as on Ctrl-C.
//!
//! Only the Operator role may call it; every other caller is refused before
//! anything is signalled. The handler answers `202 Accepted` and only signals the
//! task; the task waits briefly before it starts, and stops the HTTP server
//! gracefully (in-flight responses finish) as its last phase, so the caller
//! always receives the answer.

use std::sync::atomic::{AtomicBool, Ordering};

use actix_web::http::StatusCode;
use actix_web::{HttpRequest, HttpResponse, web};
use tokio::sync::Notify;

use crate::helpers::{envelope_error, require_role};
use crate::state::AppState;
use wrfcoin_native_chain::security::api_auth::ApiRole;

/// The stop request shared by the route (which raises it) and `main`'s
/// graceful-shutdown task (which waits for it). Registered once as app data.
#[derive(Debug, Default)]
pub struct NodeShutdown {
    requested: AtomicBool,
    notify: Notify,
}

impl NodeShutdown {
    pub fn new() -> Self {
        Self::default()
    }

    /// Raise the request. Returns `true` the first time, `false` when a stop
    /// was already requested (the task is woken once).
    pub fn request(&self) -> bool {
        let first = !self.requested.swap(true, Ordering::SeqCst);
        if first {
            // `notify_one` stores a permit, so a request raised before the
            // task polls `wait` is not lost.
            self.notify.notify_one();
        }
        first
    }

    #[cfg(test)]
    pub fn is_requested(&self) -> bool {
        self.requested.load(Ordering::SeqCst)
    }

    /// Resolves once a stop has been requested.
    pub async fn wait(&self) {
        self.notify.notified().await;
    }
}

/// How long `main`'s shutdown task waits after a route-raised request before
/// it starts, so the `202` reaches the page first.
pub const RESPONSE_GRACE: std::time::Duration = std::time::Duration::from_millis(250);

/// `POST /node/shutdown` (Operator).
pub async fn shutdown_node(
    req: HttpRequest,
    state: web::Data<AppState>,
    shutdown: Option<web::Data<NodeShutdown>>,
) -> HttpResponse {
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }
    let Some(shutdown) = shutdown else {
        return envelope_error(
            &req,
            StatusCode::SERVICE_UNAVAILABLE,
            "SHUTDOWN_UNAVAILABLE",
            "this process has no graceful-shutdown task to signal",
        );
    };
    let first = shutdown.request();
    if first {
        log::info!("Stop requested through POST /node/shutdown by the operator");
    }
    HttpResponse::Accepted().json(serde_json::json!({
        "status": "shutting_down",
        "already_requested": !first,
        "message": "The node is stopping: it disconnects from peers, saves its place and exits.",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_request_raised_before_anyone_waits_is_not_lost() {
        let shutdown = NodeShutdown::new();
        assert!(!shutdown.is_requested());
        assert!(shutdown.request(), "the first request is reported as first");
        assert!(!shutdown.request(), "a second request is not");
        assert!(shutdown.is_requested());
        tokio::time::timeout(std::time::Duration::from_secs(5), shutdown.wait())
            .await
            .expect("wait resolves for a request raised before it was polled");
    }

    #[tokio::test]
    async fn wait_stays_pending_without_a_request() {
        let shutdown = NodeShutdown::new();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), shutdown.wait())
                .await
                .is_err()
        );
    }
}
