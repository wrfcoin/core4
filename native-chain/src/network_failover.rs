//! WAN failover detection state machine for LTE/5G contributor node resilience.
//!
//! Monitors primary WAN connectivity via periodic TCP-connect probes and
//! maintains a state machine that tracks whether the node is on its primary
//! link, probing after a suspected outage, running on LTE failover, or fully
//! disconnected.
//!
//! This module does **not** manage OS-level routing — that is handled by
//! NetworkManager or systemd-networkd.  It observes connectivity and exposes
//! state so that other subsystems (P2P, API, weather submission) can adapt
//! their behavior (e.g., defer large syncs when on a metered connection).
//!
//! See `docs/architecture/lte-wan-failover.md` for the full design.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Tunable parameters for the failover monitor.
#[derive(Debug, Clone)]
pub struct FailoverConfig {
    /// How often to probe WAN connectivity.
    pub probe_interval: Duration,
    /// TCP connect timeout for each probe target.
    pub probe_timeout: Duration,
    /// Consecutive primary probe failures before entering `Probing`.
    pub probe_failure_threshold: u32,
    /// Time spent in `Probing` before triggering failover.
    pub grace_period: Duration,
    /// Consecutive primary successes while on failover before switching back.
    pub primary_recovery_threshold: u32,
    /// Consecutive failover probe failures before entering `Disconnected`.
    pub failover_failure_threshold: u32,
    /// TCP endpoints to probe (well-known anycast IPs).
    pub probe_targets: Vec<SocketAddr>,
}

impl Default for FailoverConfig {
    fn default() -> Self {
        Self {
            probe_interval: Duration::from_secs(15),
            probe_timeout: Duration::from_secs(5),
            probe_failure_threshold: 3,
            grace_period: Duration::from_secs(60),
            primary_recovery_threshold: 3,
            failover_failure_threshold: 3,
            probe_targets: vec![
                "1.1.1.1:443".parse().unwrap(),
                "8.8.8.8:443".parse().unwrap(),
            ],
        }
    }
}

// ---------------------------------------------------------------------------
// State machine
// ---------------------------------------------------------------------------

/// WAN failover states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WanState {
    /// Primary WAN is healthy.
    PrimaryOnline,
    /// Primary probe has failed; within grace period before failover.
    Probing,
    /// Switched to LTE/5G backup.
    FailoverActive,
    /// Both primary and failover interfaces are down.
    Disconnected,
}

impl fmt::Display for WanState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PrimaryOnline => write!(f, "PrimaryOnline"),
            Self::Probing => write!(f, "Probing"),
            Self::FailoverActive => write!(f, "FailoverActive"),
            Self::Disconnected => write!(f, "Disconnected"),
        }
    }
}

/// Runtime snapshot of contributor hardware/network capability telemetry.
///
/// Populated by the WAN failover monitor on each probe tick (once wired into
/// `AppState` and spawned as a background task) and by any future
/// hardware-telemetry publisher.  Read by `GET /node/capability`.
///
/// All fields carry conservative defaults so the route is stable before the
/// monitor background task is wired up — callers can treat a fresh
/// `Default::default()` as "no active telemetry yet".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeCapabilitySnapshot {
    /// Current WAN failover state.
    pub wan_state: WanState,
    /// Whether the node is running on a metered (cellular) link.
    pub is_metered: bool,
    /// Total WAN failover activations since node startup.
    pub failover_count: u64,
    /// Gossipsub mesh peers across all topics (0 when P2P is disabled).
    pub mesh_peer_count: usize,
    /// Whether the node is configured to operate as a Wi-Fi access point.
    pub ap_mode_enabled: bool,
}

impl Default for NodeCapabilitySnapshot {
    fn default() -> Self {
        Self {
            wan_state: WanState::PrimaryOnline,
            is_metered: false,
            failover_count: 0,
            mesh_peer_count: 0,
            ap_mode_enabled: false,
        }
    }
}

/// Events emitted by the state machine on transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WanEvent {
    /// Primary WAN recovered from a degraded or failover state.
    PrimaryRecovered,
    /// Entered the probing grace period after primary failures.
    ProbingStarted,
    /// Failover to LTE/5G activated.
    FailoverActivated,
    /// All connectivity lost.
    Disconnected,
}

// ---------------------------------------------------------------------------
// Modem interface trait
// ---------------------------------------------------------------------------

/// Modem registration status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModemRegistration {
    /// Not registered with any network.
    NotRegistered,
    /// Registered on home network.
    Home,
    /// Searching for a network.
    Searching,
    /// Registration denied.
    Denied,
    /// Registered on a roaming network.
    Roaming,
}

/// Abstraction over USB modem control.
///
/// The production implementation calls `mmcli` (ModemManager) or reads sysfs.
/// Test code uses [`MockModem`].
pub trait ModemInterface: Send + Sync {
    /// Check whether the modem hardware is present and recognized.
    fn is_present(&self) -> bool;
    /// Signal strength in dBm (negative). None if no signal or modem absent.
    fn signal_strength_dbm(&self) -> Option<i32>;
    /// Current network registration status.
    fn registration_status(&self) -> ModemRegistration;
    /// Request the modem to establish a data connection.
    fn connect(&self) -> Result<(), String>;
    /// Request the modem to tear down the data connection.
    fn disconnect(&self) -> Result<(), String>;
}

// ---------------------------------------------------------------------------
// Probe result
// ---------------------------------------------------------------------------

/// Result of a single connectivity probe cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeResult {
    /// At least one probe target was reachable.
    Reachable,
    /// All probe targets timed out or refused.
    Unreachable,
}

/// Probe the configured targets via TCP connect.  Returns `Reachable` if any
/// target accepts a connection within the timeout.
pub fn probe_connectivity(targets: &[SocketAddr], timeout: Duration) -> ProbeResult {
    for target in targets {
        if TcpStream::connect_timeout(target, timeout).is_ok() {
            return ProbeResult::Reachable;
        }
    }
    ProbeResult::Unreachable
}

// ---------------------------------------------------------------------------
// Failover state machine
// ---------------------------------------------------------------------------

/// WAN failover monitor state machine.
///
/// Call [`FailoverMonitor::on_probe_result`] after each probe cycle.  The
/// monitor returns any state-transition event so callers can log, emit
/// metrics, or adjust node behavior.
pub struct FailoverMonitor {
    config: FailoverConfig,
    state: WanState,
    /// Consecutive primary probe failures while in PrimaryOnline.
    consecutive_failures: u32,
    /// When the Probing state was entered.
    probing_since: Option<Instant>,
    /// Consecutive primary successes while in FailoverActive.
    recovery_successes: u32,
    /// Consecutive failover probe failures while in FailoverActive.
    consecutive_failover_failures: u32,
    /// Counter of failover activations (lifetime of this monitor instance).
    pub failover_count: u64,
}

impl FailoverMonitor {
    pub fn new(config: FailoverConfig) -> Self {
        Self {
            config,
            state: WanState::PrimaryOnline,
            consecutive_failures: 0,
            probing_since: None,
            recovery_successes: 0,
            consecutive_failover_failures: 0,
            failover_count: 0,
        }
    }

    /// Current state of the WAN failover state machine.
    pub fn state(&self) -> WanState {
        self.state
    }

    /// Whether the node is currently on a metered (cellular) connection.
    pub fn is_metered(&self) -> bool {
        self.state == WanState::FailoverActive
    }

    /// Feed a probe result into the state machine.  Returns `Some(event)` on
    /// a state transition, `None` if the state is unchanged.
    pub fn on_probe_result(&mut self, result: ProbeResult) -> Option<WanEvent> {
        match self.state {
            WanState::PrimaryOnline => self.handle_primary_online(result),
            WanState::Probing => self.handle_probing(result),
            WanState::FailoverActive => self.handle_failover_active(result),
            WanState::Disconnected => self.handle_disconnected(result),
        }
    }

    fn handle_primary_online(&mut self, result: ProbeResult) -> Option<WanEvent> {
        match result {
            ProbeResult::Reachable => {
                self.consecutive_failures = 0;
                None
            }
            ProbeResult::Unreachable => {
                self.consecutive_failures += 1;
                if self.consecutive_failures >= self.config.probe_failure_threshold {
                    self.state = WanState::Probing;
                    self.probing_since = Some(Instant::now());
                    self.consecutive_failures = 0;
                    Some(WanEvent::ProbingStarted)
                } else {
                    None
                }
            }
        }
    }

    fn handle_probing(&mut self, result: ProbeResult) -> Option<WanEvent> {
        match result {
            ProbeResult::Reachable => {
                // Primary recovered during grace period.
                self.state = WanState::PrimaryOnline;
                self.probing_since = None;
                Some(WanEvent::PrimaryRecovered)
            }
            ProbeResult::Unreachable => {
                let elapsed = self
                    .probing_since
                    .map(|t| t.elapsed())
                    .unwrap_or(Duration::ZERO);
                if elapsed >= self.config.grace_period {
                    self.state = WanState::FailoverActive;
                    self.probing_since = None;
                    self.recovery_successes = 0;
                    self.consecutive_failover_failures = 0;
                    self.failover_count += 1;
                    Some(WanEvent::FailoverActivated)
                } else {
                    None
                }
            }
        }
    }

    fn handle_failover_active(&mut self, result: ProbeResult) -> Option<WanEvent> {
        match result {
            ProbeResult::Reachable => {
                // Could be primary recovering OR LTE is working.  We optimistically
                // count successes toward primary recovery.
                self.recovery_successes += 1;
                self.consecutive_failover_failures = 0;
                if self.recovery_successes >= self.config.primary_recovery_threshold {
                    self.state = WanState::PrimaryOnline;
                    self.recovery_successes = 0;
                    Some(WanEvent::PrimaryRecovered)
                } else {
                    None
                }
            }
            ProbeResult::Unreachable => {
                // LTE is also failing — require consecutive failures before
                // declaring full disconnection, mirroring the primary path.
                self.recovery_successes = 0;
                self.consecutive_failover_failures += 1;
                if self.consecutive_failover_failures >= self.config.failover_failure_threshold {
                    self.consecutive_failover_failures = 0;
                    self.state = WanState::Disconnected;
                    Some(WanEvent::Disconnected)
                } else {
                    None
                }
            }
        }
    }

    fn handle_disconnected(&mut self, result: ProbeResult) -> Option<WanEvent> {
        match result {
            ProbeResult::Reachable => {
                // Some connectivity restored — assume primary first.
                self.state = WanState::PrimaryOnline;
                self.consecutive_failures = 0;
                self.recovery_successes = 0;
                Some(WanEvent::PrimaryRecovered)
            }
            ProbeResult::Unreachable => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Mock modem for tests
// ---------------------------------------------------------------------------

/// A controllable mock modem for unit tests.
#[cfg(test)]
pub struct MockModem {
    pub present: bool,
    pub signal: Option<i32>,
    pub registration: ModemRegistration,
    pub connected: bool,
}

#[cfg(test)]
impl Default for MockModem {
    fn default() -> Self {
        Self {
            present: true,
            signal: Some(-67),
            registration: ModemRegistration::Home,
            connected: false,
        }
    }
}

#[cfg(test)]
impl ModemInterface for MockModem {
    fn is_present(&self) -> bool {
        self.present
    }
    fn signal_strength_dbm(&self) -> Option<i32> {
        if self.present {
            self.signal
        } else {
            None
        }
    }
    fn registration_status(&self) -> ModemRegistration {
        if self.present {
            self.registration
        } else {
            ModemRegistration::NotRegistered
        }
    }
    fn connect(&self) -> Result<(), String> {
        if self.present && self.registration == ModemRegistration::Home {
            Ok(())
        } else {
            Err("modem not registered".into())
        }
    }
    fn disconnect(&self) -> Result<(), String> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn default_monitor() -> FailoverMonitor {
        FailoverMonitor::new(FailoverConfig {
            // Use zero grace period for deterministic tests.
            grace_period: Duration::ZERO,
            ..FailoverConfig::default()
        })
    }

    #[test]
    fn starts_in_primary_online() {
        let monitor = default_monitor();
        assert_eq!(monitor.state(), WanState::PrimaryOnline);
        assert!(!monitor.is_metered());
    }

    #[test]
    fn stays_primary_on_reachable_probes() {
        let mut monitor = default_monitor();
        for _ in 0..10 {
            let event = monitor.on_probe_result(ProbeResult::Reachable);
            assert!(event.is_none());
            assert_eq!(monitor.state(), WanState::PrimaryOnline);
        }
    }

    #[test]
    fn enters_probing_after_threshold_failures() {
        let mut monitor = default_monitor();
        // First 2 failures: still PrimaryOnline (threshold = 3).
        assert!(monitor.on_probe_result(ProbeResult::Unreachable).is_none());
        assert!(monitor.on_probe_result(ProbeResult::Unreachable).is_none());
        assert_eq!(monitor.state(), WanState::PrimaryOnline);
        // Third failure: transitions to Probing.
        let event = monitor.on_probe_result(ProbeResult::Unreachable);
        assert_eq!(event, Some(WanEvent::ProbingStarted));
        assert_eq!(monitor.state(), WanState::Probing);
    }

    #[test]
    fn recovers_from_probing_on_success() {
        let mut monitor = default_monitor();
        // Enter probing.
        for _ in 0..3 {
            monitor.on_probe_result(ProbeResult::Unreachable);
        }
        assert_eq!(monitor.state(), WanState::Probing);
        // One success recovers.
        let event = monitor.on_probe_result(ProbeResult::Reachable);
        assert_eq!(event, Some(WanEvent::PrimaryRecovered));
        assert_eq!(monitor.state(), WanState::PrimaryOnline);
    }

    #[test]
    fn failover_activates_after_grace_period() {
        let mut monitor = default_monitor();
        // Enter probing.
        for _ in 0..3 {
            monitor.on_probe_result(ProbeResult::Unreachable);
        }
        assert_eq!(monitor.state(), WanState::Probing);
        // Grace period is zero, so next unreachable triggers failover.
        let event = monitor.on_probe_result(ProbeResult::Unreachable);
        assert_eq!(event, Some(WanEvent::FailoverActivated));
        assert_eq!(monitor.state(), WanState::FailoverActive);
        assert!(monitor.is_metered());
        assert_eq!(monitor.failover_count, 1);
    }

    #[test]
    fn primary_recovers_from_failover_after_threshold_successes() {
        let mut monitor = default_monitor();
        // Drive to FailoverActive.
        for _ in 0..3 {
            monitor.on_probe_result(ProbeResult::Unreachable);
        }
        monitor.on_probe_result(ProbeResult::Unreachable); // grace=0 → failover
        assert_eq!(monitor.state(), WanState::FailoverActive);

        // 2 successes: not enough (threshold = 3).
        assert!(monitor.on_probe_result(ProbeResult::Reachable).is_none());
        assert!(monitor.on_probe_result(ProbeResult::Reachable).is_none());
        assert_eq!(monitor.state(), WanState::FailoverActive);

        // Third success: switch back.
        let event = monitor.on_probe_result(ProbeResult::Reachable);
        assert_eq!(event, Some(WanEvent::PrimaryRecovered));
        assert_eq!(monitor.state(), WanState::PrimaryOnline);
        assert!(!monitor.is_metered());
    }

    #[test]
    fn disconnected_when_failover_also_fails() {
        let mut monitor = default_monitor();
        // Drive to FailoverActive.
        for _ in 0..3 {
            monitor.on_probe_result(ProbeResult::Unreachable);
        }
        monitor.on_probe_result(ProbeResult::Unreachable);
        assert_eq!(monitor.state(), WanState::FailoverActive);

        // Failover also unreachable — requires 3 consecutive failures
        // (mirroring the primary failure threshold).
        assert!(monitor.on_probe_result(ProbeResult::Unreachable).is_none());
        assert_eq!(monitor.state(), WanState::FailoverActive);
        assert!(monitor.on_probe_result(ProbeResult::Unreachable).is_none());
        assert_eq!(monitor.state(), WanState::FailoverActive);
        let event = monitor.on_probe_result(ProbeResult::Unreachable);
        assert_eq!(event, Some(WanEvent::Disconnected));
        assert_eq!(monitor.state(), WanState::Disconnected);
    }

    #[test]
    fn recovers_from_disconnected() {
        let mut monitor = default_monitor();
        // Drive to Disconnected.
        for _ in 0..3 {
            monitor.on_probe_result(ProbeResult::Unreachable);
        }
        monitor.on_probe_result(ProbeResult::Unreachable); // → FailoverActive
        for _ in 0..3 {
            monitor.on_probe_result(ProbeResult::Unreachable); // → Disconnected (after 3)
        }
        assert_eq!(monitor.state(), WanState::Disconnected);

        // Stays disconnected on more failures.
        assert!(monitor.on_probe_result(ProbeResult::Unreachable).is_none());

        // Recovers on success.
        let event = monitor.on_probe_result(ProbeResult::Reachable);
        assert_eq!(event, Some(WanEvent::PrimaryRecovered));
        assert_eq!(monitor.state(), WanState::PrimaryOnline);
    }

    #[test]
    fn failure_counter_resets_on_success() {
        let mut monitor = default_monitor();
        // 2 failures, then a success.
        monitor.on_probe_result(ProbeResult::Unreachable);
        monitor.on_probe_result(ProbeResult::Unreachable);
        monitor.on_probe_result(ProbeResult::Reachable);
        // 2 more failures: should NOT trigger probing (counter was reset).
        assert!(monitor.on_probe_result(ProbeResult::Unreachable).is_none());
        assert!(monitor.on_probe_result(ProbeResult::Unreachable).is_none());
        assert_eq!(monitor.state(), WanState::PrimaryOnline);
    }

    #[test]
    fn recovery_counter_resets_on_failover_failure() {
        let mut monitor = default_monitor();
        // Drive to FailoverActive.
        for _ in 0..3 {
            monitor.on_probe_result(ProbeResult::Unreachable);
        }
        monitor.on_probe_result(ProbeResult::Unreachable);
        assert_eq!(monitor.state(), WanState::FailoverActive);

        // 2 successes, then a failure resets the recovery counter.
        monitor.on_probe_result(ProbeResult::Reachable);
        monitor.on_probe_result(ProbeResult::Reachable);
        monitor.on_probe_result(ProbeResult::Unreachable);
        // Recovery counter was reset; need 3 fresh successes now.
        assert_eq!(monitor.state(), WanState::FailoverActive);
        monitor.on_probe_result(ProbeResult::Reachable);
        monitor.on_probe_result(ProbeResult::Reachable);
        // Only 2 successes — not enough.
        assert_eq!(monitor.state(), WanState::FailoverActive);
    }

    #[test]
    fn failover_failure_counter_resets_on_success() {
        let mut monitor = default_monitor();
        // Drive to FailoverActive.
        for _ in 0..3 {
            monitor.on_probe_result(ProbeResult::Unreachable);
        }
        monitor.on_probe_result(ProbeResult::Unreachable);
        assert_eq!(monitor.state(), WanState::FailoverActive);

        // 2 failover failures, then a success resets the counter.
        monitor.on_probe_result(ProbeResult::Unreachable);
        monitor.on_probe_result(ProbeResult::Unreachable);
        monitor.on_probe_result(ProbeResult::Reachable);
        // 2 more failures: should NOT disconnect (counter was reset).
        monitor.on_probe_result(ProbeResult::Unreachable);
        monitor.on_probe_result(ProbeResult::Unreachable);
        assert_eq!(monitor.state(), WanState::FailoverActive);
    }

    #[test]
    fn failover_count_increments() {
        let mut monitor = default_monitor();
        assert_eq!(monitor.failover_count, 0);

        // First failover.
        for _ in 0..3 {
            monitor.on_probe_result(ProbeResult::Unreachable);
        }
        monitor.on_probe_result(ProbeResult::Unreachable);
        assert_eq!(monitor.failover_count, 1);

        // Recover, then failover again.
        for _ in 0..3 {
            monitor.on_probe_result(ProbeResult::Reachable);
        }
        assert_eq!(monitor.state(), WanState::PrimaryOnline);
        for _ in 0..3 {
            monitor.on_probe_result(ProbeResult::Unreachable);
        }
        monitor.on_probe_result(ProbeResult::Unreachable);
        assert_eq!(monitor.failover_count, 2);
    }

    #[test]
    fn wan_state_display() {
        assert_eq!(WanState::PrimaryOnline.to_string(), "PrimaryOnline");
        assert_eq!(WanState::Probing.to_string(), "Probing");
        assert_eq!(WanState::FailoverActive.to_string(), "FailoverActive");
        assert_eq!(WanState::Disconnected.to_string(), "Disconnected");
    }

    #[test]
    fn wan_state_serializes_to_json() {
        let json = serde_json::to_string(&WanState::FailoverActive).unwrap();
        assert_eq!(json, "\"FailoverActive\"");
        let recovered: WanState = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered, WanState::FailoverActive);
    }

    // --- MockModem tests ---

    #[test]
    fn mock_modem_default_is_present_and_registered() {
        let modem = MockModem::default();
        assert!(modem.is_present());
        assert_eq!(modem.signal_strength_dbm(), Some(-67));
        assert_eq!(modem.registration_status(), ModemRegistration::Home);
        assert!(modem.connect().is_ok());
    }

    #[test]
    fn mock_modem_absent_returns_none() {
        let modem = MockModem {
            present: false,
            ..MockModem::default()
        };
        assert!(!modem.is_present());
        assert_eq!(modem.signal_strength_dbm(), None);
        assert_eq!(modem.registration_status(), ModemRegistration::NotRegistered);
        assert!(modem.connect().is_err());
    }
}
