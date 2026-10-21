//! #2135 — WebSocket connection, subscription and queue ceilings.
//!
//! Pure accounting and policy; the actor wiring is `launch_api/websocket.rs`.
//!
//! * [`WsAdmission`] counts open connections node-wide and per client IP, and
//!   subscriptions node-wide, and refuses at each ceiling independently.
//! * [`OutboundWindow`] is the per-connection flow control. Frames written to a
//!   socket the client is not reading accumulate in memory with no signal from
//!   the transport, so the window counts frames sent since the client's last
//!   pong: past `max_outbound_queue` unacknowledged frames the client is a slow
//!   consumer and the policy decides — **disconnect** by default (a client that
//!   silently loses events is quietly wrong, which is worse than an honest
//!   close), or **drop oldest** (bounded buffer, newest kept, gaps logged).
//! * [`IdleClock`] reaps a connection that has sent nothing — not even a pong —
//!   for `idle_timeout`, so an abandoned socket with live subscriptions does
//!   not hold its slot forever.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// What to do with a client that stops draining its socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlowConsumerPolicy {
    /// Keep the newest `max_outbound_queue` frames, drop the oldest, keep going.
    DropOldest,
    /// Close the connection. The default: a client must never be left quietly
    /// missing events it believes it received.
    DisconnectClient,
}

/// The ceilings. Every one is enforced independently of the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WsLimits {
    pub max_connections: u32,
    pub max_per_client_ip: u32,
    pub max_subscriptions_per_conn: u32,
    /// Node-wide, independent of the per-connection cap.
    pub max_subscriptions_total: u32,
    /// Frames sent since the last pong before the slow-consumer policy applies.
    pub max_outbound_queue: usize,
    pub slow_consumer_policy: SlowConsumerPolicy,
    pub idle_timeout: Duration,
}

impl Default for WsLimits {
    fn default() -> Self {
        Self {
            max_connections: 1_024,
            max_per_client_ip: 16,
            max_subscriptions_per_conn: 32,
            max_subscriptions_total: 16_384,
            max_outbound_queue: 256,
            slow_consumer_policy: SlowConsumerPolicy::DisconnectClient,
            idle_timeout: Duration::from_secs(300),
        }
    }
}

impl SlowConsumerPolicy {
    pub fn parse(word: &str) -> Option<Self> {
        match word.trim().to_ascii_lowercase().as_str() {
            "disconnect" | "disconnect-client" | "disconnect_client" => Some(Self::DisconnectClient),
            "drop-oldest" | "drop_oldest" | "drop" => Some(Self::DropOldest),
            _ => None,
        }
    }
}

/// Why a connection or subscription was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsRefusal {
    TooManyConnections { open: u32, max: u32 },
    TooManyFromIp { ip: String, open: u32, max: u32 },
    SubscriptionLimit { max: u32 },
    NodeSubscriptionLimit { total: u32, max: u32 },
}

impl WsRefusal {
    pub fn code(&self) -> &'static str {
        match self {
            Self::TooManyConnections { .. } => "WS_TOO_MANY_CONNECTIONS",
            Self::TooManyFromIp { .. } => "WS_TOO_MANY_FROM_IP",
            Self::SubscriptionLimit { .. } => "WS_SUBSCRIPTION_LIMIT",
            Self::NodeSubscriptionLimit { .. } => "WS_NODE_SUBSCRIPTION_LIMIT",
        }
    }
}

impl fmt::Display for WsRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyConnections { open, max } => write!(f, "{open} WebSocket connections open, limit {max}"),
            Self::TooManyFromIp { ip, open, max } => {
                write!(f, "{open} WebSocket connections open from {ip}, limit {max}")
            }
            Self::SubscriptionLimit { max } => write!(f, "subscription limit of {max} reached"),
            Self::NodeSubscriptionLimit { total, max } => {
                write!(f, "{total} subscriptions open node-wide, limit {max}")
            }
        }
    }
}

/// Node-wide counters. One instance per node, behind a lock in the caller.
#[derive(Debug, Default)]
pub struct WsAdmission {
    connections: u32,
    per_ip: HashMap<String, u32>,
    subscriptions_total: u32,
}

impl WsAdmission {
    /// Admit a connection from `ip`, or say why not. The global and per-IP
    /// ceilings are checked independently; whichever trips first is reported.
    pub fn try_connect(&mut self, ip: &str, limits: &WsLimits) -> Result<(), WsRefusal> {
        if self.connections >= limits.max_connections {
            return Err(WsRefusal::TooManyConnections {
                open: self.connections,
                max: limits.max_connections,
            });
        }
        let from_ip = self.per_ip.get(ip).copied().unwrap_or(0);
        if from_ip >= limits.max_per_client_ip {
            return Err(WsRefusal::TooManyFromIp {
                ip: ip.to_string(),
                open: from_ip,
                max: limits.max_per_client_ip,
            });
        }
        self.connections += 1;
        *self.per_ip.entry(ip.to_string()).or_insert(0) += 1;
        Ok(())
    }

    /// A previously admitted connection from `ip` closed.
    pub fn disconnect(&mut self, ip: &str) {
        self.connections = self.connections.saturating_sub(1);
        if let Some(count) = self.per_ip.get_mut(ip) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.per_ip.remove(ip);
            }
        }
    }

    /// Account one more subscription on a connection that already holds
    /// `on_connection`. Per-connection and node-wide caps are independent.
    pub fn try_subscribe(&mut self, on_connection: u32, limits: &WsLimits) -> Result<(), WsRefusal> {
        if on_connection >= limits.max_subscriptions_per_conn {
            return Err(WsRefusal::SubscriptionLimit {
                max: limits.max_subscriptions_per_conn,
            });
        }
        if self.subscriptions_total >= limits.max_subscriptions_total {
            return Err(WsRefusal::NodeSubscriptionLimit {
                total: self.subscriptions_total,
                max: limits.max_subscriptions_total,
            });
        }
        self.subscriptions_total += 1;
        Ok(())
    }

    pub fn unsubscribe(&mut self, count: u32) {
        self.subscriptions_total = self.subscriptions_total.saturating_sub(count);
    }

    pub fn connections(&self) -> u32 {
        self.connections
    }

    pub fn connections_from(&self, ip: &str) -> u32 {
        self.per_ip.get(ip).copied().unwrap_or(0)
    }

    pub fn subscriptions_total(&self) -> u32 {
        self.subscriptions_total
    }
}

/// What to do with one outbound frame.
#[derive(Debug, PartialEq, Eq)]
pub enum Offer<T> {
    /// Write it now.
    Send(T),
    /// Held until the next pong (drop-oldest policy, window full).
    Queued,
    /// Held, and the oldest held frame was discarded to make room.
    QueuedDroppingOldest(T),
    /// The client is a slow consumer and the policy is to close.
    Disconnect,
}

/// Per-connection flow control keyed on the client's pongs.
#[derive(Debug)]
pub struct OutboundWindow<T> {
    unacked: usize,
    queued: VecDeque<T>,
    dropped_total: u64,
}

impl<T> Default for OutboundWindow<T> {
    fn default() -> Self {
        Self {
            unacked: 0,
            queued: VecDeque::new(),
            dropped_total: 0,
        }
    }
}

impl<T> OutboundWindow<T> {
    /// Offer a frame for delivery.
    pub fn offer(&mut self, frame: T, limits: &WsLimits) -> Offer<T> {
        if self.unacked < limits.max_outbound_queue {
            self.unacked += 1;
            return Offer::Send(frame);
        }
        match limits.slow_consumer_policy {
            SlowConsumerPolicy::DisconnectClient => Offer::Disconnect,
            SlowConsumerPolicy::DropOldest => {
                self.queued.push_back(frame);
                if self.queued.len() > limits.max_outbound_queue {
                    self.dropped_total += 1;
                    let oldest = self.queued.pop_front().expect("non-empty");
                    Offer::QueuedDroppingOldest(oldest)
                } else {
                    Offer::Queued
                }
            }
        }
    }

    /// The client answered a ping: everything sent so far is acknowledged.
    /// Returns the held frames to write now (each counts as sent again).
    pub fn on_pong(&mut self, limits: &WsLimits) -> Vec<T> {
        self.unacked = 0;
        let mut flush = Vec::new();
        while self.unacked < limits.max_outbound_queue {
            match self.queued.pop_front() {
                Some(frame) => {
                    self.unacked += 1;
                    flush.push(frame);
                }
                None => break,
            }
        }
        flush
    }

    pub fn unacked(&self) -> usize {
        self.unacked
    }

    pub fn queued(&self) -> usize {
        self.queued.len()
    }

    pub fn dropped_total(&self) -> u64 {
        self.dropped_total
    }
}

/// When did the client last say anything (frame, ping, pong)?
#[derive(Debug, Clone, Copy)]
pub struct IdleClock {
    last_activity: Instant,
}

impl Default for IdleClock {
    fn default() -> Self {
        Self {
            // determinism-ok: node-local WebSocket idle reaper (monotonic clock) for API connections; decides only when to drop a socket and never reaches block or state bytes
            last_activity: Instant::now(),
        }
    }
}

impl IdleClock {
    pub fn touch(&mut self) {
        // determinism-ok: node-local WebSocket idle reaper, as above; transport liveness only
        self.last_activity = Instant::now();
    }

    pub fn idle_for(&self) -> Duration {
        self.last_activity.elapsed()
    }

    pub fn should_reap(&self, limits: &WsLimits) -> bool {
        self.idle_for() > limits.idle_timeout
    }

    /// How often to ping so an idle client is reaped within `idle_timeout`.
    pub fn heartbeat_interval(limits: &WsLimits) -> Duration {
        (limits.idle_timeout / 3).clamp(Duration::from_secs(1), Duration::from_secs(30))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> WsLimits {
        WsLimits {
            max_connections: 3,
            max_per_client_ip: 2,
            max_subscriptions_per_conn: 2,
            max_subscriptions_total: 3,
            max_outbound_queue: 2,
            slow_consumer_policy: SlowConsumerPolicy::DisconnectClient,
            idle_timeout: Duration::from_millis(20),
        }
    }

    #[test]
    fn default_policy_is_disconnect_and_ceilings_are_positive() {
        let d = WsLimits::default();
        assert_eq!(d.slow_consumer_policy, SlowConsumerPolicy::DisconnectClient);
        assert!(d.max_connections > 0 && d.max_per_client_ip > 0);
        assert!(d.max_subscriptions_total >= d.max_subscriptions_per_conn);
        assert_eq!(SlowConsumerPolicy::parse("drop-oldest"), Some(SlowConsumerPolicy::DropOldest));
        assert_eq!(SlowConsumerPolicy::parse("Disconnect"), Some(SlowConsumerPolicy::DisconnectClient));
        assert_eq!(SlowConsumerPolicy::parse("nope"), None);
    }

    #[test]
    fn global_and_per_ip_connection_caps_are_independent() {
        let l = limits();
        let mut a = WsAdmission::default();
        a.try_connect("10.0.0.1", &l).unwrap();
        a.try_connect("10.0.0.1", &l).unwrap();
        // Per-IP cap trips while the global cap still has room.
        assert_eq!(
            a.try_connect("10.0.0.1", &l),
            Err(WsRefusal::TooManyFromIp { ip: "10.0.0.1".into(), open: 2, max: 2 })
        );
        a.try_connect("10.0.0.2", &l).unwrap();
        // Global cap trips for a fresh IP with per-IP room.
        assert_eq!(a.try_connect("10.0.0.3", &l), Err(WsRefusal::TooManyConnections { open: 3, max: 3 }));
        a.disconnect("10.0.0.1");
        assert_eq!(a.connections(), 2);
        assert_eq!(a.connections_from("10.0.0.1"), 1);
        a.try_connect("10.0.0.3", &l).unwrap();
        a.disconnect("10.0.0.2");
        assert_eq!(a.connections_from("10.0.0.2"), 0);
    }

    #[test]
    fn per_connection_and_node_wide_subscription_caps_are_independent() {
        let l = limits();
        let mut a = WsAdmission::default();
        a.try_subscribe(0, &l).unwrap();
        a.try_subscribe(1, &l).unwrap();
        assert_eq!(a.try_subscribe(2, &l), Err(WsRefusal::SubscriptionLimit { max: 2 }));
        // Another connection with zero subscriptions: node-wide cap (3) trips at the fourth.
        a.try_subscribe(0, &l).unwrap();
        assert_eq!(a.try_subscribe(0, &l), Err(WsRefusal::NodeSubscriptionLimit { total: 3, max: 3 }));
        a.unsubscribe(2);
        assert_eq!(a.subscriptions_total(), 1);
        a.try_subscribe(0, &l).unwrap();
    }

    #[test]
    fn slow_consumer_is_disconnected_once_the_window_fills() {
        let l = limits();
        let mut w = OutboundWindow::default();
        assert_eq!(w.offer("a", &l), Offer::Send("a"));
        assert_eq!(w.offer("b", &l), Offer::Send("b"));
        assert_eq!(w.offer("c", &l), Offer::Disconnect, "third unacked frame: slow consumer");
        assert_eq!(w.unacked(), 2);
        assert!(w.on_pong(&l).is_empty());
        assert_eq!(w.offer("d", &l), Offer::Send("d"));
    }

    #[test]
    fn drop_oldest_keeps_a_bounded_buffer_and_flushes_on_pong() {
        let l = WsLimits {
            slow_consumer_policy: SlowConsumerPolicy::DropOldest,
            ..limits()
        };
        let mut w = OutboundWindow::default();
        assert_eq!(w.offer(1, &l), Offer::Send(1));
        assert_eq!(w.offer(2, &l), Offer::Send(2));
        assert_eq!(w.offer(3, &l), Offer::Queued);
        assert_eq!(w.offer(4, &l), Offer::Queued);
        assert_eq!(w.offer(5, &l), Offer::QueuedDroppingOldest(3));
        assert_eq!(w.queued(), 2);
        assert_eq!(w.dropped_total(), 1);
        assert_eq!(w.on_pong(&l), vec![4, 5]);
        assert_eq!(w.unacked(), 2);
        assert_eq!(w.queued(), 0);
    }

    #[test]
    fn idle_clock_reaps_after_the_timeout_and_heartbeat_fits_inside_it() {
        let l = limits();
        let mut c = IdleClock::default();
        assert!(!c.should_reap(&l));
        std::thread::sleep(Duration::from_millis(30));
        assert!(c.should_reap(&l));
        c.touch();
        assert!(!c.should_reap(&l));
        assert!(IdleClock::heartbeat_interval(&l) <= l.idle_timeout.max(Duration::from_secs(1)));
        assert_eq!(IdleClock::heartbeat_interval(&WsLimits::default()), Duration::from_secs(30));
    }
}
