use anyhow::{Result, anyhow};
use rustc_hash::FxHashMap;

use super::types::{ExposureCapConfig, ExposureStatus, SafetyAuditEntry, SafetyAuditEvent};

pub struct ExposureTracker {
    config: ExposureCapConfig,
    per_chain_exposure: FxHashMap<String, u64>,
    audit_log: Vec<SafetyAuditEntry>,
}

impl ExposureTracker {
    pub fn new(config: ExposureCapConfig) -> Self {
        Self {
            config,
            per_chain_exposure: FxHashMap::default(),
            audit_log: Vec::new(),
        }
    }

    pub fn check_and_reserve(&mut self, chain_id: &str, amount: u64, now_ts: i64) -> Result<()> {
        if let Some(reason) = self.would_exceed_caps(chain_id, amount) {
            self.audit_log.push(SafetyAuditEntry {
                timestamp: now_ts,
                event: SafetyAuditEvent::ExposureCapBreach,
                reason: reason.clone(),
                actor: None,
            });
            return Err(anyhow!("{reason}"));
        }

        let entry = self
            .per_chain_exposure
            .entry(chain_id.to_string())
            .or_insert(0);
        *entry = entry.saturating_add(amount);
        Ok(())
    }

    pub fn release(&mut self, chain_id: &str, amount: u64) {
        if let Some(entry) = self.per_chain_exposure.get_mut(chain_id) {
            *entry = entry.saturating_sub(amount);
        }
    }

    pub fn get_chain_exposure(&self, chain_id: &str) -> u64 {
        self.per_chain_exposure.get(chain_id).copied().unwrap_or(0)
    }

    pub fn get_global_exposure(&self) -> u64 {
        self.per_chain_exposure.values().sum()
    }

    pub fn update_caps(
        &mut self,
        per_chain_cap: u64,
        global_cap: u64,
        updated_by: &str,
        now_ts: i64,
    ) -> Result<()> {
        if per_chain_cap == 0 || global_cap == 0 {
            return Err(anyhow!("Caps must be greater than zero"));
        }

        self.config.per_chain_cap = per_chain_cap;
        self.config.global_cap = global_cap;

        self.audit_log.push(SafetyAuditEntry {
            timestamp: now_ts,
            event: SafetyAuditEvent::CapUpdated,
            reason: format!("Caps updated: per_chain={per_chain_cap}, global={global_cap}"),
            actor: Some(updated_by.to_string()),
        });

        Ok(())
    }

    pub fn get_status(&self) -> ExposureStatus {
        ExposureStatus {
            per_chain: self.per_chain_exposure.clone(),
            global: self.get_global_exposure(),
            per_chain_cap: self.config.per_chain_cap,
            global_cap: self.config.global_cap,
        }
    }

    pub fn would_exceed_caps(&self, chain_id: &str, amount: u64) -> Option<String> {
        let current_chain = self.get_chain_exposure(chain_id);
        let chain_cap = self
            .config
            .per_chain_overrides
            .get(chain_id)
            .copied()
            .unwrap_or(self.config.per_chain_cap);

        if current_chain.saturating_add(amount) > chain_cap {
            return Some(format!(
                "Per-chain exposure cap exceeded for {chain_id}: current={current_chain}, amount={amount}, cap={chain_cap}"
            ));
        }

        let current_global = self.get_global_exposure();
        if current_global.saturating_add(amount) > self.config.global_cap {
            return Some(format!(
                "Global exposure cap exceeded: current={}, amount={}, cap={}",
                current_global, amount, self.config.global_cap
            ));
        }

        None
    }

    pub fn audit_log(&self) -> &[SafetyAuditEntry] {
        &self.audit_log
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> ExposureCapConfig {
        ExposureCapConfig {
            enabled: true,
            per_chain_cap: 1000,
            global_cap: 5000,
            per_chain_overrides: FxHashMap::default(),
        }
    }

    #[test]
    fn test_reserve_within_caps_succeeds() {
        let mut tracker = ExposureTracker::new(default_config());
        assert!(tracker.check_and_reserve("ethereum", 500, 1000).is_ok());
        assert_eq!(tracker.get_chain_exposure("ethereum"), 500);
        assert_eq!(tracker.get_global_exposure(), 500);
    }

    #[test]
    fn test_reserve_exceeds_per_chain_cap_fails() {
        let mut tracker = ExposureTracker::new(default_config());
        tracker.check_and_reserve("ethereum", 800, 1000).unwrap();
        let result = tracker.check_and_reserve("ethereum", 300, 2000);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Per-chain exposure cap exceeded"));
        // Exposure should not have increased on failure
        assert_eq!(tracker.get_chain_exposure("ethereum"), 800);
    }

    #[test]
    fn test_reserve_exceeds_global_cap_fails() {
        let mut tracker = ExposureTracker::new(default_config());
        tracker.check_and_reserve("ethereum", 900, 1000).unwrap();
        tracker.check_and_reserve("polygon", 900, 1000).unwrap();
        tracker.check_and_reserve("bsc", 900, 1000).unwrap();
        tracker.check_and_reserve("avalanche", 900, 1000).unwrap();
        tracker.check_and_reserve("arbitrum", 900, 1000).unwrap();
        // Global is now 4500, adding 600 should exceed global cap of 5000
        let result = tracker.check_and_reserve("optimism", 600, 2000);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Global exposure cap exceeded"));
    }

    #[test]
    fn test_release_reduces_exposure() {
        let mut tracker = ExposureTracker::new(default_config());
        tracker.check_and_reserve("ethereum", 500, 1000).unwrap();
        tracker.release("ethereum", 200);
        assert_eq!(tracker.get_chain_exposure("ethereum"), 300);
        assert_eq!(tracker.get_global_exposure(), 300);
    }

    #[test]
    fn test_governance_cap_update() {
        let mut tracker = ExposureTracker::new(default_config());
        tracker
            .update_caps(2000, 10000, "governance", 1000)
            .unwrap();
        let status = tracker.get_status();
        assert_eq!(status.per_chain_cap, 2000);
        assert_eq!(status.global_cap, 10000);
        assert_eq!(tracker.audit_log().len(), 1);
        assert_eq!(tracker.audit_log()[0].event, SafetyAuditEvent::CapUpdated);
    }
}
