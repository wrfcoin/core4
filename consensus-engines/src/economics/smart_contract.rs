//! Smart Contract Integration Types for Consensus Layer
//!
//! This module provides types and APIs that allow smart contracts to interact with
//! the consensus layer, query consensus data, and access validator information.
//! It implements security measures to prevent attacks and unauthorized access.
//!
//! Extracted from `blockchain-core/src/consensus/smart_contract_integration.rs`.
//! The async API methods that depend on blockchain-core runtime types (ProofOfWork,
//! H256, etc.) have been replaced with type definitions and configuration structs
//! that can be used by downstream crates to build their own API implementations.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Permission levels for smart contract access to consensus data
#[derive(Debug, Clone, PartialEq)]
pub enum ConsensusAccessLevel {
    /// Read-only access to basic consensus information
    ReadOnly,
    /// Access to validator and staking information
    ValidatorAccess,
    /// Access to governance-related consensus data
    GovernanceAccess,
    /// Full access for audited contracts only
    AuditedAccess,
}

/// Rate limiting configuration for smart contract API calls
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum calls per contract per second
    pub max_calls_per_second: u32,
    /// Maximum calls per contract per hour
    pub max_calls_per_hour: u32,
    /// Cooldown period after hitting limits (seconds)
    pub cooldown_seconds: u64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_calls_per_second: 10,
            max_calls_per_hour: 1000,
            cooldown_seconds: 60,
        }
    }
}

/// Consensus information accessible to smart contracts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusInfo {
    /// Current consensus algorithm name
    pub algorithm: String,
    /// Current block height
    pub block_height: u64,
    /// Current difficulty level
    pub difficulty: u64,
    /// Target block time in seconds
    pub target_block_time: u64,
    /// Last block timestamp
    pub last_block_timestamp: u64,
    /// Current validator count (if applicable)
    pub validator_count: Option<u64>,
    /// Total network stake (if applicable)
    pub total_stake: Option<u64>,
    /// Current epoch (if applicable)
    pub current_epoch: Option<u64>,
}

/// Validator information for smart contract queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartContractValidatorInfo {
    /// Validator address
    pub address: String,
    /// Validator's stake amount
    pub stake: u64,
    /// Validator's reputation score
    pub reputation: f64,
    /// Number of blocks validated
    pub blocks_validated: u64,
    /// Last active timestamp
    pub last_active: u64,
    /// Delegation information
    pub delegated_stake: u64,
    /// Commission rate (for delegated PoS)
    pub commission_rate: f64,
}

/// Rate limiting state for smart contracts
#[derive(Debug, Clone)]
pub struct RateLimitState {
    /// Calls in current second
    pub calls_this_second: u32,
    /// Calls in current hour
    pub calls_this_hour: u32,
    /// Last call timestamp
    pub last_call_time: u64,
    /// Last reset timestamp for hourly counter
    pub last_hour_reset: u64,
    /// Cooldown expiry timestamp
    pub cooldown_until: Option<u64>,
}

impl Default for RateLimitState {
    fn default() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            calls_this_second: 0,
            calls_this_hour: 0,
            last_call_time: now,
            last_hour_reset: now,
            cooldown_until: None,
        }
    }
}

/// Audit information for contracts
#[derive(Debug, Clone)]
pub struct AuditInfo {
    /// Auditor name/organization
    pub auditor: String,
    /// Audit completion timestamp
    pub audit_date: u64,
    /// Audit score (0-100)
    pub audit_score: u8,
    /// Expiry timestamp for audit validity
    pub expires_at: u64,
    /// Audit report hash
    pub report_hash: String,
}

/// Configuration for consensus API
#[derive(Debug, Clone)]
pub struct ConsensusApiConfig {
    /// Enable Sybil attack protection
    pub enable_sybil_protection: bool,
    /// Maximum validators to return in a single query
    pub max_validators_per_query: usize,
    /// Enable governance proposal access
    pub enable_governance_access: bool,
    /// Require audit for sensitive operations
    pub require_audit_for_sensitive: bool,
}

impl Default for ConsensusApiConfig {
    fn default() -> Self {
        Self {
            enable_sybil_protection: true,
            max_validators_per_query: 100,
            enable_governance_access: true,
            require_audit_for_sensitive: true,
        }
    }
}

/// Error types for smart contract consensus API
#[derive(Debug, Clone)]
pub enum ConsensusApiError {
    /// Rate limit exceeded
    RateLimitExceeded { retry_after_seconds: u64 },
    /// Insufficient permissions
    InsufficientPermissions {
        required: ConsensusAccessLevel,
        current: ConsensusAccessLevel,
    },
    /// Contract not audited for this operation
    NotAudited,
    /// Invalid query parameters
    InvalidQuery(String),
    /// System overloaded
    SystemOverloaded,
    /// Consensus data unavailable
    DataUnavailable,
}

impl std::fmt::Display for ConsensusApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConsensusApiError::RateLimitExceeded {
                retry_after_seconds,
            } => {
                write!(
                    f,
                    "Rate limit exceeded, retry after {retry_after_seconds} seconds"
                )
            }
            ConsensusApiError::InsufficientPermissions { required, current } => {
                write!(
                    f,
                    "Insufficient permissions: required {required:?}, current {current:?}"
                )
            }
            ConsensusApiError::NotAudited => write!(f, "Contract not audited for this operation"),
            ConsensusApiError::InvalidQuery(msg) => write!(f, "Invalid query: {msg}"),
            ConsensusApiError::SystemOverloaded => write!(f, "System overloaded, try again later"),
            ConsensusApiError::DataUnavailable => write!(f, "Consensus data unavailable"),
        }
    }
}

impl std::error::Error for ConsensusApiError {}

/// Network health metrics for smart contracts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkHealthMetrics {
    /// Number of active validators
    pub active_validators: u64,
    /// Average block time in seconds
    pub avg_block_time: f64,
    /// Current transaction throughput
    pub tx_throughput: f64,
    /// Network latency in milliseconds
    pub network_latency_ms: f64,
    /// Consensus participation rate (0.0 - 1.0)
    pub consensus_participation: f64,
}

/// Staking information for an address
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakeInfo {
    /// Address being queried
    pub address: String,
    /// Own stake amount
    pub own_stake: u64,
    /// Delegated stake amount
    pub delegated_stake: u64,
    /// Total stake (own + delegated)
    pub total_stake: u64,
    /// Commission rate for validators
    pub commission_rate: f64,
    /// Whether this address is a validator
    pub is_validator: bool,
}

/// AI/ML computation status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AIComputationStatus {
    /// Computation is pending
    Pending,
    /// Computation is running
    Running,
    /// Computation completed successfully
    Completed,
    /// Computation failed
    Failed,
    /// Computation was canceled
    Canceled,
}

/// AI/ML computation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIComputationResult {
    /// Execution identifier
    pub execution_id: String,
    /// Computation status
    pub status: AIComputationStatus,
    /// Result data (neural network outputs, predictions, etc.)
    pub result_data: Option<serde_json::Value>,
    /// Accuracy score (0.0 to 1.0)
    pub accuracy_score: f64,
    /// Total computation time in milliseconds
    pub computation_time_ms: u64,
    /// Resource usage during computation
    pub resource_usage: AIResourceUsage,
}

/// Resource usage for AI/ML computations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIResourceUsage {
    /// CPU usage percentage
    pub cpu_usage: f64,
    /// Memory usage in MB
    pub memory_mb: u64,
    /// GPU memory usage in MB (if applicable)
    pub gpu_memory_mb: Option<u64>,
    /// Number of processing units used
    pub processing_units_used: u32,
}

/// Neural network model configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeuralModelConfig {
    /// Model name
    pub name: String,
    /// Model type (e.g., "cnn", "lstm", "transformer")
    pub model_type: String,
    /// Model version
    pub version: String,
    /// Input dimensions
    pub input_shape: Vec<u32>,
    /// Output dimensions
    pub output_shape: Vec<u32>,
    /// Model parameters (weights, biases, etc.)
    pub parameters: serde_json::Value,
    /// Training accuracy
    pub training_accuracy: f64,
    /// Validation accuracy
    pub validation_accuracy: f64,
    /// Framework used (e.g., "tensorflow", "pytorch", "onnx")
    pub framework: String,
}

/// Check if a contract has the required permission level.
///
/// Returns `true` if `current_level` is sufficient for `required_level`.
pub fn has_permission(
    current_level: &ConsensusAccessLevel,
    required_level: &ConsensusAccessLevel,
) -> bool {
    match required_level {
        ConsensusAccessLevel::ReadOnly => true,
        ConsensusAccessLevel::ValidatorAccess => matches!(
            current_level,
            ConsensusAccessLevel::ValidatorAccess
                | ConsensusAccessLevel::GovernanceAccess
                | ConsensusAccessLevel::AuditedAccess
        ),
        ConsensusAccessLevel::GovernanceAccess => matches!(
            current_level,
            ConsensusAccessLevel::GovernanceAccess | ConsensusAccessLevel::AuditedAccess
        ),
        ConsensusAccessLevel::AuditedAccess => {
            matches!(current_level, ConsensusAccessLevel::AuditedAccess)
        }
    }
}

/// Check rate limit for a contract, returning an error if limits are exceeded.
///
/// Updates the rate limit state in place and returns `Ok(())` if within limits,
/// or `Err(ConsensusApiError::RateLimitExceeded)` otherwise.
pub fn check_rate_limit(
    state: &mut RateLimitState,
    config: &RateLimitConfig,
) -> Result<(), ConsensusApiError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Check if in cooldown
    if let Some(cooldown_until) = state.cooldown_until {
        if now < cooldown_until {
            return Err(ConsensusApiError::RateLimitExceeded {
                retry_after_seconds: cooldown_until - now,
            });
        } else {
            state.cooldown_until = None;
        }
    }

    // Reset hourly counter if needed
    if now - state.last_hour_reset >= 3600 {
        state.calls_this_hour = 0;
        state.last_hour_reset = now;
    }

    // Reset per-second counter if needed
    if now > state.last_call_time {
        state.calls_this_second = 0;
    }

    // Check rate limits
    if state.calls_this_second >= config.max_calls_per_second
        || state.calls_this_hour >= config.max_calls_per_hour
    {
        state.cooldown_until = Some(now + config.cooldown_seconds);
        return Err(ConsensusApiError::RateLimitExceeded {
            retry_after_seconds: config.cooldown_seconds,
        });
    }

    // Update counters
    state.calls_this_second += 1;
    state.calls_this_hour += 1;
    state.last_call_time = now;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission_hierarchy() {
        // ReadOnly is accessible to everyone
        assert!(has_permission(
            &ConsensusAccessLevel::ReadOnly,
            &ConsensusAccessLevel::ReadOnly
        ));

        // ReadOnly cannot access validator data
        assert!(!has_permission(
            &ConsensusAccessLevel::ReadOnly,
            &ConsensusAccessLevel::ValidatorAccess
        ));

        // ValidatorAccess can access ReadOnly and ValidatorAccess
        assert!(has_permission(
            &ConsensusAccessLevel::ValidatorAccess,
            &ConsensusAccessLevel::ReadOnly
        ));
        assert!(has_permission(
            &ConsensusAccessLevel::ValidatorAccess,
            &ConsensusAccessLevel::ValidatorAccess
        ));

        // AuditedAccess can access everything
        assert!(has_permission(
            &ConsensusAccessLevel::AuditedAccess,
            &ConsensusAccessLevel::ReadOnly
        ));
        assert!(has_permission(
            &ConsensusAccessLevel::AuditedAccess,
            &ConsensusAccessLevel::ValidatorAccess
        ));
        assert!(has_permission(
            &ConsensusAccessLevel::AuditedAccess,
            &ConsensusAccessLevel::GovernanceAccess
        ));
        assert!(has_permission(
            &ConsensusAccessLevel::AuditedAccess,
            &ConsensusAccessLevel::AuditedAccess
        ));
    }

    #[test]
    fn test_rate_limiting() {
        let config = RateLimitConfig {
            max_calls_per_second: 2,
            max_calls_per_hour: 1000,
            cooldown_seconds: 60,
        };
        let mut state = RateLimitState::default();

        // First two calls should succeed
        assert!(check_rate_limit(&mut state, &config).is_ok());
        assert!(check_rate_limit(&mut state, &config).is_ok());

        // Third call should fail
        assert!(check_rate_limit(&mut state, &config).is_err());
    }

    #[test]
    fn test_consensus_api_error_display() {
        let err = ConsensusApiError::RateLimitExceeded {
            retry_after_seconds: 30,
        };
        assert!(err.to_string().contains("30"));

        let err = ConsensusApiError::NotAudited;
        assert!(err.to_string().contains("not audited"));
    }
}
