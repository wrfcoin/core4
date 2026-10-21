//! #2135 — WebSocket connection, subscription and queue ceilings on the launch_api side.
//!
//! The accounting and policies (`WsAdmission`, `OutboundWindow`, `IdleClock`) are
//! unit-tested in shared-protocol. These tests prove the launch_api wiring: the
//! handshake refuses a saturated node or client IP **before the upgrade** with the
//! same trusted-proxy-aware identity the rate limiter uses, tickets release their
//! slots however the connection ends, configuration falls back to the conservative
//! policy, and — at source level — the actor applies the heartbeat, slow-consumer
//! and pong-flush paths.

#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/helpers.rs"]
mod helpers;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/rate_limit.rs"]
mod rate_limit;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/state.rs"]
mod state;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/websocket.rs"]
mod websocket;

include!("support/launch_api_test_support.inc.rs");

use std::net::SocketAddr;

use actix_web::{HttpResponse, http::StatusCode, test as actix_test};
use websocket::{AdmissionTicket, WsGate, ws_limits_from_values};
use wrfcoin_native_chain::security::ws_limits::{SlowConsumerPolicy, WsLimits, WsRefusal};

fn gate(max_connections: u64, max_per_ip: u64) -> &'static WsGate {
    Box::leak(Box::new(WsGate::new(ws_limits_from_values(
        Some(max_connections),
        Some(max_per_ip),
        None,
        None,
        None,
        None,
        None,
    ))))
}

fn handshake_from(peer: &str) -> actix_web::HttpRequest {
    actix_test::TestRequest::get()
        .uri("/ws")
        .peer_addr(peer.parse::<SocketAddr>().unwrap())
        .to_http_request()
}

async fn refusal(resp: HttpResponse) -> (StatusCode, Option<String>, serde_json::Value) {
    let status = resp.status();
    let retry_after = resp
        .headers()
        .get("Retry-After")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let body = actix_web::body::to_bytes(resp.into_body()).await.unwrap();
    (status, retry_after, serde_json::from_slice(&body).unwrap())
}

#[actix_web::test]
async fn handshake_admission_enforces_global_and_per_ip_caps_independently() {
    let gate = gate(3, 2);

    let a1: AdmissionTicket = websocket::admit(&handshake_from("10.0.0.1:5001"), gate).ok().expect("first from .1");
    let a2 = websocket::admit(&handshake_from("10.0.0.1:5002"), gate).ok().expect("second from .1");
    assert_eq!(gate.connections(), 2);
    assert_eq!(gate.connections_from("10.0.0.1"), 2);

    // Per-IP cap trips while the node still has room.
    let (status, retry, json) = refusal(websocket::admit(&handshake_from("10.0.0.1:5003"), gate).err().unwrap()).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(retry.as_deref(), Some(websocket::WS_REFUSED_RETRY_AFTER_SECS.to_string().as_str()));
    assert_eq!(json["error"]["code"], serde_json::json!("WS_TOO_MANY_FROM_IP"));
    assert_eq!(gate.connections(), 2, "a refusal holds no slot");

    let b1 = websocket::admit(&handshake_from("10.0.0.2:5001"), gate).ok().expect("first from .2");
    assert_eq!(gate.connections(), 3);

    // Global cap trips for a fresh IP with per-IP room.
    let (status, _, json) = refusal(websocket::admit(&handshake_from("10.0.0.3:5001"), gate).err().unwrap()).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(json["error"]["code"], serde_json::json!("WS_TOO_MANY_CONNECTIONS"));

    // Dropping a ticket — the actor stopping, however it stopped — frees the slot.
    drop(a1);
    assert_eq!(gate.connections(), 2);
    assert_eq!(gate.connections_from("10.0.0.1"), 1);
    let c1 = websocket::admit(&handshake_from("10.0.0.3:5001"), gate).ok().expect("room again");
    assert_eq!(gate.connections(), 3);

    drop((a2, b1, c1));
    assert_eq!(gate.connections(), 0);
    assert_eq!(gate.connections_from("10.0.0.1"), 0);
    assert_eq!(gate.connections_from("10.0.0.3"), 0);
}

#[actix_web::test]
async fn forwarding_headers_from_an_untrusted_peer_do_not_split_the_per_ip_cap() {
    // No WRFCOIN_TRUSTED_PROXY in this process: X-Forwarded-For is ignored, so a
    // caller cannot mint fresh identities by varying the header.
    let gate = gate(10, 2);
    let mut held = Vec::new();
    for (i, forged) in ["1.1.1.1", "2.2.2.2"].iter().enumerate() {
        let req = actix_test::TestRequest::get()
            .uri("/ws")
            .peer_addr(format!("10.0.0.9:{}", 6000 + i).parse::<SocketAddr>().unwrap())
            .insert_header(("X-Forwarded-For", *forged))
            .to_http_request();
        held.push(websocket::admit(&req, gate).ok().expect("under the cap"));
    }
    let req = actix_test::TestRequest::get()
        .uri("/ws")
        .peer_addr("10.0.0.9:6002".parse::<SocketAddr>().unwrap())
        .insert_header(("X-Forwarded-For", "3.3.3.3"))
        .to_http_request();
    let (status, _, json) = refusal(websocket::admit(&req, gate).err().unwrap()).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(json["error"]["code"], serde_json::json!("WS_TOO_MANY_FROM_IP"));
    assert_eq!(gate.connections_from("10.0.0.9"), 2);
    drop(held);
}

#[actix_web::test]
async fn subscription_ceilings_are_per_connection_and_node_wide_independently() {
    let gate: &'static WsGate = Box::leak(Box::new(WsGate::new(ws_limits_from_values(
        None,
        None,
        Some(2),
        Some(3),
        None,
        None,
        None,
    ))));
    gate.try_subscribe(0).unwrap();
    gate.try_subscribe(1).unwrap();
    assert!(matches!(gate.try_subscribe(2), Err(WsRefusal::SubscriptionLimit { max: 2 })));
    gate.try_subscribe(0).unwrap();
    assert!(matches!(gate.try_subscribe(0), Err(WsRefusal::NodeSubscriptionLimit { total: 3, max: 3 })));
    gate.unsubscribe(3);
    assert_eq!(gate.subscriptions_total(), 0);
}

#[actix_web::test]
async fn configuration_falls_back_to_the_conservative_policy() {
    let d = ws_limits_from_values(None, None, None, None, None, None, None);
    let defaults = WsLimits::default();
    assert_eq!(d.max_connections, defaults.max_connections);
    assert_eq!(d.max_per_client_ip, defaults.max_per_client_ip);
    assert_eq!(d.max_subscriptions_per_conn, 32, "the existing per-connection cap is the default");
    assert_eq!(d.slow_consumer_policy, SlowConsumerPolicy::DisconnectClient);
    assert_eq!(d.idle_timeout, defaults.idle_timeout);

    let garbage = ws_limits_from_values(None, None, None, None, None, Some("whatever"), None);
    assert_eq!(garbage.slow_consumer_policy, SlowConsumerPolicy::DisconnectClient, "unknown word keeps disconnect");
    let drop = ws_limits_from_values(Some(7), Some(1), Some(4), Some(9), Some(5), Some("drop-oldest"), Some(60));
    assert_eq!(drop.slow_consumer_policy, SlowConsumerPolicy::DropOldest);
    assert_eq!((drop.max_connections, drop.max_per_client_ip, drop.max_subscriptions_per_conn), (7, 1, 4));
    assert_eq!((drop.max_subscriptions_total, drop.max_outbound_queue), (9, 5));
    assert_eq!(drop.idle_timeout, std::time::Duration::from_secs(60));
    let node = websocket::ws_gate();
    assert!(node.limits().max_connections >= 1);
}

/// Source pins for the actor paths a TestRequest cannot drive: admission before
/// the upgrade, heartbeat reaping, slow-consumer close, pong flush, and slot
/// release on every exit.
#[actix_web::test]
async fn the_actor_applies_the_ceilings_on_every_path() {
    let source = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bin/launch_api/websocket.rs")).unwrap();
    let ws_index = source.find("pub async fn ws_index(").unwrap();
    let body = &source[ws_index..ws_index + 1500];
    let admit = body.find("admit(&req, ws_gate())").expect("handshake admission");
    let start = body.find("ws::start(").expect("upgrade");
    assert!(admit < start, "admission must precede the upgrade");

    assert!(source.contains("ctx.run_interval(heartbeat"), "heartbeat interval");
    assert!(source.contains("act.idle.should_reap(&act.limits)"), "idle reaping");
    assert!(source.contains("Offer::Disconnect =>"), "slow-consumer disconnect path");
    assert!(source.contains("self.window.on_pong(&self.limits)"), "pong flush");
    assert!(source.contains("self.idle.touch()"), "client frames count as activity");
    assert!(source.contains("impl Drop for AdmissionTicket"), "slot released on drop");
    assert!(source.contains("self.ticket.gate.try_subscribe("), "node-wide subscription accounting");
    assert!(!source.contains("self.subscriptions.remove(&channel);"), "unsubscribe must go through the accounting path");
}
