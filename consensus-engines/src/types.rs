//! Common types used across consensus algorithms
//!
//! This module defines shared data structures, types, and configuration
//! options used by different consensus implementations.

use serde::{Deserialize, Serialize};
use rustc_hash::FxHashMap;
use std::fmt;
use thiserror::Error;

/// Result type for consensus operations
pub type ConsensusResult<T> = Result<T, ConsensusError>;

/// Errors that can occur during consensus operations
#[derive(Debug, Error, Clone)]
pub enum ConsensusError {
    /// Block validation failed
    #[error("Block validation failed: {0}")]
    ValidationFailed(String),

    /// Block creation failed
    #[error("Block creation failed: {0}")]
    CreationFailed(String),

    /// Mining/processing failed
    #[error("Mining failed: {0}")]
    MiningFailed(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigurationError(String),

    /// Initialization error
    #[error("Initialization error: {0}")]
    InitializationError(String),

    /// Insufficient resources (stake, computational power, etc.)
    #[error("Insufficient resources: {0}")]
    InsufficientResources(String),

    /// Invalid consensus state
    #[error("Invalid consensus state: {0}")]
    InvalidState(String),

    /// Network error
    #[error("Network error: {0}")]
    NetworkError(String),

    /// Serialization/deserialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// System time error
    #[error("System time error: {0}")]
    SystemTimeError(String),

    /// Unauthorized access
    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    /// Malicious validator detected
    #[error("Malicious validator: {0}")]
    MaliciousValidator(String),

    /// Invalid signature
    #[error("Invalid signature: {0}")]
    InvalidSignature(String),

    /// Security validation failed
    #[error("Security validation failed: {0}")]
    SecurityValidationFailed(String),

    /// Transition failed
    #[error("Transition failed: {0}")]
    TransitionFailed(String),

    /// Invalid input
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// Invalid session
    #[error("Invalid session: {0}")]
    InvalidSession(String),

    /// Circuit breaker open
    #[error("Circuit breaker open: {0}")]
    CircuitBreakerOpen(String),

    /// Invalid operation
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    /// Security violation detected
    #[error("Security violation: {0}")]
    SecurityViolation(String),

    /// Internal error
    #[error("Internal error: {0}")]
    InternalError(String),

    /// Byzantine failure detected
    #[error("Byzantine failure: {0}")]
    ByzantineFailure(String),

    /// Optimization disabled
    #[error("Optimization disabled: {0}")]
    OptimizationDisabled(String),

    /// Validation error in testing scenarios
    #[error("Validation error: {0}")]
    ValidationError(String),

    /// Attack simulation error
    #[error("Attack simulation error: {0}")]
    AttackSimulationError(String),

    /// Network partition error
    #[error("Network partition error: {0}")]
    NetworkPartitionError(String),

    /// Block production refused because the node is in partition
    /// read-only mode (#1217).
    ///
    /// Emitted by `PopConsensusAdapter::create_next_block` (and the
    /// sibling guard in `ProofOfPublishing::create_next_block`) when the
    /// mempool reports a `PartitionMode` that does not allow block
    /// production. HTTP handlers map this variant to a 503 with code
    /// `PARTITION_READ_ONLY` so the operator's mining loop backs off
    /// instead of retry-storming.
    ///
    /// The string payload describes the current mode label for
    /// operator-facing logs.
    #[error("Block production refused: partition read-only ({0})")]
    PartitionReadOnly(String),

    /// Fault injection error
    #[error("Fault injection error: {0}")]
    FaultInjectionError(String),

    /// System error (general system-level errors)
    #[error("System error: {0}")]
    SystemError(String),

    /// Insufficient data for analysis or operation
    #[error("Insufficient data: {0}")]
    InsufficientData(String),

    /// Generic error
    #[error("Consensus error: {0}")]
    Other(String),

    /// General error for common cases
    #[error("General error: {0}")]
    General(String),
}

impl From<serde_json::Error> for ConsensusError {
    fn from(error: serde_json::Error) -> Self {
        ConsensusError::SerializationError(error.to_string())
    }
}

/// Consensus algorithm type identifier
///
/// This is the consensus-engines crate's own enum for identifying consensus
/// algorithm types. It covers the algorithms supported by this crate, plus
/// additional variants for future extensibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConsensusType {
    /// Proof of Work
    ProofOfWork,
    /// Proof of Stake
    ProofOfStake,
    /// Delegated Proof of Stake
    DelegatedProofOfStake,
    /// Proof of Publishing (for climate data validation)
    ProofOfPublishing,
    /// Proof of Presence (for IoT/sensor data validation)
    ProofOfPresence,
    /// Proof of Usefulness (for computational validation)
    ///
    /// **Placeholder — not yet implemented.**
    ///
    /// This variant is registered in the enum to reserve the identifier and
    /// signal future intent: PoU rewards nodes that perform verifiably useful
    /// computation (e.g. WRF model runs, ML inference jobs).  Until an
    /// implementation is contributed, `ConsensusFactory::create` returns
    /// `ConsensusError::ConfigurationError` for this variant — callers must
    /// handle that error path and MUST NOT unwrap the result.
    ProofOfUsefulness,
    /// Proof of Authority (permissioned validation)
    ProofOfAuthority,
    /// Byzantine Fault Tolerance
    BFT,
    /// HotStuff BFT consensus
    HotStuff,
    /// DAG-based consensus
    DAG,
    /// Raft consensus
    Raft,
    /// Tendermint consensus
    Tendermint,
}

impl fmt::Display for ConsensusType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConsensusType::ProofOfWork => write!(f, "ProofOfWork"),
            ConsensusType::ProofOfStake => write!(f, "ProofOfStake"),
            ConsensusType::DelegatedProofOfStake => write!(f, "DelegatedProofOfStake"),
            ConsensusType::ProofOfPublishing => write!(f, "ProofOfPublishing"),
            ConsensusType::ProofOfPresence => write!(f, "ProofOfPresence"),
            ConsensusType::ProofOfUsefulness => write!(f, "ProofOfUsefulness"),
            ConsensusType::ProofOfAuthority => write!(f, "ProofOfAuthority"),
            ConsensusType::BFT => write!(f, "BFT"),
            ConsensusType::HotStuff => write!(f, "HotStuff"),
            ConsensusType::DAG => write!(f, "DAG"),
            ConsensusType::Raft => write!(f, "Raft"),
            ConsensusType::Tendermint => write!(f, "Tendermint"),
        }
    }
}

/// Configuration parameters for consensus algorithms
#[derive(Debug, Clone)]
pub struct ConsensusConfig {
    /// Target block time in seconds
    pub target_block_time: u64,

    /// Block time in seconds (alias for target_block_time)
    pub block_time: u64,

    /// Consensus algorithm type
    pub consensus_type: String,

    /// Mining difficulty (for PoW)
    pub difficulty: Option<u64>,

    /// Difficulty adjustment interval in blocks
    pub difficulty_adjustment_interval: u64,

    /// Transaction fee in base units
    pub transaction_fee: u64,

    /// Minimum stake required (for PoS)
    pub min_stake: Option<u64>,

    /// Maximum block size in bytes
    pub max_block_size: u64,

    /// Maximum transactions per block
    pub max_transactions_per_block: u64,

    /// Gas limit per block
    pub gas_limit: u64,

    /// Algorithm-specific parameters
    pub algorithm_params: FxHashMap<String, serde_json::Value>,
}

impl Default for ConsensusConfig {
    fn default() -> Self {
        Self {
            target_block_time: 10, // 10 seconds
            block_time: 10,        // 10 seconds
            consensus_type: "pos".to_string(),
            difficulty: Some(1),
            difficulty_adjustment_interval: 2016, // blocks
            transaction_fee: 1000,                // base units
            min_stake: Some(1000),
            max_block_size: 1_000_000, // 1MB
            max_transactions_per_block: 1000,
            gas_limit: 8_000_000,
            algorithm_params: FxHashMap::default(),
        }
    }
}

/// Climate publication data for Proof-of-Publishing consensus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClimatePublication {
    /// Unique content hash
    pub content_hash: String,

    /// Type of climate data
    pub data_type: ClimateDataType,

    /// Author/contributor address
    pub author_address: String,

    /// Publication timestamp
    pub timestamp: u64,

    /// Quality score (0.0 to 1.0)
    pub quality_score: f64,

    /// Verification proofs
    pub verification_proofs: Vec<VerificationProof>,

    /// Raw content (optional, for small data)
    pub content: Option<String>,

    /// External data reference (for large data)
    pub data_reference: Option<String>,

    /// Publication metadata
    pub metadata: FxHashMap<String, serde_json::Value>,
}

/// Types of climate data that can be published
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ClimateDataType {
    /// Weather station data
    WeatherData,

    /// Research paper or study
    ResearchPaper,

    /// Climate model execution results
    ModelResults,

    /// IoT sensor readings
    SensorReading,

    /// Computational NFT execution results
    ComputationalNFT,

    /// Environmental monitoring data
    EnvironmentalData,

    /// Carbon footprint data
    CarbonFootprint,

    /// Renewable energy production data
    RenewableEnergy,
}

/// Verification proof for climate publications
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationProof {
    /// Type of verification
    pub proof_type: VerificationProofType,

    /// Proof data/signature
    pub proof_data: String,

    /// Verifier information
    pub verifier: String,

    /// Verification timestamp
    pub timestamp: u64,

    /// Verification score (0.0 to 1.0)
    pub score: f64,
}

/// Types of verification proofs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VerificationProofType {
    /// Cryptographic signature
    DigitalSignature,

    /// Physics-based consistency check
    PhysicsValidation,

    /// Cross-reference with other data sources
    CrossReference,

    /// Computational verification
    ComputationalProof,

    /// Peer review
    PeerReview,

    /// Automated quality assessment
    QualityAssessment,
}

/// Publication scoring criteria for PoP consensus
#[derive(Debug, Clone)]
pub struct PublicationScore {
    /// Base quality score
    pub quality: f64,

    /// Uniqueness score (novelty)
    pub uniqueness: f64,

    /// Timeliness score
    pub timeliness: f64,

    /// Verification strength
    pub verification: f64,

    /// Author reputation score
    pub reputation: f64,

    /// Final weighted score
    pub total_score: f64,
}

impl PublicationScore {
    /// Calculate the total score from individual components
    pub fn calculate_total(&mut self, weights: &ScoreWeights) {
        self.total_score = self.quality * weights.quality
            + self.uniqueness * weights.uniqueness
            + self.timeliness * weights.timeliness
            + self.verification * weights.verification
            + self.reputation * weights.reputation;
    }
}

/// Weights for different scoring components
#[derive(Debug, Clone)]
pub struct ScoreWeights {
    pub quality: f64,
    pub uniqueness: f64,
    pub timeliness: f64,
    pub verification: f64,
    pub reputation: f64,
}

impl Default for ScoreWeights {
    fn default() -> Self {
        Self {
            quality: 0.3,
            uniqueness: 0.25,
            timeliness: 0.15,
            verification: 0.2,
            reputation: 0.1,
        }
    }
}

/// Validator information for PoS consensus
#[derive(Debug, Clone)]
pub struct ValidatorInfo {
    /// Validator address
    pub address: String,

    /// Staked amount
    pub stake: u64,

    /// Validator reputation score
    pub reputation: f64,

    /// Number of blocks validated
    pub blocks_validated: u64,

    /// Last activity timestamp
    pub last_activity: u64,

    /// Whether validator is currently active
    pub is_active: bool,
}

/// Mining statistics for PoW consensus
#[derive(Debug, Clone)]
pub struct MiningStats {
    /// Current hash rate
    pub hash_rate: u64,

    /// Number of blocks mined
    pub blocks_mined: u64,

    /// Mining efficiency
    pub efficiency: f64,

    /// Power consumption estimate
    pub power_consumption: Option<f64>,

    /// Mining pool information
    pub pool_info: Option<String>,
}
