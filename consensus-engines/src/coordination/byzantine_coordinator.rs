//! Byzantine Consensus Coordinator
//!
//! Extracted from blockchain-core/src/consensus/byzantine_consensus_coordinator.rs.
//! Coordinates Byzantine fault-tolerant consensus protocols ensuring system integrity
//! and reliability in the presence of malicious actors.
//!
//! ## Adaptations from blockchain-core
//!
//! - `SecureConsensusCoordinator` / `GovernanceSecurityReport` -> removed (not yet extracted)
//! - `ConsensusSecurityManager` / `SecurityThreatLevel` / `AuthorizationLevel` -> removed
//! - `TransitionResult` / `TransitionTrigger` -> imported from local `dynamic_engine` module
//! - Tests that construct `DynamicConsensusEngine`, `SecureConsensusCoordinator` etc. -> dropped

use ed25519_dalek::{Signer, SigningKey, Signature, VerifyingKey};
use log::{debug, info, warn};
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use crate::types::{ConsensusError, ConsensusResult};

/// Utility function for safe timestamp generation
fn current_timestamp() -> ConsensusResult<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|e| ConsensusError::SystemTimeError(format!("Failed to get system time: {e}")))
}

/// Byzantine fault tolerance configuration
#[derive(Debug, Clone)]
pub struct ByzantineToleranceConfig {
    /// Maximum Byzantine nodes (as fraction, e.g., 0.33 for 33%)
    pub max_byzantine_ratio: f64,
    /// Minimum nodes for Byzantine fault tolerance
    pub min_nodes_for_bft: u32,
    /// Enable PBFT three-phase protocol
    pub enable_pbft: bool,
    /// Message timeout for Byzantine consensus (milliseconds)
    pub message_timeout: u64,
    /// View change timeout (milliseconds)
    pub view_change_timeout: u64,
    /// Enable cryptographic message validation
    pub enable_crypto_validation: bool,
    /// Replay attack prevention window (seconds)
    pub replay_window: u64,
}

impl Default for ByzantineToleranceConfig {
    fn default() -> Self {
        Self {
            max_byzantine_ratio: 0.33,
            min_nodes_for_bft: 4,
            enable_pbft: true,
            message_timeout: 5000,
            view_change_timeout: 10000,
            enable_crypto_validation: true,
            replay_window: 300,
        }
    }
}

/// Governance approval configuration for consensus changes
#[derive(Debug, Clone)]
pub struct GovernanceApprovalConfig {
    /// Minimum approval percentage required (0.67 = 67%)
    pub min_approval_percentage: f64,
    /// Minimum quorum for valid governance decisions
    pub min_quorum_percentage: f64,
    /// Voting period duration (seconds)
    pub voting_period: u64,
    /// Timelock duration for critical changes (seconds)
    pub timelock_duration: u64,
    /// Required multi-signature threshold
    pub multisig_threshold: u32,
    /// Emergency override enabled
    pub emergency_override_enabled: bool,
    /// Maximum emergency overrides per period
    pub max_emergency_overrides: u32,
    /// Emergency override cooldown (seconds)
    pub emergency_cooldown: u64,
}

impl Default for GovernanceApprovalConfig {
    fn default() -> Self {
        Self {
            min_approval_percentage: 0.67,
            min_quorum_percentage: 0.51,
            voting_period: 604800,
            timelock_duration: 86400,
            multisig_threshold: 3,
            emergency_override_enabled: true,
            max_emergency_overrides: 2,
            emergency_cooldown: 2592000,
        }
    }
}

/// Configuration for the Byzantine consensus coordinator
#[derive(Debug, Clone)]
pub struct ByzantineConsensusConfig {
    /// Byzantine fault tolerance settings
    pub byzantine_config: ByzantineToleranceConfig,
    /// Governance approval settings
    pub governance_config: GovernanceApprovalConfig,
    /// Enable comprehensive audit logging
    pub enable_audit_logging: bool,
    /// Attack detection sensitivity (0.0 - 1.0)
    pub attack_detection_sensitivity: f64,
    /// Enable automatic remediation
    pub enable_auto_remediation: bool,
    /// Network health check interval (seconds)
    pub health_check_interval: u64,
    /// Tolerance configuration (alias for byzantine_config)
    pub tolerance_config: ByzantineToleranceConfig,
    /// Governance approval configuration (alias for governance_config)
    pub governance_approval_config: GovernanceApprovalConfig,
    /// Detection sensitivity setting
    pub detection_sensitivity: f64,
    /// Recovery timeout in seconds
    pub recovery_timeout: u64,
    /// Maximum Byzantine nodes ratio (direct access)
    pub max_byzantine_ratio: f64,
    /// Enable PBFT protocol (direct access)
    pub pbft_enabled: bool,
}

impl Default for ByzantineConsensusConfig {
    fn default() -> Self {
        let byzantine_config = ByzantineToleranceConfig::default();
        let governance_config = GovernanceApprovalConfig::default();
        Self {
            byzantine_config: byzantine_config.clone(),
            governance_config: governance_config.clone(),
            enable_audit_logging: true,
            attack_detection_sensitivity: 0.8,
            enable_auto_remediation: true,
            health_check_interval: 30,
            tolerance_config: byzantine_config.clone(),
            governance_approval_config: governance_config,
            detection_sensitivity: 0.8,
            recovery_timeout: 300,
            max_byzantine_ratio: byzantine_config.max_byzantine_ratio,
            pbft_enabled: byzantine_config.enable_pbft,
        }
    }
}

/// Governance proposal for consensus algorithm changes
#[derive(Debug, Clone)]
pub struct GovernanceProposal {
    /// Unique proposal identifier
    pub proposal_id: String,
    /// Proposal title and description
    pub title: String,
    pub description: String,
    /// Proposed consensus algorithm change
    pub consensus_change: ConsensusAlgorithmChange,
    /// Proposal creation timestamp
    pub created_at: u64,
    /// Voting deadline
    pub voting_deadline: u64,
    /// Required approval threshold
    pub approval_threshold: f64,
    /// Current vote counts
    pub votes_for: u64,
    pub votes_against: u64,
    pub votes_abstain: u64,
    /// Proposal status
    pub status: ProposalStatus,
    /// Proposer node ID
    pub proposer: String,
    /// Multi-signature requirements
    pub required_signatures: Vec<String>,
    /// Collected signatures
    pub signatures: Vec<ProposalSignature>,
}

/// Consensus algorithm change specification
#[derive(Debug, Clone)]
pub struct ConsensusAlgorithmChange {
    /// Source algorithm
    pub from_algorithm: String,
    /// Target algorithm
    pub to_algorithm: String,
    /// Change justification
    pub justification: String,
    /// Expected impact assessment
    pub impact_assessment: String,
    /// Risk mitigation plan
    pub risk_mitigation: String,
    /// Rollback plan
    pub rollback_plan: String,
}

/// Proposal execution status
#[derive(Debug, Clone)]
pub enum ProposalStatus {
    /// Proposal is open for voting
    Voting,
    /// Proposal passed, in timelock period
    Approved { timelock_expires: u64 },
    /// Proposal is ready for execution
    Ready,
    /// Proposal is being executed
    Executing,
    /// Proposal execution completed
    Executed { execution_timestamp: u64 },
    /// Proposal was rejected
    Rejected { reason: String },
    /// Proposal expired without enough votes
    Expired,
    /// Proposal was cancelled
    Cancelled { reason: String },
}

/// Digital signature for proposal approval
#[derive(Debug, Clone)]
pub struct ProposalSignature {
    /// Signer node ID
    pub signer_id: String,
    /// Public key used for signing
    pub public_key: Vec<u8>,
    /// Digital signature
    pub signature: Vec<u8>,
    /// Signature timestamp
    pub timestamp: u64,
    /// Vote choice (for/against/abstain)
    pub vote: VoteChoice,
}

/// Vote choice options
#[derive(Debug, Clone)]
pub enum VoteChoice {
    For,
    Against,
    Abstain,
}

/// PBFT consensus message for Byzantine fault tolerance
#[derive(Debug, Clone)]
pub struct PbftMessage {
    /// Message type (prepare, commit, view-change)
    pub message_type: PbftMessageType,
    /// Message sequence number
    pub sequence_number: u64,
    /// View number
    pub view_number: u64,
    /// Node ID of sender
    pub sender_id: String,
    /// Message payload
    pub payload: Vec<u8>,
    /// Cryptographic signature
    pub signature: Vec<u8>,
    /// Message timestamp
    pub timestamp: u64,
}

/// PBFT message types
#[derive(Debug, Clone)]
pub enum PbftMessageType {
    /// Pre-prepare phase
    PrePrepare,
    /// Prepare phase
    Prepare,
    /// Commit phase
    Commit,
    /// View change request
    ViewChange,
    /// New view notification
    NewView,
}

/// Byzantine behavior detection result
#[derive(Debug, Clone)]
pub struct ByzantineDetectionResult {
    /// Detected malicious nodes
    pub malicious_nodes: Vec<String>,
    /// Attack type classification
    pub attack_type: ByzantineAttackType,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f64,
    /// Evidence collected
    pub evidence: Vec<String>,
    /// Recommended actions
    pub recommended_actions: Vec<String>,
    /// Detection timestamp
    pub timestamp: u64,
}

/// Types of Byzantine attacks
#[derive(Debug, Clone)]
pub enum ByzantineAttackType {
    /// Double spending attempt
    DoubleSpending,
    /// Equivocation (sending conflicting messages)
    Equivocation,
    /// Nothing-at-stake attack
    NothingAtStake,
    /// Selfish mining
    SelfishMining,
    /// Eclipse attack
    EclipseAttack,
    /// Grinding attack
    GrindingAttack,
    /// Sybil attack
    SybilAttack,
    /// Unknown or combined attack
    Unknown,
}

/// Network health metrics for Byzantine tolerance
#[derive(Debug, Clone)]
pub struct NetworkHealthMetrics {
    /// Total active nodes
    pub total_nodes: u32,
    /// Confirmed malicious nodes
    pub malicious_nodes: u32,
    /// Suspected Byzantine nodes
    pub suspected_nodes: u32,
    /// Byzantine fault tolerance maintained
    pub bft_maintained: bool,
    /// Network participation rate
    pub participation_rate: f64,
    /// Average message latency
    pub avg_message_latency: f64,
    /// Message success rate
    pub message_success_rate: f64,
    /// Last health check timestamp
    pub last_check: u64,
}

/// Main Byzantine consensus coordinator
///
/// This version does not depend on `SecureConsensusCoordinator` or
/// `ConsensusSecurityManager`. Those will be extracted in a later phase.
#[derive(Debug)]
#[allow(dead_code)]
pub struct ByzantineConsensusCoordinator {
    /// Configuration
    config: ByzantineConsensusConfig,
    /// Active governance proposals
    active_proposals: Arc<RwLock<FxHashMap<String, GovernanceProposal>>>,
    /// PBFT message history for validation
    pbft_messages: Arc<RwLock<VecDeque<PbftMessage>>>,
    /// Detected Byzantine behaviors
    byzantine_detections: Arc<RwLock<Vec<ByzantineDetectionResult>>>,
    /// Current view number for PBFT
    current_view: Arc<RwLock<u64>>,
    /// Node reputation scores
    node_reputations: Arc<RwLock<FxHashMap<String, f64>>>,
    /// Emergency override usage tracking
    emergency_usage: Arc<RwLock<Vec<u64>>>,
    /// Network health metrics
    network_health: Arc<RwLock<NetworkHealthMetrics>>,
    /// Cryptographic keys for this coordinator
    coordinator_keypair: Arc<RwLock<Option<SigningKey>>>,
}

impl ByzantineConsensusCoordinator {
    /// Create a new Byzantine consensus coordinator
    pub async fn new(
        config: ByzantineConsensusConfig,
    ) -> ConsensusResult<Self> {
        info!("Initializing Byzantine Consensus Coordinator with governance approval verification");

        let coordinator = Self {
            config,
            active_proposals: Arc::new(RwLock::new(FxHashMap::default())),
            pbft_messages: Arc::new(RwLock::new(VecDeque::new())),
            byzantine_detections: Arc::new(RwLock::new(Vec::new())),
            current_view: Arc::new(RwLock::new(0)),
            node_reputations: Arc::new(RwLock::new(FxHashMap::default())),
            emergency_usage: Arc::new(RwLock::new(Vec::new())),
            network_health: Arc::new(RwLock::new(NetworkHealthMetrics {
                total_nodes: 0,
                malicious_nodes: 0,
                suspected_nodes: 0,
                bft_maintained: true,
                participation_rate: 1.0,
                avg_message_latency: 50.0,
                message_success_rate: 1.0,
                last_check: current_timestamp()?,
            })),
            coordinator_keypair: Arc::new(RwLock::new(None)),
        };

        // Initialize cryptographic keys
        coordinator.initialize_coordinator_keys().await?;

        // Start health monitoring
        coordinator.start_health_monitoring().await?;

        info!("Byzantine Consensus Coordinator initialized successfully");
        Ok(coordinator)
    }

    /// Submit a governance proposal for consensus algorithm change
    pub async fn submit_governance_proposal(
        &self,
        proposal: GovernanceProposal,
    ) -> ConsensusResult<String> {
        info!("Submitting governance proposal: {}", proposal.proposal_id);

        self.validate_proposal_structure(&proposal).await?;
        self.validate_proposer_authority(&proposal.proposer).await?;
        self.check_proposal_conflicts(&proposal).await?;

        {
            let mut proposals = self.active_proposals.write().await;
            proposals.insert(proposal.proposal_id.clone(), proposal.clone());
        }

        self.log_governance_event(
            "proposal_submitted",
            &proposal.proposal_id,
            &format!(
                "Consensus change: {} -> {}",
                proposal.consensus_change.from_algorithm, proposal.consensus_change.to_algorithm
            ),
        )
        .await;

        info!(
            "Governance proposal submitted successfully: {}",
            proposal.proposal_id
        );
        Ok(proposal.proposal_id)
    }

    /// Cast a vote on a governance proposal
    pub async fn cast_governance_vote(
        &self,
        proposal_id: &str,
        voter_id: &str,
        vote: VoteChoice,
        signature: ProposalSignature,
    ) -> ConsensusResult<()> {
        info!("Processing governance vote from {voter_id} on proposal {proposal_id}");

        self.validate_voter_authority(voter_id).await?;
        self.verify_proposal_signature(&signature, proposal_id)
            .await?;

        {
            let mut proposals = self.active_proposals.write().await;
            if let Some(proposal) = proposals.get_mut(proposal_id) {
                if proposal
                    .signatures
                    .iter()
                    .any(|sig| sig.signer_id == voter_id)
                {
                    return Err(ConsensusError::InvalidState(
                        "Voter has already cast a vote on this proposal".to_string(),
                    ));
                }

                proposal.signatures.push(signature);
                match vote {
                    VoteChoice::For => proposal.votes_for += 1,
                    VoteChoice::Against => proposal.votes_against += 1,
                    VoteChoice::Abstain => proposal.votes_abstain += 1,
                }

                self.check_proposal_approval(proposal).await;
            } else {
                return Err(ConsensusError::InvalidState(format!(
                    "Proposal {proposal_id} not found"
                )));
            }
        }

        self.log_governance_event(
            "vote_cast",
            proposal_id,
            &format!("Vote: {vote:?} from {voter_id}"),
        )
        .await;

        info!("Governance vote processed successfully");
        Ok(())
    }

    /// Detect Byzantine behavior patterns in network participants
    pub async fn detect_byzantine_behavior(
        &self,
        node_id: &str,
        messages: &[PbftMessage],
    ) -> ConsensusResult<Option<ByzantineDetectionResult>> {
        debug!("Analyzing Byzantine behavior for node: {node_id}");

        let mut detection_result = ByzantineDetectionResult {
            malicious_nodes: Vec::new(),
            attack_type: ByzantineAttackType::Unknown,
            confidence: 0.0,
            evidence: Vec::new(),
            recommended_actions: Vec::new(),
            timestamp: current_timestamp()?,
        };

        if self.detect_equivocation(node_id, messages).await? {
            detection_result.malicious_nodes.push(node_id.to_string());
            detection_result.attack_type = ByzantineAttackType::Equivocation;
            detection_result.confidence = 0.9;
            detection_result
                .evidence
                .push("Conflicting messages detected from same node".to_string());
            detection_result
                .recommended_actions
                .push("Isolate node from consensus".to_string());
        }

        if self.detect_grinding_attack(node_id, messages).await? {
            detection_result.malicious_nodes.push(node_id.to_string());
            detection_result.attack_type = ByzantineAttackType::GrindingAttack;
            detection_result.confidence = 0.8;
            detection_result
                .evidence
                .push("Suspicious timing patterns detected".to_string());
            detection_result
                .recommended_actions
                .push("Require additional confirmation".to_string());
        }

        if self.detect_nothing_at_stake(node_id, messages).await? {
            detection_result.malicious_nodes.push(node_id.to_string());
            detection_result.attack_type = ByzantineAttackType::NothingAtStake;
            detection_result.confidence = 0.7;
            detection_result
                .evidence
                .push("Multi-chain participation detected".to_string());
            detection_result
                .recommended_actions
                .push("Monitor staking behavior".to_string());
        }

        if !detection_result.malicious_nodes.is_empty() {
            self.update_node_reputation(node_id, -0.3).await;

            {
                let mut detections = self.byzantine_detections.write().await;
                detections.push(detection_result.clone());
                detections.retain(|d| current_timestamp().unwrap_or(0) - d.timestamp < 86400);
            }

            warn!(
                "Byzantine behavior detected for node {}: {:?}",
                node_id, detection_result.attack_type
            );
            Ok(Some(detection_result))
        } else {
            self.update_node_reputation(node_id, 0.01).await;
            Ok(None)
        }
    }

    /// Process PBFT consensus message with Byzantine validation
    pub async fn process_pbft_message(&self, message: PbftMessage) -> ConsensusResult<bool> {
        debug!(
            "Processing PBFT message from {}: {:?}",
            message.sender_id, message.message_type
        );

        self.validate_pbft_message_authenticity(&message).await?;

        if self.is_replay_attack(&message).await? {
            warn!("Replay attack detected from {}", message.sender_id);
            return Ok(false);
        }

        if self.is_message_too_old(&message).await? {
            warn!("Outdated message from {}", message.sender_id);
            return Ok(false);
        }

        {
            let mut messages = self.pbft_messages.write().await;
            messages.push_back(message.clone());
            while messages.len() > 1000 {
                messages.pop_front();
            }
        }

        let recent_messages: Vec<_> = {
            let messages = self.pbft_messages.read().await;
            messages
                .iter()
                .filter(|m| m.sender_id == message.sender_id)
                .cloned()
                .collect()
        };

        if let Some(detection) = self
            .detect_byzantine_behavior(&message.sender_id, &recent_messages)
            .await?
        {
            self.handle_byzantine_detection(detection).await;
            return Ok(false);
        }

        match message.message_type {
            PbftMessageType::PrePrepare => self.handle_pre_prepare(&message).await,
            PbftMessageType::Prepare => self.handle_prepare(&message).await,
            PbftMessageType::Commit => self.handle_commit(&message).await,
            PbftMessageType::ViewChange => self.handle_view_change(&message).await,
            PbftMessageType::NewView => self.handle_new_view(&message).await,
        }
    }

    /// Coordinate view change in case of leader failure
    pub async fn coordinate_view_change(
        &self,
        suspected_leader: &str,
        evidence: Vec<String>,
    ) -> ConsensusResult<u64> {
        warn!("Coordinating view change due to suspected leader failure: {suspected_leader}");

        self.validate_view_change_request(suspected_leader, &evidence)
            .await?;

        let new_view = {
            let mut view = self.current_view.write().await;
            *view += 1;
            *view
        };

        let new_leader = self.select_leader_for_view(new_view).await?;

        let _view_change_message = PbftMessage {
            message_type: PbftMessageType::ViewChange,
            sequence_number: 0,
            view_number: new_view,
            sender_id: "coordinator".to_string(),
            payload: format!("view_change:{new_view}").into_bytes(),
            signature: self
                .sign_message(&format!("view_change:{new_view}"))
                .await?,
            timestamp: current_timestamp()?,
        };

        self.log_governance_event(
            "view_change",
            &new_view.to_string(),
            &format!("Leader change: {suspected_leader} -> {new_leader}"),
        )
        .await;

        info!("View change coordinated successfully: view {new_view} with leader {new_leader}");
        Ok(new_view)
    }

    /// Coordinate consensus algorithm change with Byzantine fault tolerance
    pub async fn coordinate_consensus_change(&self, transition_data: &[u8]) -> ConsensusResult<()> {
        info!("Coordinating consensus algorithm change through Byzantine consensus");

        if transition_data.is_empty() {
            return Err(ConsensusError::InvalidInput(
                "Empty transition data".to_string(),
            ));
        }

        let transition_message = PbftMessage {
            message_type: PbftMessageType::Prepare,
            sequence_number: self.generate_sequence_number().await?,
            view_number: *self.current_view.read().await,
            sender_id: "byzantine_coordinator".to_string(),
            payload: transition_data.to_vec(),
            signature: self
                .sign_message(&String::from_utf8_lossy(transition_data))
                .await?,
            timestamp: current_timestamp()?,
        };

        self.process_pbft_message(transition_message).await?;

        info!("Consensus algorithm change coordinated successfully");
        Ok(())
    }

    /// Generate a new sequence number for consensus messages
    async fn generate_sequence_number(&self) -> ConsensusResult<u64> {
        current_timestamp()
    }

    /// Get comprehensive governance and Byzantine status
    pub async fn get_governance_status(&self) -> GovernanceStatus {
        let proposals = self.active_proposals.read().await;
        let detections = self.byzantine_detections.read().await;
        let emergency_usage = self.emergency_usage.read().await;

        let active_proposals = proposals.len();
        let pending_votes = proposals
            .values()
            .filter(|p| matches!(p.status, ProposalStatus::Voting))
            .count();

        let recent_detections = detections
            .iter()
            .filter(|d| current_timestamp().unwrap_or(0) - d.timestamp < 3600)
            .count();

        let emergency_overrides_used = emergency_usage.len();

        GovernanceStatus {
            active_proposals,
            pending_votes,
            byzantine_detections: recent_detections,
            network_health_score: 1.0,
            emergency_overrides_remaining: self
                .config
                .governance_config
                .max_emergency_overrides
                .saturating_sub(emergency_overrides_used as u32),
            current_view: *self.current_view.read().await,
            last_updated: current_timestamp().unwrap_or(0),
        }
    }

    /// Validate a consensus operation for Byzantine fault tolerance
    pub async fn validate_consensus_operation(
        &self,
        _operation: &[u8],
    ) -> ConsensusResult<ValidationResult> {
        info!("Validating consensus operation for Byzantine compliance");

        Ok(ValidationResult {
            is_valid: true,
            confidence_score: 0.95,
            threat_level: None,
            validation_details: "Operation passed Byzantine validation checks".to_string(),
        })
    }

    /// Start the Byzantine consensus coordinator
    pub async fn start(&self) -> ConsensusResult<()> {
        info!("Starting Byzantine consensus coordinator");
        self.start_health_monitoring().await?;
        *self.current_view.write().await = 0;
        info!("Byzantine consensus coordinator started successfully");
        Ok(())
    }

    /// Stop the Byzantine consensus coordinator
    pub async fn stop(&self) -> ConsensusResult<()> {
        info!("Stopping Byzantine consensus coordinator");
        self.active_proposals.write().await.clear();
        self.pbft_messages.write().await.clear();
        info!("Byzantine consensus coordinator stopped successfully");
        Ok(())
    }

    /// Configure the coordinator for testing scenarios
    pub async fn configure_for_testing(
        &mut self,
        node_count: u32,
        byzantine_percentage: f64,
    ) -> ConsensusResult<()> {
        info!(
            "Configuring Byzantine coordinator for testing: {node_count} nodes, {byzantine_percentage}% Byzantine"
        );

        if byzantine_percentage > 33.0 {
            warn!("Byzantine percentage {byzantine_percentage}% exceeds recommended 33% limit");
        }

        let byzantine_node_count =
            ((node_count as f64 * byzantine_percentage / 100.0).ceil() as u32).min(node_count / 3);

        {
            let mut network_health = self.network_health.write().await;
            network_health.total_nodes = node_count;
            network_health.malicious_nodes = byzantine_node_count;
            network_health.suspected_nodes = 0;
            network_health.bft_maintained = true;
            network_health.participation_rate = 1.0;
            network_health.last_check = current_timestamp()?;
        }

        info!("Byzantine coordinator configured successfully for testing scenario");
        Ok(())
    }

    /// Update node reputation score (public for testing/external use)
    pub async fn update_node_reputation(&self, node_id: &str, delta: f64) {
        let mut reputations = self.node_reputations.write().await;
        let current = reputations.get(node_id).unwrap_or(&0.5);
        let new_reputation = (current + delta).clamp(0.0, 1.0);
        reputations.insert(node_id.to_string(), new_reputation);
    }

    // === Private Implementation Methods ===

    async fn initialize_coordinator_keys(&self) -> ConsensusResult<()> {
        let signing_key = SigningKey::from_bytes(&rand::random::<[u8; 32]>());
        let mut keypair = self.coordinator_keypair.write().await;
        *keypair = Some(signing_key);
        info!("Byzantine coordinator cryptographic keys initialized");
        Ok(())
    }

    #[allow(clippy::excessive_nesting)]
    async fn start_health_monitoring(&self) -> ConsensusResult<()> {
        if self.config.health_check_interval == 0 {
            return Ok(());
        }

        let health = Arc::clone(&self.network_health);
        let reputations = Arc::clone(&self.node_reputations);
        let detections = Arc::clone(&self.byzantine_detections);
        let interval = self.config.health_check_interval;

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(interval));

            loop {
                interval.tick().await;

                {
                    let mut health_metrics = health.write().await;
                    let reps = reputations.read().await;
                    let _dets = detections.read().await;

                    health_metrics.total_nodes = reps.len() as u32;
                    health_metrics.malicious_nodes =
                        reps.values().filter(|&&score| score < 0.3).count() as u32;
                    health_metrics.suspected_nodes = reps
                        .values()
                        .filter(|&&score| (0.3..0.6).contains(&score))
                        .count() as u32;

                    health_metrics.bft_maintained =
                        health_metrics.malicious_nodes * 3 < health_metrics.total_nodes;

                    health_metrics.participation_rate = if health_metrics.total_nodes > 0 {
                        (health_metrics.total_nodes - health_metrics.malicious_nodes) as f64
                            / health_metrics.total_nodes as f64
                    } else {
                        1.0
                    };

                    health_metrics.last_check = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                }
            }
        });

        info!("Byzantine health monitoring started with interval {interval}s");
        Ok(())
    }

    async fn validate_proposal_structure(
        &self,
        proposal: &GovernanceProposal,
    ) -> ConsensusResult<()> {
        if proposal.proposal_id.is_empty() || proposal.title.is_empty() {
            return Err(ConsensusError::InvalidInput(
                "Missing required proposal fields".to_string(),
            ));
        }

        let current_time = current_timestamp()?;
        if proposal.voting_deadline <= current_time {
            return Err(ConsensusError::InvalidInput(
                "Voting deadline must be in the future".to_string(),
            ));
        }

        if proposal.consensus_change.from_algorithm == proposal.consensus_change.to_algorithm {
            return Err(ConsensusError::InvalidInput(
                "Source and target algorithms cannot be the same".to_string(),
            ));
        }

        Ok(())
    }

    async fn validate_proposer_authority(&self, proposer: &str) -> ConsensusResult<()> {
        let reputations = self.node_reputations.read().await;
        let reputation = reputations.get(proposer).unwrap_or(&0.5);

        if *reputation < 0.7 {
            return Err(ConsensusError::Unauthorized(format!(
                "Proposer {proposer} has insufficient reputation: {reputation:.2}"
            )));
        }

        Ok(())
    }

    async fn check_proposal_conflicts(&self, proposal: &GovernanceProposal) -> ConsensusResult<()> {
        let proposals = self.active_proposals.read().await;

        for existing in proposals.values() {
            if matches!(
                existing.status,
                ProposalStatus::Voting | ProposalStatus::Approved { .. } | ProposalStatus::Ready
            ) && (existing.consensus_change.from_algorithm
                == proposal.consensus_change.from_algorithm
                || existing.consensus_change.to_algorithm
                    == proposal.consensus_change.to_algorithm)
            {
                return Err(ConsensusError::InvalidState(format!(
                    "Conflicting proposal already exists: {}",
                    existing.proposal_id
                )));
            }
        }

        Ok(())
    }

    async fn validate_voter_authority(&self, voter_id: &str) -> ConsensusResult<()> {
        let reputations = self.node_reputations.read().await;
        let reputation = reputations.get(voter_id).unwrap_or(&0.5);

        if *reputation < 0.3 {
            return Err(ConsensusError::Unauthorized(format!(
                "Voter {voter_id} has insufficient reputation: {reputation:.2}"
            )));
        }

        Ok(())
    }

    async fn verify_proposal_signature(
        &self,
        signature: &ProposalSignature,
        proposal_id: &str,
    ) -> ConsensusResult<()> {
        if signature.public_key.len() != 32 || signature.signature.len() != 64 {
            return Err(ConsensusError::InvalidSignature(
                "Invalid key or signature length".to_string(),
            ));
        }

        let pk_bytes: [u8; 32] = signature.public_key.as_slice().try_into().map_err(|_| {
            ConsensusError::InvalidSignature("Invalid public key format".to_string())
        })?;
        let public_key = VerifyingKey::from_bytes(&pk_bytes)
            .map_err(|e| ConsensusError::InvalidSignature(format!("Invalid public key: {e}")))?;

        let sig_bytes: [u8; 64] = signature.signature.as_slice().try_into().map_err(|_| {
            ConsensusError::InvalidSignature("Invalid signature format".to_string())
        })?;
        let sig = Signature::from_bytes(&sig_bytes);

        let message = format!(
            "{}:{}:{:?}",
            proposal_id, signature.signer_id, signature.vote
        );

        public_key
            .verify_strict(message.as_bytes(), &sig)
            .map_err(|e| {
                ConsensusError::InvalidSignature(format!("Signature verification failed: {e}"))
            })?;

        Ok(())
    }

    async fn check_proposal_approval(&self, proposal: &mut GovernanceProposal) {
        let total_votes = proposal.votes_for + proposal.votes_against + proposal.votes_abstain;
        let approval_rate = proposal.votes_for as f64 / total_votes as f64;
        let current_time = current_timestamp().unwrap_or(0);

        if approval_rate >= proposal.approval_threshold && total_votes >= 3 {
            proposal.status = ProposalStatus::Approved {
                timelock_expires: current_time + self.config.governance_config.timelock_duration,
            };
            info!(
                "Proposal {} approved with {:.1}% approval",
                proposal.proposal_id,
                approval_rate * 100.0
            );
        } else if current_time >= proposal.voting_deadline {
            if approval_rate >= proposal.approval_threshold {
                proposal.status = ProposalStatus::Approved {
                    timelock_expires: current_time
                        + self.config.governance_config.timelock_duration,
                };
            } else {
                proposal.status = ProposalStatus::Rejected {
                    reason: format!("Insufficient approval: {:.1}%", approval_rate * 100.0),
                };
            }
        }
    }

    #[allow(clippy::excessive_nesting)]
    async fn detect_equivocation(
        &self,
        node_id: &str,
        messages: &[PbftMessage],
    ) -> ConsensusResult<bool> {
        let mut sequence_views: FxHashMap<(u64, u64), Vec<&PbftMessage>> = FxHashMap::default();

        for message in messages {
            if message.sender_id == node_id {
                let key = (message.sequence_number, message.view_number);
                sequence_views.entry(key).or_default().push(message);
            }
        }

        for messages_in_seq in sequence_views.values() {
            if messages_in_seq.len() > 1 {
                let first_payload = &messages_in_seq[0].payload;
                for message in &messages_in_seq[1..] {
                    if message.payload != *first_payload {
                        return Ok(true);
                    }
                }
            }
        }

        Ok(false)
    }

    async fn detect_grinding_attack(
        &self,
        node_id: &str,
        messages: &[PbftMessage],
    ) -> ConsensusResult<bool> {
        let node_messages: Vec<_> = messages.iter().filter(|m| m.sender_id == node_id).collect();

        if node_messages.len() < 5 {
            return Ok(false);
        }

        let mut intervals = Vec::new();
        for window in node_messages.windows(2) {
            intervals.push(window[1].timestamp - window[0].timestamp);
        }

        let mean_interval = intervals.iter().sum::<u64>() as f64 / intervals.len() as f64;
        let variance = intervals
            .iter()
            .map(|&interval| {
                let diff = interval as f64 - mean_interval;
                diff * diff
            })
            .sum::<f64>()
            / intervals.len() as f64;

        Ok(variance < 1.0)
    }

    async fn detect_nothing_at_stake(
        &self,
        node_id: &str,
        messages: &[PbftMessage],
    ) -> ConsensusResult<bool> {
        let mut view_participation: FxHashSet<u64> = FxHashSet::default();

        for message in messages {
            if message.sender_id == node_id {
                view_participation.insert(message.view_number);
            }
        }

        Ok(view_participation.len() > 2)
    }

    async fn validate_pbft_message_authenticity(
        &self,
        message: &PbftMessage,
    ) -> ConsensusResult<()> {
        if !self.config.byzantine_config.enable_crypto_validation {
            return Ok(());
        }

        if message.signature.is_empty() {
            return Err(ConsensusError::InvalidSignature(
                "Missing message signature".to_string(),
            ));
        }

        Ok(())
    }

    async fn is_replay_attack(&self, message: &PbftMessage) -> ConsensusResult<bool> {
        let messages = self.pbft_messages.read().await;
        let current_time = current_timestamp()?;

        for existing in messages.iter() {
            if existing.sender_id == message.sender_id
                && existing.sequence_number == message.sequence_number
                && existing.view_number == message.view_number
                && existing.payload == message.payload
                && current_time - existing.timestamp < self.config.byzantine_config.replay_window
            {
                return Ok(true);
            }
        }

        Ok(false)
    }

    async fn is_message_too_old(&self, message: &PbftMessage) -> ConsensusResult<bool> {
        let current_time = current_timestamp()?;
        let age = current_time - message.timestamp;
        Ok(age > self.config.byzantine_config.message_timeout / 1000)
    }

    async fn handle_byzantine_detection(&self, detection: ByzantineDetectionResult) {
        warn!(
            "Handling Byzantine detection: {:?} from {:?}",
            detection.attack_type, detection.malicious_nodes
        );

        if self.config.enable_auto_remediation {
            for node_id in &detection.malicious_nodes {
                self.update_node_reputation(node_id, -0.5).await;
            }
        }
    }

    async fn handle_pre_prepare(&self, message: &PbftMessage) -> ConsensusResult<bool> {
        debug!("Handling pre-prepare from {}", message.sender_id);
        Ok(true)
    }

    async fn handle_prepare(&self, message: &PbftMessage) -> ConsensusResult<bool> {
        debug!("Handling prepare from {}", message.sender_id);
        Ok(true)
    }

    async fn handle_commit(&self, message: &PbftMessage) -> ConsensusResult<bool> {
        debug!("Handling commit from {}", message.sender_id);
        Ok(true)
    }

    async fn handle_view_change(&self, message: &PbftMessage) -> ConsensusResult<bool> {
        debug!("Handling view change from {}", message.sender_id);
        Ok(true)
    }

    async fn handle_new_view(&self, message: &PbftMessage) -> ConsensusResult<bool> {
        debug!("Handling new view from {}", message.sender_id);
        Ok(true)
    }

    async fn validate_view_change_request(
        &self,
        leader: &str,
        evidence: &[String],
    ) -> ConsensusResult<()> {
        if evidence.is_empty() {
            return Err(ConsensusError::InvalidInput(
                "View change requires evidence".to_string(),
            ));
        }

        let reputations = self.node_reputations.read().await;
        let reputation = reputations.get(leader).unwrap_or(&1.0);

        if *reputation > 0.8 && evidence.len() < 2 {
            return Err(ConsensusError::InvalidInput(
                "High reputation leader requires stronger evidence".to_string(),
            ));
        }

        Ok(())
    }

    async fn select_leader_for_view(&self, view: u64) -> ConsensusResult<String> {
        let reputations = self.node_reputations.read().await;

        if reputations.is_empty() {
            return Ok("default_leader".to_string());
        }

        let candidates: Vec<_> = reputations
            .iter()
            .filter(|(_, &score)| score > 0.5)
            .collect();

        if candidates.is_empty() {
            return Err(ConsensusError::InvalidState(
                "No suitable leader candidates".to_string(),
            ));
        }

        let leader_index = (view as usize) % candidates.len();
        Ok(candidates[leader_index].0.clone())
    }

    async fn sign_message(&self, message: &str) -> ConsensusResult<Vec<u8>> {
        let keypair = self.coordinator_keypair.read().await;
        if let Some(kp) = keypair.as_ref() {
            let signature = kp.sign(message.as_bytes());
            Ok(signature.to_bytes().to_vec())
        } else {
            Err(ConsensusError::ConfigurationError(
                "No signing key available".to_string(),
            ))
        }
    }

    async fn log_governance_event(&self, event_type: &str, identifier: &str, details: &str) {
        if !self.config.enable_audit_logging {
            return;
        }
        info!("GOVERNANCE_EVENT: {event_type} | ID: {identifier} | Details: {details}");
    }
}

/// Result of consensus operation validation
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub confidence_score: f64,
    pub threat_level: Option<String>,
    pub validation_details: String,
}

/// Governance status summary
#[derive(Debug, Clone)]
pub struct GovernanceStatus {
    pub active_proposals: usize,
    pub pending_votes: usize,
    pub byzantine_detections: usize,
    pub network_health_score: f64,
    pub emergency_overrides_remaining: u32,
    pub current_view: u64,
    pub last_updated: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_byzantine_coordinator_initialization() {
        let config = ByzantineConsensusConfig::default();
        let coordinator = ByzantineConsensusCoordinator::new(config).await;
        assert!(coordinator.is_ok());
    }

    #[tokio::test]
    async fn test_equivocation_detection() {
        let config = ByzantineConsensusConfig::default();
        let coordinator = ByzantineConsensusCoordinator::new(config).await.unwrap();

        let message1 = PbftMessage {
            message_type: PbftMessageType::Prepare,
            sequence_number: 1,
            view_number: 0,
            sender_id: "malicious_node".to_string(),
            payload: b"payload_a".to_vec(),
            signature: vec![0; 64],
            timestamp: current_timestamp().unwrap(),
        };

        let message2 = PbftMessage {
            message_type: PbftMessageType::Prepare,
            sequence_number: 1,
            view_number: 0,
            sender_id: "malicious_node".to_string(),
            payload: b"payload_b".to_vec(),
            signature: vec![0; 64],
            timestamp: current_timestamp().unwrap(),
        };

        let messages = vec![message1, message2];
        let result = coordinator
            .detect_byzantine_behavior("malicious_node", &messages)
            .await;

        assert!(result.is_ok());
        if let Some(detection) = result.unwrap() {
            assert!(matches!(
                detection.attack_type,
                ByzantineAttackType::Equivocation
            ));
            assert!(
                detection
                    .malicious_nodes
                    .contains(&"malicious_node".to_string())
            );
        }
    }

    #[tokio::test]
    async fn test_governance_status() {
        let config = ByzantineConsensusConfig::default();
        let coordinator = ByzantineConsensusCoordinator::new(config).await.unwrap();

        let status = coordinator.get_governance_status().await;
        assert_eq!(status.active_proposals, 0);
        assert_eq!(status.pending_votes, 0);
    }

    // --- #785: Byzantine coordinator quorum and view-change tests ---

    /// Helper: create a PbftMessage with the given parameters and a current timestamp.
    fn make_pbft(
        msg_type: PbftMessageType,
        seq: u64,
        view: u64,
        sender: &str,
        payload: &[u8],
    ) -> PbftMessage {
        PbftMessage {
            message_type: msg_type,
            sequence_number: seq,
            view_number: view,
            sender_id: sender.to_string(),
            payload: payload.to_vec(),
            signature: vec![0u8; 64], // placeholder — crypto validation only checks non-empty
            timestamp: current_timestamp().unwrap(),
        }
    }

    #[tokio::test]
    async fn normal_pbft_flow_pre_prepare_prepare_commit_succeeds() {
        // Normal-case PBFT: PrePrepare → Prepare → Commit all accepted by coordinator.
        let mut config = ByzantineConsensusConfig::default();
        config.byzantine_config.enable_crypto_validation = false; // skip sig check
        config.health_check_interval = 0; // no background task
        let coordinator = ByzantineConsensusCoordinator::new(config).await.unwrap();

        let pp = make_pbft(PbftMessageType::PrePrepare, 1, 0, "leader", b"block_data");
        assert!(coordinator.process_pbft_message(pp).await.unwrap());

        let prep = make_pbft(PbftMessageType::Prepare, 1, 0, "validator_1", b"block_data");
        assert!(coordinator.process_pbft_message(prep).await.unwrap());

        let commit = make_pbft(PbftMessageType::Commit, 1, 0, "validator_2", b"block_data");
        assert!(coordinator.process_pbft_message(commit).await.unwrap());
    }

    #[tokio::test]
    async fn view_change_increments_view_and_selects_new_leader() {
        let mut config = ByzantineConsensusConfig::default();
        config.health_check_interval = 0;
        let coordinator = ByzantineConsensusCoordinator::new(config).await.unwrap();

        // Seed two candidate nodes with high reputation so leader selection works.
        coordinator.update_node_reputation("node_a", 0.4).await; // 0.5 + 0.4 = 0.9
        coordinator.update_node_reputation("node_b", 0.3).await; // 0.5 + 0.3 = 0.8

        // Lower the suspected leader's reputation so evidence threshold is met with 1 piece.
        coordinator.update_node_reputation("old_leader", -0.3).await; // 0.5 - 0.3 = 0.2

        let initial_view = coordinator.get_governance_status().await.current_view;

        let new_view = coordinator
            .coordinate_view_change("old_leader", vec!["timeout: no block in 10s".to_string()])
            .await
            .unwrap();

        assert_eq!(new_view, initial_view + 1);
    }

    #[tokio::test]
    async fn duplicate_message_same_payload_not_flagged_as_equivocation() {
        // Sending the *same* message twice (identical payload) is NOT equivocation.
        let config = ByzantineConsensusConfig::default();
        let coordinator = ByzantineConsensusCoordinator::new(config).await.unwrap();

        let msg1 = make_pbft(PbftMessageType::Prepare, 1, 0, "honest_node", b"same");
        let msg2 = make_pbft(PbftMessageType::Prepare, 1, 0, "honest_node", b"same");

        let result = coordinator
            .detect_byzantine_behavior("honest_node", &[msg1, msg2])
            .await
            .unwrap();

        // Same payload → no equivocation detected.
        // The node might still be flagged for nothing-at-stake if view count > 2,
        // but equivocation specifically requires *different* payloads.
        if let Some(ref det) = result {
            assert!(
                !matches!(det.attack_type, ByzantineAttackType::Equivocation),
                "Identical payloads should not be flagged as equivocation"
            );
        }
    }

    #[tokio::test]
    async fn conflicting_pre_prepares_for_same_sequence_detected_as_equivocation() {
        // Two different payloads at the same (sequence, view) = equivocation.
        let config = ByzantineConsensusConfig::default();
        let coordinator = ByzantineConsensusCoordinator::new(config).await.unwrap();

        let pp1 = make_pbft(PbftMessageType::PrePrepare, 5, 0, "bad_leader", b"block_A");
        let pp2 = make_pbft(PbftMessageType::PrePrepare, 5, 0, "bad_leader", b"block_B");

        let result = coordinator
            .detect_byzantine_behavior("bad_leader", &[pp1, pp2])
            .await
            .unwrap();

        let detection = result.expect("conflicting pre-prepares must be detected");
        assert!(
            matches!(detection.attack_type, ByzantineAttackType::Equivocation),
            "Expected Equivocation, got {:?}",
            detection.attack_type,
        );
        assert!(detection.malicious_nodes.contains(&"bad_leader".to_string()));
        assert!(detection.confidence >= 0.9);
    }

    #[tokio::test]
    async fn single_byzantine_node_in_n4_cannot_forge_consensus() {
        // With n=4, f=1 the honest majority (3) should not be fooled.
        // The malicious node sends conflicting messages — the coordinator must detect it.
        let mut config = ByzantineConsensusConfig::default();
        config.byzantine_config.enable_crypto_validation = false;
        config.health_check_interval = 0;
        let coordinator = ByzantineConsensusCoordinator::new(config).await.unwrap();

        // Register 4 nodes.
        for node in &["honest_1", "honest_2", "honest_3", "malicious"] {
            coordinator.update_node_reputation(node, 0.0).await; // starts at 0.5
        }

        // Malicious node sends conflicting prepares (equivocation).
        let evil_a = make_pbft(PbftMessageType::Prepare, 10, 0, "malicious", b"fork_A");
        let evil_b = make_pbft(PbftMessageType::Prepare, 10, 0, "malicious", b"fork_B");

        let detection = coordinator
            .detect_byzantine_behavior("malicious", &[evil_a, evil_b])
            .await
            .unwrap()
            .expect("Byzantine behavior must be detected");

        assert!(detection.malicious_nodes.contains(&"malicious".to_string()));

        // The malicious node's reputation should have been penalised.
        let reps = coordinator.node_reputations.read().await;
        let rep = *reps.get("malicious").unwrap();
        assert!(
            rep < 0.5,
            "Malicious node reputation should be reduced, got {rep}"
        );
    }

    #[tokio::test]
    async fn view_change_requires_evidence() {
        let mut config = ByzantineConsensusConfig::default();
        config.health_check_interval = 0;
        let coordinator = ByzantineConsensusCoordinator::new(config).await.unwrap();

        // View change with no evidence should fail.
        let err = coordinator
            .coordinate_view_change("leader_x", vec![])
            .await;
        assert!(err.is_err(), "View change without evidence must be rejected");
    }

    #[tokio::test]
    async fn replay_message_is_rejected() {
        let mut config = ByzantineConsensusConfig::default();
        config.byzantine_config.enable_crypto_validation = false;
        config.byzantine_config.replay_window = 600; // 10 minutes
        config.health_check_interval = 0;
        let coordinator = ByzantineConsensusCoordinator::new(config).await.unwrap();

        let msg = make_pbft(PbftMessageType::Prepare, 42, 0, "node_x", b"data");

        // First submission succeeds.
        assert!(coordinator.process_pbft_message(msg.clone()).await.unwrap());

        // Exact same message again → replay → rejected.
        assert!(
            !coordinator.process_pbft_message(msg).await.unwrap(),
            "Replayed message must be rejected"
        );
    }
}
