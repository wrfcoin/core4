//! Hard Fork Coordination
//!
//! Manages the lifecycle of hard forks from proposal through signaling, lock-in,
//! and activation. Tracks validator signaling for fork support, negotiates peer
//! version compatibility, and provides fork choice rules during transitions.
//! Includes an emergency fork mechanism for critical security patches with
//! accelerated activation.

use serde::{Deserialize, Serialize};
use rustc_hash::FxHashMap;

/// Default signaling threshold for fork lock-in (percentage of blocks).
pub const DEFAULT_SIGNALING_THRESHOLD: f64 = 0.75;

/// Default signaling period in blocks.
pub const DEFAULT_SIGNALING_PERIOD: u64 = 2016;

/// Default emergency fork activation delay in blocks.
pub const DEFAULT_EMERGENCY_ACTIVATION_DELAY: u64 = 100;

/// Errors that can occur during fork coordination.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ForkError {
    #[error("Fork not found: {0}")]
    ForkNotFound(String),

    #[error("Fork already exists: {0}")]
    ForkAlreadyExists(String),

    #[error("Invalid fork transition from {from:?} to {to:?}")]
    InvalidTransition { from: ForkStatus, to: ForkStatus },

    #[error("Fork {0} has not reached signaling threshold")]
    ThresholdNotReached(String),

    #[error("Fork {0} is not locked in")]
    NotLockedIn(String),

    #[error("Incompatible version: peer has {peer_version}, fork requires {required_version}")]
    IncompatibleVersion {
        peer_version: String,
        required_version: String,
    },

    #[error("Activation block {activation} has not been reached (current: {current})")]
    ActivationNotReached { activation: u64, current: u64 },

    #[error("Fork {0} cannot be signaled in current state")]
    CannotSignal(String),

    #[error("Arithmetic overflow when computing fork activation block")]
    ArithmeticOverflow,
}

/// Status of a hard fork in its lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ForkStatus {
    /// Fork has been proposed but signaling has not started.
    Proposed,
    /// Validators are signaling support for the fork.
    Signaling,
    /// Fork has reached the signaling threshold and is locked in for activation.
    Locked,
    /// Fork has been activated at the designated block.
    Active,
    /// Fork activation is complete and all nodes have transitioned.
    Completed,
}

/// Specification of a hard fork.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForkSpec {
    /// Human-readable fork name (e.g., "quantum-shield-upgrade").
    pub fork_name: String,
    /// Block height at which the fork activates.
    pub activation_block: u64,
    /// Minimum node version required for this fork.
    pub required_version: String,
    /// Description of the changes introduced by this fork.
    pub changes_description: String,
    /// Current status of the fork.
    pub status: ForkStatus,
    /// Block height at which signaling started.
    pub signaling_start_block: Option<u64>,
    /// Block height at which the fork was locked in.
    pub locked_at_block: Option<u64>,
    /// Block height at which the fork was activated.
    pub activated_at_block: Option<u64>,
}

impl ForkSpec {
    /// Create a new fork specification.
    pub fn new(
        fork_name: String,
        activation_block: u64,
        required_version: String,
        changes_description: String,
    ) -> Self {
        Self {
            fork_name,
            activation_block,
            required_version,
            changes_description,
            status: ForkStatus::Proposed,
            signaling_start_block: None,
            locked_at_block: None,
            activated_at_block: None,
        }
    }
}

/// An emergency fork with accelerated activation for critical security patches.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmergencyFork {
    /// The fork specification.
    pub spec: ForkSpec,
    /// Reason for the emergency (e.g., security vulnerability description).
    pub reason: String,
    /// Number of blocks before activation (shorter than normal).
    pub activation_delay: u64,
    /// Whether this emergency fork bypasses normal signaling requirements.
    pub bypass_signaling: bool,
}

impl EmergencyFork {
    /// Create a new emergency fork.
    pub fn new(
        fork_name: String,
        required_version: String,
        reason: String,
        current_block: u64,
        activation_delay: u64,
    ) -> Result<Self, ForkError> {
        let activation_block = current_block
            .checked_add(activation_delay)
            .ok_or(ForkError::ArithmeticOverflow)?;

        Ok(Self {
            spec: ForkSpec::new(
                fork_name,
                activation_block,
                required_version,
                format!("EMERGENCY: {reason}"),
            ),
            reason,
            activation_delay,
            bypass_signaling: true,
        })
    }
}

/// Tracks validator signaling for fork support.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SignalingTracker {
    /// Fork name being tracked.
    fork_name: String,
    /// Signaling threshold (fraction of blocks, e.g., 0.75 for 75%).
    threshold: f64,
    /// Signaling period in blocks.
    signaling_period: u64,
    /// Block at which signaling started.
    start_block: u64,
    /// Total blocks produced during the signaling period.
    total_blocks: u64,
    /// Blocks signaling support for the fork.
    signaling_blocks: u64,
    /// Validators that have signaled support.
    signaling_validators: FxHashMap<String, u64>,
}

impl SignalingTracker {
    /// Create a new signaling tracker.
    pub fn new(fork_name: String, threshold: f64, signaling_period: u64, start_block: u64) -> Self {
        Self {
            fork_name,
            threshold,
            signaling_period,
            start_block,
            total_blocks: 0,
            signaling_blocks: 0,
            signaling_validators: FxHashMap::default(),
        }
    }

    /// Record a block, optionally signaling support for the fork.
    pub fn record_block(&mut self, block_height: u64, validator: &str, signals_support: bool) {
        if block_height < self.start_block
            || block_height >= self.start_block + self.signaling_period
        {
            return;
        }

        self.total_blocks += 1;

        if signals_support {
            self.signaling_blocks += 1;
            *self
                .signaling_validators
                .entry(validator.to_string())
                .or_insert(0) += 1;
        }
    }

    /// Get the current signaling percentage (0.0 to 1.0).
    pub fn signaling_percentage(&self) -> f64 {
        if self.total_blocks == 0 {
            return 0.0;
        }
        self.signaling_blocks as f64 / self.total_blocks as f64
    }

    /// Check if the signaling threshold has been reached.
    pub fn threshold_reached(&self) -> bool {
        self.signaling_percentage() >= self.threshold
    }

    /// Check if the signaling period has ended.
    pub fn period_ended(&self, current_block: u64) -> bool {
        current_block >= self.start_block + self.signaling_period
    }

    /// Get the number of unique validators that have signaled.
    pub fn signaling_validator_count(&self) -> usize {
        self.signaling_validators.len()
    }

    /// Get the total blocks counted.
    pub fn total_blocks(&self) -> u64 {
        self.total_blocks
    }

    /// Get the signaling blocks counted.
    pub fn signaling_blocks(&self) -> u64 {
        self.signaling_blocks
    }
}

/// Negotiates peer version compatibility for fork transitions.
#[derive(Debug, Clone)]
pub struct VersionNegotiator {
    /// Current node version.
    local_version: String,
    /// Active forks and their required versions.
    active_forks: FxHashMap<String, String>,
}

impl VersionNegotiator {
    /// Create a new version negotiator.
    pub fn new(local_version: String) -> Self {
        Self {
            local_version,
            active_forks: FxHashMap::default(),
        }
    }

    /// Register an active fork's version requirement.
    pub fn register_fork_requirement(&mut self, fork_name: String, required_version: String) {
        self.active_forks.insert(fork_name, required_version);
    }

    /// Check if a peer version is compatible with all active forks.
    ///
    /// Uses semantic versioning comparison: the peer version must be >= the
    /// required version for each active fork.
    pub fn is_compatible(&self, peer_version: &str) -> Result<(), ForkError> {
        for required in self.active_forks.values() {
            if !Self::version_gte(peer_version, required) {
                return Err(ForkError::IncompatibleVersion {
                    peer_version: peer_version.to_string(),
                    required_version: required.clone(),
                });
            }
        }
        Ok(())
    }

    /// Check if version `a` is greater than or equal to version `b`.
    ///
    /// Performs a simple lexicographic comparison of dot-separated version
    /// components parsed as integers. Falls back to string comparison if
    /// parsing fails.
    fn version_gte(a: &str, b: &str) -> bool {
        let parse_version = |v: &str| -> Vec<u64> {
            v.split('.')
                .filter_map(|part| part.parse::<u64>().ok())
                .collect()
        };

        let va = parse_version(a);
        let vb = parse_version(b);

        for i in 0..va.len().max(vb.len()) {
            let a_part = va.get(i).copied().unwrap_or(0);
            let b_part = vb.get(i).copied().unwrap_or(0);
            match a_part.cmp(&b_part) {
                std::cmp::Ordering::Greater => return true,
                std::cmp::Ordering::Less => return false,
                std::cmp::Ordering::Equal => continue,
            }
        }

        true // equal versions are compatible
    }

    /// Get the local node version.
    pub fn local_version(&self) -> &str {
        &self.local_version
    }
}

/// Determines which chain to follow during fork transitions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForkChoiceRule {
    /// Active forks ordered by priority.
    active_forks: Vec<String>,
    /// Map of fork name to activation block.
    activation_blocks: FxHashMap<String, u64>,
}

impl ForkChoiceRule {
    /// Create a new fork choice rule.
    pub fn new() -> Self {
        Self {
            active_forks: Vec::new(),
            activation_blocks: FxHashMap::default(),
        }
    }

    /// Register a fork for fork-choice consideration.
    pub fn register_fork(&mut self, fork_name: String, activation_block: u64) {
        self.active_forks.push(fork_name.clone());
        self.activation_blocks.insert(fork_name, activation_block);
    }

    /// Determine whether a chain following fork rules should be preferred.
    ///
    /// Returns true if the block at the given height should follow the fork
    /// rules (i.e., the fork is active at that height).
    pub fn should_follow_fork(&self, fork_name: &str, block_height: u64) -> bool {
        self.activation_blocks
            .get(fork_name)
            .map(|&activation| block_height >= activation)
            .unwrap_or(false)
    }

    /// Get all forks that are active at a given block height.
    pub fn active_forks_at_height(&self, block_height: u64) -> Vec<&str> {
        self.active_forks
            .iter()
            .filter(|name| {
                self.activation_blocks
                    .get(name.as_str())
                    .map(|&activation| block_height >= activation)
                    .unwrap_or(false)
            })
            .map(|s| s.as_str())
            .collect()
    }
}

impl Default for ForkChoiceRule {
    fn default() -> Self {
        Self::new()
    }
}

/// Records the history of all past forks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForkHistory {
    /// All forks that have been completed or are in progress.
    forks: Vec<ForkSpec>,
}

impl ForkHistory {
    /// Create a new empty fork history.
    pub fn new() -> Self {
        Self { forks: Vec::new() }
    }

    /// Record a fork in the history.
    pub fn record(&mut self, fork: ForkSpec) {
        self.forks.push(fork);
    }

    /// Get all recorded forks.
    pub fn all_forks(&self) -> &[ForkSpec] {
        &self.forks
    }

    /// Get forks by status.
    pub fn forks_by_status(&self, status: &ForkStatus) -> Vec<&ForkSpec> {
        self.forks.iter().filter(|f| f.status == *status).collect()
    }

    /// Get the total number of recorded forks.
    pub fn total_forks(&self) -> usize {
        self.forks.len()
    }
}

impl Default for ForkHistory {
    fn default() -> Self {
        Self::new()
    }
}

/// Coordinates the full lifecycle of hard forks.
///
/// Manages fork proposals, signaling, lock-in, activation, and completion.
/// Integrates with the version negotiator and fork choice rule engine.
pub struct ForkCoordinator {
    /// All known forks by name.
    forks: FxHashMap<String, ForkSpec>,
    /// Signaling trackers for forks in the Signaling state.
    signaling_trackers: FxHashMap<String, SignalingTracker>,
    /// Version negotiator for peer compatibility.
    version_negotiator: VersionNegotiator,
    /// Fork choice rules.
    fork_choice: ForkChoiceRule,
    /// Fork history.
    history: ForkHistory,
    /// Signaling threshold for lock-in.
    signaling_threshold: f64,
    /// Signaling period in blocks.
    signaling_period: u64,
}

impl ForkCoordinator {
    /// Create a new fork coordinator.
    pub fn new(local_version: String, signaling_threshold: f64, signaling_period: u64) -> Self {
        Self {
            forks: FxHashMap::default(),
            signaling_trackers: FxHashMap::default(),
            version_negotiator: VersionNegotiator::new(local_version),
            fork_choice: ForkChoiceRule::new(),
            history: ForkHistory::new(),
            signaling_threshold,
            signaling_period,
        }
    }

    /// Propose a new hard fork.
    pub fn propose_fork(&mut self, spec: ForkSpec) -> Result<(), ForkError> {
        if self.forks.contains_key(&spec.fork_name) {
            return Err(ForkError::ForkAlreadyExists(spec.fork_name.clone()));
        }

        self.forks.insert(spec.fork_name.clone(), spec);
        Ok(())
    }

    /// Start the signaling period for a proposed fork.
    pub fn start_signaling(
        &mut self,
        fork_name: &str,
        current_block: u64,
    ) -> Result<(), ForkError> {
        let fork = self
            .forks
            .get_mut(fork_name)
            .ok_or_else(|| ForkError::ForkNotFound(fork_name.to_string()))?;

        if fork.status != ForkStatus::Proposed {
            return Err(ForkError::InvalidTransition {
                from: fork.status.clone(),
                to: ForkStatus::Signaling,
            });
        }

        fork.status = ForkStatus::Signaling;
        fork.signaling_start_block = Some(current_block);

        let tracker = SignalingTracker::new(
            fork_name.to_string(),
            self.signaling_threshold,
            self.signaling_period,
            current_block,
        );
        self.signaling_trackers
            .insert(fork_name.to_string(), tracker);

        Ok(())
    }

    /// Record a block's signal (or lack thereof) for a fork.
    pub fn record_signal(
        &mut self,
        fork_name: &str,
        block_height: u64,
        validator: &str,
        signals_support: bool,
    ) -> Result<(), ForkError> {
        let fork = self
            .forks
            .get(fork_name)
            .ok_or_else(|| ForkError::ForkNotFound(fork_name.to_string()))?;

        if fork.status != ForkStatus::Signaling {
            return Err(ForkError::CannotSignal(fork_name.to_string()));
        }

        if let Some(tracker) = self.signaling_trackers.get_mut(fork_name) {
            tracker.record_block(block_height, validator, signals_support);
        }

        Ok(())
    }

    /// Attempt to lock in a fork after the signaling period.
    ///
    /// Transitions the fork from Signaling to Locked if the threshold was reached.
    pub fn try_lock_in(&mut self, fork_name: &str, current_block: u64) -> Result<bool, ForkError> {
        let tracker = self
            .signaling_trackers
            .get(fork_name)
            .ok_or_else(|| ForkError::ForkNotFound(fork_name.to_string()))?;

        if !tracker.period_ended(current_block) {
            return Ok(false);
        }

        if !tracker.threshold_reached() {
            return Err(ForkError::ThresholdNotReached(fork_name.to_string()));
        }

        let fork = self
            .forks
            .get_mut(fork_name)
            .ok_or_else(|| ForkError::ForkNotFound(fork_name.to_string()))?;

        fork.status = ForkStatus::Locked;
        fork.locked_at_block = Some(current_block);

        Ok(true)
    }

    /// Activate a locked-in fork at the designated block height.
    pub fn activate_fork(&mut self, fork_name: &str, current_block: u64) -> Result<(), ForkError> {
        let fork = self
            .forks
            .get_mut(fork_name)
            .ok_or_else(|| ForkError::ForkNotFound(fork_name.to_string()))?;

        if fork.status != ForkStatus::Locked {
            return Err(ForkError::NotLockedIn(fork_name.to_string()));
        }

        if current_block < fork.activation_block {
            return Err(ForkError::ActivationNotReached {
                activation: fork.activation_block,
                current: current_block,
            });
        }

        fork.status = ForkStatus::Active;
        fork.activated_at_block = Some(current_block);

        // Register version requirement
        self.version_negotiator
            .register_fork_requirement(fork_name.to_string(), fork.required_version.clone());

        // Register fork choice rule
        self.fork_choice
            .register_fork(fork_name.to_string(), fork.activation_block);

        // Record in history
        self.history.record(fork.clone());

        Ok(())
    }

    /// Complete a fork (mark transition as finished).
    pub fn complete_fork(&mut self, fork_name: &str) -> Result<(), ForkError> {
        let fork = self
            .forks
            .get_mut(fork_name)
            .ok_or_else(|| ForkError::ForkNotFound(fork_name.to_string()))?;

        if fork.status != ForkStatus::Active {
            return Err(ForkError::InvalidTransition {
                from: fork.status.clone(),
                to: ForkStatus::Completed,
            });
        }

        fork.status = ForkStatus::Completed;
        Ok(())
    }

    /// Fast-track an emergency fork (bypasses normal signaling).
    pub fn emergency_fork(&mut self, emergency: EmergencyFork) -> Result<(), ForkError> {
        let fork_name = emergency.spec.fork_name.clone();

        if self.forks.contains_key(&fork_name) {
            return Err(ForkError::ForkAlreadyExists(fork_name));
        }

        let mut spec = emergency.spec;

        if emergency.bypass_signaling {
            // Skip directly to Locked status
            spec.status = ForkStatus::Locked;
            spec.locked_at_block = Some(
                spec.activation_block
                    .saturating_sub(emergency.activation_delay),
            );
        }

        self.forks.insert(fork_name, spec);
        Ok(())
    }

    /// Get a fork specification by name.
    pub fn get_fork(&self, fork_name: &str) -> Option<&ForkSpec> {
        self.forks.get(fork_name)
    }

    /// Get the signaling tracker for a fork.
    pub fn get_signaling(&self, fork_name: &str) -> Option<&SignalingTracker> {
        self.signaling_trackers.get(fork_name)
    }

    /// Check if a peer version is compatible with all active forks.
    pub fn check_peer_compatibility(&self, peer_version: &str) -> Result<(), ForkError> {
        self.version_negotiator.is_compatible(peer_version)
    }

    /// Get the fork choice rule engine.
    pub fn fork_choice(&self) -> &ForkChoiceRule {
        &self.fork_choice
    }

    /// Get the fork history.
    pub fn history(&self) -> &ForkHistory {
        &self.history
    }

    /// Get all forks.
    pub fn all_forks(&self) -> &FxHashMap<String, ForkSpec> {
        &self.forks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_coordinator() -> ForkCoordinator {
        ForkCoordinator::new("1.0.0".to_string(), 0.75, 100)
    }

    fn make_fork(name: &str, activation: u64, version: &str) -> ForkSpec {
        ForkSpec::new(
            name.to_string(),
            activation,
            version.to_string(),
            format!("Test fork: {}", name),
        )
    }

    #[test]
    fn test_propose_fork() {
        let mut coord = make_coordinator();
        let fork = make_fork("test-fork", 1000, "2.0.0");

        coord.propose_fork(fork).unwrap();

        let stored = coord.get_fork("test-fork").unwrap();
        assert_eq!(stored.status, ForkStatus::Proposed);
        assert_eq!(stored.activation_block, 1000);
    }

    #[test]
    fn test_propose_duplicate_fork() {
        let mut coord = make_coordinator();
        let fork = make_fork("test-fork", 1000, "2.0.0");

        coord.propose_fork(fork).unwrap();

        let duplicate = make_fork("test-fork", 2000, "3.0.0");
        let result = coord.propose_fork(duplicate);
        assert!(matches!(result, Err(ForkError::ForkAlreadyExists(_))));
    }

    #[test]
    fn test_fork_signaling_reaches_threshold() {
        let mut coord = make_coordinator();
        let fork = make_fork("upgrade-1", 1000, "2.0.0");

        coord.propose_fork(fork).unwrap();
        coord.start_signaling("upgrade-1", 500).unwrap();

        // Record 100 blocks: 80 signal support (80% > 75% threshold)
        for i in 0..100 {
            let signals = i < 80;
            coord
                .record_signal("upgrade-1", 500 + i, &format!("val-{}", i % 10), signals)
                .unwrap();
        }

        let tracker = coord.get_signaling("upgrade-1").unwrap();
        assert!(tracker.threshold_reached());
        assert!((tracker.signaling_percentage() - 0.80).abs() < 0.001);

        // Lock in should succeed
        let locked = coord.try_lock_in("upgrade-1", 600).unwrap();
        assert!(locked);

        let fork = coord.get_fork("upgrade-1").unwrap();
        assert_eq!(fork.status, ForkStatus::Locked);
    }

    #[test]
    fn test_fork_signaling_below_threshold() {
        let mut coord = make_coordinator();
        let fork = make_fork("upgrade-2", 1000, "2.0.0");

        coord.propose_fork(fork).unwrap();
        coord.start_signaling("upgrade-2", 500).unwrap();

        // Record 100 blocks: only 50 signal support (50% < 75%)
        for i in 0..100 {
            let signals = i < 50;
            coord
                .record_signal("upgrade-2", 500 + i, &format!("val-{}", i % 10), signals)
                .unwrap();
        }

        let tracker = coord.get_signaling("upgrade-2").unwrap();
        assert!(!tracker.threshold_reached());

        // Lock in should fail
        let result = coord.try_lock_in("upgrade-2", 600);
        assert!(matches!(result, Err(ForkError::ThresholdNotReached(_))));
    }

    #[test]
    fn test_fork_activates_at_correct_block() {
        let mut coord = make_coordinator();
        let fork = make_fork("upgrade-3", 1000, "2.0.0");

        coord.propose_fork(fork).unwrap();
        coord.start_signaling("upgrade-3", 500).unwrap();

        // Signal enough support
        for i in 0..100 {
            coord
                .record_signal("upgrade-3", 500 + i, &format!("val-{}", i % 5), true)
                .unwrap();
        }

        coord.try_lock_in("upgrade-3", 600).unwrap();

        // Activate too early should fail
        let result = coord.activate_fork("upgrade-3", 999);
        assert!(matches!(
            result,
            Err(ForkError::ActivationNotReached { .. })
        ));

        // Activate at correct block
        coord.activate_fork("upgrade-3", 1000).unwrap();

        let fork = coord.get_fork("upgrade-3").unwrap();
        assert_eq!(fork.status, ForkStatus::Active);
        assert_eq!(fork.activated_at_block, Some(1000));
    }

    #[test]
    fn test_emergency_fork_fast_tracks() {
        let mut coord = make_coordinator();

        let emergency = EmergencyFork::new(
            "security-patch-1".to_string(),
            "1.1.0".to_string(),
            "Critical vulnerability in consensus".to_string(),
            500,
            100,
        ).unwrap();

        coord.emergency_fork(emergency).unwrap();

        let fork = coord.get_fork("security-patch-1").unwrap();
        // Emergency fork should skip to Locked status
        assert_eq!(fork.status, ForkStatus::Locked);
        assert_eq!(fork.activation_block, 600);

        // Should be activatable at the emergency activation block
        coord.activate_fork("security-patch-1", 600).unwrap();
        assert_eq!(
            coord.get_fork("security-patch-1").unwrap().status,
            ForkStatus::Active
        );
    }

    #[test]
    fn test_incompatible_version_rejected() {
        let mut coord = make_coordinator();
        let fork = make_fork("v2-upgrade", 1000, "2.0.0");

        coord.propose_fork(fork).unwrap();
        coord.start_signaling("v2-upgrade", 500).unwrap();

        // Signal and lock in
        for i in 0..100 {
            coord
                .record_signal("v2-upgrade", 500 + i, "val-0", true)
                .unwrap();
        }
        coord.try_lock_in("v2-upgrade", 600).unwrap();
        coord.activate_fork("v2-upgrade", 1000).unwrap();

        // Check compatibility
        assert!(coord.check_peer_compatibility("2.0.0").is_ok());
        assert!(coord.check_peer_compatibility("2.1.0").is_ok());
        assert!(coord.check_peer_compatibility("3.0.0").is_ok());

        let result = coord.check_peer_compatibility("1.9.0");
        assert!(matches!(result, Err(ForkError::IncompatibleVersion { .. })));
    }

    #[test]
    fn test_version_comparison() {
        assert!(VersionNegotiator::version_gte("2.0.0", "1.0.0"));
        assert!(VersionNegotiator::version_gte("1.1.0", "1.0.0"));
        assert!(VersionNegotiator::version_gte("1.0.1", "1.0.0"));
        assert!(VersionNegotiator::version_gte("2.0.0", "2.0.0"));
        assert!(!VersionNegotiator::version_gte("1.0.0", "2.0.0"));
        assert!(!VersionNegotiator::version_gte("1.9.9", "2.0.0"));
    }

    #[test]
    fn test_fork_choice_rule() {
        let mut rule = ForkChoiceRule::new();
        rule.register_fork("fork-a".to_string(), 100);
        rule.register_fork("fork-b".to_string(), 200);

        assert!(!rule.should_follow_fork("fork-a", 50));
        assert!(rule.should_follow_fork("fork-a", 100));
        assert!(rule.should_follow_fork("fork-a", 150));

        let active_at_150 = rule.active_forks_at_height(150);
        assert_eq!(active_at_150.len(), 1);
        assert!(active_at_150.contains(&"fork-a"));

        let active_at_250 = rule.active_forks_at_height(250);
        assert_eq!(active_at_250.len(), 2);
    }

    #[test]
    fn test_fork_history() {
        let mut coord = make_coordinator();

        // Complete a fork lifecycle
        let fork = make_fork("completed-fork", 1000, "2.0.0");
        coord.propose_fork(fork).unwrap();
        coord.start_signaling("completed-fork", 500).unwrap();

        for i in 0..100 {
            coord
                .record_signal("completed-fork", 500 + i, "val-0", true)
                .unwrap();
        }

        coord.try_lock_in("completed-fork", 600).unwrap();
        coord.activate_fork("completed-fork", 1000).unwrap();

        assert_eq!(coord.history().total_forks(), 1);
        assert_eq!(coord.history().all_forks()[0].fork_name, "completed-fork");
    }

    #[test]
    fn test_signaling_outside_period_ignored() {
        let mut coord = make_coordinator();
        let fork = make_fork("test-fork", 1000, "2.0.0");
        coord.propose_fork(fork).unwrap();
        coord.start_signaling("test-fork", 500).unwrap();

        // Signal before the period starts (should be ignored)
        coord
            .record_signal("test-fork", 499, "val-0", true)
            .unwrap();

        // Signal after the period ends (should be ignored)
        coord
            .record_signal("test-fork", 601, "val-0", true)
            .unwrap();

        let tracker = coord.get_signaling("test-fork").unwrap();
        assert_eq!(tracker.total_blocks(), 0);
    }

    #[test]
    fn test_cannot_signal_non_signaling_fork() {
        let mut coord = make_coordinator();
        let fork = make_fork("test-fork", 1000, "2.0.0");
        coord.propose_fork(fork).unwrap();

        // Fork is in Proposed state, not Signaling
        let result = coord.record_signal("test-fork", 500, "val-0", true);
        assert!(matches!(result, Err(ForkError::CannotSignal(_))));
    }

    #[test]
    fn test_complete_fork_lifecycle() {
        let mut coord = make_coordinator();

        // Propose
        let fork = make_fork("full-lifecycle", 1000, "2.0.0");
        coord.propose_fork(fork).unwrap();
        assert_eq!(
            coord.get_fork("full-lifecycle").unwrap().status,
            ForkStatus::Proposed
        );

        // Signal
        coord.start_signaling("full-lifecycle", 500).unwrap();
        assert_eq!(
            coord.get_fork("full-lifecycle").unwrap().status,
            ForkStatus::Signaling
        );

        for i in 0..100 {
            coord
                .record_signal("full-lifecycle", 500 + i, "val-0", true)
                .unwrap();
        }

        // Lock
        coord.try_lock_in("full-lifecycle", 600).unwrap();
        assert_eq!(
            coord.get_fork("full-lifecycle").unwrap().status,
            ForkStatus::Locked
        );

        // Activate
        coord.activate_fork("full-lifecycle", 1000).unwrap();
        assert_eq!(
            coord.get_fork("full-lifecycle").unwrap().status,
            ForkStatus::Active
        );

        // Complete
        coord.complete_fork("full-lifecycle").unwrap();
        assert_eq!(
            coord.get_fork("full-lifecycle").unwrap().status,
            ForkStatus::Completed
        );
    }

    #[test]
    fn emergency_fork_rejects_activation_delay_overflow() {
        let result = EmergencyFork::new(
            "overflow-test".to_string(),
            "1.1.0".to_string(),
            "Deliberate overflow test".to_string(),
            u64::MAX - 10,
            u64::MAX,
        );

        assert!(
            matches!(result, Err(ForkError::ArithmeticOverflow)),
            "expected ArithmeticOverflow error, got {result:?}"
        );
    }
}
