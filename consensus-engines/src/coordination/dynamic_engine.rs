//! Dynamic Consensus Engine Coordination
//!
//! Extracted from blockchain-core/src/consensus/dynamic_consensus_engine.rs.
//! Provides dynamic consensus algorithm switching based on network conditions,
//! governance decisions, and emergency scenarios.
//!
//! ## Adaptations from blockchain-core
//!
//! - `ConsensusAlgorithm` trait references -> `ConsensusEngine` trait from `crate::traits`
//! - `crate::consensus::engine::ConsensusEngine` (struct) -> `crate::engine::ConsensusEngineManager`
//! - `crate::blockchain::block::ConsensusType` -> `crate::types::ConsensusType`
//! - Raft coordination (RaftConsensusManager, RaftIntegrationCoordinator, LogEntryType) -> stubbed out
//! - ByzantineConsensusCoordinator reference -> uses local byzantine_coordinator module
//! - Algorithm factory (create_algorithm_instance) -> stubbed placeholder

use rustc_hash::FxHashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use log::{debug, error, info, warn};
use uuid::Uuid;

use crate::engine::ConsensusEngineManager;
use crate::factory::ConsensusFactory;
use crate::traits::NetworkStats;
use crate::types::{ConsensusConfig, ConsensusError, ConsensusResult, ConsensusType};

/// Network condition thresholds for dynamic algorithm switching
#[derive(Debug, Clone)]
pub struct NetworkThresholds {
    /// Maximum acceptable block time (seconds)
    pub max_block_time: f64,
    /// Minimum acceptable transaction throughput (tx/s)
    pub min_throughput: f64,
    /// Maximum acceptable network latency (ms)
    pub max_latency: f64,
    /// Minimum participation rate for consensus
    pub min_participation: f64,
    /// Maximum acceptable hash rate variance (for PoW)
    pub max_hash_rate_variance: f64,
}

impl Default for NetworkThresholds {
    fn default() -> Self {
        Self {
            max_block_time: 30.0,
            min_throughput: 100.0,
            max_latency: 1000.0,
            min_participation: 0.67,
            max_hash_rate_variance: 0.3,
        }
    }
}

/// Governance configuration for consensus transitions
#[derive(Debug, Clone)]
pub struct GovernanceConfig {
    /// Minimum approval percentage for governance transitions
    pub min_approval_percentage: f64,
    /// Voting period duration (seconds)
    pub voting_period: u64,
    /// Minimum quorum for valid governance decisions
    pub min_quorum: f64,
    /// Emergency override enabled
    pub emergency_override_enabled: bool,
}

impl Default for GovernanceConfig {
    fn default() -> Self {
        Self {
            min_approval_percentage: 0.67,
            voting_period: 604800, // 1 week
            min_quorum: 0.51,
            emergency_override_enabled: true,
        }
    }
}

/// Configuration for the dynamic consensus engine
#[derive(Debug, Clone)]
pub struct DynamicConsensusConfig {
    /// Base consensus configuration
    pub consensus_config: ConsensusConfig,
    /// Network monitoring thresholds
    pub network_thresholds: NetworkThresholds,
    /// Governance configuration
    pub governance_config: GovernanceConfig,
    /// Enable automatic algorithm switching based on conditions
    pub enable_automatic_switching: bool,
    /// Monitoring interval (seconds)
    pub monitoring_interval: u64,
    /// Algorithm performance history depth
    pub performance_history_depth: usize,
    /// Emergency transition timeout (seconds)
    pub emergency_timeout: u64,
    /// Transition cooldown period in seconds
    pub transition_cooldown: u64,
    /// Enable emergency transitions
    pub emergency_transition_enabled: bool,
    /// Leader election parameters
    pub leader_election_params: LeaderElectionParams,
    /// Adaptive parameters for algorithm tuning
    pub adaptive_parameters: AdaptiveParameters,
}

impl Default for DynamicConsensusConfig {
    fn default() -> Self {
        Self {
            consensus_config: ConsensusConfig::default(),
            network_thresholds: NetworkThresholds::default(),
            governance_config: GovernanceConfig::default(),
            enable_automatic_switching: true,
            monitoring_interval: 60,
            performance_history_depth: 100,
            emergency_timeout: 1800,  // 30 minutes
            transition_cooldown: 300, // 5 minutes
            emergency_transition_enabled: true,
            leader_election_params: LeaderElectionParams::default(),
            adaptive_parameters: AdaptiveParameters::default(),
        }
    }
}

/// Leader election parameters
#[derive(Debug, Clone)]
pub struct LeaderElectionParams {
    /// Election timeout in milliseconds
    pub election_timeout: u64,
    /// Heartbeat interval in milliseconds
    pub heartbeat_interval: u64,
    /// Maximum number of candidates
    pub max_candidates: u32,
    /// Minimum candidate count
    pub min_candidates: usize,
    /// Enable randomized timeouts
    pub randomized_timeouts: bool,
    /// Selection criteria weights
    pub criteria_weights: FxHashMap<String, f64>,
    /// Geographic distribution requirements
    pub geographic_diversity: bool,
    /// Election timeout (alias for election_timeout)
    pub timeout: u64,
}

impl Default for LeaderElectionParams {
    fn default() -> Self {
        let mut criteria_weights = FxHashMap::default();
        criteria_weights.insert("reputation".to_string(), 0.4);
        criteria_weights.insert("stake".to_string(), 0.3);
        criteria_weights.insert("performance".to_string(), 0.2);
        criteria_weights.insert("availability".to_string(), 0.1);

        Self {
            election_timeout: 5000,
            heartbeat_interval: 1000,
            max_candidates: 10,
            min_candidates: 3,
            randomized_timeouts: true,
            criteria_weights,
            geographic_diversity: true,
            timeout: 5000,
        }
    }
}

/// Adaptive parameters for dynamic consensus tuning
#[derive(Debug, Clone)]
pub struct AdaptiveParameters {
    /// Enable adaptive block size
    pub adaptive_block_size: bool,
    /// Enable adaptive gas limit
    pub adaptive_gas_limit: bool,
    /// Enable adaptive timeout adjustments
    pub adaptive_timeouts: bool,
    /// Learning rate for parameter adjustments
    pub learning_rate: f64,
}

impl Default for AdaptiveParameters {
    fn default() -> Self {
        Self {
            adaptive_block_size: true,
            adaptive_gas_limit: true,
            adaptive_timeouts: true,
            learning_rate: 0.1,
        }
    }
}

/// Algorithm performance metrics
#[derive(Debug, Clone)]
pub struct AlgorithmPerformance {
    /// Algorithm type
    pub algorithm_type: ConsensusType,
    /// Average block time
    pub avg_block_time: f64,
    /// Transaction throughput
    pub tx_throughput: f64,
    /// Network latency
    pub network_latency: f64,
    /// Resource efficiency (0.0 - 1.0)
    pub resource_efficiency: f64,
    /// Security score (0.0 - 1.0)
    pub security_score: f64,
    /// Timestamp of measurement
    pub timestamp: u64,
}

/// Transition trigger that initiated algorithm switch
#[derive(Debug, Clone)]
pub enum TransitionTrigger {
    /// Manual governance decision
    Governance {
        proposal_id: String,
        approval_percentage: f64,
        vote_count: u64,
    },
    /// Automatic switch based on network conditions
    NetworkConditions {
        condition: String,
        metric_values: FxHashMap<String, f64>,
        threshold_breached: String,
    },
    /// Emergency override
    Emergency {
        reason: String,
        authorization_key: String,
        override_signature: String,
    },
    /// Adaptive optimization
    Adaptive {
        workload_type: String,
        efficiency_improvement: f64,
    },
}

/// Result of a consensus algorithm transition
#[derive(Debug, Clone)]
pub struct TransitionResult {
    /// Unique transition identifier
    pub transition_id: String,
    /// Success status
    pub success: bool,
    /// Source algorithm
    pub source_algorithm: ConsensusType,
    /// Target algorithm
    pub target_algorithm: ConsensusType,
    /// Transition trigger
    pub trigger: TransitionTrigger,
    /// Transition duration (milliseconds)
    pub transition_duration: u64,
    /// Performance before transition
    pub pre_transition_performance: Option<AlgorithmPerformance>,
    /// Performance after transition
    pub post_transition_performance: Option<AlgorithmPerformance>,
    /// Error message if failed
    pub error_message: Option<String>,
    /// Timestamp
    pub timestamp: u64,
    /// Security validation passed flag
    pub security_validation_passed: bool,
    /// Integrity score
    pub integrity_score: Option<f64>,
}

/// Network health status
#[derive(Debug, Clone)]
pub struct NetworkHealth {
    /// Overall health score (0.0 - 1.0)
    pub overall_score: f64,
    /// Active validator count
    pub active_validators: usize,
    /// Network participation rate
    pub participation_rate: f64,
    /// Average network latency (ms)
    pub avg_latency: f64,
    /// Byzantine fault tolerance maintained
    pub byzantine_tolerance: bool,
    /// Detected issues
    pub issues: Vec<String>,
}

/// Dynamic consensus engine with algorithm switching support
///
/// This is the consensus-engines adaptation. Raft coordination has been stubbed
/// out since `RaftConsensusManager` / `RaftIntegrationCoordinator` are not yet
/// extracted into this crate.
#[derive(Debug)]
#[allow(dead_code)]
pub struct DynamicConsensusEngine {
    /// Configuration
    config: DynamicConsensusConfig,
    /// Underlying consensus engine manager
    consensus_engine: Arc<RwLock<ConsensusEngineManager>>,
    /// Performance history for each algorithm
    performance_history: Arc<RwLock<FxHashMap<ConsensusType, Vec<AlgorithmPerformance>>>>,
    /// Current network statistics
    current_network_stats: Arc<RwLock<NetworkStats>>,
    /// Active transition tracking
    active_transitions: Arc<RwLock<FxHashMap<String, TransitionResult>>>,
    /// Monitoring task handle
    monitoring_handle: Option<tokio::task::JoinHandle<()>>,
    /// Emergency authorizations
    emergency_authorizations: Arc<RwLock<FxHashMap<String, (String, u64)>>>, // key -> (reason, expiry)
}

impl DynamicConsensusEngine {
    /// Create a new dynamic consensus engine
    ///
    /// Unlike the blockchain-core version, this does not initialize Raft coordination.
    /// Pass a pre-constructed `ConsensusEngineManager` instead.
    pub async fn new(
        config: DynamicConsensusConfig,
        mut consensus_engine: ConsensusEngineManager,
        _governance: GovernanceConfig,
    ) -> ConsensusResult<Self> {
        info!("Initializing Dynamic Consensus Engine (without Raft coordination)");
        Self::wire_testnet_consensus(&config, &mut consensus_engine).await?;

        let engine = Self {
            config,
            consensus_engine: Arc::new(RwLock::new(consensus_engine)),
            performance_history: Arc::new(RwLock::new(FxHashMap::default())),
            current_network_stats: Arc::new(RwLock::new(NetworkStats {
                avg_block_time: 10.0,
                tx_throughput: 100.0,
                hash_rate: None,
                total_stake: None,
                active_validators: 1,
                network_latency_ms: 100.0,
            })),
            active_transitions: Arc::new(RwLock::new(FxHashMap::default())),
            monitoring_handle: None,
            emergency_authorizations: Arc::new(RwLock::new(FxHashMap::default())),
        };

        info!("Dynamic Consensus Engine initialized successfully");
        Ok(engine)
    }

    /// Start monitoring network conditions for automatic switching
    pub async fn start_monitoring(&mut self) -> ConsensusResult<()> {
        if !self.config.enable_automatic_switching {
            return Ok(());
        }

        info!("Starting network condition monitoring");

        let config = self.config.clone();
        let network_stats = Arc::clone(&self.current_network_stats);

        let handle = tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(Duration::from_secs(config.monitoring_interval));

            loop {
                interval.tick().await;

                // Analyze conditions for potential switches
                if let Err(e) = Self::analyze_switching_conditions(&config, &network_stats).await {
                    error!("Failed to analyze switching conditions: {e:?}");
                }
            }
        });

        self.monitoring_handle = Some(handle);
        Ok(())
    }

    /// Stop monitoring
    pub fn stop_monitoring(&mut self) {
        if let Some(handle) = self.monitoring_handle.take() {
            handle.abort();
            info!("Network monitoring stopped");
        }
    }

    /// Perform emergency consensus transition
    pub async fn emergency_consensus_transition(
        &mut self,
        target_algorithm: ConsensusType,
        authorization_key: String,
        reason: String,
    ) -> ConsensusResult<TransitionResult> {
        warn!(
            "EMERGENCY CONSENSUS TRANSITION requested to {target_algorithm:?} - Reason: {reason}"
        );

        // Validate emergency authorization
        self.validate_emergency_authorization(&authorization_key, &reason)
            .await?;

        let transition_id = Uuid::new_v4().to_string();
        let start_time = Instant::now();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Get current algorithm for transition record
        let source_algorithm = self.detect_current_algorithm().await?;

        // Record pre-transition performance
        let pre_transition_performance =
            self.collect_current_performance(&source_algorithm).await?;

        // Create transition record
        let mut transition_result = TransitionResult {
            transition_id: transition_id.clone(),
            success: false,
            source_algorithm,
            target_algorithm,
            trigger: TransitionTrigger::Emergency {
                reason: reason.clone(),
                authorization_key: authorization_key.clone(),
                override_signature: format!("emergency_sig_{}", Uuid::new_v4()),
            },
            transition_duration: 0,
            pre_transition_performance: Some(pre_transition_performance),
            post_transition_performance: None,
            error_message: None,
            timestamp,
            security_validation_passed: false,
            integrity_score: None,
        };

        // Store active transition
        {
            let mut active = self.active_transitions.write().await;
            active.insert(transition_id.clone(), transition_result.clone());
        }

        if transition_result.source_algorithm != transition_result.target_algorithm {
            let mut manager = self.consensus_engine.write().await;
            manager.switch_algorithm(target_algorithm).await?;
        }

        // Collect post-transition performance (simulated)
        let post_transition_performance =
            self.collect_current_performance(&target_algorithm).await?;

        transition_result.success = true;
        transition_result.transition_duration = start_time.elapsed().as_millis() as u64;
        transition_result.post_transition_performance = Some(post_transition_performance);
        transition_result.security_validation_passed = true;

        info!(
            "Emergency consensus transition completed successfully: {transition_id} -> {target_algorithm:?}"
        );

        // Update transition record
        {
            let mut active = self.active_transitions.write().await;
            active.insert(transition_id.clone(), transition_result.clone());
        }

        // Clean up emergency authorization
        {
            let mut auth = self.emergency_authorizations.write().await;
            auth.remove(&authorization_key);
        }

        Ok(transition_result)
    }

    /// Coordinate leader election
    pub async fn coordinate_leader_election(
        &self,
        candidate_nodes: &[String],
        election_params: LeaderElectionParams,
    ) -> ConsensusResult<String> {
        info!(
            "Coordinating leader election with {} candidates",
            candidate_nodes.len()
        );

        if candidate_nodes.len() < election_params.min_candidates {
            return Err(ConsensusError::InvalidState(format!(
                "Insufficient candidates: {} < {}",
                candidate_nodes.len(),
                election_params.min_candidates
            )));
        }

        // Evaluate each candidate based on configured criteria
        let mut candidate_scores = FxHashMap::default();

        for candidate in candidate_nodes {
            let score = self
                .evaluate_candidate_score(candidate, &election_params)
                .await?;
            candidate_scores.insert(candidate.clone(), score);
            debug!("Candidate {candidate} score: {score:.3}");
        }

        // Select leader with highest score
        let elected_leader = candidate_scores
            .iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(candidate, _)| candidate.clone())
            .ok_or_else(|| ConsensusError::InvalidState("No valid leader candidate".to_string()))?;

        let elected_score = candidate_scores[&elected_leader];
        info!("Leader elected: {elected_leader} (score: {elected_score:.3})");

        Ok(elected_leader)
    }

    /// Get current network health status
    pub async fn get_network_health(&self) -> NetworkHealth {
        let stats = self.current_network_stats.read().await;

        // Calculate overall health score based on multiple factors
        let mut health_score = 1.0;
        let mut issues = Vec::new();

        // Check block time
        if stats.avg_block_time > self.config.network_thresholds.max_block_time {
            health_score *= 0.8;
            issues.push(format!("High block time: {:.1}s", stats.avg_block_time));
        }

        // Check throughput
        if stats.tx_throughput < self.config.network_thresholds.min_throughput {
            health_score *= 0.7;
            issues.push(format!("Low throughput: {:.1} tx/s", stats.tx_throughput));
        }

        // Check latency
        if stats.network_latency_ms > self.config.network_thresholds.max_latency {
            health_score *= 0.6;
            issues.push(format!("High latency: {:.1}ms", stats.network_latency_ms));
        }

        let participation_rate = if stats.active_validators > 0 {
            1.0
        } else {
            0.0
        };
        let byzantine_tolerance = stats.active_validators >= 3; // Minimum for BFT

        NetworkHealth {
            overall_score: health_score,
            active_validators: stats.active_validators as usize,
            participation_rate,
            avg_latency: stats.network_latency_ms,
            byzantine_tolerance,
            issues,
        }
    }

    /// Get algorithm performance history
    pub async fn get_algorithm_performance(
        &self,
        algorithm: &ConsensusType,
    ) -> Vec<AlgorithmPerformance> {
        let history = self.performance_history.read().await;
        history.get(algorithm).cloned().unwrap_or_default()
    }

    /// Get all active transitions
    pub async fn get_active_transitions(&self) -> FxHashMap<String, TransitionResult> {
        self.active_transitions.read().await.clone()
    }

    /// Authorize emergency transition
    pub async fn authorize_emergency_transition(
        &self,
        authorization_key: String,
        reason: String,
        expiry_seconds: u64,
    ) -> ConsensusResult<()> {
        let expiry = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + expiry_seconds;

        let mut auth = self.emergency_authorizations.write().await;
        auth.insert(authorization_key.clone(), (reason, expiry));

        info!(
            "Emergency authorization granted: {authorization_key} (expires in {expiry_seconds}s)"
        );
        Ok(())
    }

    // === Private Implementation Methods ===

    /// Detect the current consensus algorithm type heuristically
    async fn detect_current_algorithm(&self) -> ConsensusResult<ConsensusType> {
        let active_name = {
            let manager = self.consensus_engine.read().await;
            manager.get_algorithm_name().await
        };

        if let Some(active) = Self::map_algorithm_name(&active_name) {
            debug!("Detected active consensus algorithm from engine manager: {active:?}");
            return Ok(active);
        }

        let stats = self.current_network_stats.read().await;

        let detected_algorithm = if stats.hash_rate.is_some() && stats.hash_rate.unwrap() > 0 {
            ConsensusType::ProofOfWork
        } else if stats.total_stake.is_some() && stats.total_stake.unwrap() > 0 {
            ConsensusType::ProofOfStake
        } else if stats.active_validators > 1 {
            ConsensusType::ProofOfAuthority
        } else {
            ConsensusType::ProofOfWork
        };

        debug!("Detected consensus algorithm through heuristics: {detected_algorithm:?}");
        Ok(detected_algorithm)
    }

    fn map_algorithm_name(name: &str) -> Option<ConsensusType> {
        match name {
            "ProofOfPublishing" => Some(ConsensusType::ProofOfPublishing),
            "ProofOfWork" => Some(ConsensusType::ProofOfWork),
            "ProofOfStake" => Some(ConsensusType::ProofOfStake),
            "ProofOfAuthority" => Some(ConsensusType::ProofOfAuthority),
            "DelegatedProofOfStake" => Some(ConsensusType::DelegatedProofOfStake),
            "Raft" => Some(ConsensusType::Raft),
            "HotStuff" => Some(ConsensusType::HotStuff),
            "DAG" => Some(ConsensusType::DAG),
            _ => None,
        }
    }

    async fn wire_testnet_consensus(
        config: &DynamicConsensusConfig,
        consensus_engine: &mut ConsensusEngineManager,
    ) -> ConsensusResult<()> {
        consensus_engine.set_switching_enabled(true);

        Self::register_algorithm_if_missing(
            consensus_engine,
            ConsensusType::ProofOfPublishing,
            &config.consensus_config,
        )?;
        Self::register_algorithm_if_missing(
            consensus_engine,
            ConsensusType::ProofOfWork,
            &config.consensus_config,
        )?;
        Self::register_algorithm_if_missing(
            consensus_engine,
            ConsensusType::ProofOfStake,
            &config.consensus_config,
        )?;

        let active_name = consensus_engine.get_algorithm_name().await;
        let active = Self::map_algorithm_name(&active_name);
        if active != Some(ConsensusType::ProofOfPublishing) {
            consensus_engine
                .switch_algorithm(ConsensusType::ProofOfPublishing)
                .await?;
        }

        Ok(())
    }

    fn register_algorithm_if_missing(
        consensus_engine: &mut ConsensusEngineManager,
        consensus_type: ConsensusType,
        config: &ConsensusConfig,
    ) -> ConsensusResult<()> {
        if consensus_engine.is_algorithm_registered(&consensus_type) {
            return Ok(());
        }

        let algorithm = ConsensusFactory::create(consensus_type, config)?;
        consensus_engine.register_algorithm(consensus_type, algorithm);
        Ok(())
    }

    /// Validate emergency authorization
    async fn validate_emergency_authorization(
        &self,
        authorization_key: &str,
        reason: &str,
    ) -> ConsensusResult<()> {
        let auth = self.emergency_authorizations.read().await;

        if let Some((stored_reason, expiry)) = auth.get(authorization_key) {
            let current_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();

            if current_time > *expiry {
                return Err(ConsensusError::Unauthorized(
                    "Emergency authorization expired".to_string(),
                ));
            }

            if stored_reason != reason {
                return Err(ConsensusError::Unauthorized(
                    "Emergency authorization reason mismatch".to_string(),
                ));
            }

            Ok(())
        } else {
            Err(ConsensusError::Unauthorized(
                "Invalid emergency authorization key".to_string(),
            ))
        }
    }

    /// Collect current performance metrics for an algorithm
    async fn collect_current_performance(
        &self,
        algorithm: &ConsensusType,
    ) -> ConsensusResult<AlgorithmPerformance> {
        let stats = self.current_network_stats.read().await;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Ok(AlgorithmPerformance {
            algorithm_type: *algorithm,
            avg_block_time: stats.avg_block_time,
            tx_throughput: stats.tx_throughput,
            network_latency: stats.network_latency_ms,
            resource_efficiency: 0.8,
            security_score: 0.9,
            timestamp,
        })
    }

    /// Evaluate candidate score for leader election
    async fn evaluate_candidate_score(
        &self,
        candidate: &str,
        params: &LeaderElectionParams,
    ) -> ConsensusResult<f64> {
        let mut score = 0.0;

        for (criteria, weight) in &params.criteria_weights {
            let criteria_score = match criteria.as_str() {
                "reputation" => self.get_candidate_reputation(candidate).await,
                "stake" => self.get_candidate_stake(candidate).await,
                "performance" => self.get_candidate_performance(candidate).await,
                "availability" => self.get_candidate_availability(candidate).await,
                _ => 0.5,
            };

            score += criteria_score * weight;
        }

        Ok(score.clamp(0.0, 1.0))
    }

    async fn get_candidate_reputation(&self, _candidate: &str) -> f64 {
        0.8
    }

    async fn get_candidate_stake(&self, _candidate: &str) -> f64 {
        0.7
    }

    async fn get_candidate_performance(&self, _candidate: &str) -> f64 {
        0.85
    }

    async fn get_candidate_availability(&self, _candidate: &str) -> f64 {
        0.9
    }

    /// Analyze conditions for automatic algorithm switching
    async fn analyze_switching_conditions(
        config: &DynamicConsensusConfig,
        network_stats: &Arc<RwLock<NetworkStats>>,
    ) -> ConsensusResult<()> {
        let stats = network_stats.read().await;
        let thresholds = &config.network_thresholds;

        if stats.avg_block_time > thresholds.max_block_time {
            info!(
                "Block time threshold breached: {:.1}s > {:.1}s",
                stats.avg_block_time, thresholds.max_block_time
            );
        }

        if stats.tx_throughput < thresholds.min_throughput {
            info!(
                "Throughput threshold breached: {:.1} < {:.1} tx/s",
                stats.tx_throughput, thresholds.min_throughput
            );
        }

        if stats.network_latency_ms > thresholds.max_latency {
            info!(
                "Latency threshold breached: {:.1}ms > {:.1}ms",
                stats.network_latency_ms, thresholds.max_latency
            );
        }

        Ok(())
    }
}

impl Drop for DynamicConsensusEngine {
    fn drop(&mut self) {
        self.stop_monitoring();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factory::ConsensusFactory;

    #[test]
    fn test_default_config() {
        let config = DynamicConsensusConfig::default();
        assert!(config.enable_automatic_switching);
        assert_eq!(config.monitoring_interval, 60);
        assert_eq!(config.emergency_timeout, 1800);
    }

    #[test]
    fn test_network_thresholds() {
        let thresholds = NetworkThresholds::default();
        assert_eq!(thresholds.max_block_time, 30.0);
        assert_eq!(thresholds.min_throughput, 100.0);
        assert_eq!(thresholds.max_latency, 1000.0);
    }

    #[test]
    fn test_leader_election_params() {
        let params = LeaderElectionParams::default();
        assert_eq!(params.min_candidates, 3);
        assert!(params.criteria_weights.contains_key("reputation"));
        assert!(params.criteria_weights.contains_key("stake"));
    }

    #[test]
    fn test_transition_trigger_variants() {
        let governance = TransitionTrigger::Governance {
            proposal_id: "p1".to_string(),
            approval_percentage: 0.75,
            vote_count: 100,
        };
        assert!(matches!(governance, TransitionTrigger::Governance { .. }));

        let emergency = TransitionTrigger::Emergency {
            reason: "test".to_string(),
            authorization_key: "key".to_string(),
            override_signature: "sig".to_string(),
        };
        assert!(matches!(emergency, TransitionTrigger::Emergency { .. }));
    }

    #[tokio::test]
    async fn test_new_wires_pop_as_active_algorithm() {
        let pow_config = ConsensusFactory::default_config(ConsensusType::ProofOfWork);
        let initial =
            ConsensusFactory::create(ConsensusType::ProofOfWork, &pow_config).unwrap();
        let manager = ConsensusEngineManager::new(initial, pow_config.clone());

        let mut dynamic_config = DynamicConsensusConfig::default();
        dynamic_config.consensus_config =
            ConsensusFactory::default_config(ConsensusType::ProofOfPublishing);

        let engine = DynamicConsensusEngine::new(
            dynamic_config,
            manager,
            GovernanceConfig::default(),
        )
        .await
        .unwrap();

        assert_eq!(
            engine.detect_current_algorithm().await.unwrap(),
            ConsensusType::ProofOfPublishing
        );
    }

    #[tokio::test]
    async fn test_emergency_transition_switches_engine_algorithm() {
        let pow_config = ConsensusFactory::default_config(ConsensusType::ProofOfWork);
        let initial =
            ConsensusFactory::create(ConsensusType::ProofOfWork, &pow_config).unwrap();
        let manager = ConsensusEngineManager::new(initial, pow_config.clone());

        let mut dynamic_config = DynamicConsensusConfig::default();
        dynamic_config.consensus_config =
            ConsensusFactory::default_config(ConsensusType::ProofOfPublishing);

        let mut engine = DynamicConsensusEngine::new(
            dynamic_config,
            manager,
            GovernanceConfig::default(),
        )
        .await
        .unwrap();

        engine
            .authorize_emergency_transition(
                "auth-key".to_string(),
                "degraded network".to_string(),
                60,
            )
            .await
            .unwrap();

        let transition = engine
            .emergency_consensus_transition(
                ConsensusType::ProofOfStake,
                "auth-key".to_string(),
                "degraded network".to_string(),
            )
            .await
            .unwrap();

        assert!(transition.success);
        assert_eq!(transition.source_algorithm, ConsensusType::ProofOfPublishing);
        assert_eq!(transition.target_algorithm, ConsensusType::ProofOfStake);
        assert_eq!(
            engine.detect_current_algorithm().await.unwrap(),
            ConsensusType::ProofOfStake
        );
    }
}
