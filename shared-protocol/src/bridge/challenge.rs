use anyhow::Result;
use rustc_hash::FxHashMap;

use super::types::{
    ChallengeEntry, ChallengeOutcome, ChallengeWindowConfig, ChallengeWindowState,
    SafetyAuditEntry, SafetyAuditEvent,
};

pub struct ChallengeWindowManager {
    config: ChallengeWindowConfig,
    active_windows: FxHashMap<String, ChallengeWindowState>,
    audit_log: Vec<SafetyAuditEntry>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
enum ChallengeWindowError {
    #[error("SP_BRIDGE_WINDOW_DUPLICATE: challenge window already exists for tx {tx_id}")]
    DuplicateWindow { tx_id: String },
    #[error("SP_BRIDGE_INV_WINDOW_MISSING_AFTER_INSERT: challenge window missing for tx {tx_id}")]
    InvariantWindowMissingAfterInsert { tx_id: String },
    #[error("SP_BRIDGE_WINDOW_NOT_FOUND: no challenge window for tx {tx_id}")]
    WindowNotFound { tx_id: String },
    #[error("SP_BRIDGE_WINDOW_INACTIVE: challenge window for tx {tx_id} is no longer active")]
    WindowInactive { tx_id: String },
    #[error("SP_BRIDGE_WINDOW_EXPIRED: challenge window for tx {tx_id} has expired")]
    WindowExpired { tx_id: String },
}

impl ChallengeWindowManager {
    pub fn new(config: ChallengeWindowConfig) -> Self {
        Self {
            config,
            active_windows: FxHashMap::default(),
            audit_log: Vec::new(),
        }
    }

    pub fn open_window(
        &mut self,
        tx_id: &str,
        chain_id: &str,
        now_ts: i64,
    ) -> Result<&ChallengeWindowState> {
        if self.active_windows.contains_key(tx_id) {
            return Err(ChallengeWindowError::DuplicateWindow {
                tx_id: tx_id.to_string(),
            }
            .into());
        }

        let duration = self.duration_for_chain(chain_id);
        let closes_at = now_ts + duration as i64;

        let state = ChallengeWindowState {
            tx_id: tx_id.to_string(),
            chain_id: chain_id.to_string(),
            opened_at: now_ts,
            closes_at,
            challengers: Vec::new(),
            is_active: true,
        };

        self.audit_log.push(SafetyAuditEntry {
            timestamp: now_ts,
            event: SafetyAuditEvent::ChallengeOpened,
            reason: format!(
                "Challenge window opened for tx {tx_id} on chain {chain_id}, closes at {closes_at}"
            ),
            actor: None,
        });

        self.active_windows.insert(tx_id.to_string(), state);
        self.require_window_after_insert(tx_id)
    }

    pub fn submit_challenge(
        &mut self,
        tx_id: &str,
        challenger_id: &str,
        reason: &str,
        now_ts: i64,
    ) -> Result<()> {
        let window = self.require_window_mut(tx_id)?;

        if !window.is_active {
            return Err(ChallengeWindowError::WindowInactive {
                tx_id: tx_id.to_string(),
            }
            .into());
        }
        if now_ts > window.closes_at {
            return Err(ChallengeWindowError::WindowExpired {
                tx_id: tx_id.to_string(),
            }
            .into());
        }

        window.challengers.push(ChallengeEntry {
            challenger_id: challenger_id.to_string(),
            reason: reason.to_string(),
            timestamp: now_ts,
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
        let window = self.require_window_mut(tx_id)?;

        let outcome = if upheld {
            window.is_active = false;
            ChallengeOutcome::ChallengeUpheld
        } else {
            // Re-open window so the transaction can proceed after challenge rejection
            window.is_active = true;
            ChallengeOutcome::ChallengeRejected
        };

        self.audit_log.push(SafetyAuditEntry {
            timestamp: now_ts,
            event: SafetyAuditEvent::ChallengeResolved,
            reason: format!("Challenge for tx {tx_id} resolved: {outcome:?}. Note: {note}"),
            actor: None,
        });

        Ok(outcome)
    }

    pub fn is_finalization_ready(&self, tx_id: &str, now_ts: i64) -> bool {
        let Some(window) = self.active_windows.get(tx_id) else {
            return false;
        };
        window.is_active && now_ts > window.closes_at && window.challengers.is_empty()
    }

    pub fn get_window(&self, tx_id: &str) -> Option<&ChallengeWindowState> {
        self.active_windows.get(tx_id)
    }

    pub fn active_window_count(&self) -> usize {
        self.active_windows.values().filter(|w| w.is_active).count()
    }

    pub fn duration_for_chain(&self, chain_id: &str) -> u64 {
        self.config
            .per_chain_overrides
            .get(chain_id)
            .copied()
            .unwrap_or(self.config.default_duration_secs)
    }

    pub fn audit_log(&self) -> &[SafetyAuditEntry] {
        &self.audit_log
    }

    fn require_window_after_insert(&self, tx_id: &str) -> Result<&ChallengeWindowState> {
        self.active_windows.get(tx_id).ok_or_else(|| {
            ChallengeWindowError::InvariantWindowMissingAfterInsert {
                tx_id: tx_id.to_string(),
            }
            .into()
        })
    }

    fn require_window_mut(&mut self, tx_id: &str) -> Result<&mut ChallengeWindowState> {
        self.active_windows.get_mut(tx_id).ok_or_else(|| {
            ChallengeWindowError::WindowNotFound {
                tx_id: tx_id.to_string(),
            }
            .into()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> ChallengeWindowConfig {
        ChallengeWindowConfig {
            enabled: true,
            default_duration_secs: 3600,
            per_chain_overrides: FxHashMap::default(),
        }
    }

    #[test]
    fn test_open_window_sets_correct_timestamps() {
        let mut manager = ChallengeWindowManager::new(default_config());
        let window = manager.open_window("tx1", "ethereum", 1000).unwrap();
        assert_eq!(window.opened_at, 1000);
        assert_eq!(window.closes_at, 1000 + 3600);
        assert!(window.is_active);
        assert!(window.challengers.is_empty());
    }

    #[test]
    fn test_finalization_blocked_during_active_window() {
        let mut manager = ChallengeWindowManager::new(default_config());
        manager.open_window("tx1", "ethereum", 1000).unwrap();
        // Within the window period, finalization should not be ready
        assert!(!manager.is_finalization_ready("tx1", 2000));
    }

    #[test]
    fn test_finalization_allowed_after_expiry() {
        let mut manager = ChallengeWindowManager::new(default_config());
        manager.open_window("tx1", "ethereum", 1000).unwrap();
        // After window expires with no challengers, finalization should be ready
        assert!(manager.is_finalization_ready("tx1", 5000));
    }

    #[test]
    fn test_open_window_duplicate_returns_typed_error() {
        let mut manager = ChallengeWindowManager::new(default_config());
        manager.open_window("tx1", "ethereum", 1000).unwrap();
        let err = manager
            .open_window("tx1", "ethereum", 1001)
            .expect_err("duplicate tx id must be rejected");
        let typed = err
            .downcast_ref::<ChallengeWindowError>()
            .expect("error should preserve typed bridge challenge invariant");
        assert_eq!(
            typed,
            &ChallengeWindowError::DuplicateWindow {
                tx_id: "tx1".to_string()
            }
        );
        assert!(err.to_string().contains("SP_BRIDGE_WINDOW_DUPLICATE"));
    }

    #[test]
    fn test_post_insert_lookup_invariant_returns_typed_error_without_panic() {
        let manager = ChallengeWindowManager::new(default_config());
        let err = manager
            .require_window_after_insert("missing-tx")
            .expect_err("missing window should produce deterministic typed error");
        let typed = err
            .downcast_ref::<ChallengeWindowError>()
            .expect("error should preserve typed bridge challenge invariant");
        assert_eq!(
            typed,
            &ChallengeWindowError::InvariantWindowMissingAfterInsert {
                tx_id: "missing-tx".to_string()
            }
        );
        assert!(
            err.to_string()
                .contains("SP_BRIDGE_INV_WINDOW_MISSING_AFTER_INSERT")
        );
    }

    #[test]
    fn test_challenge_submission_records_entry() {
        let mut manager = ChallengeWindowManager::new(default_config());
        manager.open_window("tx1", "ethereum", 1000).unwrap();
        manager
            .submit_challenge("tx1", "validator-1", "suspicious amount", 2000)
            .unwrap();
        let window = manager.get_window("tx1").unwrap();
        assert_eq!(window.challengers.len(), 1);
        assert_eq!(window.challengers[0].challenger_id, "validator-1");
        assert_eq!(window.challengers[0].reason, "suspicious amount");
        // With challengers present, finalization is not ready even after expiry
        assert!(!manager.is_finalization_ready("tx1", 5000));
    }

    #[test]
    fn test_resolve_upheld_deactivates_window() {
        let mut manager = ChallengeWindowManager::new(default_config());
        manager.open_window("tx1", "ethereum", 1000).unwrap();
        manager
            .submit_challenge("tx1", "validator-1", "suspicious", 2000)
            .unwrap();
        let outcome = manager
            .resolve_challenge("tx1", true, "confirmed fraud", 3000)
            .unwrap();
        assert_eq!(outcome, ChallengeOutcome::ChallengeUpheld);
        let window = manager.get_window("tx1").unwrap();
        assert!(!window.is_active);
    }

    #[test]
    fn test_per_chain_duration_override() {
        let mut overrides = FxHashMap::default();
        overrides.insert("polygon".to_string(), 1800u64);
        let config = ChallengeWindowConfig {
            enabled: true,
            default_duration_secs: 3600,
            per_chain_overrides: overrides,
        };
        let mut manager = ChallengeWindowManager::new(config);

        let eth_window = manager.open_window("tx1", "ethereum", 1000).unwrap();
        assert_eq!(eth_window.closes_at, 1000 + 3600);

        let poly_window = manager.open_window("tx2", "polygon", 1000).unwrap();
        assert_eq!(poly_window.closes_at, 1000 + 1800);
    }
}
