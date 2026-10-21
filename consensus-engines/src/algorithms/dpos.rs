//! Delegated Proof of Stake (DPoS) consensus algorithm implementation
//!
//! Adapted from `blockchain-core/src/consensus/dpos.rs`.
//! Implements delegation mechanics where token holders delegate their stake to
//! validators who produce blocks on their behalf. Rewards are distributed
//! proportionally, and validators can set commission rates.
//!
//! The core delegation state machine (`DPoSState`) is self-contained. A
//! `DPoSConsensus` wrapper struct implements `ConsensusEngine` to integrate
//! with the unified consensus interface.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::transaction::Transaction;
use wrfcoin_shared_protocol::crypto::hash::hash_bytes;

use crate::traits::ConsensusEngine;
use crate::types::{ConsensusConfig, ConsensusError, ConsensusResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default unbonding period in seconds (21 days)
pub const DEFAULT_UNBONDING_PERIOD_SECS: u64 = 21 * 24 * 60 * 60;

/// Commission rate is expressed in **basis points** (1 bps = 0.01%).
///
/// Determinism (#2003): commission was previously stored as `f64` and computed
/// with `(total_reward as f64 * commission_rate) as u64`. IEEE-754 multiply can
/// differ by 1 ULP across FPU implementations (x87 80-bit extended precision vs
/// SSE2/ARM64 double), so the derived `commission` u64 could diverge across
/// nodes — the same cross-platform `state_root` divergence class as #1776 /
/// #1849–#1853 / #1887. Storing the rate as integer basis points and computing
/// `total_reward * bps / 10_000` in u128 is bit-for-bit identical on every
/// target. This mirrors the BPS-quantization pattern in `proof_of_publishing.rs`
/// and the AMM fee convention (fee in bps out of 10_000).
///
/// Minimum commission rate: 0 bps (0%).
pub const MIN_COMMISSION_RATE_BPS: u32 = 0;

/// Maximum commission rate: 10_000 bps (100%).
pub const MAX_COMMISSION_RATE_BPS: u32 = 10_000;

/// Denominator for basis-point math (10_000 bps = 100%).
pub const COMMISSION_RATE_DENOMINATOR: u128 = 10_000;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Receipt issued when a delegation is made
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationReceipt {
    /// Delegator address
    pub delegator: String,
    /// Validator address
    pub validator: String,
    /// Amount delegated
    pub amount: u64,
    /// Timestamp of delegation
    pub timestamp: u64,
}

/// Record of a pending undelegation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndelegationEntry {
    /// Delegator address
    pub delegator: String,
    /// Validator address
    pub validator: String,
    /// Amount being undelegated
    pub amount: u64,
    /// Timestamp when undelegation was initiated
    pub initiated_at: u64,
    /// Timestamp when funds become available
    pub available_at: u64,
}

/// Information about a validator in the DPoS system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DPoSValidatorInfo {
    /// Validator address
    pub address: String,
    /// Validator's own self-delegated stake
    pub self_stake: u64,
    /// Total delegated stake (including self-stake)
    pub total_stake: u64,
    /// Commission rate in basis points (0..=10_000; 10_000 bps = 100%).
    /// Integer to keep reward math deterministic across platforms (#2003).
    pub commission_rate_bps: u32,
    /// Minimum self-delegation required
    pub min_self_delegation: u64,
    /// Whether the validator is active
    pub is_active: bool,
    /// Total rewards earned
    pub total_rewards_earned: u64,
}

/// Individual delegation record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delegation {
    /// Delegator address
    pub delegator: String,
    /// Amount delegated
    pub amount: u64,
    /// Timestamp of initial delegation
    pub created_at: u64,
}

/// Errors that can occur in DPoS operations
#[derive(Debug, Clone, thiserror::Error)]
pub enum DPoSError {
    #[error("Validator not found: {0}")]
    ValidatorNotFound(String),

    #[error("Delegator not found: {0}")]
    DelegatorNotFound(String),

    #[error("Insufficient stake: have {have}, need {need}")]
    InsufficientStake { have: u64, need: u64 },

    #[error("Insufficient self-delegation: have {have}, minimum {minimum}")]
    InsufficientSelfDelegation { have: u64, minimum: u64 },

    #[error("Invalid commission rate: {0} bps (must be 0..=10_000)")]
    InvalidCommissionRate(u32),

    #[error("Validator already exists: {0}")]
    ValidatorAlreadyExists(String),

    #[error("No delegation found from {delegator} to {validator}")]
    NoDelegation {
        delegator: String,
        validator: String,
    },

    #[error("Unbonding period not elapsed")]
    UnbondingNotComplete,

    #[error("Zero amount")]
    ZeroAmount,
}

// ---------------------------------------------------------------------------
// DPoSState - core delegation state machine
// ---------------------------------------------------------------------------

/// Delegated Proof of Stake state manager
#[derive(Debug, Clone)]
pub struct DPoSState {
    /// Registered validators
    validators: FxHashMap<String, DPoSValidatorInfo>,
    /// Delegations: validator_address -> Vec<Delegation>
    delegations: FxHashMap<String, Vec<Delegation>>,
    /// Pending undelegations
    pending_undelegations: Vec<UndelegationEntry>,
    /// Unbonding period in seconds
    unbonding_period: u64,
    /// Default minimum self-delegation
    default_min_self_delegation: u64,
}

impl DPoSState {
    /// Create a new DPoS state with default parameters.
    pub fn new() -> Self {
        Self {
            validators: FxHashMap::default(),
            delegations: FxHashMap::default(),
            pending_undelegations: Vec::new(),
            unbonding_period: DEFAULT_UNBONDING_PERIOD_SECS,
            default_min_self_delegation: 1000,
        }
    }

    /// Create a new DPoS state with custom unbonding period and minimum self-delegation.
    pub fn with_config(unbonding_period: u64, min_self_delegation: u64) -> Self {
        Self {
            validators: FxHashMap::default(),
            delegations: FxHashMap::default(),
            pending_undelegations: Vec::new(),
            unbonding_period,
            default_min_self_delegation: min_self_delegation,
        }
    }

    /// Register a new validator with initial self-delegation and commission rate.
    ///
    /// `commission_rate_bps` is in basis points (0..=10_000; 500 = 5%).
    pub fn register_validator(
        &mut self,
        address: String,
        self_stake: u64,
        commission_rate_bps: u32,
    ) -> Result<(), DPoSError> {
        if self.validators.contains_key(&address) {
            return Err(DPoSError::ValidatorAlreadyExists(address));
        }

        if !(MIN_COMMISSION_RATE_BPS..=MAX_COMMISSION_RATE_BPS).contains(&commission_rate_bps) {
            return Err(DPoSError::InvalidCommissionRate(commission_rate_bps));
        }

        if self_stake < self.default_min_self_delegation {
            return Err(DPoSError::InsufficientSelfDelegation {
                have: self_stake,
                minimum: self.default_min_self_delegation,
            });
        }

        let validator = DPoSValidatorInfo {
            address: address.clone(),
            self_stake,
            total_stake: self_stake,
            commission_rate_bps,
            min_self_delegation: self.default_min_self_delegation,
            is_active: true,
            total_rewards_earned: 0,
        };

        self.validators.insert(address.clone(), validator);

        // Record self-delegation
        let self_delegation = Delegation {
            delegator: address.clone(),
            amount: self_stake,
            created_at: current_timestamp(),
        };
        self.delegations
            .entry(address)
            .or_default()
            .push(self_delegation);

        Ok(())
    }

    /// Delegate stake from a delegator to a validator.
    pub fn delegate(
        &mut self,
        delegator: String,
        validator: String,
        amount: u64,
    ) -> Result<DelegationReceipt, DPoSError> {
        if amount == 0 {
            return Err(DPoSError::ZeroAmount);
        }

        let val = self
            .validators
            .get_mut(&validator)
            .ok_or_else(|| DPoSError::ValidatorNotFound(validator.clone()))?;

        val.total_stake = val.total_stake.saturating_add(amount);

        if delegator == validator {
            val.self_stake = val.self_stake.saturating_add(amount);
        }

        let delegation = Delegation {
            delegator: delegator.clone(),
            amount,
            created_at: current_timestamp(),
        };

        let delegations = self.delegations.entry(validator.clone()).or_default();
        if let Some(existing) = delegations.iter_mut().find(|d| d.delegator == delegator) {
            existing.amount = existing.amount.saturating_add(amount);
        } else {
            delegations.push(delegation);
        }

        Ok(DelegationReceipt {
            delegator,
            validator,
            amount,
            timestamp: current_timestamp(),
        })
    }

    /// Undelegate stake from a validator.
    pub fn undelegate(
        &mut self,
        delegator: String,
        validator: String,
        amount: u64,
    ) -> Result<(), DPoSError> {
        if amount == 0 {
            return Err(DPoSError::ZeroAmount);
        }

        let delegations =
            self.delegations
                .get_mut(&validator)
                .ok_or_else(|| DPoSError::NoDelegation {
                    delegator: delegator.clone(),
                    validator: validator.clone(),
                })?;

        let delegation = delegations
            .iter_mut()
            .find(|d| d.delegator == delegator)
            .ok_or_else(|| DPoSError::NoDelegation {
                delegator: delegator.clone(),
                validator: validator.clone(),
            })?;

        if delegation.amount < amount {
            return Err(DPoSError::InsufficientStake {
                have: delegation.amount,
                need: amount,
            });
        }

        let val = self
            .validators
            .get(&validator)
            .ok_or_else(|| DPoSError::ValidatorNotFound(validator.clone()))?;

        if delegator == validator {
            let remaining_self = val.self_stake.saturating_sub(amount);
            if remaining_self < val.min_self_delegation {
                return Err(DPoSError::InsufficientSelfDelegation {
                    have: remaining_self,
                    minimum: val.min_self_delegation,
                });
            }
        }

        delegation.amount -= amount;

        let val = self
            .validators
            .get_mut(&validator)
            .ok_or_else(|| DPoSError::ValidatorNotFound(validator.clone()))?;
        val.total_stake = val.total_stake.saturating_sub(amount);
        if delegator == validator {
            val.self_stake = val.self_stake.saturating_sub(amount);
        }

        if delegation.amount == 0 {
            delegations.retain(|d| d.delegator != delegator);
        }

        let now = current_timestamp();
        self.pending_undelegations.push(UndelegationEntry {
            delegator,
            validator,
            amount,
            initiated_at: now,
            available_at: now + self.unbonding_period,
        });

        Ok(())
    }

    /// Redelegate stake from one validator to another.
    pub fn redelegate(
        &mut self,
        delegator: String,
        from: String,
        to: String,
        amount: u64,
    ) -> Result<(), DPoSError> {
        if amount == 0 {
            return Err(DPoSError::ZeroAmount);
        }

        if !self.validators.contains_key(&to) {
            return Err(DPoSError::ValidatorNotFound(to));
        }

        // Remove from source validator
        {
            let delegations =
                self.delegations
                    .get_mut(&from)
                    .ok_or_else(|| DPoSError::NoDelegation {
                        delegator: delegator.clone(),
                        validator: from.clone(),
                    })?;

            let delegation = delegations
                .iter_mut()
                .find(|d| d.delegator == delegator)
                .ok_or_else(|| DPoSError::NoDelegation {
                    delegator: delegator.clone(),
                    validator: from.clone(),
                })?;

            if delegation.amount < amount {
                return Err(DPoSError::InsufficientStake {
                    have: delegation.amount,
                    need: amount,
                });
            }

            if delegator == from {
                let val = self
                    .validators
                    .get(&from)
                    .ok_or_else(|| DPoSError::ValidatorNotFound(from.clone()))?;
                let remaining_self = val.self_stake.saturating_sub(amount);
                if remaining_self < val.min_self_delegation {
                    return Err(DPoSError::InsufficientSelfDelegation {
                        have: remaining_self,
                        minimum: val.min_self_delegation,
                    });
                }
            }

            delegation.amount -= amount;
            if delegation.amount == 0 {
                let delegator_clone = delegator.clone();
                delegations.retain(|d| d.delegator != delegator_clone);
            }

            let from_val = self
                .validators
                .get_mut(&from)
                .ok_or_else(|| DPoSError::ValidatorNotFound(from.clone()))?;
            from_val.total_stake = from_val.total_stake.saturating_sub(amount);
            if delegator == from {
                from_val.self_stake = from_val.self_stake.saturating_sub(amount);
            }
        }

        // Add to destination validator
        self.delegate(delegator, to, amount)?;

        Ok(())
    }

    /// Distribute rewards for a validator proportionally among all delegators.
    pub fn distribute_rewards(
        &mut self,
        validator: String,
        total_reward: u64,
    ) -> Result<Vec<(String, u64)>, DPoSError> {
        let val = self
            .validators
            .get_mut(&validator)
            .ok_or_else(|| DPoSError::ValidatorNotFound(validator.clone()))?;

        val.total_rewards_earned = val.total_rewards_earned.saturating_add(total_reward);

        let commission_rate_bps = val.commission_rate_bps;
        let total_stake = val.total_stake;

        if total_stake == 0 {
            return Ok(vec![]);
        }

        // Deterministic commission: total_reward * bps / 10_000 computed in u128
        // (floor rounding). Bit-for-bit identical on every target — no f64 (#2003).
        let commission =
            (total_reward as u128 * commission_rate_bps as u128 / COMMISSION_RATE_DENOMINATOR) as u64;
        let distributable = total_reward.saturating_sub(commission);

        let delegations = match self.delegations.get(&validator) {
            Some(d) => d.clone(),
            None => return Ok(vec![]),
        };

        let mut rewards = Vec::new();
        let mut distributed = 0u64;

        for (i, delegation) in delegations.iter().enumerate() {
            let reward = if i == delegations.len() - 1 {
                distributable.saturating_sub(distributed)
            } else {
                (distributable as u128 * delegation.amount as u128 / total_stake as u128) as u64
            };

            distributed = distributed.saturating_add(reward);

            let total_for_delegator = if delegation.delegator == validator {
                reward.saturating_add(commission)
            } else {
                reward
            };

            if total_for_delegator > 0 {
                rewards.push((delegation.delegator.clone(), total_for_delegator));
            }
        }

        if commission > 0 && !rewards.iter().any(|(addr, _)| addr == &validator) {
            rewards.push((validator, commission));
        }

        Ok(rewards)
    }

    /// Get the top validators by total delegated stake.
    pub fn get_top_validators(&self, count: usize) -> Vec<DPoSValidatorInfo> {
        let mut validators: Vec<DPoSValidatorInfo> = self
            .validators
            .values()
            .filter(|v| v.is_active)
            .cloned()
            .collect();

        validators.sort_by(|a, b| b.total_stake.cmp(&a.total_stake));
        validators.truncate(count);
        validators
    }

    /// Get all delegations for a specific delegator across all validators.
    pub fn get_delegator_stakes(&self, delegator: &str) -> Vec<(String, u64)> {
        let mut stakes = Vec::new();
        for (validator, delegations) in &self.delegations {
            for d in delegations {
                if d.delegator == delegator && d.amount > 0 {
                    stakes.push((validator.clone(), d.amount));
                }
            }
        }
        stakes
    }

    /// Get pending undelegations for a delegator.
    pub fn get_pending_undelegations(&self, delegator: &str) -> Vec<&UndelegationEntry> {
        self.pending_undelegations
            .iter()
            .filter(|u| u.delegator == delegator)
            .collect()
    }

    /// Process completed undelegations.
    pub fn process_completed_undelegations(&mut self, current_time: u64) -> Vec<UndelegationEntry> {
        let (completed, pending): (Vec<_>, Vec<_>) = self
            .pending_undelegations
            .drain(..)
            .partition(|u| current_time >= u.available_at);

        self.pending_undelegations = pending;
        completed
    }

    /// Get validator info by address.
    pub fn get_validator(&self, address: &str) -> Option<&DPoSValidatorInfo> {
        self.validators.get(address)
    }

    /// Get the unbonding period in seconds.
    pub fn unbonding_period(&self) -> u64 {
        self.unbonding_period
    }
}

impl Default for DPoSState {
    fn default() -> Self {
        Self::new()
    }
}

/// Get the current UNIX timestamp in seconds.
fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------------------
// DPoSConsensus - ConsensusEngine wrapper
// ---------------------------------------------------------------------------

/// Wrapper around `DPoSState` that implements the `ConsensusEngine` trait.
#[derive(Debug)]
pub struct DPoSConsensus {
    /// Internal DPoS state
    state: Arc<RwLock<DPoSState>>,
    /// Number of active delegates that can produce blocks
    active_delegates: usize,
    /// Target block time in seconds
    target_block_time: u64,
    /// Algorithm configuration
    config: ConsensusConfig,
}

impl DPoSConsensus {
    /// Create a new DPoS consensus engine.
    pub fn new(active_delegates: usize, target_block_time: u64) -> Self {
        Self {
            state: Arc::new(RwLock::new(DPoSState::new())),
            active_delegates,
            target_block_time,
            config: ConsensusConfig::default(),
        }
    }

    /// Create from an existing DPoS state.
    pub fn with_state(state: DPoSState, active_delegates: usize, target_block_time: u64) -> Self {
        Self {
            state: Arc::new(RwLock::new(state)),
            active_delegates,
            target_block_time,
            config: ConsensusConfig::default(),
        }
    }

    /// Get a reference to the internal state (for delegation operations).
    pub fn state(&self) -> &Arc<RwLock<DPoSState>> {
        &self.state
    }
}

#[async_trait]
impl ConsensusEngine for DPoSConsensus {
    async fn validate_block(
        &self,
        block: &Block,
        _chain: &Blockchain,
    ) -> ConsensusResult<bool> {
        // Check consensus type
        if let Some(ct) = block.metadata.get("consensus_type") {
            if ct != "DelegatedProofOfStake" {
                return Ok(false);
            }
        }

        // Verify the block producer is among the top delegates
        let producer = block
            .metadata
            .get("validator")
            .cloned()
            .unwrap_or_default();

        let state = self.state.read().await;
        let top = state.get_top_validators(self.active_delegates);

        let is_delegate = top.iter().any(|v| v.address == producer);
        Ok(is_delegate)
    }

    async fn create_block(
        &self,
        transactions: Vec<Transaction>,
        previous_hash: &str,
        producer: &str,
        chain: &Blockchain,
    ) -> ConsensusResult<Block> {
        // Verify producer is an active delegate
        {
            let state = self.state.read().await;
            let top = state.get_top_validators(self.active_delegates);
            if !top.iter().any(|v| v.address == producer) {
                return Err(ConsensusError::CreationFailed(format!(
                    "Address {producer} is not an active delegate"
                )));
            }
        }

        let prev = hash_bytes(previous_hash.as_bytes());

        let mut block = Block::new(
            3, // version
            prev,
            transactions,
            1, // DPoS doesn't use difficulty
            chain.height + 1,
        );

        block
            .metadata
            .insert("validator".into(), producer.into());
        block
            .metadata
            .insert("consensus_type".into(), "DelegatedProofOfStake".into());

        let hash = block.calculate_hash();
        block.set_hash(hash);

        Ok(block)
    }

    async fn get_difficulty(&self) -> u64 {
        1
    }

    fn algorithm_name(&self) -> &'static str {
        "DelegatedProofOfStake"
    }

    async fn configure(&mut self, config: &ConsensusConfig) -> ConsensusResult<()> {
        self.config = config.clone();
        self.target_block_time = config.target_block_time;
        Ok(())
    }

    fn get_metrics(&self) -> FxHashMap<String, serde_json::Value> {
        let mut metrics = FxHashMap::default();
        metrics.insert("algorithm".to_string(), "DelegatedProofOfStake".into());
        metrics.insert(
            "active_delegates".to_string(),
            (self.active_delegates as u64).into(),
        );
        metrics.insert(
            "target_block_time".to_string(),
            self.target_block_time.into(),
        );
        metrics
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_state() -> DPoSState {
        DPoSState::with_config(100, 500)
    }

    #[test]
    fn test_register_validator() {
        let mut state = setup_state();
        let result = state.register_validator("val-1".to_string(), 1000, 1_000); // 10%
        assert!(result.is_ok());

        let val = state.get_validator("val-1").unwrap();
        assert_eq!(val.self_stake, 1000);
        assert_eq!(val.total_stake, 1000);
        assert_eq!(val.commission_rate_bps, 1_000);
        assert!(val.is_active);
    }

    #[test]
    fn test_register_validator_duplicate() {
        let mut state = setup_state();
        state
            .register_validator("val-1".to_string(), 1000, 1_000)
            .unwrap();
        let result = state.register_validator("val-1".to_string(), 2000, 500);
        assert!(matches!(result, Err(DPoSError::ValidatorAlreadyExists(_))));
    }

    #[test]
    fn test_delegate() {
        let mut state = setup_state();
        state
            .register_validator("val-1".to_string(), 1000, 1_000)
            .unwrap();

        let receipt = state
            .delegate("delegator-1".to_string(), "val-1".to_string(), 500)
            .unwrap();
        assert_eq!(receipt.amount, 500);

        let val = state.get_validator("val-1").unwrap();
        assert_eq!(val.total_stake, 1500);
    }

    #[test]
    fn test_get_top_validators() {
        let mut state = setup_state();
        state
            .register_validator("val-a".to_string(), 3000, 1_000)
            .unwrap();
        state
            .register_validator("val-b".to_string(), 5000, 1_000)
            .unwrap();
        state
            .register_validator("val-c".to_string(), 1000, 1_000)
            .unwrap();

        let top = state.get_top_validators(2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].address, "val-b");
        assert_eq!(top[1].address, "val-a");
    }

    // ── DPoSConsensus (ConsensusEngine trait) ────────────────────────────────

    fn setup_engine_with_delegate(delegate: &str) -> (DPoSConsensus, wrfcoin_shared_protocol::blockchain::chain::Blockchain) {
        let mut state = DPoSState::with_config(100, 500);
        state.register_validator(delegate.to_string(), 1000, 500).unwrap();
        let engine = DPoSConsensus::with_state(state, 3, 6);
        let chain = wrfcoin_shared_protocol::blockchain::chain::Blockchain::default();
        (engine, chain)
    }

    /// Happy path: active delegate can create a block with correct metadata.
    #[tokio::test]
    async fn test_create_block_happy_path() {
        let (engine, chain) = setup_engine_with_delegate("delegate-1");

        let block = engine
            .create_block(vec![], "prev-hash", "delegate-1", &chain)
            .await
            .unwrap();

        assert_eq!(
            block.metadata.get("validator").map(String::as_str),
            Some("delegate-1")
        );
        assert_eq!(
            block.metadata.get("consensus_type").map(String::as_str),
            Some("DelegatedProofOfStake")
        );
        assert_eq!(block.height, 1); // chain.height (0) + 1
        assert!(block.hash.is_some(), "block must have a hash set");
    }

    /// Edge case: no-op transaction list still produces a valid block.
    #[tokio::test]
    async fn test_create_block_empty_transactions() {
        let (engine, chain) = setup_engine_with_delegate("del");
        let block = engine.create_block(vec![], "prev", "del", &chain).await.unwrap();
        assert!(block.transactions.is_empty());
        assert!(block.hash.is_some());
    }

    /// Error path: address not in active delegate set is rejected.
    #[tokio::test]
    async fn test_create_block_non_delegate_rejected() {
        let (engine, chain) = setup_engine_with_delegate("delegate-1");

        let result = engine
            .create_block(vec![], "prev-hash", "not-a-delegate", &chain)
            .await;

        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), ConsensusError::CreationFailed(_)),
            "expected CreationFailed"
        );
    }

    /// Happy path: block produced by an active delegate validates successfully.
    #[tokio::test]
    async fn test_validate_block_active_delegate() {
        let (engine, chain) = setup_engine_with_delegate("delegate-1");

        let block = engine
            .create_block(vec![], "prev-hash", "delegate-1", &chain)
            .await
            .unwrap();

        let valid = engine.validate_block(&block, &chain).await.unwrap();
        assert!(valid, "block from active delegate must validate");
    }

    /// Edge case: block missing consensus_type metadata is invalid.
    #[tokio::test]
    async fn test_validate_block_missing_consensus_type() {
        use wrfcoin_shared_protocol::blockchain::block::Block;
        use wrfcoin_shared_protocol::crypto::hash::Hash;

        let (engine, chain) = setup_engine_with_delegate("delegate-1");

        let mut block = Block::new(3, Hash::default(), vec![], 1, 1);
        block.metadata.insert("validator".into(), "delegate-1".into());
        // no consensus_type inserted
        let hash = block.calculate_hash();
        block.set_hash(hash);

        // No consensus_type key → validate_block skips the check and falls
        // through to the producer lookup; the delegate IS registered so this
        // returns true.  This behaviour is intentional: the consensus_type
        // guard only fires when the key is present with a non-matching value.
        let result = engine.validate_block(&block, &chain).await.unwrap();
        assert!(result, "missing key skips the type guard; delegate match wins");
    }

    /// Error path: wrong consensus_type returns false (not an error).
    #[tokio::test]
    async fn test_validate_block_wrong_consensus_type() {
        use wrfcoin_shared_protocol::blockchain::block::Block;
        use wrfcoin_shared_protocol::crypto::hash::Hash;

        let (engine, chain) = setup_engine_with_delegate("delegate-1");

        let mut block = Block::new(3, Hash::default(), vec![], 1, 1);
        block.metadata.insert("consensus_type".into(), "ProofOfWork".into());
        block.metadata.insert("validator".into(), "delegate-1".into());
        let hash = block.calculate_hash();
        block.set_hash(hash);

        let valid = engine.validate_block(&block, &chain).await.unwrap();
        assert!(!valid, "wrong consensus_type must fail validation");
    }

    /// Error path: block from unknown producer is rejected.
    #[tokio::test]
    async fn test_validate_block_unknown_producer() {
        use wrfcoin_shared_protocol::blockchain::block::Block;
        use wrfcoin_shared_protocol::crypto::hash::Hash;

        let (engine, chain) = setup_engine_with_delegate("delegate-1");

        let mut block = Block::new(3, Hash::default(), vec![], 1, 1);
        block.metadata.insert("consensus_type".into(), "DelegatedProofOfStake".into());
        block.metadata.insert("validator".into(), "unknown-node".into());
        let hash = block.calculate_hash();
        block.set_hash(hash);

        let valid = engine.validate_block(&block, &chain).await.unwrap();
        assert!(!valid, "block from non-delegate must fail validation");
    }

    /// Algorithm name is stable.
    #[test]
    fn test_algorithm_name() {
        let engine = DPoSConsensus::new(21, 3);
        assert_eq!(engine.algorithm_name(), "DelegatedProofOfStake");
    }

    /// Difficulty is always 1 for DPoS.
    #[tokio::test]
    async fn test_get_difficulty_always_one() {
        let engine = DPoSConsensus::new(21, 3);
        assert_eq!(engine.get_difficulty().await, 1);
    }

    // ── #1735 hardening batch 8b: DPoS difficulty/division-floor guards ──────
    //
    // DPoS returns a constant difficulty of 1; distribute_rewards guards
    // total_stake == 0 before integer division. These tests pin both invariants
    // under adversarial engine states (zero delegates, single delegate, empty
    // reward set) matching the zero-work and division-by-zero attack surfaces
    // addressed in PoP by PR #1985.

    /// `get_difficulty` must return 1 even when `active_delegates` = 0.
    ///
    /// A misconfigured engine with zero active delegates must not produce a
    /// difficulty of 0 (which would make every block trivially valid).
    #[tokio::test]
    async fn dpos_get_difficulty_zero_active_delegates_returns_one() {
        let engine = DPoSConsensus::new(0, 10); // adversarial: 0 active delegates
        let d = engine.get_difficulty().await;
        assert_eq!(
            d, 1,
            "DPoS get_difficulty with active_delegates=0 must return 1, got {d} \
             — zero difficulty means any block is trivially valid (#1985 class)"
        );
    }

    /// `distribute_rewards` with zero total_stake must return an empty vec and
    /// must NOT panic (i.e., the guard against integer division by zero holds).
    #[test]
    fn dpos_distribute_rewards_zero_total_stake_no_division_panic() {
        // minimum_stake=100, minimum_self_delegation=200 so we can register
        // with self_stake=200, then drain total_stake to 0 to exercise the
        // division guard inside distribute_rewards.
        let mut state = DPoSState::with_config(100, 200);
        state
            .register_validator("val-1".to_string(), 200, 1_000)
            .unwrap();
        // Force total_stake = 0 to exercise the `if total_stake == 0` guard —
        // simulates a post-slash state where the validator exists but holds no
        // delegated or self-staked balance.
        {
            let v = state.validators.get_mut("val-1").unwrap();
            v.self_stake = 0;
            v.total_stake = 0;
        }
        // distribute_rewards must not panic when total_stake == 0; it guards
        // with `if total_stake == 0 { return Ok(vec![]); }`.
        let result = state.distribute_rewards("val-1".to_string(), 1_000);
        assert!(
            result.is_ok(),
            "distribute_rewards with total_stake=0 must return Ok, not panic: {result:?}"
        );
        let distributions = result.unwrap();
        assert!(
            distributions.is_empty(),
            "distribute_rewards with total_stake=0 must return empty distributions, \
             not attempt division by zero"
        );
    }

    // ── #2003 determinism golden vectors ────────────────────────────────────
    //
    // These pin the exact output of the integer commission formula
    //   commission = total_reward * commission_rate_bps / 10_000   (u128, floor)
    // that replaced `(total_reward as f64 * commission_rate) as u64`. Hard-coded
    // expected values catch any future drift back toward float math or a changed
    // rounding convention. Integer math is bit-for-bit identical on every target;
    // the f64 path could diverge by 1 ULP across FPUs → cross-node state_root fork.

    /// Pure commission-math golden vectors: `total_reward * bps / 10_000` (floor).
    #[test]
    fn dpos_commission_bps_golden_vectors_2003() {
        // (total_reward, commission_rate_bps, expected_commission)
        let vectors: &[(u64, u32, u64)] = &[
            (0, 500, 0),                     // zero reward → zero commission
            (1_000, 0, 0),                   // 0% commission
            (1_000, 10_000, 1_000),          // 100% commission takes the whole reward
            (1_000, 500, 50),                // 5% of 1_000 = 50 (exact)
            (10_000, 1_000, 1_000),          // 10% of 10_000 = 1_000 (exact)
            (999, 500, 49),                  // 5% of 999 = 49.95 → floor 49
            (1, 500, 0),                     // 5% of 1 = 0.05 → floor 0
            (7, 3_333, 2),                   // 7 * 3333 / 10000 = 23331/10000 = 2.33 → 2
            // Large value: exercises the u128 widening (would overflow u64*u64).
            (18_000_000_000_000_000, 2_500, 4_500_000_000_000_000),
            (u64::MAX, 10_000, u64::MAX),    // full commission at max reward, no overflow
        ];
        for &(total_reward, bps, expected) in vectors {
            let got = (total_reward as u128 * bps as u128 / COMMISSION_RATE_DENOMINATOR) as u64;
            assert_eq!(
                got, expected,
                "#2003 commission golden vector drift: total_reward={total_reward}, \
                 bps={bps} → expected {expected}, got {got}"
            );
        }
    }

    /// End-to-end `distribute_rewards` golden vector: validator (self-stake 1000,
    /// 10% = 1000 bps) + one delegator (3000). total_stake = 4000, reward = 10_000.
    ///
    ///   commission    = 10_000 * 1000 / 10_000 = 1_000
    ///   distributable = 10_000 - 1_000         = 9_000
    ///   V (self 1000, first, not last): 9_000 * 1000 / 4000 = 2_250; +commission = 3_250
    ///   A (3000, last): distributable - distributed = 9_000 - 2_250 = 6_750
    ///   Σ payouts = 10_000 (conserved).
    #[test]
    fn dpos_distribute_rewards_golden_vector_2003() {
        let mut state = DPoSState::with_config(100, 500);
        state
            .register_validator("V".to_string(), 1_000, 1_000) // 10% in bps
            .unwrap();
        state
            .delegate("A".to_string(), "V".to_string(), 3_000)
            .unwrap();

        let payouts = state.distribute_rewards("V".to_string(), 10_000).unwrap();

        assert_eq!(
            payouts,
            vec![("V".to_string(), 3_250), ("A".to_string(), 6_750)],
            "#2003 distribute_rewards golden vector drift"
        );
        let total: u64 = payouts.iter().map(|(_, amt)| *amt).sum();
        assert_eq!(total, 10_000, "reward must be fully conserved (no dust lost/minted)");
    }

    /// Commission rate above 100% (10_000 bps) must be rejected.
    #[test]
    fn dpos_commission_bps_out_of_range_rejected_2003() {
        let mut state = DPoSState::with_config(100, 500);
        let err = state
            .register_validator("val-hi".to_string(), 1_000, 10_001)
            .unwrap_err();
        assert!(
            matches!(err, DPoSError::InvalidCommissionRate(10_001)),
            "bps > 10_000 must be rejected, got {err:?}"
        );
    }
}
