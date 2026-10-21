use actix::prelude::*;
use actix_web::{
    Error, HttpRequest, HttpResponse,
    http::{StatusCode, header},
    web,
};
use actix_web_actors::ws;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use wrfcoin_native_chain::security::api_auth::{ApiRole, AuthPrincipal};
use wrfcoin_native_chain::security::ws_limits::{
    IdleClock, Offer, OutboundWindow, SlowConsumerPolicy, WsAdmission, WsLimits, WsRefusal,
};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
#[cfg(test)]
use wrfcoin_native_chain::security::api_auth::Identity;

// ---------------------------------------------------------------------------
// Event types that flow through the broadcast channel
// ---------------------------------------------------------------------------

/// Events broadcast to WebSocket subscribers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum WsEvent {
    NewBlock {
        height: u64,
        hash: String,
    },
    GovernanceUpdate {
        proposal_id: String,
        status: String,
    },
    WalletActivity {
        address: String,
        tx_hash: String,
        amount: u64,
    },
    BridgeLock {
        tx_id: String,
        amount: u128,
    },
    SystemStatus {
        status: String,
    },
    /// Emitted when a weather-data or mining reward is distributed (#971).
    RewardDistributed {
        contributor: String,
        amount: u64,
        reason: String,
        is_first_reward: bool,
        block_height: u64,
    },
}

impl WsEvent {
    /// Returns the channel name this event belongs to.
    fn channel(&self) -> String {
        match self {
            WsEvent::NewBlock { .. } => "blocks".to_string(),
            WsEvent::GovernanceUpdate { .. } => "governance".to_string(),
            WsEvent::WalletActivity { address, .. } => format!("wallet:{address}"),
            WsEvent::BridgeLock { .. } => "blocks".to_string(),
            WsEvent::SystemStatus { .. } => "blocks".to_string(),
            WsEvent::RewardDistributed { contributor, .. } => {
                format!("rewards:{contributor}")
            },
        }
    }

    /// Returns the snake_case event name used in the launch-contract wire envelope.
    ///
    /// The launch contract suite (infra) and the backend WS bridge both expect
    /// event names in `snake_case` rather than the `PascalCase` variant names
    /// produced by the internal `#[serde(tag = "type")]` representation.
    fn event_name(&self) -> &'static str {
        match self {
            WsEvent::NewBlock { .. } => "new_block",
            WsEvent::GovernanceUpdate { .. } => "governance_update",
            WsEvent::WalletActivity { .. } => "wallet_activity",
            WsEvent::BridgeLock { .. } => "bridge_lock",
            WsEvent::SystemStatus { .. } => "system_status",
            WsEvent::RewardDistributed { .. } => "reward_distributed",
        }
    }

    /// Serializes this event into the launch-contract-compatible wire envelope:
    ///
    /// ```json
    /// { "event": "new_block", "channel": "blocks", "payload": { "height": 1, "hash": "…" } }
    /// ```
    ///
    /// The internal serde representation uses `#[serde(tag = "type", content = "payload")]`
    /// which produces `{ "type": "NewBlock", "payload": … }`.  This method extracts the
    /// `payload` object and repackages it with the gateway-style keys (`event`, `channel`,
    /// `payload`) that the launch contract suite and infra scripts expect.
    fn to_launch_frame(&self) -> serde_json::Value {
        let raw = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        let payload = raw
            .get("payload")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        serde_json::json!({
            "event":   self.event_name(),
            "channel": self.channel(),
            "payload": payload,
        })
    }
}

// ---------------------------------------------------------------------------
// WebSocket authentication errors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WsAuthError {
    MissingToken,
    InvalidToken,
}

// ---------------------------------------------------------------------------
// Client-to-server messages
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(tag = "action")]
enum ClientMessage {
    #[serde(rename = "subscribe")]
    Subscribe { channel: String },
    #[serde(rename = "unsubscribe")]
    Unsubscribe { channel: String },
}

const MAX_SUBSCRIPTIONS_PER_CONN: usize = 32;
/// Maximum byte length for a channel name string.  Prevents authenticated
/// clients from allocating unbounded memory via arbitrarily long channel
/// name subscriptions (MAX_SUBSCRIPTIONS_PER_CONN × unbounded → DoS).
const MAX_WS_CHANNEL_NAME_LEN: usize = 256;

// ---------------------------------------------------------------------------
// #2135: connection, subscription and queue ceilings
// ---------------------------------------------------------------------------

pub const WS_MAX_CONNECTIONS_ENV: &str = "WRFCOIN_WS_MAX_CONNECTIONS";
pub const WS_MAX_PER_IP_ENV: &str = "WRFCOIN_WS_MAX_PER_IP";
pub const WS_MAX_SUBSCRIPTIONS_PER_CONN_ENV: &str = "WRFCOIN_WS_MAX_SUBSCRIPTIONS_PER_CONN";
pub const WS_MAX_SUBSCRIPTIONS_TOTAL_ENV: &str = "WRFCOIN_WS_MAX_SUBSCRIPTIONS_TOTAL";
pub const WS_MAX_OUTBOUND_QUEUE_ENV: &str = "WRFCOIN_WS_MAX_OUTBOUND_QUEUE";
/// `disconnect` (default) or `drop-oldest`.
pub const WS_SLOW_CONSUMER_ENV: &str = "WRFCOIN_WS_SLOW_CONSUMER";
pub const WS_IDLE_TIMEOUT_SECS_ENV: &str = "WRFCOIN_WS_IDLE_TIMEOUT_SECS";
/// Seconds a refused WebSocket client is told to wait.
pub const WS_REFUSED_RETRY_AFTER_SECS: u64 = 5;

fn env_u64(name: &str) -> Option<u64> {
    // determinism-ok: node-local WebSocket admission limits read from operator config; they bound API connections and never reach block or state bytes
    std::env::var(name).ok().and_then(|v| v.trim().parse::<u64>().ok()).filter(|v| *v > 0)
}

/// Build the ceilings from explicit values (tests) — `None` keeps the default.
/// `MAX_SUBSCRIPTIONS_PER_CONN` stays the per-connection default.
pub fn ws_limits_from_values(
    max_connections: Option<u64>,
    max_per_client_ip: Option<u64>,
    max_subscriptions_per_conn: Option<u64>,
    max_subscriptions_total: Option<u64>,
    max_outbound_queue: Option<u64>,
    slow_consumer: Option<&str>,
    idle_timeout_secs: Option<u64>,
) -> WsLimits {
    let d = WsLimits {
        max_subscriptions_per_conn: MAX_SUBSCRIPTIONS_PER_CONN as u32,
        ..WsLimits::default()
    };
    let u32_of = |v: Option<u64>, default: u32| v.map(|v| v.min(u32::MAX as u64) as u32).unwrap_or(default);
    WsLimits {
        max_connections: u32_of(max_connections, d.max_connections),
        max_per_client_ip: u32_of(max_per_client_ip, d.max_per_client_ip),
        max_subscriptions_per_conn: u32_of(max_subscriptions_per_conn, d.max_subscriptions_per_conn),
        max_subscriptions_total: u32_of(max_subscriptions_total, d.max_subscriptions_total),
        max_outbound_queue: max_outbound_queue.map(|v| v as usize).unwrap_or(d.max_outbound_queue),
        // An unrecognised policy word is NOT a silent fallback to dropping: it
        // keeps the default (disconnect), which is the conservative one.
        slow_consumer_policy: slow_consumer
            .and_then(SlowConsumerPolicy::parse)
            .unwrap_or(d.slow_consumer_policy),
        idle_timeout: idle_timeout_secs.map(Duration::from_secs).unwrap_or(d.idle_timeout),
    }
}

pub fn ws_limits_from_env() -> WsLimits {
    // determinism-ok: node-local WebSocket slow-consumer policy from operator config; affects API delivery only, never consensus state
    let slow = std::env::var(WS_SLOW_CONSUMER_ENV).ok();
    ws_limits_from_values(
        env_u64(WS_MAX_CONNECTIONS_ENV),
        env_u64(WS_MAX_PER_IP_ENV),
        env_u64(WS_MAX_SUBSCRIPTIONS_PER_CONN_ENV),
        env_u64(WS_MAX_SUBSCRIPTIONS_TOTAL_ENV),
        env_u64(WS_MAX_OUTBOUND_QUEUE_ENV),
        slow.as_deref(),
        env_u64(WS_IDLE_TIMEOUT_SECS_ENV),
    )
}

/// Node-wide WebSocket ceilings and the counters behind them.
pub struct WsGate {
    limits: WsLimits,
    admission: Mutex<WsAdmission>,
}

impl WsGate {
    pub fn new(limits: WsLimits) -> Self {
        Self {
            limits,
            admission: Mutex::new(WsAdmission::default()),
        }
    }

    pub fn limits(&self) -> &WsLimits {
        &self.limits
    }

    fn admission(&self) -> std::sync::MutexGuard<'_, WsAdmission> {
        self.admission.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn try_connect(&self, ip: &str) -> Result<(), WsRefusal> {
        self.admission().try_connect(ip, &self.limits)
    }

    pub fn disconnect(&self, ip: &str) {
        self.admission().disconnect(ip);
    }

    pub fn try_subscribe(&self, on_connection: u32) -> Result<(), WsRefusal> {
        self.admission().try_subscribe(on_connection, &self.limits)
    }

    pub fn unsubscribe(&self, count: u32) {
        self.admission().unsubscribe(count);
    }

    pub fn connections(&self) -> u32 {
        self.admission().connections()
    }

    pub fn connections_from(&self, ip: &str) -> u32 {
        self.admission().connections_from(ip)
    }

    pub fn subscriptions_total(&self) -> u32 {
        self.admission().subscriptions_total()
    }
}

/// The node's gate, configured once from the environment.
pub fn ws_gate() -> &'static WsGate {
    static GATE: OnceLock<WsGate> = OnceLock::new();
    GATE.get_or_init(|| WsGate::new(ws_limits_from_env()))
}

/// Proof that a connection was admitted; releases its slot and its
/// subscriptions when dropped, whichever way the actor ends.
pub struct AdmissionTicket {
    gate: &'static WsGate,
    ip: String,
    subscriptions: u32,
    counted: bool,
}

impl AdmissionTicket {
    /// A ticket that holds no slot (unit tests that construct `WsConn` directly).
    pub fn uncounted(gate: &'static WsGate) -> Self {
        Self {
            gate,
            ip: String::new(),
            subscriptions: 0,
            counted: false,
        }
    }

    pub fn client_ip(&self) -> &str {
        &self.ip
    }
}

impl Drop for AdmissionTicket {
    fn drop(&mut self) {
        if self.counted {
            self.gate.disconnect(&self.ip);
        }
        if self.subscriptions > 0 {
            self.gate.unsubscribe(self.subscriptions);
        }
    }
}

/// 503 + `Retry-After`, sent instead of the upgrade.
fn ws_refused_response(req: &HttpRequest, refusal: &WsRefusal) -> HttpResponse {
    let mut response = crate::helpers::envelope_error(
        req,
        StatusCode::SERVICE_UNAVAILABLE,
        refusal.code(),
        refusal.to_string(),
    );
    response.headers_mut().insert(
        header::RETRY_AFTER,
        // A u64 is always a valid header value: a total conversion, no parse (#3308).
        header::HeaderValue::from(WS_REFUSED_RETRY_AFTER_SECS),
    );
    response
}

/// Admit or refuse a handshake against `gate`, before the upgrade. The client
/// IP is the same trusted-proxy-aware identity the rate limiter uses (#1830),
/// so the per-IP cap and the per-IP rate limit agree about who a caller is.
pub fn admit(req: &HttpRequest, gate: &'static WsGate) -> Result<AdmissionTicket, HttpResponse> {
    let peer_ip = req.peer_addr().map(|a| a.ip());
    let xff = req.headers().get("X-Forwarded-For").and_then(|v| v.to_str().ok());
    let xri = req.headers().get("X-Real-IP").and_then(|v| v.to_str().ok());
    let ip = crate::rate_limit::resolve_client_ip(peer_ip, xff, xri, &crate::rate_limit::trusted_proxies_from_env());
    match gate.try_connect(&ip) {
        Ok(()) => Ok(AdmissionTicket {
            gate,
            ip,
            subscriptions: 0,
            counted: true,
        }),
        Err(refusal) => {
            log::warn!("WS handshake refused (#2135): {refusal}");
            Err(ws_refused_response(req, &refusal))
        }
    }
}

#[derive(Deserialize)]
struct WsTokenQuery {
    token: Option<String>,
}

// `token` is a WebSocket bearer credential. A derived `Debug` would print it
// verbatim, so it is hand-implemented to redact the value while still revealing
// whether a token was present — matching the codebase's redacted-Debug pattern
// (`PrivateKey([REDACTED])`, `StealthKeys`) so a future `{:?}` cannot leak it (#2129).
impl std::fmt::Debug for WsTokenQuery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WsTokenQuery")
            .field("token", &self.token.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubscriptionError {
    UnknownChannel,
    Forbidden,
    LimitReached,
    /// #2135: the node-wide subscription ceiling, independent of the per-connection one.
    NodeLimitReached,
}

fn trim_nonempty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn parse_ws_protocol_token(protocol_header: &str) -> Option<String> {
    let mut awaiting_token = false;

    for part in protocol_header.split(',') {
        let Some(part) = trim_nonempty(part) else {
            continue;
        };

        if awaiting_token {
            return Some(part.to_string());
        }

        // Check compact form case-insensitively: "bearer.token" or "Bearer.token" etc.
        if part.len() > 7 && part[..7].eq_ignore_ascii_case("bearer.") {
            let token = &part[7..];
            return trim_nonempty(token).map(str::to_owned);
        }

        if part.eq_ignore_ascii_case("bearer") {
            awaiting_token = true;
        }
    }

    None
}

fn extract_ws_query_token(req: &HttpRequest) -> Option<String> {
    web::Query::<WsTokenQuery>::from_query(req.query_string())
        .ok()
        .and_then(|query| {
            query
                .token
                .as_deref()
                .and_then(trim_nonempty)
                .map(str::to_owned)
        })
}

fn extract_ws_bearer_token(req: &HttpRequest) -> Option<String> {
    crate::helpers::bearer_token_from_request(req)
        .map(str::to_owned)
        .or_else(|| {
            req.headers()
                .get(header::SEC_WEBSOCKET_PROTOCOL)
                .and_then(|value| value.to_str().ok())
                .and_then(parse_ws_protocol_token)
        })
        .or_else(|| extract_ws_query_token(req))
}

fn resolve_ws_principal(
    req: &HttpRequest,
    token_store: &FxHashMap<String, AuthPrincipal>,
) -> Result<AuthPrincipal, WsAuthError> {
    let token = extract_ws_bearer_token(req).ok_or(WsAuthError::MissingToken)?;
    token_store
        .get(token.as_str())
        .cloned()
        .ok_or(WsAuthError::InvalidToken)
}

fn ws_auth_error_response(req: &HttpRequest, error: WsAuthError) -> HttpResponse {
    match error {
        WsAuthError::MissingToken => crate::helpers::envelope_error(
            req,
            StatusCode::UNAUTHORIZED,
            "UNAUTHORIZED",
            "Missing websocket bearer token",
        ),
        WsAuthError::InvalidToken => crate::helpers::envelope_error(
            req,
            StatusCode::FORBIDDEN,
            "FORBIDDEN",
            "Invalid API token",
        ),
    }
}

fn subscription_error_frame(channel: &str, error: SubscriptionError) -> serde_json::Value {
    let (code, message) = match error {
        SubscriptionError::UnknownChannel => (
            "UNKNOWN_CHANNEL",
            format!("unknown websocket channel '{channel}'"),
        ),
        SubscriptionError::Forbidden => (
            "FORBIDDEN",
            format!("subscription to '{channel}' is not permitted for this identity"),
        ),
        SubscriptionError::LimitReached => (
            "SUBSCRIPTION_LIMIT",
            format!("subscription limit of {} reached", ws_gate().limits().max_subscriptions_per_conn),
        ),
        SubscriptionError::NodeLimitReached => (
            "WS_NODE_SUBSCRIPTION_LIMIT",
            "node-wide subscription limit reached; retry later".to_string(),
        ),
    };

    serde_json::json!({
        "event": "error",
        "payload": {
            "code": code,
            "channel": channel,
            "message": message,
        }
    })
}

// ---------------------------------------------------------------------------
// WebSocket authentication functions
// ---------------------------------------------------------------------------

fn authorize_subscription(
    principal: &AuthPrincipal,
    channel: &str,
) -> Result<(), SubscriptionError> {
    match channel {
        "blocks" | "governance" => {
            if principal.satisfies_role(ApiRole::Reader) {
                Ok(())
            } else {
                Err(SubscriptionError::Forbidden)
            }
        },
        _ if channel.starts_with("wallet:") => {
            let addr = channel
                .strip_prefix("wallet:")
                .and_then(trim_nonempty)
                .ok_or(SubscriptionError::UnknownChannel)?;
            if principal.role == ApiRole::Operator {
                return Ok(());
            }
            match principal
                .identity
                .as_ref()
                .and_then(|identity| identity.wallet_address.as_deref())
            {
                Some(caller_addr) if caller_addr.eq_ignore_ascii_case(addr) => Ok(()),
                _ => Err(SubscriptionError::Forbidden),
            }
        },
        _ if channel.starts_with("rewards:") => {
            let contributor_id = channel
                .strip_prefix("rewards:")
                .and_then(trim_nonempty)
                .ok_or(SubscriptionError::UnknownChannel)?;
            if principal.role == ApiRole::Operator {
                return Ok(());
            }
            match principal
                .identity
                .as_ref()
                .and_then(|identity| identity.contributor_id.as_deref())
            {
                Some(caller_id) if caller_id == contributor_id => Ok(()),
                _ => Err(SubscriptionError::Forbidden),
            }
        },
        _ => Err(SubscriptionError::UnknownChannel),
    }
}

#[cfg(test)]
fn can_subscribe(principal: &AuthPrincipal, channel: &str) -> bool {
    authorize_subscription(principal, channel).is_ok()
}

// ---------------------------------------------------------------------------
// WebSocket actor
// ---------------------------------------------------------------------------

pub struct WsConn {
    event_tx: broadcast::Sender<WsEvent>,
    /// The authenticated caller's principal (role + identity claims)
    caller_principal: AuthPrincipal,
    /// Channels this client is subscribed to.  An empty set means the client
    /// has not subscribed to anything yet and will receive **no** events (the
    /// backend must explicitly subscribe after connecting).
    subscriptions: FxHashSet<String>,
    /// #2135: the admitted slot; released on drop, whichever way the actor ends.
    ticket: AdmissionTicket,
    /// #2135: the ceilings this connection lives under.
    limits: WsLimits,
    /// #2135: frames sent since the client's last pong; the slow-consumer signal.
    window: OutboundWindow<String>,
    /// #2135: when the client last said anything; the idle-reap signal.
    idle: IdleClock,
}

impl WsConn {
    /// Test constructor: an uncounted ticket on the node gate.
    pub fn new(event_tx: broadcast::Sender<WsEvent>, caller_principal: AuthPrincipal) -> Self {
        Self::with_ticket(event_tx, caller_principal, AdmissionTicket::uncounted(ws_gate()))
    }

    pub fn with_ticket(
        event_tx: broadcast::Sender<WsEvent>,
        caller_principal: AuthPrincipal,
        ticket: AdmissionTicket,
    ) -> Self {
        let limits = *ticket.gate.limits();
        Self {
            event_tx,
            caller_principal,
            subscriptions: FxHashSet::default(),
            ticket,
            limits,
            window: OutboundWindow::default(),
            idle: IdleClock::default(),
        }
    }

    /// Returns `true` if `event` matches at least one active subscription.
    fn is_subscribed(&self, event: &WsEvent) -> bool {
        let ch = event.channel();
        self.subscriptions.contains(&ch)
    }

    fn try_subscribe(&mut self, channel: &str) -> Result<(), SubscriptionError> {
        // Reject oversized channel names before any auth or hash-set work.
        if channel.len() > MAX_WS_CHANNEL_NAME_LEN {
            return Err(SubscriptionError::UnknownChannel);
        }
        authorize_subscription(&self.caller_principal, channel)?;
        if !self.subscriptions.contains(channel) {
            // #2135: per-connection and node-wide ceilings, independently.
            if self.subscriptions.len() as u32 >= self.limits.max_subscriptions_per_conn {
                return Err(SubscriptionError::LimitReached);
            }
            match self.ticket.gate.try_subscribe(self.subscriptions.len() as u32) {
                Ok(()) => self.ticket.subscriptions += 1,
                Err(WsRefusal::SubscriptionLimit { .. }) => return Err(SubscriptionError::LimitReached),
                Err(_) => return Err(SubscriptionError::NodeLimitReached),
            }
        }
        self.subscriptions.insert(channel.to_string());
        Ok(())
    }

    fn unsubscribe(&mut self, channel: &str) {
        if self.subscriptions.remove(channel) {
            self.ticket.gate.unsubscribe(1);
            self.ticket.subscriptions = self.ticket.subscriptions.saturating_sub(1);
        }
    }
}

impl Actor for WsConn {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        // Emit a `connected` handshake so the launch contract suite and backend
        // bridge can confirm the connection before subscribing.  The `authenticated`
        // field mirrors the backend WS bridge convention.
        let connected = serde_json::json!({
            "event": "connected",
            "payload": { "authenticated": true },
        });
        ctx.text(connected.to_string());

        let mut rx = self.event_tx.subscribe();
        let addr = ctx.address();

        // Forward broadcast events into the actor's mailbox so that
        // subscription filtering runs on the actor thread.
        ctx.spawn(
            async move {
                loop {
                    match rx.recv().await {
                        Ok(event) => {
                            addr.do_send(BroadcastEvent(event));
                        },
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            log::warn!("WS client lagged, skipped {n} events");
                        },
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
            .into_actor(self),
        );

        // #2135: ping on a cadence that reaps an idle client inside `idle_timeout`.
        // A client that answers nothing — no frame, no pong — is closed; its
        // slot and subscriptions are released by the ticket's drop.
        let heartbeat = IdleClock::heartbeat_interval(&self.limits);
        ctx.run_interval(heartbeat, |act, ctx| {
            if act.idle.should_reap(&act.limits) {
                log::info!("WS client idle for {:?}; reaping (#2135)", act.idle.idle_for());
                ctx.close(Some(ws::CloseReason {
                    code: ws::CloseCode::Policy,
                    description: Some("idle timeout".into()),
                }));
                ctx.stop();
            } else {
                ctx.ping(b"hb");
            }
        });

        log::debug!("WebSocket client connected (ip={})", self.ticket.client_ip());
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        log::debug!("WebSocket client disconnected");
    }
}

// ---------------------------------------------------------------------------
// Internal message: a broadcast event arrived
// ---------------------------------------------------------------------------

#[derive(Message)]
#[rtype(result = "()")]
struct BroadcastEvent(WsEvent);

impl Handler<BroadcastEvent> for WsConn {
    type Result = ();

    fn handle(&mut self, msg: BroadcastEvent, ctx: &mut Self::Context) {
        if !self.is_subscribed(&msg.0) {
            return;
        }
        // #2135: frames are counted against the client's last pong. Past the
        // ceiling the client is a slow consumer: disconnect (default) or keep a
        // bounded buffer of the newest frames, logging every drop.
        match self.window.offer(msg.0.to_launch_frame().to_string(), &self.limits) {
            Offer::Send(frame) => ctx.text(frame),
            Offer::Queued => {}
            Offer::QueuedDroppingOldest(_) => log::warn!(
                "WS slow consumer (drop-oldest): dropped the oldest queued frame; {} dropped on this connection",
                self.window.dropped_total()
            ),
            Offer::Disconnect => {
                log::warn!(
                    "WS slow consumer: {} frames unacknowledged, limit {}; disconnecting (#2135)",
                    self.window.unacked(),
                    self.limits.max_outbound_queue
                );
                ctx.close(Some(ws::CloseReason {
                    code: ws::CloseCode::Policy,
                    description: Some("slow consumer".into()),
                }));
                ctx.stop();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Handle raw WebSocket frames from the client
// ---------------------------------------------------------------------------

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for WsConn {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        if msg.is_ok() {
            self.idle.touch();
        }
        match msg {
            Ok(ws::Message::Ping(data)) => ctx.pong(&data),
            Ok(ws::Message::Pong(_)) => {
                // #2135: everything sent so far reached the client; flush what
                // the drop-oldest policy held back.
                for frame in self.window.on_pong(&self.limits) {
                    ctx.text(frame);
                }
            },
            Ok(ws::Message::Text(text)) => {
                match serde_json::from_str::<ClientMessage>(&text) {
                    Ok(ClientMessage::Subscribe { channel }) => {
                        log::debug!("WS subscribe: {channel}");
                        match self.try_subscribe(&channel) {
                            Ok(()) => {
                                // Ack uses the launch-contract envelope:
                                // { "event": "subscribed", "payload": { "channel": "…" } }
                                let ack = serde_json::json!({
                                    "event": "subscribed",
                                    "payload": { "channel": channel },
                                });
                                ctx.text(ack.to_string());
                            },
                            Err(error) => {
                                if error == SubscriptionError::Forbidden {
                                    log::warn!(
                                        "WS subscribe denied for {channel}: unauthorized (role={:?}, identity={:?})",
                                        self.caller_principal.role,
                                        self.caller_principal.identity
                                    );
                                }
                                ctx.text(subscription_error_frame(&channel, error).to_string());
                            },
                        }
                    }
                    Ok(ClientMessage::Unsubscribe { channel }) => {
                        log::debug!("WS unsubscribe: {channel}");
                        self.unsubscribe(&channel);
                        let ack = serde_json::json!({
                            "event": "unsubscribed",
                            "payload": { "channel": channel },
                        });
                        ctx.text(ack.to_string());
                    },
                    Err(_) => {
                        let err = serde_json::json!({
                            "event": "error",
                            "payload": {
                                "message": "unknown message; expected {\"action\":\"subscribe\"|\"unsubscribe\",\"channel\":\"...\"}"
                            }
                        });
                        ctx.text(err.to_string());
                    },
                }
            },
            Ok(ws::Message::Binary(_)) => {
                log::debug!("WS binary message ignored");
            },
            Ok(ws::Message::Close(reason)) => {
                ctx.close(reason);
                ctx.stop();
            },
            Ok(ws::Message::Continuation(_)) => {},
            Ok(ws::Message::Nop) => {},
            Err(e) => {
                log::warn!("WS protocol error: {e}");
                ctx.stop();
            },
        }
    }
}

// ---------------------------------------------------------------------------
// HTTP entry point: GET /ws
// ---------------------------------------------------------------------------

pub async fn ws_index(
    req: HttpRequest,
    stream: web::Payload,
    state: web::Data<crate::state::AppState>,
) -> Result<HttpResponse, Error> {
    let principal = match resolve_ws_principal(&req, state.token_store.as_ref()) {
        Ok(principal) => principal,
        Err(error) => return Ok(ws_auth_error_response(&req, error)),
    };

    log::debug!(
        "WS connection authenticated: role={:?}, identity={:?}",
        principal.role,
        principal.identity
    );

    // #2135: global and per-IP connection ceilings, refused before the upgrade.
    let ticket = match admit(&req, ws_gate()) {
        Ok(ticket) => ticket,
        Err(response) => return Ok(response),
    };

    ws::start(
        WsConn::with_ticket(state.event_tx.clone(), principal, ticket),
        &req,
        stream,
    )
}

// ---------------------------------------------------------------------------
// Helper: create a broadcast channel with a reasonable buffer
// ---------------------------------------------------------------------------

/// Creates a new broadcast channel for WebSocket events.
///
/// The capacity of 256 is large enough to absorb short bursts (e.g. a block
/// with many governance transitions) without dropping events for connected
/// clients, while small enough to bound memory.
pub fn new_event_channel() -> (broadcast::Sender<WsEvent>, broadcast::Receiver<WsEvent>) {
    broadcast::channel(4_096) // raised from 256 for 1K+ station reward events (#841)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{http::header, test::TestRequest};

    fn test_token_store() -> FxHashMap<String, AuthPrincipal> {
        FxHashMap::from_iter([
            (
                "operator-token".to_string(),
                AuthPrincipal::with_role(ApiRole::Operator),
            ),
            (
                "reader-token".to_string(),
                AuthPrincipal::with_role(ApiRole::Reader),
            ),
        ])
    }

    #[test]
    fn new_block_event_channel_is_blocks() {
        let event = WsEvent::NewBlock {
            height: 1,
            hash: "abc".to_string(),
        };
        assert_eq!(event.channel(), "blocks");
    }

    #[test]
    fn governance_event_channel() {
        let event = WsEvent::GovernanceUpdate {
            proposal_id: "prop-1".to_string(),
            status: "passed".to_string(),
        };
        assert_eq!(event.channel(), "governance");
    }

    #[test]
    fn wallet_event_channel() {
        let event = WsEvent::WalletActivity {
            address: "0xabc".to_string(),
            tx_hash: "0x123".to_string(),
            amount: 100,
        };
        assert_eq!(event.channel(), "wallet:0xabc");
    }

    #[test]
    fn subscription_filtering() {
        let (tx, _rx) = new_event_channel();
        let principal = AuthPrincipal::with_role(ApiRole::Reader);
        let mut conn = WsConn::new(tx, principal);
        let block_event = WsEvent::NewBlock {
            height: 1,
            hash: "abc".to_string(),
        };

        // Not subscribed to anything yet
        assert!(!conn.is_subscribed(&block_event));

        // Subscribe to blocks
        conn.subscriptions.insert("blocks".to_string());
        assert!(conn.is_subscribed(&block_event));

        // Governance event should not match
        let gov_event = WsEvent::GovernanceUpdate {
            proposal_id: "p1".to_string(),
            status: "active".to_string(),
        };
        assert!(!conn.is_subscribed(&gov_event));
    }

    #[test]
    fn reward_event_channel() {
        let event = WsEvent::RewardDistributed {
            contributor: "abc123".to_string(),
            amount: 5_000_000_000,
            reason: "weather_contribution".to_string(),
            is_first_reward: true,
            block_height: 42,
        };
        assert_eq!(event.channel(), "rewards:abc123");
    }

    #[test]
    fn reward_event_subscription_filtering() {
        let (tx, _rx) = new_event_channel();
        let principal = AuthPrincipal::with_role(ApiRole::Reader);
        let mut conn = WsConn::new(tx, principal);

        let event = WsEvent::RewardDistributed {
            contributor: "abc123".to_string(),
            amount: 100,
            reason: "coinbase".to_string(),
            is_first_reward: false,
            block_height: 1,
        };

        assert!(!conn.is_subscribed(&event));

        conn.subscriptions.insert("rewards:abc123".to_string());
        assert!(conn.is_subscribed(&event));

        let other = WsEvent::RewardDistributed {
            contributor: "def456".to_string(),
            amount: 100,
            reason: "coinbase".to_string(),
            is_first_reward: false,
            block_height: 1,
        };
        assert!(!conn.is_subscribed(&other));
    }

    // -----------------------------------------------------------------------
    // Launch-contract envelope tests (#1184)
    // -----------------------------------------------------------------------

    #[test]
    fn new_block_launch_frame_shape() {
        let event = WsEvent::NewBlock {
            height: 1001,
            hash: "0xdeadbeef".to_string(),
        };
        let frame = event.to_launch_frame();
        assert_eq!(frame["event"], "new_block");
        assert_eq!(frame["channel"], "blocks");
        assert_eq!(frame["payload"]["height"], 1001_u64);
        assert_eq!(frame["payload"]["hash"], "0xdeadbeef");
    }

    #[test]
    fn governance_launch_frame_shape() {
        let event = WsEvent::GovernanceUpdate {
            proposal_id: "prop-99".to_string(),
            status: "passed".to_string(),
        };
        let frame = event.to_launch_frame();
        assert_eq!(frame["event"], "governance_update");
        assert_eq!(frame["channel"], "governance");
        assert_eq!(frame["payload"]["proposal_id"], "prop-99");
    }

    #[test]
    fn wallet_activity_launch_frame_shape() {
        let event = WsEvent::WalletActivity {
            address: "0xabc".to_string(),
            tx_hash: "0x123".to_string(),
            amount: 500,
        };
        let frame = event.to_launch_frame();
        assert_eq!(frame["event"], "wallet_activity");
        assert_eq!(frame["channel"], "wallet:0xabc");
        assert_eq!(frame["payload"]["amount"], 500_u64);
    }

    #[test]
    fn reward_distributed_launch_frame_shape() {
        let event = WsEvent::RewardDistributed {
            contributor: "abc123".to_string(),
            amount: 5_000_000_000,
            reason: "weather_contribution".to_string(),
            is_first_reward: true,
            block_height: 42,
        };
        let frame = event.to_launch_frame();
        assert_eq!(frame["event"], "reward_distributed");
        assert_eq!(frame["channel"], "rewards:abc123");
        assert_eq!(frame["payload"]["is_first_reward"], true);
    }

    #[test]
    fn launch_frame_has_no_type_field() {
        // The old internal format used "type": "NewBlock"; the launch frame must not.
        let event = WsEvent::NewBlock {
            height: 1,
            hash: "abc".to_string(),
        };
        let frame = event.to_launch_frame();
        assert!(
            frame.get("type").is_none(),
            "frame must not contain a 'type' key"
        );
    }

    #[test]
    fn parse_ws_protocol_token_supports_pair_form() {
        assert_eq!(
            parse_ws_protocol_token("json, bearer, operator-token").as_deref(),
            Some("operator-token")
        );
    }

    #[test]
    fn parse_ws_protocol_token_supports_compact_form() {
        assert_eq!(
            parse_ws_protocol_token("bearer.reader-token").as_deref(),
            Some("reader-token")
        );
    }

    #[test]
    fn parse_ws_protocol_token_compact_form_case_insensitive() {
        // Regression test for #1397: compact bearer prefix should be case-insensitive.
        assert_eq!(
            parse_ws_protocol_token("bearer.lowercase-token").as_deref(),
            Some("lowercase-token")
        );
        assert_eq!(
            parse_ws_protocol_token("Bearer.capitalized-token").as_deref(),
            Some("capitalized-token")
        );
        assert_eq!(
            parse_ws_protocol_token("BEARER.uppercase-token").as_deref(),
            Some("uppercase-token")
        );
    }

    #[test]
    fn resolve_ws_principal_accepts_authorization_header() {
        let req = TestRequest::default()
            .insert_header((header::AUTHORIZATION, "Bearer reader-token"))
            .to_http_request();

        let principal =
            resolve_ws_principal(&req, &test_token_store()).expect("auth header should pass");
        assert_eq!(principal, AuthPrincipal::with_role(ApiRole::Reader));
    }

    #[test]
    fn resolve_ws_principal_accepts_query_token() {
        let req = TestRequest::with_uri("/ws?token=operator-token").to_http_request();

        let principal =
            resolve_ws_principal(&req, &test_token_store()).expect("query token should pass");
        assert_eq!(principal, AuthPrincipal::with_role(ApiRole::Operator));
    }

    #[test]
    fn resolve_ws_principal_accepts_protocol_header_token() {
        let req = TestRequest::default()
            .insert_header((header::SEC_WEBSOCKET_PROTOCOL, "bearer, operator-token"))
            .to_http_request();

        let principal = resolve_ws_principal(&req, &test_token_store())
            .expect("protocol header token should pass");
        assert_eq!(principal, AuthPrincipal::with_role(ApiRole::Operator));
    }

    #[test]
    fn resolve_ws_principal_rejects_missing_token() {
        let req = TestRequest::default().to_http_request();
        let error = resolve_ws_principal(&req, &test_token_store()).expect_err("missing token");
        assert_eq!(error, WsAuthError::MissingToken);
    }

    #[test]
    fn resolve_ws_principal_rejects_invalid_token() {
        let req = TestRequest::default()
            .insert_header((header::AUTHORIZATION, "Bearer wrong-token"))
            .to_http_request();
        let error = resolve_ws_principal(&req, &test_token_store()).expect_err("invalid token");
        assert_eq!(error, WsAuthError::InvalidToken);
    }

    #[test]
    fn reader_can_subscribe_public_channels_only() {
        let (tx, _rx) = new_event_channel();
        let mut conn = WsConn::new(tx, AuthPrincipal::with_role(ApiRole::Reader));

        assert_eq!(conn.try_subscribe("blocks"), Ok(()));
        assert_eq!(conn.try_subscribe("wallet:0xabc"), Err(SubscriptionError::Forbidden));
    }

    #[test]
    fn operator_can_subscribe_private_channels() {
        let (tx, _rx) = new_event_channel();
        let mut conn = WsConn::new(tx, AuthPrincipal::with_role(ApiRole::Operator));

        assert_eq!(conn.try_subscribe("wallet:0xabc"), Ok(()));
        assert_eq!(conn.try_subscribe("rewards:miner-1"), Ok(()));
    }

    #[test]
    fn unknown_channels_are_rejected() {
        let (tx, _rx) = new_event_channel();
        let mut conn = WsConn::new(tx, AuthPrincipal::with_role(ApiRole::Operator));

        assert_eq!(
            conn.try_subscribe("wallet:"),
            Err(SubscriptionError::UnknownChannel)
        );
        assert_eq!(
            conn.try_subscribe("totally-made-up"),
            Err(SubscriptionError::UnknownChannel)
        );
    }

    #[test]
    fn subscription_limit_is_enforced() {
        let (tx, _rx) = new_event_channel();
        let mut conn = WsConn::new(tx, AuthPrincipal::with_role(ApiRole::Operator));

        for index in 0..MAX_SUBSCRIPTIONS_PER_CONN {
            conn.subscriptions.insert(format!("wallet:addr-{index}"));
        }

        assert_eq!(
            conn.try_subscribe("rewards:extra"),
            Err(SubscriptionError::LimitReached)
        );
    }

    #[test]
    fn channel_name_too_long_is_rejected() {
        // A channel name longer than MAX_WS_CHANNEL_NAME_LEN must be rejected
        // as UnknownChannel before any auth or hash-set work is attempted.
        let (tx, _rx) = new_event_channel();
        let mut conn = WsConn::new(tx, AuthPrincipal::with_role(ApiRole::Operator));
        let long_name = "x".repeat(MAX_WS_CHANNEL_NAME_LEN + 1);
        assert_eq!(
            conn.try_subscribe(&long_name),
            Err(SubscriptionError::UnknownChannel)
        );
    }

    // -----------------------------------------------------------------------
    // Authentication and authorization tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_ws_protocol_token_bearer_format() {
        let token = parse_ws_protocol_token("bearer.my-secret-token");
        assert_eq!(token, Some("my-secret-token".to_string()));
    }

    #[test]
    fn parse_ws_protocol_token_case_insensitive() {
        let token = parse_ws_protocol_token("Bearer.my-secret-token");
        assert_eq!(token, Some("my-secret-token".to_string()));

        let token = parse_ws_protocol_token("BEARER.my-secret-token");
        assert_eq!(token, Some("my-secret-token".to_string()));
    }

    #[test]
    fn parse_ws_protocol_token_invalid_format() {
        assert_eq!(parse_ws_protocol_token("invalid"), None);
        assert_eq!(parse_ws_protocol_token("bearer-my-token"), None);
        assert_eq!(parse_ws_protocol_token(""), None);
    }

    #[test]
    fn operator_can_subscribe_to_any_channel() {
        let principal = AuthPrincipal::with_role(ApiRole::Operator);

        assert!(can_subscribe(&principal, "blocks"));
        assert!(can_subscribe(&principal, "governance"));
        assert!(can_subscribe(&principal, "wallet:0xabc123"));
        assert!(can_subscribe(&principal, "rewards:contributor-1"));
    }

    #[test]
    fn reader_can_subscribe_to_public_channels() {
        let principal = AuthPrincipal::with_role(ApiRole::Reader);

        assert!(can_subscribe(&principal, "blocks"));
        assert!(can_subscribe(&principal, "governance"));
    }

    #[test]
    fn reader_cannot_subscribe_to_private_channels() {
        let principal = AuthPrincipal::with_role(ApiRole::Reader);

        assert!(!can_subscribe(&principal, "wallet:0xabc123"));
        assert!(!can_subscribe(&principal, "rewards:contributor-1"));
    }

    #[test]
    fn wallet_identity_can_subscribe_to_own_wallet_channel() {
        let principal = AuthPrincipal::with_identity(
            ApiRole::Reader,
            Identity::wallet("0xabc123"),
        );

        assert!(can_subscribe(&principal, "wallet:0xabc123"));
    }

    #[test]
    fn wallet_identity_case_insensitive_match() {
        let principal = AuthPrincipal::with_identity(
            ApiRole::Reader,
            Identity::wallet("0xABC123"),
        );

        assert!(can_subscribe(&principal, "wallet:0xabc123"));
        assert!(can_subscribe(&principal, "wallet:0xABC123"));
    }

    #[test]
    fn wallet_identity_cannot_subscribe_to_other_wallet_channel() {
        let principal = AuthPrincipal::with_identity(
            ApiRole::Reader,
            Identity::wallet("0xabc123"),
        );

        assert!(!can_subscribe(&principal, "wallet:0xdef456"));
    }

    #[test]
    fn contributor_identity_can_subscribe_to_own_rewards_channel() {
        let principal = AuthPrincipal::with_identity(
            ApiRole::Contributor,
            Identity::contributor("station-1"),
        );

        assert!(can_subscribe(&principal, "rewards:station-1"));
    }

    #[test]
    fn contributor_identity_cannot_subscribe_to_other_rewards_channel() {
        let principal = AuthPrincipal::with_identity(
            ApiRole::Contributor,
            Identity::contributor("station-1"),
        );

        assert!(!can_subscribe(&principal, "rewards:station-2"));
    }

    #[test]
    fn contributor_without_identity_is_fail_closed_for_private_channels() {
        let principal = AuthPrincipal::with_role(ApiRole::Contributor);

        assert!(!can_subscribe(&principal, "wallet:0xabc123"));
        assert!(!can_subscribe(&principal, "rewards:station-1"));
    }

    #[test]
    fn oracle_identity_cannot_subscribe_to_wallet_channels() {
        let principal = AuthPrincipal::with_identity(
            ApiRole::Oracle,
            Identity::oracle_node("oracle-1"),
        );

        assert!(!can_subscribe(&principal, "wallet:0xabc123"));
        assert!(!can_subscribe(&principal, "rewards:contributor-1"));
    }
}
