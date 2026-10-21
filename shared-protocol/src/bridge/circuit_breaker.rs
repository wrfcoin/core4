use anyhow::{Result, anyhow};

use super::types::{
    BridgeSafetyStatus, CircuitBreakerConfig, HaltTrigger, SafetyAuditEntry, SafetyAuditEvent,
};
use rustc_hash::FxHashMap;

pub struct CircuitBreaker {
    config: CircuitBreakerConfig,
    halted: bool,
    halt_reason: Option<String>,
    halted_at: Option<i64>,
    anomaly_window_start: i64,
    failures_in_window: u32,
    critical_alerts_in_window: u32,
    audit_log: Vec<SafetyAuditEntry>,
}

impl CircuitBreaker {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            config,
            halted: false,
            halt_reason: None,
            halted_at: None,
            anomaly_window_start: 0,
            failures_in_window: 0,
            critical_alerts_in_window: 0,
            audit_log: Vec::new(),
        }
    }

    pub fn record_failure(&mut self, context: &str, now_ts: i64) -> bool {
        self.maybe_reset_window(now_ts);
        self.failures_in_window += 1;

        self.audit_log.push(SafetyAuditEntry {
            timestamp: now_ts,
            event: SafetyAuditEvent::AnomalyFailure,
            reason: format!("Failure recorded: {context}"),
            actor: None,
        });

        if self.failures_in_window >= self.config.failure_threshold {
            self.trigger_halt(
                format!(
                    "Failure threshold exceeded: {} failures in anomaly window (context: {})",
                    self.failures_in_window, context
                ),
                HaltTrigger::AnomalyThresholdExceeded,
                now_ts,
            );
            return true;
        }
        false
    }

    pub fn record_critical_alert(&mut self, alert_type: &str, now_ts: i64) -> bool {
        self.maybe_reset_window(now_ts);
        self.critical_alerts_in_window += 1;

        if self.critical_alerts_in_window >= self.config.critical_alert_threshold {
            self.trigger_halt(
                format!(
                    "Critical alert threshold exceeded: {} alerts (type: {})",
                    self.critical_alerts_in_window, alert_type
                ),
                HaltTrigger::AnomalyThresholdExceeded,
                now_ts,
            );
            return true;
        }
        false
    }

    pub fn trigger_halt(&mut self, reason: String, _trigger: HaltTrigger, now_ts: i64) {
        self.halted = true;
        self.halt_reason = Some(reason.clone());
        self.halted_at = Some(now_ts);

        self.audit_log.push(SafetyAuditEntry {
            timestamp: now_ts,
            event: SafetyAuditEvent::AutoHalt,
            reason,
            actor: None,
        });
    }

    pub fn resume(&mut self, reason: &str, operator: &str, now_ts: i64) -> Result<()> {
        if reason.trim().is_empty() {
            return Err(anyhow!("Resume reason is required"));
        }
        if operator.trim().is_empty() {
            return Err(anyhow!("Operator identity is required for resume"));
        }

        self.halted = false;
        self.halt_reason = None;
        self.halted_at = None;
        self.failures_in_window = 0;
        self.critical_alerts_in_window = 0;
        self.anomaly_window_start = now_ts;

        self.audit_log.push(SafetyAuditEntry {
            timestamp: now_ts,
            event: SafetyAuditEvent::Resume,
            reason: reason.to_string(),
            actor: Some(operator.to_string()),
        });

        Ok(())
    }

    pub fn ensure_not_halted(&self) -> Result<()> {
        if self.halted {
            let reason = self.halt_reason.as_deref().unwrap_or("unknown reason");
            return Err(anyhow!("Bridge halted: {reason}"));
        }
        Ok(())
    }

    pub fn is_halted(&self) -> bool {
        self.halted
    }

    pub fn get_status(&self) -> BridgeSafetyStatus {
        BridgeSafetyStatus {
            halted: self.halted,
            halt_reason: self.halt_reason.clone(),
            halted_at: self.halted_at,
            per_chain_exposure: FxHashMap::default(),
            global_exposure: 0,
            per_chain_cap: 0,
            global_cap: 0,
            anomaly_failures: self.failures_in_window,
            challenge_windows_active: 0,
            stuck_transactions: 0,
        }
    }

    pub fn audit_log(&self) -> &[SafetyAuditEntry] {
        &self.audit_log
    }

    fn maybe_reset_window(&mut self, now_ts: i64) {
        let window_elapsed =
            (now_ts - self.anomaly_window_start) as u64 >= self.config.anomaly_window_secs;
        if window_elapsed {
            self.anomaly_window_start = now_ts;
            self.failures_in_window = 0;
            self.critical_alerts_in_window = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> CircuitBreakerConfig {
        CircuitBreakerConfig {
            enabled: true,
            failure_threshold: 3,
            critical_alert_threshold: 2,
            anomaly_window_secs: 600,
        }
    }

    #[test]
    fn test_not_halted_by_default() {
        let cb = CircuitBreaker::new(default_config());
        assert!(!cb.is_halted());
        assert!(cb.ensure_not_halted().is_ok());
    }

    #[test]
    fn test_failure_threshold_triggers_halt() {
        let mut cb = CircuitBreaker::new(default_config());
        assert!(!cb.record_failure("fail-1", 100));
        assert!(!cb.record_failure("fail-2", 200));
        assert!(cb.record_failure("fail-3", 300)); // third failure triggers halt
        assert!(cb.is_halted());
        assert!(cb.ensure_not_halted().is_err());
    }

    #[test]
    fn test_anomaly_window_resets_after_expiry() {
        let mut cb = CircuitBreaker::new(default_config());
        cb.record_failure("fail-1", 100);
        cb.record_failure("fail-2", 200);
        // Window expires (600 secs) — counters reset
        assert!(!cb.record_failure("fail-after-reset", 800));
        assert!(!cb.is_halted());
    }

    #[test]
    fn test_resume_clears_halt_and_counters() {
        let mut cb = CircuitBreaker::new(default_config());
        cb.record_failure("fail-1", 100);
        cb.record_failure("fail-2", 200);
        cb.record_failure("fail-3", 300);
        assert!(cb.is_halted());

        cb.resume("incident resolved", "operator-1", 400).unwrap();
        assert!(!cb.is_halted());
        assert!(cb.ensure_not_halted().is_ok());
    }

    #[test]
    fn test_resume_requires_reason() {
        let mut cb = CircuitBreaker::new(default_config());
        cb.trigger_halt("test halt".to_string(), HaltTrigger::ManualHalt, 100);
        let result = cb.resume("", "operator-1", 200);
        assert!(result.is_err());
        assert!(cb.is_halted());
    }

    #[test]
    fn test_critical_alert_threshold() {
        let mut cb = CircuitBreaker::new(default_config());
        assert!(!cb.record_critical_alert("suspicious-pattern", 100));
        assert!(cb.record_critical_alert("double-spend-attempt", 200));
        assert!(cb.is_halted());
    }
}
