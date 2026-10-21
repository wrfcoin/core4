//! HotStuff BFT Type Definitions
//!
//! Core data structures for the HotStuff Byzantine fault-tolerant consensus protocol.
//! HotStuff provides O(n) message complexity through linear view changes and
//! pipelined voting phases.
//!
//! Adapted from `blockchain-core/src/consensus/hotstuff/types.rs`.

use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime};

/// HotStuff phase in the three-phase commit protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HotStuffPhase {
    /// Prepare phase - leader proposes, replicas vote
    Prepare,
    /// Pre-commit phase - QC on prepare votes
    PreCommit,
    /// Commit phase - QC on pre-commit votes
    Commit,
    /// Decision phase - block is finalized
    Decide,
}

impl HotStuffPhase {
    /// Get the next phase in the protocol
    pub fn next(&self) -> Option<Self> {
        match self {
            HotStuffPhase::Prepare => Some(HotStuffPhase::PreCommit),
            HotStuffPhase::PreCommit => Some(HotStuffPhase::Commit),
            HotStuffPhase::Commit => Some(HotStuffPhase::Decide),
            HotStuffPhase::Decide => None,
        }
    }

    /// Check if this is a voting phase
    pub fn is_voting_phase(&self) -> bool {
        matches!(
            self,
            HotStuffPhase::Prepare | HotStuffPhase::PreCommit | HotStuffPhase::Commit
        )
    }
}

/// View number in HotStuff (analogous to round number)
pub type ViewNumber = u64;

/// Block height
pub type BlockHeight = u64;

/// Node identifier
pub type NodeId = String;

/// Block hash (32 bytes, hex encoded)
pub type BlockHash = String;

/// Signature bytes
pub type SignatureBytes = Vec<u8>;

/// Public key bytes
pub type PublicKeyBytes = Vec<u8>;

/// Quorum Certificate (QC) - aggregated proof of votes
///
/// A QC proves that a quorum of replicas voted for a particular block
/// in a specific phase. QCs are chained together to form the consensus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuorumCertificate {
    /// The phase this QC certifies
    pub phase: HotStuffPhase,
    /// View number when votes were collected
    pub view_number: ViewNumber,
    /// Hash of the block being certified
    pub block_hash: BlockHash,
    /// Block height
    pub block_height: BlockHeight,
    /// Aggregated BLS signature (or individual signatures if BLS unavailable)
    pub aggregated_signature: AggregatedSignature,
    /// Number of signers
    pub signer_count: u32,
    /// Bitmap of which validators signed (for efficient verification)
    pub signer_bitmap: Vec<u8>,
    /// Timestamp when QC was formed
    pub timestamp: u64,
}

impl QuorumCertificate {
    /// Create a new QC with the given parameters
    pub fn new(
        phase: HotStuffPhase,
        view_number: ViewNumber,
        block_hash: BlockHash,
        block_height: BlockHeight,
        aggregated_signature: AggregatedSignature,
        signer_count: u32,
        signer_bitmap: Vec<u8>,
    ) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Self {
            phase,
            view_number,
            block_hash,
            block_height,
            aggregated_signature,
            signer_count,
            signer_bitmap,
            timestamp,
        }
    }

    /// Check if this QC has enough signers for a quorum (2f+1)
    pub fn has_quorum(&self, total_validators: u32) -> bool {
        let required = Self::quorum_threshold(total_validators);
        self.signer_count >= required
    }

    /// Calculate quorum threshold (2f+1 where n = 3f+1)
    pub fn quorum_threshold(total_validators: u32) -> u32 {
        // For n = 3f + 1, quorum = 2f + 1 = (2n + 1) / 3
        (2 * total_validators).div_ceil(3)
    }
}

/// Aggregated signature (BLS or multi-signature)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AggregatedSignature {
    /// BLS aggregate signature (single signature for all voters)
    BLS(SignatureBytes),
    /// Multi-signature (individual signatures combined)
    MultiSig(Vec<IndividualSignature>),
    /// Threshold signature (k-of-n)
    Threshold {
        signature: SignatureBytes,
        threshold: u32,
        total: u32,
    },
}

/// Individual signature from a validator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndividualSignature {
    /// Signer's node ID
    pub signer_id: NodeId,
    /// Signature bytes
    pub signature: SignatureBytes,
    /// Signer's public key
    pub public_key: PublicKeyBytes,
}

/// HotStuff block proposal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotStuffProposal {
    /// Block hash
    pub block_hash: BlockHash,
    /// Block height
    pub block_height: BlockHeight,
    /// Parent block hash
    pub parent_hash: BlockHash,
    /// View number
    pub view_number: ViewNumber,
    /// Leader who created this proposal
    pub leader_id: NodeId,
    /// QC justifying this proposal (from parent block)
    pub justify_qc: QuorumCertificate,
    /// Block payload (transactions, state root, etc.)
    pub payload: ProposalPayload,
    /// Leader's signature on the proposal
    pub signature: SignatureBytes,
    /// Proposal timestamp
    pub timestamp: u64,
}

/// Block payload in a proposal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposalPayload {
    /// Transaction hashes included in this block
    pub transaction_hashes: Vec<String>,
    /// State root after applying transactions
    pub state_root: String,
    /// Timestamp
    pub timestamp: u64,
    /// Extra data (consensus-specific)
    pub extra_data: Vec<u8>,
}

/// Vote message in HotStuff
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotStuffVote {
    /// Phase being voted on
    pub phase: HotStuffPhase,
    /// View number
    pub view_number: ViewNumber,
    /// Block hash being voted for
    pub block_hash: BlockHash,
    /// Block height
    pub block_height: BlockHeight,
    /// Voter's node ID
    pub voter_id: NodeId,
    /// Voter's signature
    pub signature: SignatureBytes,
    /// Timestamp
    pub timestamp: u64,
}

impl HotStuffVote {
    /// Create a new vote
    pub fn new(
        phase: HotStuffPhase,
        view_number: ViewNumber,
        block_hash: BlockHash,
        block_height: BlockHeight,
        voter_id: NodeId,
        signature: SignatureBytes,
    ) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Self {
            phase,
            view_number,
            block_hash,
            block_height,
            voter_id,
            signature,
            timestamp,
        }
    }

    /// Create the message to be signed
    pub fn sign_message(&self) -> Vec<u8> {
        let mut message = Vec::new();
        message.extend_from_slice(&[self.phase as u8]);
        message.extend_from_slice(&self.view_number.to_le_bytes());
        message.extend_from_slice(self.block_hash.as_bytes());
        message.extend_from_slice(&self.block_height.to_le_bytes());
        message
    }
}

/// New view message for view change (leader election)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewViewMessage {
    /// New view number
    pub new_view: ViewNumber,
    /// Sender's node ID
    pub sender_id: NodeId,
    /// Highest QC known by sender (prepareQC)
    pub high_qc: QuorumCertificate,
    /// Sender's signature
    pub signature: SignatureBytes,
    /// Timestamp
    pub timestamp: u64,
}

/// Timeout certificate for view change
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutCertificate {
    /// View that timed out
    pub view_number: ViewNumber,
    /// Highest QC seen by any participant
    pub high_qc: QuorumCertificate,
    /// Signatures from nodes that timed out
    pub timeout_signatures: Vec<TimeoutSignature>,
    /// Number of timeouts
    pub timeout_count: u32,
}

/// Individual timeout signature
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutSignature {
    /// Node that timed out
    pub node_id: NodeId,
    /// Their highest QC view
    pub high_qc_view: ViewNumber,
    /// Signature
    pub signature: SignatureBytes,
}

/// Pipelined block in the chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelinedBlock {
    /// Block hash
    pub hash: BlockHash,
    /// Block height
    pub height: BlockHeight,
    /// Parent hash
    pub parent_hash: BlockHash,
    /// View number when proposed
    pub view_number: ViewNumber,
    /// Proposal data
    pub proposal: HotStuffProposal,
    /// Prepare QC (if received)
    pub prepare_qc: Option<QuorumCertificate>,
    /// Pre-commit QC (if received)
    pub precommit_qc: Option<QuorumCertificate>,
    /// Commit QC (if received)
    pub commit_qc: Option<QuorumCertificate>,
    /// Whether block is finalized (committed)
    pub is_committed: bool,
}

impl PipelinedBlock {
    /// Get the current phase based on QCs received
    pub fn current_phase(&self) -> HotStuffPhase {
        if self.commit_qc.is_some() {
            HotStuffPhase::Decide
        } else if self.precommit_qc.is_some() {
            HotStuffPhase::Commit
        } else if self.prepare_qc.is_some() {
            HotStuffPhase::PreCommit
        } else {
            HotStuffPhase::Prepare
        }
    }
}

/// HotStuff consensus state
#[derive(Debug, Clone)]
pub struct HotStuffState {
    /// Current view number
    pub current_view: ViewNumber,
    /// Current leader for this view
    pub current_leader: NodeId,
    /// This node's ID
    pub node_id: NodeId,
    /// Whether this node is the current leader
    pub is_leader: bool,
    /// Current phase (for non-pipelined mode)
    pub current_phase: HotStuffPhase,
    /// Locked QC (highest pre-commit QC seen)
    pub locked_qc: Option<QuorumCertificate>,
    /// Prepared QC (highest prepare QC seen)
    pub prepare_qc: Option<QuorumCertificate>,
    /// Committed block height
    pub committed_height: BlockHeight,
    /// Latest proposed block
    pub latest_proposal: Option<HotStuffProposal>,
    /// Pending votes for current view
    pub pending_votes: FxHashMap<HotStuffPhase, Vec<HotStuffVote>>,
}

/// Validator information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotStuffValidator {
    /// Validator's node ID
    pub node_id: NodeId,
    /// Public key for signature verification
    pub public_key: PublicKeyBytes,
    /// Voting power (usually stake-weighted)
    pub voting_power: u64,
    /// Whether validator is active
    pub is_active: bool,
    /// Index in the validator set (for bitmap)
    pub index: u32,
}

/// Validator set for a view
#[derive(Debug, Clone)]
pub struct ValidatorSet {
    /// All validators
    pub validators: Vec<HotStuffValidator>,
    /// Total voting power
    pub total_voting_power: u128,
    /// Mapping from node ID to validator
    pub validators_by_id: FxHashMap<NodeId, HotStuffValidator>,
    /// View this set is valid for
    pub valid_from_view: ViewNumber,
}

impl ValidatorSet {
    /// Create a new validator set
    pub fn new(validators: Vec<HotStuffValidator>, valid_from_view: ViewNumber) -> Self {
        let total_voting_power = validators.iter().map(|v| u128::from(v.voting_power)).sum();
        let validators_by_id = validators
            .iter()
            .map(|v| (v.node_id.clone(), v.clone()))
            .collect();

        Self {
            validators,
            total_voting_power,
            validators_by_id,
            valid_from_view,
        }
    }

    /// Get leader for a view (round-robin based on index)
    pub fn get_leader(&self, view: ViewNumber) -> Option<&HotStuffValidator> {
        let active: Vec<_> = self.validators.iter().filter(|v| v.is_active).collect();
        if active.is_empty() {
            return None;
        }
        let leader_idx = (view as usize) % active.len();
        Some(active[leader_idx])
    }

    /// Get validator by ID
    pub fn get_validator(&self, node_id: &str) -> Option<&HotStuffValidator> {
        self.validators_by_id.get(node_id)
    }

    /// Get quorum threshold
    pub fn quorum_threshold(&self) -> u128 {
        // For voting power: 2/3 + 1
        (2 * self.total_voting_power).div_ceil(3)
    }

    /// Count of active validators
    pub fn active_count(&self) -> u32 {
        self.validators.iter().filter(|v| v.is_active).count() as u32
    }
}

/// HotStuff consensus configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotStuffConfig {
    /// Enable pipelining (concurrent phase processing)
    pub enable_pipelining: bool,
    /// View timeout duration
    pub view_timeout: Duration,
    /// Delta for optimistic responsiveness (network delay bound)
    pub delta: Duration,
    /// Minimum validators for consensus
    pub min_validators: u32,
    /// Maximum block size (bytes)
    pub max_block_size: usize,
    /// Maximum transactions per block
    pub max_transactions_per_block: usize,
    /// Enable optimistic mode (fast path without timeout)
    pub enable_optimistic: bool,
    /// Signature scheme
    pub signature_scheme: SignatureScheme,
    /// Enable equivocation detection
    pub enable_equivocation_detection: bool,
    /// Leader rotation interval (views)
    pub leader_rotation_interval: u64,
}

impl Default for HotStuffConfig {
    fn default() -> Self {
        Self {
            enable_pipelining: true,
            view_timeout: Duration::from_secs(5),
            delta: Duration::from_millis(500),
            min_validators: 4,               // Minimum for 3f+1 with f=1
            max_block_size: 2 * 1024 * 1024, // 2MB
            max_transactions_per_block: 5000,
            enable_optimistic: true,
            signature_scheme: SignatureScheme::Ed25519,
            enable_equivocation_detection: true,
            leader_rotation_interval: 1, // Rotate every view
        }
    }
}

/// Supported signature schemes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignatureScheme {
    /// Ed25519 signatures (individual)
    Ed25519,
    /// BLS signatures (aggregatable)
    BLS,
    /// Secp256k1 signatures
    Secp256k1,
}

/// HotStuff message types for network communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HotStuffMessage {
    /// Block proposal from leader
    Proposal(HotStuffProposal),
    /// Vote on a proposal
    Vote(HotStuffVote),
    /// New view message for view change
    NewView(NewViewMessage),
    /// Timeout notification
    Timeout(TimeoutSignature),
    /// Sync request (get missing blocks/QCs)
    SyncRequest(SyncRequest),
    /// Sync response
    SyncResponse(SyncResponse),
}

/// Sync request for catching up
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRequest {
    /// Requester's node ID
    pub requester_id: NodeId,
    /// Start height
    pub from_height: BlockHeight,
    /// End height (if known)
    pub to_height: Option<BlockHeight>,
    /// Request QCs as well
    pub include_qcs: bool,
}

/// Sync response with blocks and QCs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResponse {
    /// Blocks in the range
    pub blocks: Vec<PipelinedBlock>,
    /// Highest QC known
    pub high_qc: Option<QuorumCertificate>,
}

/// Metrics for HotStuff consensus
#[derive(Debug, Clone, Default)]
pub struct HotStuffMetrics {
    /// Total proposals created
    pub proposals_created: u64,
    /// Total votes sent
    pub votes_sent: u64,
    /// Total votes received
    pub votes_received: u64,
    /// Total QCs formed
    pub qcs_formed: u64,
    /// Total blocks committed
    pub blocks_committed: u64,
    /// View changes
    pub view_changes: u64,
    /// Timeouts
    pub timeouts: u64,
    /// Average latency to commit (ms)
    pub avg_commit_latency_ms: f64,
    /// Messages sent
    pub messages_sent: u64,
    /// Messages received
    pub messages_received: u64,
}

/// Error types for HotStuff consensus
#[derive(Debug, Clone, thiserror::Error)]
pub enum HotStuffError {
    #[error("Invalid signature: {0}")]
    InvalidSignature(String),
    #[error("Invalid QC: {0}")]
    InvalidQC(String),
    #[error("Not leader for view {0}")]
    NotLeader(ViewNumber),
    #[error("View mismatch: expected {expected}, got {got}")]
    ViewMismatch {
        expected: ViewNumber,
        got: ViewNumber,
    },
    #[error("Invalid proposal: {0}")]
    InvalidProposal(String),
    #[error("Invalid vote: {0}")]
    InvalidVote(String),
    #[error("Invalid new view message: {0}")]
    InvalidNewView(String),
    #[error("Equivocation detected: {0}")]
    Equivocation(String),
    #[error("Safety violation: {0}")]
    SafetyViolation(String),
    #[error("Liveness issue: {0}")]
    LivenessIssue(String),
    #[error("Not enough validators: have {have}, need {need}")]
    NotEnoughValidators { have: u32, need: u32 },
    #[error("Timeout in view {0}")]
    Timeout(ViewNumber),
    #[error("Network error: {0}")]
    NetworkError(String),
    #[error("Crypto error: {0}")]
    CryptoError(String),
    #[error("State error: {0}")]
    StateError(String),
}
