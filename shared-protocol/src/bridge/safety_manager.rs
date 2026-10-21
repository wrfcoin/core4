use anyhow::Result;

use super::alerting::AlertingHookManager;
use super::challenge::ChallengeWindowManager;
use super::circuit_breaker::CircuitBreaker;
use super::exposure::ExposureTracker;
use super::stuck_detector::StuckStateDetector;
use super::types::{
    BridgeSafetyConfig, BridgeSafetyStatus, ChallengeOutcome, HaltTrigger, SafetyAuditEntry,
    SafetyEvent, StuckTransactionInfo,
};

pub struct BridgeSafetyManager {
    pub challenge_windows: ChallengeWindowManager,
    pub exposure: ExposureTracker,
    pub circuit_breaker: CircuitBreaker,
    pub stuck_detector: StuckStateDetector,
    pub alerting: AlertingHookManager,
}

impl BridgeSafetyManager {
    pub fn new(config: BridgeSafetyConfig) -> Self {
        Self {
            challenge_windows: ChallengeWindowManager::new(config.challenge_window),
            exposure: ExposureTracker::new(config.exposure_caps),
            circuit_breaker: CircuitBreaker::new(config.circuit_breaker),
            stuck_detector: StuckStateDetector::new(config.stuck_state),
            alerting: AlertingHookManager::new(),
        }
    }

    /// Pre-flight check for a bridge operation: verifies the bridge is not
    /// halted and reserves exposure. On exposure breach, triggers an auto-halt.
    pub fn check_bridge_operation(
        &mut self,
        chain_id: &str,
        amount: u64,
        now_ts: i64,
    ) -> Result<()> {
        self.circuit_breaker.ensure_not_halted()?;

        if let Err(e) = self.exposure.check_and_reserve(chain_id, amount, now_ts) {
            let status = self.exposure.get_status();
            let cap = status.per_chain_cap;
            self.alerting.emit_event(SafetyEvent::ExposureCapBreached {
                chain_id: chain_id.to_string(),
                amount,
                cap,
            });

            self.circuit_breaker.trigger_halt(
                format!("Exposure cap breach on {chain_id}: {e}"),
                HaltTrigger::ExposureCapBreach,
                now_ts,
            );
            self.alerting.emit_event(SafetyEvent::BridgeHalted {
                reason: format!("Exposure cap breach on {chain_id}"),
                trigger: HaltTrigger::ExposureCapBreach,
            });

            return Err(e);
        }

        Ok(())
    }

    /// Release exposure after a successful bridge operation.
    pub fn complete_bridge_operation(&mut self, chain_id: &str, amount: u64) {
        self.exposure.release(chain_id, amount);
    }

    /// Release exposure and record a failure for a failed bridge operation.
    pub fn fail_bridge_operation(
        &mut self,
        chain_id: &str,
        amount: u64,
        context: &str,
        now_ts: i64,
    ) {
        self.exposure.release(chain_id, amount);
        let halted = self.circuit_breaker.record_failure(context, now_ts);
        if halted {
            self.alerting.emit_event(SafetyEvent::BridgeHalted {
                reason: format!("Failure threshold exceeded: {context}"),
                trigger: HaltTrigger::AnomalyThresholdExceeded,
            });
        }
    }

    pub fn open_challenge(&mut self, tx_id: &str, chain_id: &str, now_ts: i64) -> Result<()> {
        let window = self
            .challenge_windows
            .open_window(tx_id, chain_id, now_ts)?;
        self.alerting
            .emit_event(SafetyEvent::ChallengeWindowOpened {
                tx_id: tx_id.to_string(),
                chain_id: chain_id.to_string(),
                closes_at: window.closes_at,
            });
        Ok(())
    }

    pub fn submit_challenge(
        &mut self,
        tx_id: &str,
        challenger: &str,
        reason: &str,
        now_ts: i64,
    ) -> Result<()> {
        self.challenge_windows
            .submit_challenge(tx_id, challenger, reason, now_ts)?;
        self.alerting.emit_event(SafetyEvent::ChallengeSubmitted {
            tx_id: tx_id.to_string(),
            challenger_id: challenger.to_string(),
        });
        Ok(())
    }

    pub fn resolve_challenge(
        &mut self,
        tx_id: &str,
        upheld: bool,
        note: &str,
        now_ts: i64,
    ) -> Result<ChallengeOutcome> {
        let outcome = self
            .challenge_windows
            .resolve_challenge(tx_id, upheld, note, now_ts)?;

        self.alerting
            .emit_event(SafetyEvent::ChallengeWindowClosed {
                tx_id: tx_id.to_string(),
                outcome: outcome.clone(),
            });

        if outcome == ChallengeOutcome::ChallengeUpheld {
            self.circuit_breaker
                .record_critical_alert("challenge_upheld", now_ts);
        }

        Ok(outcome)
    }

    pub fn halt(&mut self, reason: &str, trigger: HaltTrigger, now_ts: i64) {
        self.circuit_breaker
            .trigger_halt(reason.to_string(), trigger.clone(), now_ts);
        self.alerting.emit_event(SafetyEvent::BridgeHalted {
            reason: reason.to_string(),
            trigger,
        });
    }

    pub fn resume(&mut self, reason: &str, operator: &str, now_ts: i64) -> Result<()> {
        self.circuit_breaker.resume(reason, operator, now_ts)?;
        self.alerting.emit_event(SafetyEvent::BridgeResumed {
            reason: reason.to_string(),
            operator: operator.to_string(),
        });
        Ok(())
    }

    pub fn scan_stuck(
        &mut self,
        transactions: &[(String, String, i64)],
        now_ts: i64,
    ) -> Vec<StuckTransactionInfo> {
        let stuck = self.stuck_detector.scan_for_stuck(transactions, now_ts);
        for info in &stuck {
            self.alerting
                .emit_event(SafetyEvent::StuckTransactionDetected {
                    tx_id: info.tx_id.clone(),
                    status: info.status.clone(),
                    stuck_duration_secs: info.stuck_duration_secs,
                });
        }
        stuck
    }

    pub fn get_status(&self, _now_ts: i64) -> BridgeSafetyStatus {
        let cb_status = self.circuit_breaker.get_status();
        let exposure_status = self.exposure.get_status();

        BridgeSafetyStatus {
            halted: cb_status.halted,
            halt_reason: cb_status.halt_reason,
            halted_at: cb_status.halted_at,
            per_chain_exposure: exposure_status.per_chain,
            global_exposure: exposure_status.global,
            per_chain_cap: exposure_status.per_chain_cap,
            global_cap: exposure_status.global_cap,
            anomaly_failures: cb_status.anomaly_failures,
            challenge_windows_active: self.challenge_windows.active_window_count(),
            stuck_transactions: 0,
        }
    }

    pub fn get_audit_log(&self) -> Vec<SafetyAuditEntry> {
        let mut entries = Vec::new();
        entries.extend_from_slice(self.circuit_breaker.audit_log());
        entries.extend_from_slice(self.exposure.audit_log());
        entries.extend_from_slice(self.challenge_windows.audit_log());
        entries.sort_by_key(|e| e.timestamp);
        entries
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::BridgeSafetyConfig;
    use super::*;

    fn test_manager() -> BridgeSafetyManager {
        BridgeSafetyManager::new(BridgeSafetyConfig::default())
    }

    #[test]
    fn test_check_operation_halted_bridge_rejected() {
        let mut manager = test_manager();
        manager.halt("manual halt for testing", HaltTrigger::ManualHalt, 1000);

        let result = manager.check_bridge_operation("ethereum", 100, 2000);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Bridge halted"));
    }

    #[test]
    fn test_exposure_breach_triggers_halt() {
        let mut config = BridgeSafetyConfig::default();
        config.exposure_caps.per_chain_cap = 500;
        config.exposure_caps.global_cap = 1000;
        let mut manager = BridgeSafetyManager::new(config);

        // First operation within caps succeeds
        assert!(
            manager
                .check_bridge_operation("ethereum", 400, 1000)
                .is_ok()
        );

        // Second operation exceeds per-chain cap and triggers halt
        let result = manager.check_bridge_operation("ethereum", 200, 2000);
        assert!(result.is_err());
        assert!(manager.circuit_breaker.is_halted());
    }

    #[test]
    fn test_full_halt_resume_cycle() {
        let mut manager = test_manager();
        assert!(!manager.circuit_breaker.is_halted());

        manager.halt("suspicious activity", HaltTrigger::ManualHalt, 1000);
        assert!(manager.circuit_breaker.is_halted());

        // Operations are blocked while halted
        assert!(
            manager
                .check_bridge_operation("ethereum", 100, 2000)
                .is_err()
        );

        // Resume
        manager
            .resume("incident resolved", "operator-1", 3000)
            .unwrap();
        assert!(!manager.circuit_breaker.is_halted());

        // Operations work again
        assert!(
            manager
                .check_bridge_operation("ethereum", 100, 4000)
                .is_ok()
        );
    }
}
