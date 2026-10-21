use rustc_hash::FxHashMap;

use super::types::{StuckAction, StuckStateConfig, StuckTransactionInfo};

pub struct StuckStateDetector {
    config: StuckStateConfig,
    retry_counts: FxHashMap<String, u32>,
}

impl StuckStateDetector {
    pub fn new(config: StuckStateConfig) -> Self {
        Self {
            config,
            retry_counts: FxHashMap::default(),
        }
    }

    /// Scans a list of transactions for stuck ones.
    ///
    /// Each transaction is represented as `(tx_id, status, timestamp)` to stay
    /// decoupled from any specific bridge transaction type.
    pub fn scan_for_stuck(
        &self,
        transactions: &[(String, String, i64)],
        now_ts: i64,
    ) -> Vec<StuckTransactionInfo> {
        let mut stuck = Vec::new();

        for (tx_id, status, timestamp) in transactions {
            let timeout = self.get_timeout_for_status(status);
            let elapsed = (now_ts - timestamp) as u64;

            if elapsed > timeout {
                stuck.push(StuckTransactionInfo {
                    tx_id: tx_id.clone(),
                    status: status.clone(),
                    stuck_since: *timestamp,
                    stuck_duration_secs: elapsed,
                    retry_count: self.get_retry_count(tx_id),
                });
            }
        }

        stuck
    }

    pub fn get_timeout_for_status(&self, status: &str) -> u64 {
        match status {
            "pending" => self.config.pending_timeout_secs,
            "in_progress" | "processing" => self.config.in_progress_timeout_secs,
            _ => self.config.pending_timeout_secs,
        }
    }

    pub fn attempt_recovery(&mut self, tx_id: &str) -> StuckAction {
        let count = self.retry_counts.entry(tx_id.to_string()).or_insert(0);
        *count += 1;

        if *count > self.config.max_retries {
            StuckAction::Failed
        } else {
            StuckAction::Retried
        }
    }

    pub fn get_retry_count(&self, tx_id: &str) -> u32 {
        self.retry_counts.get(tx_id).copied().unwrap_or(0)
    }

    pub fn clear_retry_count(&mut self, tx_id: &str) {
        self.retry_counts.remove(tx_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> StuckStateConfig {
        StuckStateConfig {
            enabled: true,
            pending_timeout_secs: 1800,
            in_progress_timeout_secs: 900,
            max_retries: 3,
        }
    }

    #[test]
    fn test_detect_stuck_pending() {
        let detector = StuckStateDetector::new(default_config());
        let transactions = vec![(
            "tx1".to_string(),
            "pending".to_string(),
            1000i64, // timestamp
        )];
        // now_ts = 3000 means 2000 seconds elapsed, exceeding 1800s timeout
        let stuck = detector.scan_for_stuck(&transactions, 3000);
        assert_eq!(stuck.len(), 1);
        assert_eq!(stuck[0].tx_id, "tx1");
        assert_eq!(stuck[0].stuck_duration_secs, 2000);
    }

    #[test]
    fn test_no_false_positive_on_recent_tx() {
        let detector = StuckStateDetector::new(default_config());
        let transactions = vec![("tx1".to_string(), "pending".to_string(), 1000i64)];
        // now_ts = 1500 means only 500 seconds elapsed, within 1800s timeout
        let stuck = detector.scan_for_stuck(&transactions, 1500);
        assert!(stuck.is_empty());
    }

    #[test]
    fn test_recovery_increments_retry() {
        let mut detector = StuckStateDetector::new(default_config());
        assert_eq!(detector.get_retry_count("tx1"), 0);

        let action = detector.attempt_recovery("tx1");
        assert_eq!(action, StuckAction::Retried);
        assert_eq!(detector.get_retry_count("tx1"), 1);

        detector.attempt_recovery("tx1");
        detector.attempt_recovery("tx1");
        assert_eq!(detector.get_retry_count("tx1"), 3);
    }

    #[test]
    fn test_fail_after_max_retries() {
        let mut detector = StuckStateDetector::new(default_config());
        detector.attempt_recovery("tx1"); // 1
        detector.attempt_recovery("tx1"); // 2
        detector.attempt_recovery("tx1"); // 3
        let action = detector.attempt_recovery("tx1"); // 4 > max_retries(3)
        assert_eq!(action, StuckAction::Failed);
    }
}
