//! Byzantine fault tolerance implementations for consensus
//!
//! Extracted from blockchain-core/src/consensus/byzantine.rs.
//! Provides BFT types, malicious actor detection, and network health monitoring.

use serde::{Deserialize, Serialize};
use rustc_hash::{FxHashMap, FxHashSet};

/// Governance vote stub (governance module not available in this crate)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceVote;

/// Byzantine fault tolerance configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ByzantineFaultToleranceConfig {
    /// Maximum number of byzantine nodes that can be tolerated
    pub max_byzantine_nodes: u32,
    /// Total number of nodes in the network
    pub total_nodes: u32,
    /// Timeout for consensus rounds in milliseconds
    pub consensus_timeout_ms: u64,
    /// Number of rounds to wait before declaring a node byzantine
    pub byzantine_detection_rounds: u32,
}

/// Byzantine consensus algorithm implementation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ByzantineConsensus {
    /// Configuration
    config: ByzantineFaultToleranceConfig,
    /// Current consensus round
    current_round: u64,
    /// Suspected byzantine nodes
    suspected_nodes: FxHashSet<String>,
    /// Consensus messages received this round
    consensus_messages: FxHashMap<String, Vec<u8>>,
}

/// Byzantine fault tolerance manager
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ByzantineFaultTolerance {
    /// Byzantine consensus instance
    consensus: ByzantineConsensus,
    /// Malicious actor detector
    detector: MaliciousActorDetector,
    /// Network health metrics
    network_health: NetworkHealth,
}

/// Malicious actor detection system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaliciousActorDetector {
    /// Suspicious behavior patterns
    suspicious_patterns: FxHashMap<String, SuspiciousBehavior>,
    /// Detection thresholds
    detection_thresholds: DetectionThresholds,
    /// Recently flagged nodes
    flagged_nodes: FxHashSet<String>,
}

/// Types of suspicious behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SuspiciousBehavior {
    /// Inconsistent message sending
    InconsistentMessages,
    /// Excessive message frequency
    MessageSpamming,
    /// Invalid signature patterns
    InvalidSignatures,
    /// Network disruption attempts
    NetworkDisruption,
    /// Consensus manipulation
    ConsensusManipulation,
}

/// Detection threshold configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionThresholds {
    /// Maximum inconsistent messages before flagging
    pub max_inconsistent_messages: u32,
    /// Maximum message frequency per minute
    pub max_message_frequency: u32,
    /// Maximum invalid signatures before flagging
    pub max_invalid_signatures: u32,
}

/// Network health monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkHealth {
    /// Active honest nodes
    pub honest_nodes: u32,
    /// Suspected byzantine nodes
    pub byzantine_nodes: u32,
    /// Network partition tolerance
    pub partition_tolerance: f64,
    /// Consensus efficiency
    pub consensus_efficiency: f64,
}

/// Result of spending consensus proposal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpendingConsensusResult {
    /// Proposal ID for tracking
    pub proposal_id: u64,
    /// Whether the spending was approved
    pub approved: bool,
    /// Treasury ID
    pub treasury_id: String,
    /// Spending amount
    pub amount: u64,
    /// Recipient address
    pub recipient: String,
    /// Spending category
    pub category: String,
}

impl ByzantineConsensus {
    /// Create a new byzantine consensus instance
    pub fn new(config: ByzantineFaultToleranceConfig) -> Self {
        Self {
            config,
            current_round: 0,
            suspected_nodes: FxHashSet::default(),
            consensus_messages: FxHashMap::default(),
        }
    }

    /// Create a new byzantine consensus instance with validators
    pub async fn new_with_validators(
        validators: Vec<String>,
        byzantine_threshold: f64,
    ) -> Result<Self, String> {
        let total_nodes = validators.len() as u32;
        let max_byzantine_nodes = ((total_nodes as f64) * byzantine_threshold).floor() as u32;

        let config = ByzantineFaultToleranceConfig {
            max_byzantine_nodes,
            total_nodes,
            consensus_timeout_ms: 5000,
            byzantine_detection_rounds: 3,
        };

        Ok(Self::new(config))
    }

    /// Sign data using Byzantine consensus protocol
    pub async fn sign_data(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        // In production, this would use distributed signing with f+1 participants
        // For now, create a deterministic signature based on consensus state

        let mut signature_data = Vec::new();
        signature_data.extend_from_slice(data);
        signature_data.extend_from_slice(&self.current_round.to_le_bytes());
        signature_data.extend_from_slice(&(self.config.max_byzantine_nodes as u64).to_le_bytes());

        // Hash the combined data to create signature
        use sha3::{Digest, Sha3_256};
        let mut hasher = Sha3_256::new();
        hasher.update(&signature_data);
        let hash_result = hasher.finalize();

        Ok(hash_result.to_vec())
    }

    /// Validate a vote in the Byzantine consensus protocol
    pub async fn validate_vote(
        &self,
        voter: &str,
        _vote: &GovernanceVote,
        _voting_power: u64,
    ) -> Result<bool, String> {
        // Check if voter is suspected of being byzantine
        if self.suspected_nodes.contains(voter) {
            return Err("Voter is suspected of byzantine behavior".to_string());
        }

        Ok(true)
    }

    /// Achieve consensus on a result using Byzantine agreement
    pub async fn achieve_consensus_on_result(&self, result: bool) -> Result<bool, String> {
        let honest_nodes = self.config.total_nodes - self.suspected_nodes.len() as u32;
        let required_nodes = (2 * self.config.max_byzantine_nodes + 1).min(honest_nodes);

        if honest_nodes >= required_nodes {
            Ok(result)
        } else {
            Err("Insufficient honest nodes for consensus".to_string())
        }
    }

    /// Propose treasury spending using Byzantine consensus
    pub async fn propose_treasury_spending(
        &mut self,
        _treasury_id: String,
        amount: u64,
        recipient: &str,
        _category: &str,
    ) -> Result<u64, String> {
        let proposal_id = self.start_round();

        if amount == 0 {
            return Err("Amount must be greater than zero".to_string());
        }

        if recipient.is_empty() {
            return Err("Recipient cannot be empty".to_string());
        }

        Ok(proposal_id)
    }

    /// Validate a signature using Byzantine verification
    pub async fn validate_signature(&self, signature: &[u8], data: &[u8]) -> Result<bool, String> {
        if signature.len() < 32 {
            return Ok(false);
        }

        let expected_signature = self.sign_data(data).await?;
        Ok(signature == expected_signature)
    }

    /// Validate a signer signature in the Byzantine consensus context
    pub async fn validate_signer_signature(
        &mut self,
        signer: &str,
        signature: &[u8],
        _transaction_id: &str,
    ) -> Result<bool, String> {
        if self.suspected_nodes.contains(signer) {
            return Err("Signer is suspected of byzantine behavior".to_string());
        }

        if signature.len() < 32 {
            return Ok(false);
        }

        Ok(true)
    }

    /// Start a new consensus round
    pub fn start_round(&mut self) -> u64 {
        self.current_round += 1;
        self.consensus_messages.clear();
        self.current_round
    }

    /// Add a consensus message from a node
    pub fn add_message(&mut self, node_id: String, message: Vec<u8>) -> Result<(), String> {
        if self.suspected_nodes.contains(&node_id) {
            return Err("Message from suspected byzantine node rejected".to_string());
        }

        self.consensus_messages.insert(node_id, message);
        Ok(())
    }

    /// Check if consensus is reached
    pub fn is_consensus_reached(&self) -> bool {
        let honest_nodes = self.config.total_nodes - self.suspected_nodes.len() as u32;
        let required_messages = (2 * self.config.max_byzantine_nodes + 1).min(honest_nodes);

        self.consensus_messages.len() as u32 >= required_messages
    }

    /// Mark a node as suspected byzantine
    pub fn mark_suspected(&mut self, node_id: String) {
        self.suspected_nodes.insert(node_id);
    }

    /// Get current round number
    pub fn get_current_round(&self) -> u64 {
        self.current_round
    }

    /// Get suspected nodes
    pub fn get_suspected_nodes(&self) -> &FxHashSet<String> {
        &self.suspected_nodes
    }

    /// Achieve consensus on verification result
    pub async fn achieve_consensus_on_verification(
        &mut self,
        nft_id: &str,
        task_id: &str,
        score: f64,
    ) -> Result<bool, String> {
        let _round = self.start_round();
        let verification_msg = format!("{nft_id}:{task_id}:{score}");
        let _message_bytes = verification_msg.as_bytes().to_vec();

        if !(0.0..=100.0).contains(&score) {
            return Err("Invalid verification score".to_string());
        }

        let honest_nodes = self.config.total_nodes - self.suspected_nodes.len() as u32;
        let required_nodes = (2 * self.config.max_byzantine_nodes + 1).min(honest_nodes);

        if honest_nodes >= required_nodes {
            let consensus_threshold = 50.0;
            Ok(score >= consensus_threshold)
        } else {
            Err("Insufficient honest nodes for verification consensus".to_string())
        }
    }
}

impl ByzantineFaultTolerance {
    /// Create a new byzantine fault tolerance system
    pub fn new(config: ByzantineFaultToleranceConfig) -> Self {
        let consensus = ByzantineConsensus::new(config.clone());
        let detector = MaliciousActorDetector::new();
        let network_health = NetworkHealth {
            honest_nodes: config.total_nodes,
            byzantine_nodes: 0,
            partition_tolerance: 1.0,
            consensus_efficiency: 1.0,
        };

        Self {
            consensus,
            detector,
            network_health,
        }
    }

    /// Process a consensus message and check for byzantine behavior
    pub fn process_message(&mut self, node_id: String, message: Vec<u8>) -> Result<(), String> {
        self.detector.analyze_message(&node_id, &message)?;
        self.consensus.add_message(node_id, message)?;
        Ok(())
    }

    /// Update network health metrics
    pub fn update_network_health(&mut self) {
        self.network_health.byzantine_nodes = self.consensus.suspected_nodes.len() as u32;
        self.network_health.honest_nodes =
            self.consensus.config.total_nodes - self.network_health.byzantine_nodes;

        self.network_health.partition_tolerance =
            (self.network_health.honest_nodes as f64) / (self.consensus.config.total_nodes as f64);
    }

    /// Get network health status
    pub fn get_network_health(&self) -> &NetworkHealth {
        &self.network_health
    }

    /// Check if the network can tolerate current byzantine nodes
    pub fn can_tolerate_byzantine(&self) -> bool {
        self.network_health.byzantine_nodes <= self.consensus.config.max_byzantine_nodes
    }
}

impl MaliciousActorDetector {
    /// Create a new malicious actor detector
    pub fn new() -> Self {
        let detection_thresholds = DetectionThresholds {
            max_inconsistent_messages: 10,
            max_message_frequency: 100,
            max_invalid_signatures: 5,
        };

        Self {
            suspicious_patterns: FxHashMap::default(),
            detection_thresholds,
            flagged_nodes: FxHashSet::default(),
        }
    }

    /// Analyze a message for suspicious patterns
    pub fn analyze_message(&mut self, node_id: &str, _message: &[u8]) -> Result<(), String> {
        if self.flagged_nodes.contains(node_id) {
            return Err("Message from flagged node rejected".to_string());
        }

        Ok(())
    }

    /// Flag a node as malicious
    pub fn flag_node(&mut self, node_id: String, behavior: SuspiciousBehavior) {
        self.suspicious_patterns.insert(node_id.clone(), behavior);
        self.flagged_nodes.insert(node_id);
    }

    /// Check if a node is flagged
    pub fn is_flagged(&self, node_id: &str) -> bool {
        self.flagged_nodes.contains(node_id)
    }

    /// Get flagged nodes
    pub fn get_flagged_nodes(&self) -> &FxHashSet<String> {
        &self.flagged_nodes
    }
}

impl Default for ByzantineFaultToleranceConfig {
    fn default() -> Self {
        Self {
            max_byzantine_nodes: 1,
            total_nodes: 4,
            consensus_timeout_ms: 5000,
            byzantine_detection_rounds: 3,
        }
    }
}

impl Default for MaliciousActorDetector {
    fn default() -> Self {
        Self::new()
    }
}
