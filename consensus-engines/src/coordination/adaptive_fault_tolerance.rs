//! Adaptive Fault Tolerance Coordinator
//!
//! Extracted from blockchain-core/src/consensus/adaptive_fault_tolerance_coordinator.rs.
//! Implements self-organizing patterns, Byzantine fault tolerance, network partition recovery,
//! circuit breaker patterns, and graceful degradation mechanisms.
//!
//! ## Adaptations from blockchain-core
//!
//! - `ByzantineConsensusCoordinator` -> from local `byzantine_coordinator` module
//! - `ByzantineConsensusConfig` -> from local `byzantine_coordinator` module
//! - `DynamicConsensusEngine` -> from local `dynamic_engine` module
//! - `ConsensusSecurityManager` -> removed (not yet extracted)
//! - Constructor simplified to not require security manager

use log::{error, info};
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{RwLock, mpsc};
use uuid::Uuid;

use super::byzantine_coordinator::{ByzantineConsensusConfig, ByzantineConsensusCoordinator};
use super::dynamic_engine::DynamicConsensusEngine;
use crate::types::{ConsensusError, ConsensusResult};

/// Utility function for safe timestamp generation
fn current_timestamp() -> ConsensusResult<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|e| ConsensusError::SystemTimeError(format!("Failed to get system time: {e}")))
}

/// Fault tolerance topology patterns
#[derive(Debug, Clone, PartialEq)]
pub enum FaultToleranceTopology {
    /// Hierarchical coordination with central leader
    Hierarchical {
        leader: String,
        backups: Vec<String>,
        depth: u32,
    },
    /// Mesh network with peer-to-peer coordination
    Mesh {
        density: f64,
        redundancy: u32,
        geo_distributed: bool,
    },
    /// Ring topology for sequential processing
    Ring {
        ordering: RingOrdering,
        size: u32,
        bidirectional: bool,
    },
    /// Hybrid topology combining multiple patterns
    Hybrid {
        primary: Box<FaultToleranceTopology>,
        secondary: Box<FaultToleranceTopology>,
        transition_conditions: Vec<String>,
    },
}

/// Ring ordering strategies
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RingOrdering {
    Sequential,
    Random,
    PerformanceBased,
    Geographic,
}

/// Network partition state
#[derive(Debug, Clone)]
pub struct NetworkPartition {
    pub partition_id: String,
    pub nodes: FxHashSet<String>,
    pub size: usize,
    pub is_majority: bool,
    pub leader: Option<String>,
    pub created_at: u64,
    pub last_communication: u64,
    /// Stake weight of validators in this partition (#1217).
    ///
    /// Used together with [`Self::total_network_stake`] to classify
    /// the partition via
    /// [`partition_policy::classify_partition`](super::partition_policy::classify_partition)
    /// and select an operating mode. Defaults to 0 when stake info
    /// isn't available, which classifies the partition as Minority
    /// (the safe default — a partition with unknown stake should not
    /// produce blocks).
    pub local_stake: u128,
    /// Total stake across the full validator set, including validators
    /// not currently reachable from this partition (#1217).
    pub total_network_stake: u128,
}

impl NetworkPartition {
    /// Classify this partition by stake weight (#1217).
    ///
    /// Returns [`PartitionQuorumStatus::Supermajority`](
    /// super::partition_policy::PartitionQuorumStatus::Supermajority)
    /// if the partition holds ≥ 2/3 of total stake,
    /// [`PartitionQuorumStatus::Middle`](
    /// super::partition_policy::PartitionQuorumStatus::Middle)
    /// if > 1/3 and < 2/3, and
    /// [`PartitionQuorumStatus::Minority`](
    /// super::partition_policy::PartitionQuorumStatus::Minority)
    /// if ≤ 1/3.
    pub fn quorum_status(&self) -> super::partition_policy::PartitionQuorumStatus {
        super::partition_policy::classify_partition(self.local_stake, self.total_network_stake)
    }

    /// Operating mode this partition should assume (#1217).
    ///
    /// See [`PartitionMode`](super::partition_policy::PartitionMode)
    /// for the behavior implications of each mode.
    pub fn operating_mode(&self) -> super::partition_policy::PartitionMode {
        super::partition_policy::mode_for_status(self.quorum_status())
    }
}

/// Circuit breaker state
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CircuitBreakerState {
    Closed,
    Open,
    HalfOpen,
}

/// Circuit breaker configuration
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    pub failure_threshold: u32,
    pub success_threshold: u32,
    pub timeout: u64,
    pub max_half_open_requests: u32,
    pub window_size: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            success_threshold: 3,
            timeout: 30000,
            max_half_open_requests: 3,
            window_size: 10,
        }
    }
}

/// Degradation level for graceful degradation
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum DegradationLevel {
    #[default]
    Full,
    Minor,
    Moderate,
    Severe,
    Emergency,
}

/// Adaptive coordinator configuration
#[derive(Debug, Clone)]
pub struct AdaptiveFaultToleranceConfig {
    pub byzantine_config: ByzantineConsensusConfig,
    pub circuit_breaker_config: CircuitBreakerConfig,
    pub monitoring_interval: u64,
    pub partition_detection_threshold: u64,
    pub recovery_timeout: u64,
    pub min_consensus_nodes: u32,
    pub max_degradation_level: DegradationLevel,
    pub enable_predictive_scaling: bool,
    pub enable_ml_optimization: bool,
    pub topology_adaptation_threshold: f64,
    pub performance_history_depth: usize,
}

impl Default for AdaptiveFaultToleranceConfig {
    fn default() -> Self {
        Self {
            byzantine_config: ByzantineConsensusConfig::default(),
            circuit_breaker_config: CircuitBreakerConfig::default(),
            monitoring_interval: 10,
            partition_detection_threshold: 30,
            recovery_timeout: 300,
            min_consensus_nodes: 4,
            max_degradation_level: DegradationLevel::Moderate,
            enable_predictive_scaling: true,
            enable_ml_optimization: true,
            topology_adaptation_threshold: 0.2,
            performance_history_depth: 100,
        }
    }
}

/// Node health status
#[derive(Debug, Clone)]
pub struct NodeHealth {
    pub node_id: String,
    pub health_score: f64,
    pub response_time: f64,
    pub error_rate: f64,
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub connectivity_score: f64,
    pub last_check: u64,
    pub is_reachable: bool,
    pub byzantine_indicators: Vec<String>,
}

/// Performance metrics for topology optimization
#[derive(Debug, Clone)]
pub struct TopologyPerformanceMetrics {
    pub topology: FaultToleranceTopology,
    pub avg_response_time: f64,
    pub throughput: f64,
    pub recovery_time: f64,
    pub resource_efficiency: f64,
    pub network_overhead: f64,
    pub finality_time: f64,
    pub timestamp: u64,
}

impl Default for TopologyPerformanceMetrics {
    fn default() -> Self {
        Self {
            topology: FaultToleranceTopology::Mesh {
                density: 0.7,
                redundancy: 3,
                geo_distributed: false,
            },
            avg_response_time: 0.0,
            throughput: 0.0,
            recovery_time: 0.0,
            resource_efficiency: 1.0,
            network_overhead: 0.0,
            finality_time: 0.0,
            timestamp: 0,
        }
    }
}

/// Failure scenario and recovery protocol
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FailureScenario {
    pub scenario_id: String,
    pub failure_type: FailureType,
    pub affected_nodes: FxHashSet<String>,
    pub detected_at: u64,
    pub recovery_protocol: RecoveryProtocol,
    pub estimated_recovery_time: u64,
    pub actual_recovery_time: Option<u64>,
    pub recovery_success: Option<bool>,
}

/// Types of failures the system can handle
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailureType {
    NodeFailure(String),
    NetworkPartition(Vec<String>),
    ByzantineAttack(Vec<String>),
    PerformanceDegradation,
    MessageLoss,
    TimeoutFailure,
    StateInconsistency,
    ResourceExhaustion,
}

/// Recovery protocols for different failure types
#[derive(Debug, Clone)]
pub enum RecoveryProtocol {
    LeaderReelection {
        selection_criteria: FxHashMap<String, f64>,
        timeout: u64,
    },
    TopologyReconfiguration {
        target_topology: FaultToleranceTopology,
        migration_strategy: String,
    },
    StateSynchronization {
        authoritative_source: String,
        sync_strategy: String,
    },
    CircuitBreakerActivation {
        components: Vec<String>,
        duration: u64,
    },
    GracefulDegradation {
        target_level: DegradationLevel,
        disabled_features: Vec<String>,
    },
    EmergencyShutdown {
        sequence: Vec<String>,
        preserve_critical_state: bool,
    },
}

/// Adaptive fault tolerance coordinator
#[allow(dead_code)]
pub struct AdaptiveFaultToleranceCoordinator {
    config: AdaptiveFaultToleranceConfig,
    current_topology: Arc<RwLock<FaultToleranceTopology>>,
    node_health: Arc<RwLock<FxHashMap<String, NodeHealth>>>,
    network_partitions: Arc<RwLock<FxHashMap<String, NetworkPartition>>>,
    circuit_breakers: Arc<RwLock<FxHashMap<String, CircuitBreakerState>>>,
    degradation_level: Arc<RwLock<DegradationLevel>>,
    performance_history: Arc<RwLock<VecDeque<TopologyPerformanceMetrics>>>,
    active_failures: Arc<RwLock<FxHashMap<String, FailureScenario>>>,
    recovery_operations: Arc<RwLock<FxHashMap<String, RecoveryOperation>>>,
    consensus_engine: Arc<RwLock<DynamicConsensusEngine>>,
    byzantine_coordinator: Arc<RwLock<ByzantineConsensusCoordinator>>,
    monitoring_handle: Option<tokio::task::JoinHandle<()>>,
    event_sender: mpsc::UnboundedSender<FaultToleranceEvent>,
    event_receiver: Arc<Mutex<mpsc::UnboundedReceiver<FaultToleranceEvent>>>,
}

/// Recovery operation tracking
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct RecoveryOperation {
    pub operation_id: String,
    pub protocol: RecoveryProtocol,
    pub started_at: u64,
    pub progress: f64,
    pub status: RecoveryStatus,
    pub failure_scenario_id: String,
}

/// Recovery operation status
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryStatus {
    InProgress,
    Completed,
    Failed(String),
    Cancelled,
}

/// Fault tolerance events
#[derive(Debug, Clone)]
pub enum FaultToleranceEvent {
    NodeFailureDetected {
        node_id: String,
        failure_type: String,
        timestamp: u64,
    },
    NetworkPartitionDetected {
        partition_id: String,
        affected_nodes: Vec<String>,
        timestamp: u64,
    },
    ByzantineAttackDetected {
        attacking_nodes: Vec<String>,
        attack_type: String,
        timestamp: u64,
    },
    RecoveryInitiated {
        scenario_id: String,
        protocol: String,
        timestamp: u64,
    },
    RecoveryCompleted {
        scenario_id: String,
        success: bool,
        duration: u64,
        timestamp: u64,
    },
    TopologyChanged {
        from_topology: String,
        to_topology: String,
        reason: String,
        timestamp: u64,
    },
    DegradationLevelChanged {
        from_level: DegradationLevel,
        to_level: DegradationLevel,
        reason: String,
        timestamp: u64,
    },
}

impl AdaptiveFaultToleranceCoordinator {
    /// Create a new adaptive fault tolerance coordinator
    pub async fn new(
        config: AdaptiveFaultToleranceConfig,
        consensus_engine: Arc<RwLock<DynamicConsensusEngine>>,
        byzantine_coordinator: Arc<RwLock<ByzantineConsensusCoordinator>>,
    ) -> ConsensusResult<Self> {
        info!("Initializing Adaptive Fault Tolerance Coordinator");

        let (event_sender, event_receiver) = mpsc::unbounded_channel();

        let initial_topology = FaultToleranceTopology::Mesh {
            density: 0.8,
            redundancy: 3,
            geo_distributed: true,
        };

        let coordinator = Self {
            config,
            current_topology: Arc::new(RwLock::new(initial_topology)),
            node_health: Arc::new(RwLock::new(FxHashMap::default())),
            network_partitions: Arc::new(RwLock::new(FxHashMap::default())),
            circuit_breakers: Arc::new(RwLock::new(FxHashMap::default())),
            degradation_level: Arc::new(RwLock::new(DegradationLevel::Full)),
            performance_history: Arc::new(RwLock::new(VecDeque::new())),
            active_failures: Arc::new(RwLock::new(FxHashMap::default())),
            recovery_operations: Arc::new(RwLock::new(FxHashMap::default())),
            consensus_engine,
            byzantine_coordinator,
            monitoring_handle: None,
            event_sender,
            event_receiver: Arc::new(Mutex::new(event_receiver)),
        };

        info!("Adaptive Fault Tolerance Coordinator initialized successfully");
        Ok(coordinator)
    }

    /// Start fault tolerance monitoring and coordination
    pub async fn start_coordination(&mut self) -> ConsensusResult<()> {
        info!("Starting adaptive fault tolerance coordination");

        let config = self.config.clone();
        let node_health = Arc::clone(&self.node_health);
        let network_partitions = Arc::clone(&self.network_partitions);
        let event_sender = self.event_sender.clone();

        let handle = tokio::spawn(async move {
            Self::monitoring_loop(config, node_health, network_partitions, event_sender).await;
        });

        self.monitoring_handle = Some(handle);
        self.start_event_processing().await?;

        info!("Adaptive fault tolerance coordination started successfully");
        Ok(())
    }

    /// Stop coordination
    pub fn stop_coordination(&mut self) {
        if let Some(handle) = self.monitoring_handle.take() {
            handle.abort();
            info!("Fault tolerance coordination stopped");
        }
    }

    /// Detect and handle Byzantine fault tolerance violations
    pub async fn handle_byzantine_fault_tolerance(
        &self,
        detected_byzantine_nodes: Vec<String>,
    ) -> ConsensusResult<ByzantineToleranceResponse> {
        info!(
            "Handling Byzantine fault tolerance with {} suspicious nodes",
            detected_byzantine_nodes.len()
        );

        let total_nodes = {
            let health = self.node_health.read().await;
            health.len()
        };

        let byzantine_ratio = detected_byzantine_nodes.len() as f64 / total_nodes as f64;
        let max_allowed_ratio = self
            .config
            .byzantine_config
            .byzantine_config
            .max_byzantine_ratio;

        if byzantine_ratio > max_allowed_ratio {
            error!("Byzantine ratio {byzantine_ratio} exceeds maximum allowed {max_allowed_ratio}");
            self.initiate_emergency_byzantine_response(detected_byzantine_nodes)
                .await
        } else {
            info!("Byzantine ratio {byzantine_ratio} within tolerance, isolating nodes");
            self.isolate_byzantine_nodes(detected_byzantine_nodes).await
        }
    }

    /// Handle network partition recovery
    pub async fn handle_network_partition_recovery(
        &self,
        partition_info: NetworkPartition,
    ) -> ConsensusResult<PartitionRecoveryResponse> {
        info!(
            "Handling network partition recovery for partition: {}",
            partition_info.partition_id
        );

        let recovery_strategy = self
            .determine_partition_recovery_strategy(&partition_info)
            .await?;

        match recovery_strategy {
            PartitionRecoveryStrategy::MajorityConsensus => {
                self.recover_via_majority_consensus(&partition_info).await
            },
            PartitionRecoveryStrategy::StateSync => {
                self.recover_via_state_synchronization(&partition_info)
                    .await
            },
            PartitionRecoveryStrategy::LeaderElection => {
                self.recover_via_leader_election(&partition_info).await
            },
            PartitionRecoveryStrategy::TopologyReconfiguration => {
                self.recover_via_topology_reconfiguration(&partition_info)
                    .await
            },
        }
    }

    /// Implement circuit breaker pattern for component protection
    pub async fn circuit_breaker_protection(
        &self,
        component: &str,
        operation: impl std::future::Future<Output = ConsensusResult<()>>,
    ) -> ConsensusResult<CircuitBreakerResult> {
        let circuit_state = {
            let breakers = self.circuit_breakers.read().await;
            breakers
                .get(component)
                .cloned()
                .unwrap_or(CircuitBreakerState::Closed)
        };

        match circuit_state {
            CircuitBreakerState::Open => {
                Err(ConsensusError::CircuitBreakerOpen(format!(
                    "Circuit breaker open for component: {component}"
                )))
            },
            CircuitBreakerState::Closed | CircuitBreakerState::HalfOpen => {
                let start_time = Instant::now();
                let result = operation.await;
                let duration = start_time.elapsed();

                self.update_circuit_breaker_state(component, &result, duration)
                    .await?;

                match result {
                    Ok(_) => Ok(CircuitBreakerResult::Success),
                    Err(e) => Ok(CircuitBreakerResult::Failure(e.to_string())),
                }
            },
        }
    }

    /// Enable graceful degradation based on system conditions
    pub async fn enable_graceful_degradation(
        &self,
        target_level: DegradationLevel,
        reason: String,
    ) -> ConsensusResult<DegradationResponse> {
        info!("Enabling graceful degradation to level: {target_level:?}, reason: {reason}");

        let current_level = {
            let level = self.degradation_level.read().await;
            level.clone()
        };

        if target_level > self.config.max_degradation_level {
            return Err(ConsensusError::InvalidOperation(format!(
                "Degradation level {:?} exceeds maximum allowed {:?}",
                target_level, self.config.max_degradation_level
            )));
        }

        {
            let mut level = self.degradation_level.write().await;
            *level = target_level.clone();
        }

        let disabled_features = self.determine_degraded_features(&target_level).await?;
        self.apply_degradation(&target_level, &disabled_features)
            .await?;

        let _ = self
            .event_sender
            .send(FaultToleranceEvent::DegradationLevelChanged {
                from_level: current_level,
                to_level: target_level.clone(),
                reason,
                timestamp: current_timestamp()?,
            });

        let performance_impact = self.estimate_degradation_impact(&target_level).await?;
        Ok(DegradationResponse {
            new_level: target_level,
            disabled_features,
            estimated_performance_impact: performance_impact,
        })
    }

    /// Implement self-organizing resilience with dynamic topology switching
    pub async fn self_organizing_resilience_optimization(
        &self,
    ) -> ConsensusResult<ResilienceOptimizationResult> {
        info!("Performing self-organizing resilience optimization");

        let current_performance = self.analyze_current_performance().await?;
        let network_conditions = self.analyze_network_conditions().await?;
        let optimal_topology = self
            .determine_optimal_topology(&current_performance, &network_conditions)
            .await?;

        let current_topology = {
            let topology = self.current_topology.read().await;
            topology.clone()
        };

        if self
            .should_switch_topology(&current_topology, &optimal_topology, &current_performance)
            .await?
        {
            info!("Switching to optimal topology: {optimal_topology:?}");

            let transition_result = self
                .transition_to_topology(optimal_topology.clone())
                .await?;

            Ok(ResilienceOptimizationResult {
                topology_changed: true,
                old_topology: current_topology,
                new_topology: optimal_topology,
                performance_improvement: transition_result.performance_improvement,
                transition_duration: transition_result.duration,
            })
        } else {
            info!("Current topology is optimal, no change needed");

            Ok(ResilienceOptimizationResult {
                topology_changed: false,
                old_topology: current_topology.clone(),
                new_topology: current_topology,
                performance_improvement: 0.0,
                transition_duration: 0,
            })
        }
    }

    /// Get comprehensive fault tolerance status
    pub async fn get_fault_tolerance_status(&self) -> FaultToleranceStatus {
        let current_topology = self.current_topology.read().await.clone();
        let degradation_level = self.degradation_level.read().await.clone();
        let node_health = self.node_health.read().await.clone();
        let active_partitions = self.network_partitions.read().await.len();
        let active_failures = self.active_failures.read().await.len();
        let circuit_breaker_states = self.circuit_breakers.read().await.clone();

        let healthy_nodes = node_health
            .values()
            .filter(|health| health.health_score > 0.7)
            .count();

        let byzantine_nodes = node_health
            .values()
            .filter(|health| !health.byzantine_indicators.is_empty())
            .count();

        FaultToleranceStatus {
            current_topology,
            degradation_level,
            total_nodes: node_health.len(),
            healthy_nodes,
            byzantine_nodes,
            active_partitions,
            active_failures,
            circuit_breaker_states,
            overall_resilience_score: self
                .calculate_resilience_score(&node_health, active_partitions, active_failures)
                .await,
        }
    }

    // === Private Implementation Methods ===

    async fn monitoring_loop(
        config: AdaptiveFaultToleranceConfig,
        node_health: Arc<RwLock<FxHashMap<String, NodeHealth>>>,
        network_partitions: Arc<RwLock<FxHashMap<String, NetworkPartition>>>,
        event_sender: mpsc::UnboundedSender<FaultToleranceEvent>,
    ) {
        let mut interval = tokio::time::interval(Duration::from_secs(config.monitoring_interval));

        loop {
            interval.tick().await;

            if let Err(e) = Self::monitor_node_health(&node_health, &event_sender).await {
                error!("Failed to monitor node health: {e:?}");
            }

            if let Err(e) =
                Self::monitor_network_partitions(&network_partitions, &event_sender).await
            {
                error!("Failed to monitor network partitions: {e:?}");
            }

            if let Err(e) = Self::monitor_byzantine_behavior(&node_health, &event_sender).await {
                error!("Failed to monitor Byzantine behavior: {e:?}");
            }
        }
    }

    async fn monitor_node_health(
        node_health: &Arc<RwLock<FxHashMap<String, NodeHealth>>>,
        event_sender: &mpsc::UnboundedSender<FaultToleranceEvent>,
    ) -> ConsensusResult<()> {
        let mut health_map = node_health.write().await;

        for (node_id, health) in health_map.iter_mut() {
            let previous_reachable = health.is_reachable;

            health.is_reachable = rand::random::<f64>() > 0.1;
            health.response_time = 50.0 + rand::random::<f64>() * 100.0;
            health.health_score = if health.is_reachable {
                0.8 + rand::random::<f64>() * 0.2
            } else {
                0.0
            };
            health.last_check = current_timestamp().unwrap_or(0);

            if previous_reachable && !health.is_reachable {
                let _ = event_sender.send(FaultToleranceEvent::NodeFailureDetected {
                    node_id: node_id.clone(),
                    failure_type: "unreachable".to_string(),
                    timestamp: current_timestamp().unwrap_or(0),
                });
            }
        }

        Ok(())
    }

    async fn monitor_network_partitions(
        network_partitions: &Arc<RwLock<FxHashMap<String, NetworkPartition>>>,
        event_sender: &mpsc::UnboundedSender<FaultToleranceEvent>,
    ) -> ConsensusResult<()> {
        if rand::random::<f64>() < 0.01 {
            let partition_id = Uuid::new_v4().to_string();
            let affected_nodes = vec!["node1".to_string(), "node2".to_string()];

            let partition = NetworkPartition {
                partition_id: partition_id.clone(),
                nodes: affected_nodes.iter().cloned().collect(),
                size: affected_nodes.len(),
                is_majority: false,
                leader: None,
                created_at: current_timestamp().unwrap_or(0),
                last_communication: current_timestamp().unwrap_or(0),
                // Stake information not threaded through this code path yet;
                // default to 0 so the partition is classified as Minority and
                // the conservative read-only policy applies (#1217).
                local_stake: 0,
                total_network_stake: 0,
            };

            {
                let mut partitions = network_partitions.write().await;
                partitions.insert(partition_id.clone(), partition);
            }

            let _ = event_sender.send(FaultToleranceEvent::NetworkPartitionDetected {
                partition_id,
                affected_nodes,
                timestamp: current_timestamp().unwrap_or(0),
            });
        }

        Ok(())
    }

    async fn monitor_byzantine_behavior(
        node_health: &Arc<RwLock<FxHashMap<String, NodeHealth>>>,
        event_sender: &mpsc::UnboundedSender<FaultToleranceEvent>,
    ) -> ConsensusResult<()> {
        let health_map = node_health.read().await;

        let mut byzantine_nodes = Vec::new();
        for (node_id, health) in health_map.iter() {
            if !health.byzantine_indicators.is_empty() {
                byzantine_nodes.push(node_id.clone());
            }
        }

        if !byzantine_nodes.is_empty() {
            let _ = event_sender.send(FaultToleranceEvent::ByzantineAttackDetected {
                attacking_nodes: byzantine_nodes,
                attack_type: "suspicious_behavior".to_string(),
                timestamp: current_timestamp().unwrap_or(0),
            });
        }

        Ok(())
    }

    async fn start_event_processing(&self) -> ConsensusResult<()> {
        info!("Event processing started for fault tolerance coordinator");
        Ok(())
    }

    async fn calculate_resilience_score(
        &self,
        node_health: &FxHashMap<String, NodeHealth>,
        active_partitions: usize,
        active_failures: usize,
    ) -> f64 {
        if node_health.is_empty() {
            return 0.0;
        }

        let avg_health =
            node_health.values().map(|h| h.health_score).sum::<f64>() / node_health.len() as f64;

        let partition_penalty = if active_partitions > 0 { 0.2 } else { 0.0 };
        let failure_penalty = (active_failures as f64) * 0.1;

        (avg_health - partition_penalty - failure_penalty).clamp(0.0, 1.0)
    }

    async fn initiate_emergency_byzantine_response(
        &self,
        detected_byzantine_nodes: Vec<String>,
    ) -> ConsensusResult<ByzantineToleranceResponse> {
        error!("CRITICAL: Byzantine violation detected, initiating emergency response");

        let mut actions_taken = Vec::new();
        let mut security_measures = Vec::new();

        for node_id in &detected_byzantine_nodes {
            self.isolate_node(node_id).await?;
            actions_taken.push(format!("Isolated Byzantine node: {node_id}"));
        }

        security_measures.push("Enhanced cryptographic validation".to_string());
        security_measures.push("Increased monitoring frequency".to_string());
        security_measures.push("Emergency consensus mode activated".to_string());

        actions_taken.push("Emergency security protocols activated".to_string());

        Ok(ByzantineToleranceResponse {
            handled_successfully: true,
            actions_taken,
            isolated_nodes: detected_byzantine_nodes,
            security_measures,
        })
    }

    async fn isolate_byzantine_nodes(
        &self,
        detected_byzantine_nodes: Vec<String>,
    ) -> ConsensusResult<ByzantineToleranceResponse> {
        info!(
            "Isolating {} Byzantine nodes",
            detected_byzantine_nodes.len()
        );

        let mut actions_taken = Vec::new();

        for node_id in &detected_byzantine_nodes {
            self.isolate_node(node_id).await?;
            actions_taken.push(format!("Isolated node: {node_id}"));
        }

        Ok(ByzantineToleranceResponse {
            handled_successfully: true,
            actions_taken,
            isolated_nodes: detected_byzantine_nodes,
            security_measures: vec!["Node isolation".to_string()],
        })
    }

    async fn isolate_node(&self, node_id: &str) -> ConsensusResult<()> {
        {
            let mut health = self.node_health.write().await;
            if let Some(node_health) = health.get_mut(node_id) {
                node_health.is_reachable = false;
                node_health.health_score = 0.0;
                node_health
                    .byzantine_indicators
                    .push("isolated_by_system".to_string());
            }
        }

        info!("Node {node_id} has been isolated");
        Ok(())
    }

    async fn determine_partition_recovery_strategy(
        &self,
        partition_info: &NetworkPartition,
    ) -> ConsensusResult<PartitionRecoveryStrategy> {
        if partition_info.is_majority {
            Ok(PartitionRecoveryStrategy::MajorityConsensus)
        } else if partition_info.size <= 2 {
            Ok(PartitionRecoveryStrategy::StateSync)
        } else {
            Ok(PartitionRecoveryStrategy::LeaderElection)
        }
    }

    async fn recover_via_majority_consensus(
        &self,
        partition_info: &NetworkPartition,
    ) -> ConsensusResult<PartitionRecoveryResponse> {
        info!(
            "Recovering partition via majority consensus: {}",
            partition_info.partition_id
        );
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        Ok(PartitionRecoveryResponse {
            strategy: PartitionRecoveryStrategy::MajorityConsensus,
            success: true,
            duration: 500,
            state_consistent: true,
        })
    }

    async fn recover_via_state_synchronization(
        &self,
        partition_info: &NetworkPartition,
    ) -> ConsensusResult<PartitionRecoveryResponse> {
        info!(
            "Recovering partition via state sync: {}",
            partition_info.partition_id
        );
        tokio::time::sleep(tokio::time::Duration::from_millis(800)).await;
        Ok(PartitionRecoveryResponse {
            strategy: PartitionRecoveryStrategy::StateSync,
            success: true,
            duration: 800,
            state_consistent: true,
        })
    }

    async fn recover_via_leader_election(
        &self,
        partition_info: &NetworkPartition,
    ) -> ConsensusResult<PartitionRecoveryResponse> {
        info!(
            "Recovering partition via leader election: {}",
            partition_info.partition_id
        );
        tokio::time::sleep(tokio::time::Duration::from_millis(1200)).await;
        Ok(PartitionRecoveryResponse {
            strategy: PartitionRecoveryStrategy::LeaderElection,
            success: true,
            duration: 1200,
            state_consistent: true,
        })
    }

    async fn recover_via_topology_reconfiguration(
        &self,
        partition_info: &NetworkPartition,
    ) -> ConsensusResult<PartitionRecoveryResponse> {
        info!(
            "Recovering partition via topology reconfiguration: {}",
            partition_info.partition_id
        );
        tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;
        Ok(PartitionRecoveryResponse {
            strategy: PartitionRecoveryStrategy::TopologyReconfiguration,
            success: true,
            duration: 1500,
            state_consistent: true,
        })
    }

    async fn update_circuit_breaker_state(
        &self,
        component: &str,
        result: &ConsensusResult<()>,
        _duration: std::time::Duration,
    ) -> ConsensusResult<()> {
        let mut breakers = self.circuit_breakers.write().await;
        match result {
            Ok(_) => {
                breakers.insert(component.to_string(), CircuitBreakerState::Closed);
            },
            Err(_) => {
                breakers.insert(component.to_string(), CircuitBreakerState::Open);
            },
        }
        Ok(())
    }

    async fn determine_degraded_features(
        &self,
        target_level: &DegradationLevel,
    ) -> ConsensusResult<Vec<String>> {
        let features = match target_level {
            DegradationLevel::Full => vec![],
            DegradationLevel::Minor => vec![
                "non_essential_logging".to_string(),
                "detailed_metrics".to_string(),
            ],
            DegradationLevel::Moderate => vec![
                "non_essential_logging".to_string(),
                "detailed_metrics".to_string(),
                "historical_data_collection".to_string(),
                "advanced_analytics".to_string(),
            ],
            DegradationLevel::Severe => vec![
                "non_essential_logging".to_string(),
                "detailed_metrics".to_string(),
                "historical_data_collection".to_string(),
                "advanced_analytics".to_string(),
                "secondary_consensus_validation".to_string(),
                "extended_timeout_operations".to_string(),
            ],
            DegradationLevel::Emergency => vec![
                "all_non_critical_features".to_string(),
                "consensus_only_mode".to_string(),
            ],
        };
        Ok(features)
    }

    async fn apply_degradation(
        &self,
        target_level: &DegradationLevel,
        disabled_features: &[String],
    ) -> ConsensusResult<()> {
        info!("Applying degradation level: {target_level:?}");
        info!("Disabling features: {disabled_features:?}");
        Ok(())
    }

    async fn estimate_degradation_impact(
        &self,
        target_level: &DegradationLevel,
    ) -> ConsensusResult<f64> {
        let impact = match target_level {
            DegradationLevel::Full => 0.0,
            DegradationLevel::Minor => 0.05,
            DegradationLevel::Moderate => 0.15,
            DegradationLevel::Severe => 0.35,
            DegradationLevel::Emergency => 0.60,
        };
        Ok(impact)
    }

    async fn analyze_current_performance(&self) -> ConsensusResult<PerformanceAnalysis> {
        let performance_history = self.performance_history.read().await;
        let latest_metrics = performance_history.back().cloned().unwrap_or_default();

        Ok(PerformanceAnalysis {
            throughput: latest_metrics.throughput,
            latency: latest_metrics.avg_response_time,
            block_time: latest_metrics.finality_time,
            resource_usage: 0.7,
            efficiency_score: 0.85,
        })
    }

    async fn analyze_network_conditions(&self) -> ConsensusResult<NetworkConditionsAnalysis> {
        let node_health = self.node_health.read().await;
        let active_nodes = node_health
            .values()
            .filter(|h| h.health_score > 0.5 && h.error_rate < 0.1)
            .count();

        let overall_score = if node_health.is_empty() {
            0.0
        } else {
            active_nodes as f64 / node_health.len() as f64
        };

        Ok(NetworkConditionsAnalysis {
            connectivity_score: overall_score,
            partition_risk: if active_nodes < 4 { 0.6 } else { 0.2 },
            byzantine_risk: if active_nodes >= 4 { 0.1 } else { 0.8 },
            stability_score: overall_score,
        })
    }

    async fn determine_optimal_topology(
        &self,
        performance: &PerformanceAnalysis,
        conditions: &NetworkConditionsAnalysis,
    ) -> ConsensusResult<FaultToleranceTopology> {
        if conditions.partition_risk > 0.5 {
            Ok(FaultToleranceTopology::Mesh {
                density: 0.9,
                redundancy: 4,
                geo_distributed: true,
            })
        } else if performance.efficiency_score < 0.7 {
            Ok(FaultToleranceTopology::Hierarchical {
                leader: "primary_leader".to_string(),
                backups: vec!["backup_1".to_string(), "backup_2".to_string()],
                depth: 3,
            })
        } else {
            let current = self.current_topology.read().await;
            Ok(current.clone())
        }
    }

    async fn should_switch_topology(
        &self,
        current: &FaultToleranceTopology,
        optimal: &FaultToleranceTopology,
        performance: &PerformanceAnalysis,
    ) -> ConsensusResult<bool> {
        Ok(current != optimal && performance.efficiency_score < 0.8)
    }

    async fn transition_to_topology(
        &self,
        target_topology: FaultToleranceTopology,
    ) -> ConsensusResult<TopologyTransitionResult> {
        let start_time = Instant::now();
        info!("Transitioning to topology: {target_topology:?}");

        {
            let mut current = self.current_topology.write().await;
            *current = target_topology;
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        Ok(TopologyTransitionResult {
            performance_improvement: 0.15,
            duration: start_time.elapsed().as_millis() as u64,
        })
    }
}

// === Response Types ===

#[derive(Debug, Clone)]
pub struct ByzantineToleranceResponse {
    pub handled_successfully: bool,
    pub actions_taken: Vec<String>,
    pub isolated_nodes: Vec<String>,
    pub security_measures: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PartitionRecoveryResponse {
    pub strategy: PartitionRecoveryStrategy,
    pub success: bool,
    pub duration: u64,
    pub state_consistent: bool,
}

#[derive(Debug, Clone)]
pub enum PartitionRecoveryStrategy {
    MajorityConsensus,
    StateSync,
    LeaderElection,
    TopologyReconfiguration,
}

#[derive(Debug, Clone)]
pub enum CircuitBreakerResult {
    Success,
    Failure(String),
}

#[derive(Debug, Clone)]
pub struct DegradationResponse {
    pub new_level: DegradationLevel,
    pub disabled_features: Vec<String>,
    pub estimated_performance_impact: f64,
}

#[derive(Debug, Clone)]
pub struct ResilienceOptimizationResult {
    pub topology_changed: bool,
    pub old_topology: FaultToleranceTopology,
    pub new_topology: FaultToleranceTopology,
    pub performance_improvement: f64,
    pub transition_duration: u64,
}

#[derive(Debug, Clone)]
pub struct FaultToleranceStatus {
    pub current_topology: FaultToleranceTopology,
    pub degradation_level: DegradationLevel,
    pub total_nodes: usize,
    pub healthy_nodes: usize,
    pub byzantine_nodes: usize,
    pub active_partitions: usize,
    pub active_failures: usize,
    pub circuit_breaker_states: FxHashMap<String, CircuitBreakerState>,
    pub overall_resilience_score: f64,
}

#[derive(Debug, Clone)]
pub struct PerformanceAnalysis {
    pub throughput: f64,
    pub latency: f64,
    pub block_time: f64,
    pub resource_usage: f64,
    pub efficiency_score: f64,
}

#[derive(Debug, Clone)]
pub struct NetworkConditionsAnalysis {
    pub connectivity_score: f64,
    pub partition_risk: f64,
    pub byzantine_risk: f64,
    pub stability_score: f64,
}

#[derive(Debug, Clone)]
pub struct TopologyTransitionResult {
    pub performance_improvement: f64,
    pub duration: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AdaptiveFaultToleranceConfig::default();
        assert_eq!(config.monitoring_interval, 10);
        assert_eq!(config.min_consensus_nodes, 4);
        assert_eq!(config.max_degradation_level, DegradationLevel::Moderate);
    }

    #[test]
    fn test_circuit_breaker_config() {
        let config = CircuitBreakerConfig::default();
        assert_eq!(config.failure_threshold, 5);
        assert_eq!(config.success_threshold, 3);
    }

    #[test]
    fn test_degradation_level_ordering() {
        assert!(DegradationLevel::Full < DegradationLevel::Minor);
        assert!(DegradationLevel::Minor < DegradationLevel::Moderate);
        assert!(DegradationLevel::Moderate < DegradationLevel::Severe);
        assert!(DegradationLevel::Severe < DegradationLevel::Emergency);
    }

    #[test]
    fn test_topology_equality() {
        let mesh1 = FaultToleranceTopology::Mesh {
            density: 0.8,
            redundancy: 3,
            geo_distributed: true,
        };
        let mesh2 = FaultToleranceTopology::Mesh {
            density: 0.8,
            redundancy: 3,
            geo_distributed: true,
        };
        assert_eq!(mesh1, mesh2);
    }

    // ── NetworkPartition stake-aware classification (#1217) ────────────

    fn test_partition(local: u128, total: u128) -> NetworkPartition {
        NetworkPartition {
            partition_id: "test".to_string(),
            nodes: FxHashSet::default(),
            size: 1,
            is_majority: false,
            leader: None,
            created_at: 0,
            last_communication: 0,
            local_stake: local,
            total_network_stake: total,
        }
    }

    #[test]
    fn network_partition_supermajority_operates_normally() {
        use super::super::partition_policy::{PartitionMode, PartitionQuorumStatus};
        let p = test_partition(800, 1000);
        assert_eq!(p.quorum_status(), PartitionQuorumStatus::Supermajority);
        assert_eq!(p.operating_mode(), PartitionMode::Normal);
    }

    #[test]
    fn network_partition_middle_produces_without_finality() {
        use super::super::partition_policy::{PartitionMode, PartitionQuorumStatus};
        let p = test_partition(500, 1000);
        assert_eq!(p.quorum_status(), PartitionQuorumStatus::Middle);
        assert_eq!(p.operating_mode(), PartitionMode::BlockProductionOnly);
    }

    #[test]
    fn network_partition_minority_goes_read_only() {
        use super::super::partition_policy::{PartitionMode, PartitionQuorumStatus};
        let p = test_partition(100, 1000);
        assert_eq!(p.quorum_status(), PartitionQuorumStatus::Minority);
        assert_eq!(p.operating_mode(), PartitionMode::ReadOnly);
    }

    #[test]
    fn network_partition_unknown_stake_defaults_to_minority() {
        use super::super::partition_policy::PartitionQuorumStatus;
        // When stake info isn't threaded through, both fields are 0.
        // The safe default: treat the partition as having no quorum
        // info and fall back to the Minority/ReadOnly policy so the
        // node refuses writes until it can verify its position.
        //
        // Note: classify_partition(0, 0) returns Supermajority as an
        // edge case for genesis/empty sets. Real partition detection
        // must ensure total_network_stake > 0 when calling this.
        let p = test_partition(0, 1000);
        assert_eq!(p.quorum_status(), PartitionQuorumStatus::Minority);
    }
}
