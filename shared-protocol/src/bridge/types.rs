use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BridgeSafetyConfig {
    pub challenge_window: ChallengeWindowConfig,
    pub exposure_caps: ExposureCapConfig,
    pub circuit_breaker: CircuitBreakerConfig,
    pub stuck_state: StuckStateConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeWindowConfig {
    pub enabled: bool,
    pub default_duration_secs: u64,
    pub per_chain_overrides: FxHashMap<String, u64>,
}

impl Default for ChallengeWindowConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_duration_secs: 3600,
            per_chain_overrides: FxHashMap::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExposureCapConfig {
    pub enabled: bool,
    pub per_chain_cap: u64,
    pub global_cap: u64,
    pub per_chain_overrides: FxHashMap<String, u64>,
}

impl Default for ExposureCapConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            per_chain_cap: 1_000_000,
            global_cap: 10_000_000,
            per_chain_overrides: FxHashMap::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerConfig {
    pub enabled: bool,
    pub failure_threshold: u32,
    pub critical_alert_threshold: u32,
    pub anomaly_window_secs: u64,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            failure_threshold: 5,
            critical_alert_threshold: 3,
            anomaly_window_secs: 600,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StuckStateConfig {
    pub enabled: bool,
    pub pending_timeout_secs: u64,
    pub in_progress_timeout_secs: u64,
    pub max_retries: u32,
}

impl Default for StuckStateConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            pending_timeout_secs: 1800,
            in_progress_timeout_secs: 900,
            max_retries: 3,
        }
    }
}

// ---------------------------------------------------------------------------
// Events & Audit
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SafetyEvent {
    BridgeHalted {
        reason: String,
        trigger: HaltTrigger,
    },
    BridgeResumed {
        reason: String,
        operator: String,
    },
    ExposureCapBreached {
        chain_id: String,
        amount: u64,
        cap: u64,
    },
    ChallengeWindowOpened {
        tx_id: String,
        chain_id: String,
        closes_at: i64,
    },
    ChallengeSubmitted {
        tx_id: String,
        challenger_id: String,
    },
    ChallengeWindowClosed {
        tx_id: String,
        outcome: ChallengeOutcome,
    },
    StuckTransactionDetected {
        tx_id: String,
        status: String,
        stuck_duration_secs: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum HaltTrigger {
    ExposureCapBreach,
    AnomalyThresholdExceeded,
    ChallengeQuorumReached,
    ManualHalt,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChallengeOutcome {
    NoChallenges,
    ChallengeUpheld,
    ChallengeRejected,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StuckAction {
    Escalated,
    Retried,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyAuditEntry {
    pub timestamp: i64,
    pub event: SafetyAuditEvent,
    pub reason: String,
    pub actor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SafetyAuditEvent {
    AutoHalt,
    ManualHalt,
    Resume,
    AnomalyFailure,
    ExposureCapBreach,
    ChallengeOpened,
    ChallengeResolved,
    StuckDetected,
    StuckRecovered,
    CapUpdated,
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeSafetyStatus {
    pub halted: bool,
    pub halt_reason: Option<String>,
    pub halted_at: Option<i64>,
    pub per_chain_exposure: FxHashMap<String, u64>,
    pub global_exposure: u64,
    pub per_chain_cap: u64,
    pub global_cap: u64,
    pub anomaly_failures: u32,
    pub challenge_windows_active: usize,
    pub stuck_transactions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExposureStatus {
    pub per_chain: FxHashMap<String, u64>,
    pub global: u64,
    pub per_chain_cap: u64,
    pub global_cap: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeWindowState {
    pub tx_id: String,
    pub chain_id: String,
    pub opened_at: i64,
    pub closes_at: i64,
    pub challengers: Vec<ChallengeEntry>,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeEntry {
    pub challenger_id: String,
    pub reason: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StuckTransactionInfo {
    pub tx_id: String,
    pub status: String,
    pub stuck_since: i64,
    pub stuck_duration_secs: u64,
    pub retry_count: u32,
}
