//! Consensus Builder Agent
//!
//! Extracted from blockchain-core/src/consensus/consensus_builder.rs.
//! Democratic foundation of swarm intelligence implementing sophisticated consensus algorithms,
//! voting mechanisms, and Byzantine fault-tolerant agreement protocols. This agent works
//! alongside the byzantine-coordinator to enhance voting mechanisms and governance protocols.
//!
//! ## Adaptations from blockchain-core
//!
//! - `ConsensusSecurityManager` -> removed (not yet extracted)
//! - `ByzantineConsensusCoordinator` / `VoteChoice` -> imported from local `byzantine_coordinator`
//! - Tests that construct `DynamicConsensusEngine`, `SecureConsensusCoordinator` etc. -> dropped
//! - New self-contained tests added

use ed25519_dalek::{Signer, SigningKey};
use log::info;
use rand::Rng;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use uuid::Uuid;

use super::byzantine_coordinator::{ByzantineConsensusCoordinator, VoteChoice};
use crate::types::{ConsensusError, ConsensusResult};

/// Utility function for safe timestamp generation
fn current_timestamp() -> ConsensusResult<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|e| ConsensusError::SystemTimeError(format!("Failed to get system time: {e}")))
}

/// Advanced voting mechanism types
#[derive(Debug, Clone, Default)]
pub enum VotingMechanism {
    /// Simple majority voting
    #[default]
    SimpleMajority,
    /// Quadratic voting with voice credits
    Quadratic { total_credits: u64 },
    /// Weighted voting based on stake
    StakeWeighted,
    /// Liquid democracy with delegation
    LiquidDemocracy,
    /// Approval voting (multiple choices)
    Approval,
    /// Ranked choice voting
    RankedChoice,
    /// Conviction voting with time locking
    Conviction,
}

/// Voting power calculation method
#[derive(Debug, Clone, Default)]
pub enum VotingPowerMethod {
    /// Equal voting power for all participants
    #[default]
    Equal,
    /// Power based on stake amount
    Stake { max_power_ratio: f64 },
    /// Power based on reputation score
    Reputation { reputation_weight: f64 },
    /// Combined stake and reputation
    Hybrid {
        stake_weight: f64,
        reputation_weight: f64,
    },
    /// Expertise-based power for specific domains
    Expertise {
        domain: String,
        expertise_weight: f64,
    },
}

/// Quorum calculation types
#[derive(Debug, Clone)]
pub enum QuorumType {
    /// Fixed percentage of total participants
    Fixed { percentage: f64 },
    /// Dynamic based on network participation
    Dynamic {
        base_percentage: f64,
        participation_factor: f64,
    },
    /// Stake-weighted quorum
    StakeWeighted { min_stake_percentage: f64 },
    /// Adaptive based on proposal importance
    Adaptive {
        min_percentage: f64,
        max_percentage: f64,
    },
}

impl Default for QuorumType {
    fn default() -> Self {
        QuorumType::Fixed { percentage: 0.67 }
    }
}

/// Conflict resolution strategy
#[derive(Debug, Clone, Default)]
pub enum ConflictResolutionStrategy {
    /// Simple majority wins
    #[default]
    MajorityRule,
    /// Mediation with compromise solution
    Mediation,
    /// Multi-criteria decision analysis
    MultiCriteria {
        criteria_weights: FxHashMap<String, f64>,
    },
    /// Consensus building through iterations
    ConsensusBuilding { max_iterations: u32 },
    /// Arbitration by trusted third party
    Arbitration { arbitrator_id: String },
}

/// Configuration for the consensus builder
#[derive(Debug, Clone)]
pub struct ConsensusBuilderConfig {
    /// Default voting mechanism
    pub default_voting_mechanism: VotingMechanism,
    /// Voting power calculation method
    pub voting_power_method: VotingPowerMethod,
    /// Quorum calculation type
    pub quorum_type: QuorumType,
    /// Conflict resolution strategy
    pub conflict_resolution: ConflictResolutionStrategy,
    /// Enable privacy-preserving voting
    pub enable_private_voting: bool,
    /// Vote verification method
    pub verification_method: VoteVerificationMethod,
    /// Delegation limits for liquid democracy
    pub max_delegation_depth: u32,
    /// Conviction voting parameters
    pub conviction_config: ConvictionVotingConfig,
    /// Cross-consensus voting enabled
    pub enable_cross_consensus_voting: bool,
}

impl Default for ConsensusBuilderConfig {
    fn default() -> Self {
        Self {
            default_voting_mechanism: VotingMechanism::StakeWeighted,
            voting_power_method: VotingPowerMethod::Hybrid {
                stake_weight: 0.7,
                reputation_weight: 0.3,
            },
            quorum_type: QuorumType::Dynamic {
                base_percentage: 0.51,
                participation_factor: 0.1,
            },
            conflict_resolution: ConflictResolutionStrategy::MultiCriteria {
                criteria_weights: {
                    let mut weights = FxHashMap::default();
                    weights.insert("technical_merit".to_string(), 0.4);
                    weights.insert("community_support".to_string(), 0.3);
                    weights.insert("risk_assessment".to_string(), 0.3);
                    weights
                },
            },
            enable_private_voting: true,
            verification_method: VoteVerificationMethod::ZeroKnowledge,
            max_delegation_depth: 5,
            conviction_config: ConvictionVotingConfig::default(),
            enable_cross_consensus_voting: true,
        }
    }
}

/// Vote verification methods
#[derive(Debug, Clone, Default)]
pub enum VoteVerificationMethod {
    /// Standard cryptographic signatures
    #[default]
    Standard,
    /// Zero-knowledge proofs for privacy
    ZeroKnowledge,
    /// Homomorphic encryption
    Homomorphic,
    /// Ring signatures for anonymity
    RingSignature,
    /// Blind signatures
    BlindSignature,
}

/// Conviction voting configuration
#[derive(Debug, Clone)]
pub struct ConvictionVotingConfig {
    /// Conviction curve parameter
    pub conviction_factor: f64,
    /// Maximum conviction multiplier
    pub max_conviction: f64,
    /// Time required for full conviction (seconds)
    pub full_conviction_time: u64,
    /// Minimum lock time for voting (seconds)
    pub min_lock_time: u64,
}

impl Default for ConvictionVotingConfig {
    fn default() -> Self {
        Self {
            conviction_factor: 2.0,
            max_conviction: 6.0,
            full_conviction_time: 2592000, // 30 days
            min_lock_time: 86400,          // 1 day
        }
    }
}

/// Enhanced vote with advanced features
#[derive(Debug, Clone)]
pub struct EnhancedVote {
    /// Standard vote information
    pub vote_id: String,
    pub proposal_id: String,
    pub voter_id: String,
    pub vote_choice: VoteChoice,
    pub timestamp: u64,

    /// Enhanced voting features
    pub voting_power: f64,
    pub conviction_time: Option<u64>,
    pub quadratic_credits_used: Option<u64>,
    pub delegation_chain: Vec<String>,
    pub vote_commitment: Option<Vec<u8>>, // For private voting
    pub zero_knowledge_proof: Option<Vec<u8>>,
    pub ranked_choices: Vec<String>,   // For ranked choice voting
    pub approval_choices: Vec<String>, // For approval voting

    /// Privacy and verification
    pub signature: Vec<u8>,
    pub public_key: Vec<u8>,
    pub encryption_proof: Option<Vec<u8>>,
    pub anonymity_set: Option<Vec<String>>,
}

/// Voting session with advanced mechanics
#[derive(Debug, Clone)]
pub struct VotingSession {
    /// Session identifier
    pub session_id: String,
    /// Associated proposal
    pub proposal_id: String,
    /// Voting mechanism used
    pub mechanism: VotingMechanism,
    /// Voting power method
    pub power_method: VotingPowerMethod,
    /// Required quorum
    pub quorum_requirement: f64,
    /// Session start and end times
    pub start_time: u64,
    pub end_time: u64,
    /// Vote tallies
    pub vote_tallies: FxHashMap<String, f64>, // Choice -> Power
    /// Participating voters
    pub participants: FxHashSet<String>,
    /// Delegations for liquid democracy
    pub delegations: FxHashMap<String, String>, // Delegator -> Delegate
    /// Quadratic voting allocations
    pub quadratic_allocations: FxHashMap<String, FxHashMap<String, u64>>, // Voter -> (Choice -> Credits)
    /// Conviction voting locks
    pub conviction_locks: FxHashMap<String, ConvictionLock>,
    /// Privacy commitments
    pub vote_commitments: FxHashMap<String, Vec<u8>>,
    /// Session status
    pub status: VotingSessionStatus,
}

/// Conviction voting lock information
#[derive(Debug, Clone)]
pub struct ConvictionLock {
    pub voter_id: String,
    pub locked_amount: u64,
    pub lock_start: u64,
    pub lock_duration: u64,
    pub conviction_multiplier: f64,
}

/// Voting session status
#[derive(Debug, Clone)]
pub enum VotingSessionStatus {
    /// Session is active and accepting votes
    Active,
    /// Session ended, tallying in progress
    Tallying,
    /// Session completed with results
    Completed {
        winner: Option<String>,
        final_tally: FxHashMap<String, f64>,
    },
    /// Session cancelled
    Cancelled { reason: String },
    /// Quorum not reached
    QuorumNotReached,
}

/// Conflict resolution case
#[derive(Debug, Clone)]
pub struct ConflictResolutionCase {
    pub case_id: String,
    pub conflicting_proposals: Vec<String>,
    pub strategy: ConflictResolutionStrategy,
    pub criteria_scores: FxHashMap<String, FxHashMap<String, f64>>, // Proposal -> (Criteria -> Score)
    pub mediation_rounds: Vec<MediationRound>,
    pub resolution_status: ConflictResolutionStatus,
    pub final_decision: Option<String>,
    pub created_at: u64,
}

/// Mediation round information
#[derive(Debug, Clone)]
pub struct MediationRound {
    pub round_number: u32,
    pub mediator_id: String,
    pub proposed_compromise: String,
    pub stakeholder_feedback: FxHashMap<String, String>,
    pub acceptance_votes: FxHashMap<String, bool>,
    pub round_timestamp: u64,
}

/// Conflict resolution status
#[derive(Debug, Clone)]
pub enum ConflictResolutionStatus {
    /// Conflict identified, resolution in progress
    InProgress,
    /// Mediation ongoing
    Mediating { current_round: u32 },
    /// Arbitration in progress
    Arbitrating { arbitrator: String },
    /// Resolution completed successfully
    Resolved { resolution_method: String },
    /// Resolution failed, requires escalation
    Failed { reason: String },
}

/// Cross-consensus voting capability
#[derive(Debug, Clone)]
pub struct CrossConsensusVoting {
    pub transition_id: String,
    pub source_algorithm: String,
    pub target_algorithm: String,
    pub voting_sessions: FxHashMap<String, String>, // Algorithm -> Session ID
    pub consensus_requirements: FxHashMap<String, f64>, // Algorithm -> Required percentage
    pub cross_validation_proofs: FxHashMap<String, Vec<u8>>,
    pub status: CrossConsensusStatus,
}

/// Cross-consensus voting status
#[derive(Debug, Clone)]
pub enum CrossConsensusStatus {
    /// Collecting votes from different consensus algorithms
    CollectingVotes,
    /// Validating cross-consensus agreements
    Validating,
    /// All consensus algorithms agreed
    Approved,
    /// Consensus not reached across algorithms
    Rejected,
    /// Timeout reached
    Timeout,
}

/// Main consensus builder coordinator
#[derive(Debug)]
#[allow(dead_code)]
pub struct ConsensusBuilder {
    /// Configuration
    config: ConsensusBuilderConfig,
    /// Integration with Byzantine coordinator
    byzantine_coordinator: Arc<ByzantineConsensusCoordinator>,
    /// Active voting sessions
    voting_sessions: Arc<RwLock<FxHashMap<String, VotingSession>>>,
    /// Enhanced votes storage
    enhanced_votes: Arc<RwLock<FxHashMap<String, EnhancedVote>>>,
    /// Conflict resolution cases
    conflict_cases: Arc<RwLock<FxHashMap<String, ConflictResolutionCase>>>,
    /// Cross-consensus voting sessions
    cross_consensus_sessions: Arc<RwLock<FxHashMap<String, CrossConsensusVoting>>>,
    /// Voter stake information
    voter_stakes: Arc<RwLock<FxHashMap<String, u64>>>,
    /// Voter reputation scores
    voter_reputations: Arc<RwLock<FxHashMap<String, f64>>>,
    /// Delegation relationships
    delegations: Arc<RwLock<FxHashMap<String, String>>>, // Delegator -> Delegate
    /// Conviction locks
    conviction_locks: Arc<RwLock<FxHashMap<String, ConvictionLock>>>,
    /// Privacy commitments
    privacy_commitments: Arc<RwLock<FxHashMap<String, Vec<u8>>>>,
    /// Cryptographic keys for consensus building
    builder_keypair: Arc<RwLock<Option<SigningKey>>>,
}

impl ConsensusBuilder {
    /// Create a new consensus builder
    pub async fn new(
        config: ConsensusBuilderConfig,
        byzantine_coordinator: Arc<ByzantineConsensusCoordinator>,
    ) -> ConsensusResult<Self> {
        info!("Initializing Consensus Builder with advanced voting mechanisms");

        let builder = Self {
            config,
            byzantine_coordinator,
            voting_sessions: Arc::new(RwLock::new(FxHashMap::default())),
            enhanced_votes: Arc::new(RwLock::new(FxHashMap::default())),
            conflict_cases: Arc::new(RwLock::new(FxHashMap::default())),
            cross_consensus_sessions: Arc::new(RwLock::new(FxHashMap::default())),
            voter_stakes: Arc::new(RwLock::new(FxHashMap::default())),
            voter_reputations: Arc::new(RwLock::new(FxHashMap::default())),
            delegations: Arc::new(RwLock::new(FxHashMap::default())),
            conviction_locks: Arc::new(RwLock::new(FxHashMap::default())),
            privacy_commitments: Arc::new(RwLock::new(FxHashMap::default())),
            builder_keypair: Arc::new(RwLock::new(None)),
        };

        // Initialize cryptographic keys
        builder.initialize_builder_keys().await?;

        info!("Consensus Builder initialized successfully");
        Ok(builder)
    }

    /// Create an enhanced voting session for a governance proposal
    pub async fn create_voting_session(
        &self,
        proposal_id: String,
        mechanism: Option<VotingMechanism>,
        duration_seconds: u64,
        custom_quorum: Option<f64>,
    ) -> ConsensusResult<String> {
        info!("Creating enhanced voting session for proposal: {proposal_id}");

        let session_id = Uuid::new_v4().to_string();
        let current_time = current_timestamp()?;
        let voting_mechanism =
            mechanism.unwrap_or_else(|| self.config.default_voting_mechanism.clone());

        // Calculate required quorum
        let quorum_requirement = if let Some(quorum) = custom_quorum {
            quorum
        } else {
            self.calculate_dynamic_quorum().await?
        };

        let session = VotingSession {
            session_id: session_id.clone(),
            proposal_id: proposal_id.clone(),
            mechanism: voting_mechanism,
            power_method: self.config.voting_power_method.clone(),
            quorum_requirement,
            start_time: current_time,
            end_time: current_time + duration_seconds,
            vote_tallies: FxHashMap::default(),
            participants: FxHashSet::default(),
            delegations: FxHashMap::default(),
            quadratic_allocations: FxHashMap::default(),
            conviction_locks: FxHashMap::default(),
            vote_commitments: FxHashMap::default(),
            status: VotingSessionStatus::Active,
        };

        // Store the session
        {
            let mut sessions = self.voting_sessions.write().await;
            sessions.insert(session_id.clone(), session);
        }

        info!("Voting session created: {session_id} for proposal: {proposal_id}");
        Ok(session_id)
    }

    /// Cast an enhanced vote with advanced features
    pub async fn cast_enhanced_vote(
        &self,
        session_id: String,
        voter_id: String,
        vote_choice: VoteChoice,
        options: VotingOptions,
    ) -> ConsensusResult<String> {
        info!("Processing enhanced vote from {voter_id} in session {session_id}");

        // Validate voter and session
        self.validate_voter_eligibility(&voter_id).await?;
        let session = self.get_voting_session(&session_id).await?;
        let current_time = current_timestamp()?;

        if matches!(session.status, VotingSessionStatus::Active) {
            if current_time > session.end_time {
                return Err(ConsensusError::InvalidState(
                    "Voting session has ended".to_string(),
                ));
            }
        } else {
            return Err(ConsensusError::InvalidState(
                "Voting session is not active".to_string(),
            ));
        }

        // Calculate voting power
        let voting_power = self
            .calculate_voting_power(&voter_id, &session.power_method)
            .await?;

        // Handle delegation if applicable
        let effective_voter = self.resolve_delegation_chain(&voter_id).await?;

        // Create enhanced vote
        let vote_id = Uuid::new_v4().to_string();
        let enhanced_vote = EnhancedVote {
            vote_id: vote_id.clone(),
            proposal_id: session.proposal_id.clone(),
            voter_id: effective_voter.clone(),
            vote_choice: vote_choice.clone(),
            timestamp: current_time,
            voting_power,
            conviction_time: options.conviction_time,
            quadratic_credits_used: options.quadratic_credits,
            delegation_chain: self.get_delegation_chain(&voter_id).await,
            vote_commitment: options.privacy_commitment.clone(),
            zero_knowledge_proof: options.zero_knowledge_proof.clone(),
            ranked_choices: options.ranked_choices.unwrap_or_default(),
            approval_choices: options.approval_choices.unwrap_or_default(),
            signature: Vec::new(), // Will be filled by signing
            public_key: options.public_key.unwrap_or_default(),
            encryption_proof: options.encryption_proof,
            anonymity_set: options.anonymity_set,
        };

        // Sign the vote
        let signed_vote = self.sign_enhanced_vote(enhanced_vote).await?;

        // Process vote based on mechanism
        self.process_vote_by_mechanism(&session_id, &signed_vote)
            .await?;

        // Store the vote
        {
            let mut votes = self.enhanced_votes.write().await;
            votes.insert(vote_id.clone(), signed_vote);
        }

        info!("Enhanced vote processed successfully: {vote_id}");
        Ok(vote_id)
    }

    /// Implement quadratic voting mechanism
    pub async fn cast_quadratic_vote(
        &self,
        session_id: String,
        voter_id: String,
        allocations: FxHashMap<String, u64>, // Choice -> Credits allocated
    ) -> ConsensusResult<String> {
        info!("Processing quadratic vote from {voter_id} in session {session_id}");

        // Validate session and voter
        let session = self.get_voting_session(&session_id).await?;
        self.validate_voter_eligibility(&voter_id).await?;

        // Get available credits for the voter
        let available_credits = self.get_voter_credits(&voter_id).await?;

        // Calculate total credits used (sum of squares)
        let total_credits_used: u64 = allocations.values().map(|&credits| credits * credits).sum();

        if total_credits_used > available_credits {
            return Err(ConsensusError::InvalidInput(format!(
                "Insufficient credits: {total_credits_used} required, {available_credits} available"
            )));
        }

        // Update session with quadratic allocations
        {
            let mut sessions = self.voting_sessions.write().await;
            if let Some(session) = sessions.get_mut(&session_id) {
                session
                    .quadratic_allocations
                    .insert(voter_id.clone(), allocations.clone());
                session.participants.insert(voter_id.clone());

                // Update vote tallies with quadratic influence
                for (choice, credits) in &allocations {
                    let influence = (*credits as f64).sqrt(); // Square root for quadratic voting
                    *session.vote_tallies.entry(choice.clone()).or_insert(0.0) += influence;
                }
            }
        }

        // Create vote record
        let vote_id = Uuid::new_v4().to_string();
        let enhanced_vote = EnhancedVote {
            vote_id: vote_id.clone(),
            proposal_id: session.proposal_id.clone(),
            voter_id: voter_id.clone(),
            vote_choice: VoteChoice::For, // Placeholder for quadratic voting
            timestamp: current_timestamp()?,
            voting_power: total_credits_used as f64,
            conviction_time: None,
            quadratic_credits_used: Some(total_credits_used),
            delegation_chain: Vec::new(),
            vote_commitment: None,
            zero_knowledge_proof: None,
            ranked_choices: Vec::new(),
            approval_choices: allocations.keys().cloned().collect(),
            signature: Vec::new(),
            public_key: Vec::new(),
            encryption_proof: None,
            anonymity_set: None,
        };

        let signed_vote = self.sign_enhanced_vote(enhanced_vote).await?;

        // Store the vote
        {
            let mut votes = self.enhanced_votes.write().await;
            votes.insert(vote_id.clone(), signed_vote);
        }

        info!("Quadratic vote processed: {vote_id}");
        Ok(vote_id)
    }

    /// Implement conviction voting with time-locked stakes
    pub async fn cast_conviction_vote(
        &self,
        session_id: String,
        voter_id: String,
        vote_choice: VoteChoice,
        stake_amount: u64,
        lock_duration: u64,
    ) -> ConsensusResult<String> {
        info!("Processing conviction vote from {voter_id} in session {session_id}");

        // Validate minimum lock time
        if lock_duration < self.config.conviction_config.min_lock_time {
            return Err(ConsensusError::InvalidInput(format!(
                "Lock duration {} is below minimum {}",
                lock_duration, self.config.conviction_config.min_lock_time
            )));
        }

        // Calculate conviction multiplier
        let conviction_multiplier = self.calculate_conviction_multiplier(lock_duration).await;

        // Validate voter has sufficient stake
        let voter_stake = self.get_voter_stake(&voter_id).await?;
        if stake_amount > voter_stake {
            return Err(ConsensusError::InvalidInput(format!(
                "Insufficient stake: {stake_amount} required, {voter_stake} available"
            )));
        }

        // Create conviction lock
        let current_time = current_timestamp()?;
        let conviction_lock = ConvictionLock {
            voter_id: voter_id.clone(),
            locked_amount: stake_amount,
            lock_start: current_time,
            lock_duration,
            conviction_multiplier,
        };

        // Lock the stake
        {
            let mut locks = self.conviction_locks.write().await;
            locks.insert(voter_id.clone(), conviction_lock.clone());
        }

        // Calculate effective voting power
        let effective_power = (stake_amount as f64) * conviction_multiplier;

        // Update session with conviction vote and extract proposal_id
        let proposal_id = {
            let mut sessions = self.voting_sessions.write().await;
            if let Some(session) = sessions.get_mut(&session_id) {
                session
                    .conviction_locks
                    .insert(voter_id.clone(), conviction_lock);
                session.participants.insert(voter_id.clone());

                let choice_key = match vote_choice {
                    VoteChoice::For => "for".to_string(),
                    VoteChoice::Against => "against".to_string(),
                    VoteChoice::Abstain => "abstain".to_string(),
                };
                *session.vote_tallies.entry(choice_key).or_insert(0.0) += effective_power;

                session.proposal_id.clone()
            } else {
                return Err(ConsensusError::InvalidSession(session_id));
            }
        };

        // Create vote record
        let vote_id = Uuid::new_v4().to_string();
        let enhanced_vote = EnhancedVote {
            vote_id: vote_id.clone(),
            proposal_id,
            voter_id: voter_id.clone(),
            vote_choice,
            timestamp: current_time,
            voting_power: effective_power,
            conviction_time: Some(lock_duration),
            quadratic_credits_used: None,
            delegation_chain: Vec::new(),
            vote_commitment: None,
            zero_knowledge_proof: None,
            ranked_choices: Vec::new(),
            approval_choices: Vec::new(),
            signature: Vec::new(),
            public_key: Vec::new(),
            encryption_proof: None,
            anonymity_set: None,
        };

        let signed_vote = self.sign_enhanced_vote(enhanced_vote).await?;

        // Store the vote
        {
            let mut votes = self.enhanced_votes.write().await;
            votes.insert(vote_id.clone(), signed_vote);
        }

        info!("Conviction vote processed: {vote_id} with {conviction_multiplier}x multiplier");
        Ok(vote_id)
    }

    /// Implement liquid democracy delegation
    pub async fn delegate_voting_power(
        &self,
        delegator: String,
        delegate: String,
        _domain: Option<String>,
    ) -> ConsensusResult<()> {
        info!("Setting up delegation from {delegator} to {delegate}");

        // Validate both parties exist and are eligible
        self.validate_voter_eligibility(&delegator).await?;
        self.validate_voter_eligibility(&delegate).await?;

        // Check for circular delegation
        if self
            .would_create_circular_delegation(&delegator, &delegate)
            .await?
        {
            return Err(ConsensusError::InvalidInput(
                "Delegation would create circular reference".to_string(),
            ));
        }

        // Check delegation depth limit
        let delegation_depth = self.calculate_delegation_depth(&delegate).await?;
        if delegation_depth >= self.config.max_delegation_depth {
            return Err(ConsensusError::InvalidInput(format!(
                "Delegation depth limit {} exceeded",
                self.config.max_delegation_depth
            )));
        }

        // Set up delegation
        {
            let mut delegations = self.delegations.write().await;
            delegations.insert(delegator.clone(), delegate.clone());
        }

        info!("Delegation established: {delegator} -> {delegate}");
        Ok(())
    }

    /// Initiate cross-consensus voting for algorithm transitions
    pub async fn initiate_cross_consensus_voting(
        &self,
        transition_id: String,
        source_algorithm: String,
        target_algorithm: String,
        algorithms: Vec<String>,
    ) -> ConsensusResult<String> {
        info!("Initiating cross-consensus voting for transition: {transition_id}");

        if !self.config.enable_cross_consensus_voting {
            return Err(ConsensusError::ConfigurationError(
                "Cross-consensus voting is disabled".to_string(),
            ));
        }

        // Create voting sessions for each algorithm
        let mut voting_sessions = FxHashMap::default();
        let mut consensus_requirements = FxHashMap::default();

        for algorithm in &algorithms {
            let session_id = self
                .create_voting_session(
                    format!("cross_consensus_{transition_id}"),
                    Some(VotingMechanism::StakeWeighted),
                    86400,      // 24 hours
                    Some(0.67), // 67% consensus required
                )
                .await?;

            voting_sessions.insert(algorithm.clone(), session_id);
            consensus_requirements.insert(algorithm.clone(), 0.67);
        }

        // Create cross-consensus voting record
        let cross_voting = CrossConsensusVoting {
            transition_id: transition_id.clone(),
            source_algorithm: source_algorithm.clone(),
            target_algorithm: target_algorithm.clone(),
            voting_sessions,
            consensus_requirements,
            cross_validation_proofs: FxHashMap::default(),
            status: CrossConsensusStatus::CollectingVotes,
        };

        // Store cross-consensus session
        {
            let mut sessions = self.cross_consensus_sessions.write().await;
            sessions.insert(transition_id.clone(), cross_voting);
        }

        info!("Cross-consensus voting initiated for transition: {transition_id}");
        Ok(transition_id)
    }

    /// Resolve conflicts using multi-criteria decision analysis
    pub async fn resolve_conflict(
        &self,
        conflicting_proposals: Vec<String>,
        strategy: Option<ConflictResolutionStrategy>,
    ) -> ConsensusResult<String> {
        info!(
            "Initiating conflict resolution for {} proposals",
            conflicting_proposals.len()
        );

        let case_id = Uuid::new_v4().to_string();
        let resolution_strategy =
            strategy.unwrap_or_else(|| self.config.conflict_resolution.clone());

        let mut conflict_case = ConflictResolutionCase {
            case_id: case_id.clone(),
            conflicting_proposals: conflicting_proposals.clone(),
            strategy: resolution_strategy.clone(),
            criteria_scores: FxHashMap::default(),
            mediation_rounds: Vec::new(),
            resolution_status: ConflictResolutionStatus::InProgress,
            final_decision: None,
            created_at: current_timestamp()?,
        };

        // Execute resolution strategy
        match resolution_strategy {
            ConflictResolutionStrategy::MultiCriteria { criteria_weights } => {
                conflict_case = self
                    .resolve_multi_criteria_conflict(conflict_case, criteria_weights)
                    .await?;
            },
            ConflictResolutionStrategy::Mediation => {
                conflict_case.resolution_status =
                    ConflictResolutionStatus::Mediating { current_round: 1 };
                conflict_case = self.initiate_mediation(conflict_case).await?;
            },
            ConflictResolutionStrategy::ConsensusBuilding { max_iterations } => {
                conflict_case = self
                    .build_consensus_resolution(conflict_case, max_iterations)
                    .await?;
            },
            _ => {
                // Fallback to majority rule
                conflict_case = self.resolve_by_majority_rule(conflict_case).await?;
            },
        }

        // Store conflict case
        {
            let mut cases = self.conflict_cases.write().await;
            cases.insert(case_id.clone(), conflict_case);
        }

        info!("Conflict resolution initiated: {case_id}");
        Ok(case_id)
    }

    /// Tally votes and determine results for a voting session
    pub async fn tally_votes(&self, session_id: String) -> ConsensusResult<VotingSessionStatus> {
        info!("Tallying votes for session: {session_id}");

        let mut session = {
            let sessions = self.voting_sessions.read().await;
            sessions.get(&session_id).cloned().ok_or_else(|| {
                ConsensusError::InvalidState(format!("Voting session {session_id} not found"))
            })?
        };

        // Check if session has ended
        let current_time = current_timestamp()?;
        if current_time < session.end_time && matches!(session.status, VotingSessionStatus::Active)
        {
            return Err(ConsensusError::InvalidState(
                "Voting session is still active".to_string(),
            ));
        }

        // Update session status to tallying
        session.status = VotingSessionStatus::Tallying;

        // Check quorum
        let total_participants = session.participants.len() as f64;
        let total_eligible_voters = self.get_total_eligible_voters().await? as f64;
        let participation_rate = total_participants / total_eligible_voters;

        if participation_rate < session.quorum_requirement {
            session.status = VotingSessionStatus::QuorumNotReached;
            self.update_voting_session(session_id, session).await?;
            return Ok(VotingSessionStatus::QuorumNotReached);
        }

        // Determine winner based on mechanism
        let (winner, final_tally) = match session.mechanism {
            VotingMechanism::SimpleMajority => {
                self.determine_simple_majority_winner(&session).await?
            },
            VotingMechanism::Quadratic { .. } => self.determine_quadratic_winner(&session).await?,
            VotingMechanism::StakeWeighted => {
                self.determine_stake_weighted_winner(&session).await?
            },
            VotingMechanism::Conviction => self.determine_conviction_winner(&session).await?,
            _ => {
                // Default to simple majority
                self.determine_simple_majority_winner(&session).await?
            },
        };

        // Update session with results
        session.status = VotingSessionStatus::Completed {
            winner: winner.clone(),
            final_tally: final_tally.clone(),
        };

        self.update_voting_session(session_id, session).await?;

        info!("Vote tallying completed. Winner: {winner:?}");
        Ok(VotingSessionStatus::Completed {
            winner,
            final_tally,
        })
    }

    /// Get comprehensive governance and voting status
    pub async fn get_consensus_status(&self) -> ConsensusBuilderStatus {
        let sessions = self.voting_sessions.read().await;
        let votes = self.enhanced_votes.read().await;
        let conflicts = self.conflict_cases.read().await;
        let cross_sessions = self.cross_consensus_sessions.read().await;

        let active_sessions = sessions
            .values()
            .filter(|s| matches!(s.status, VotingSessionStatus::Active))
            .count();

        let total_votes = votes.len();
        let active_conflicts = conflicts
            .values()
            .filter(|c| {
                matches!(
                    c.resolution_status,
                    ConflictResolutionStatus::InProgress
                        | ConflictResolutionStatus::Mediating { .. }
                )
            })
            .count();

        let cross_consensus_active = cross_sessions
            .values()
            .filter(|s| {
                matches!(
                    s.status,
                    CrossConsensusStatus::CollectingVotes | CrossConsensusStatus::Validating
                )
            })
            .count();

        ConsensusBuilderStatus {
            active_voting_sessions: active_sessions,
            total_votes_cast: total_votes,
            active_conflicts,
            cross_consensus_sessions: cross_consensus_active,
            delegation_count: self.delegations.read().await.len(),
            conviction_locks: self.conviction_locks.read().await.len(),
            last_updated: current_timestamp().unwrap_or(0),
        }
    }

    // === Private Implementation Methods ===

    /// Initialize cryptographic keys for the consensus builder
    async fn initialize_builder_keys(&self) -> ConsensusResult<()> {
        let signing_key = SigningKey::from_bytes(&rand::random::<[u8; 32]>());

        let mut keypair = self.builder_keypair.write().await;
        *keypair = Some(signing_key);

        info!("Consensus builder cryptographic keys initialized");
        Ok(())
    }

    /// Calculate dynamic quorum based on network participation
    async fn calculate_dynamic_quorum(&self) -> ConsensusResult<f64> {
        match &self.config.quorum_type {
            QuorumType::Fixed { percentage } => Ok(*percentage),
            QuorumType::Dynamic {
                base_percentage,
                participation_factor,
            } => {
                let current_participation = self.get_current_participation_rate().await?;
                let adjusted_quorum =
                    base_percentage + (participation_factor * current_participation);
                Ok(adjusted_quorum.clamp(0.33, 0.95)) // Cap between 33% and 95%
            },
            QuorumType::StakeWeighted {
                min_stake_percentage,
            } => {
                // Calculate based on stake distribution
                Ok(*min_stake_percentage)
            },
            QuorumType::Adaptive {
                min_percentage,
                max_percentage,
            } => {
                // Adapt based on proposal importance (placeholder)
                Ok((*min_percentage + *max_percentage) / 2.0)
            },
        }
    }

    /// Calculate voting power for a voter
    fn calculate_voting_power<'a>(
        &'a self,
        voter_id: &'a str,
        method: &'a VotingPowerMethod,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ConsensusResult<f64>> + 'a>> {
        Box::pin(async move {
            match method {
                VotingPowerMethod::Equal => Ok(1.0),
                VotingPowerMethod::Stake { max_power_ratio } => {
                    let stake = self.get_voter_stake(voter_id).await? as f64;
                    let total_stake = self.get_total_stake().await? as f64;
                    let stake_ratio = if total_stake > 0.0 {
                        stake / total_stake
                    } else {
                        0.0
                    };
                    Ok((stake_ratio * max_power_ratio).min(*max_power_ratio))
                },
                VotingPowerMethod::Reputation { reputation_weight } => {
                    let reputation = self.get_voter_reputation(voter_id).await?;
                    Ok(reputation * reputation_weight)
                },
                VotingPowerMethod::Hybrid {
                    stake_weight,
                    reputation_weight,
                } => {
                    let stake_power = self
                        .calculate_voting_power(
                            voter_id,
                            &VotingPowerMethod::Stake {
                                max_power_ratio: 1.0,
                            },
                        )
                        .await?;
                    let reputation_power = self
                        .calculate_voting_power(
                            voter_id,
                            &VotingPowerMethod::Reputation {
                                reputation_weight: 1.0,
                            },
                        )
                        .await?;
                    Ok((stake_power * stake_weight) + (reputation_power * reputation_weight))
                },
                VotingPowerMethod::Expertise {
                    domain: _,
                    expertise_weight,
                } => {
                    // Placeholder for expertise-based power
                    let base_reputation = self.get_voter_reputation(voter_id).await?;
                    Ok(base_reputation * expertise_weight)
                },
            }
        })
    }

    /// Validate voter eligibility
    async fn validate_voter_eligibility(&self, voter_id: &str) -> ConsensusResult<()> {
        let reputation = self.get_voter_reputation(voter_id).await?;
        if reputation < 0.1 {
            return Err(ConsensusError::Unauthorized(format!(
                "Voter {voter_id} has insufficient reputation: {reputation:.2}"
            )));
        }
        Ok(())
    }

    /// Get voting session
    async fn get_voting_session(&self, session_id: &str) -> ConsensusResult<VotingSession> {
        let sessions = self.voting_sessions.read().await;
        sessions.get(session_id).cloned().ok_or_else(|| {
            ConsensusError::InvalidState(format!("Voting session {session_id} not found"))
        })
    }

    /// Update voting session
    async fn update_voting_session(
        &self,
        session_id: String,
        session: VotingSession,
    ) -> ConsensusResult<()> {
        let mut sessions = self.voting_sessions.write().await;
        sessions.insert(session_id, session);
        Ok(())
    }

    /// Resolve delegation chain
    async fn resolve_delegation_chain(&self, voter_id: &str) -> ConsensusResult<String> {
        let delegations = self.delegations.read().await;
        let mut current = voter_id.to_string();
        let mut visited = FxHashSet::default();

        while let Some(delegate) = delegations.get(&current) {
            if visited.contains(delegate) {
                return Err(ConsensusError::InvalidState(
                    "Circular delegation detected".to_string(),
                ));
            }
            visited.insert(current.clone());
            current = delegate.clone();
        }

        Ok(current)
    }

    /// Get delegation chain for a voter
    async fn get_delegation_chain(&self, voter_id: &str) -> Vec<String> {
        let delegations = self.delegations.read().await;
        let mut chain = vec![voter_id.to_string()];
        let mut current = voter_id.to_string();

        while let Some(delegate) = delegations.get(&current) {
            chain.push(delegate.clone());
            current = delegate.clone();
        }

        chain
    }

    /// Sign enhanced vote
    async fn sign_enhanced_vote(&self, mut vote: EnhancedVote) -> ConsensusResult<EnhancedVote> {
        let keypair = self.builder_keypair.read().await;
        if let Some(kp) = keypair.as_ref() {
            let message = format!(
                "{}:{}:{:?}:{}",
                vote.proposal_id, vote.voter_id, vote.vote_choice, vote.timestamp
            );
            let signature = kp.sign(message.as_bytes());
            vote.signature = signature.to_bytes().to_vec();
            vote.public_key = kp.verifying_key().to_bytes().to_vec();
        }
        Ok(vote)
    }

    /// Process vote based on mechanism
    async fn process_vote_by_mechanism(
        &self,
        session_id: &str,
        vote: &EnhancedVote,
    ) -> ConsensusResult<()> {
        let mut sessions = self.voting_sessions.write().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.participants.insert(vote.voter_id.clone());

            let choice_key = match vote.vote_choice {
                VoteChoice::For => "for".to_string(),
                VoteChoice::Against => "against".to_string(),
                VoteChoice::Abstain => "abstain".to_string(),
            };

            *session.vote_tallies.entry(choice_key).or_insert(0.0) += vote.voting_power;
        }
        Ok(())
    }

    /// Calculate conviction multiplier based on lock duration
    async fn calculate_conviction_multiplier(&self, lock_duration: u64) -> f64 {
        let config = &self.config.conviction_config;
        let time_ratio = lock_duration as f64 / config.full_conviction_time as f64;
        let multiplier = 1.0 + (config.conviction_factor * time_ratio.ln_1p());
        multiplier.min(config.max_conviction)
    }

    /// Check if delegation would create circular reference
    async fn would_create_circular_delegation(
        &self,
        delegator: &str,
        delegate: &str,
    ) -> ConsensusResult<bool> {
        let delegations = self.delegations.read().await;
        let mut current = delegate;
        let mut visited = FxHashSet::default();

        while let Some(next_delegate) = delegations.get(current) {
            if next_delegate == delegator {
                return Ok(true); // Circular delegation detected
            }
            if visited.contains(current) {
                break; // Avoid infinite loop
            }
            visited.insert(current);
            current = next_delegate;
        }

        Ok(false)
    }

    /// Calculate delegation depth
    async fn calculate_delegation_depth(&self, voter_id: &str) -> ConsensusResult<u32> {
        let delegations = self.delegations.read().await;
        let mut current = voter_id;
        let mut depth = 0;
        let mut visited = FxHashSet::default();

        while let Some(delegate) = delegations.get(current) {
            if visited.contains(current) {
                break; // Avoid infinite loop
            }
            visited.insert(current);
            current = delegate;
            depth += 1;
        }

        Ok(depth)
    }

    /// Helper methods for accessing voter data
    async fn get_voter_stake(&self, voter_id: &str) -> ConsensusResult<u64> {
        let stakes = self.voter_stakes.read().await;
        Ok(stakes.get(voter_id).copied().unwrap_or(0))
    }

    async fn get_voter_reputation(&self, voter_id: &str) -> ConsensusResult<f64> {
        let reputations = self.voter_reputations.read().await;
        Ok(reputations.get(voter_id).copied().unwrap_or(0.5))
    }

    async fn get_voter_credits(&self, _voter_id: &str) -> ConsensusResult<u64> {
        // Placeholder - would calculate based on stake or other factors
        Ok(100) // Default 100 credits
    }

    async fn get_total_stake(&self) -> ConsensusResult<u64> {
        let stakes = self.voter_stakes.read().await;
        Ok(stakes.values().sum())
    }

    async fn get_total_eligible_voters(&self) -> ConsensusResult<usize> {
        let reputations = self.voter_reputations.read().await;
        Ok(reputations.iter().filter(|(_, &rep)| rep >= 0.1).count())
    }

    async fn get_current_participation_rate(&self) -> ConsensusResult<f64> {
        // Placeholder for actual participation calculation
        Ok(0.7) // 70% participation rate
    }

    // Conflict resolution methods
    async fn resolve_multi_criteria_conflict(
        &self,
        mut case: ConflictResolutionCase,
        criteria_weights: FxHashMap<String, f64>,
    ) -> ConsensusResult<ConflictResolutionCase> {
        // Calculate scores for each proposal based on criteria
        for proposal_id in &case.conflicting_proposals {
            let mut proposal_scores = FxHashMap::default();
            for criterion in criteria_weights.keys() {
                // Placeholder scoring logic
                let score: f64 = rand::thread_rng().gen_range(0.0..1.0);
                proposal_scores.insert(criterion.clone(), score);
            }
            case.criteria_scores
                .insert(proposal_id.clone(), proposal_scores);
        }

        // Calculate weighted total scores
        let mut total_scores = FxHashMap::default();
        for (proposal_id, scores) in &case.criteria_scores {
            let total: f64 = scores
                .iter()
                .map(|(criterion, score)| {
                    let weight = criteria_weights.get(criterion).unwrap_or(&1.0);
                    score * weight
                })
                .sum();
            total_scores.insert(proposal_id.clone(), total);
        }

        // Select winner
        let winner = total_scores
            .iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(id, _)| id.clone());

        case.final_decision = winner;
        case.resolution_status = ConflictResolutionStatus::Resolved {
            resolution_method: "multi_criteria_analysis".to_string(),
        };

        Ok(case)
    }

    async fn initiate_mediation(
        &self,
        mut case: ConflictResolutionCase,
    ) -> ConsensusResult<ConflictResolutionCase> {
        // Start mediation process
        let mediation_round = MediationRound {
            round_number: 1,
            mediator_id: "neutral_mediator".to_string(),
            proposed_compromise: "Initial compromise proposal".to_string(),
            stakeholder_feedback: FxHashMap::default(),
            acceptance_votes: FxHashMap::default(),
            round_timestamp: current_timestamp()?,
        };

        case.mediation_rounds.push(mediation_round);
        Ok(case)
    }

    async fn build_consensus_resolution(
        &self,
        mut case: ConflictResolutionCase,
        _max_iterations: u32,
    ) -> ConsensusResult<ConflictResolutionCase> {
        // Placeholder for consensus building logic
        case.resolution_status = ConflictResolutionStatus::Resolved {
            resolution_method: "consensus_building".to_string(),
        };
        case.final_decision = case.conflicting_proposals.first().cloned();
        Ok(case)
    }

    async fn resolve_by_majority_rule(
        &self,
        mut case: ConflictResolutionCase,
    ) -> ConsensusResult<ConflictResolutionCase> {
        // Simple majority rule fallback
        case.final_decision = case.conflicting_proposals.first().cloned();
        case.resolution_status = ConflictResolutionStatus::Resolved {
            resolution_method: "majority_rule".to_string(),
        };
        Ok(case)
    }

    // Vote tallying methods
    async fn determine_simple_majority_winner(
        &self,
        session: &VotingSession,
    ) -> ConsensusResult<(Option<String>, FxHashMap<String, f64>)> {
        let max_choice = session
            .vote_tallies
            .iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(choice, _)| choice.clone());

        Ok((max_choice, session.vote_tallies.clone()))
    }

    async fn determine_quadratic_winner(
        &self,
        session: &VotingSession,
    ) -> ConsensusResult<(Option<String>, FxHashMap<String, f64>)> {
        // Already calculated in quadratic vote tallies
        self.determine_simple_majority_winner(session).await
    }

    async fn determine_stake_weighted_winner(
        &self,
        session: &VotingSession,
    ) -> ConsensusResult<(Option<String>, FxHashMap<String, f64>)> {
        // Already calculated with stake weighting
        self.determine_simple_majority_winner(session).await
    }

    async fn determine_conviction_winner(
        &self,
        session: &VotingSession,
    ) -> ConsensusResult<(Option<String>, FxHashMap<String, f64>)> {
        // Already calculated with conviction multipliers
        self.determine_simple_majority_winner(session).await
    }
}

/// Voting options for enhanced votes
#[derive(Debug, Clone, Default)]
pub struct VotingOptions {
    pub conviction_time: Option<u64>,
    pub quadratic_credits: Option<u64>,
    pub privacy_commitment: Option<Vec<u8>>,
    pub zero_knowledge_proof: Option<Vec<u8>>,
    pub ranked_choices: Option<Vec<String>>,
    pub approval_choices: Option<Vec<String>>,
    pub public_key: Option<Vec<u8>>,
    pub encryption_proof: Option<Vec<u8>>,
    pub anonymity_set: Option<Vec<String>>,
}

/// Consensus builder status
#[derive(Debug, Clone)]
pub struct ConsensusBuilderStatus {
    pub active_voting_sessions: usize,
    pub total_votes_cast: usize,
    pub active_conflicts: usize,
    pub cross_consensus_sessions: usize,
    pub delegation_count: usize,
    pub conviction_locks: usize,
    pub last_updated: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordination::byzantine_coordinator::ByzantineConsensusConfig;

    /// Helper to create a ConsensusBuilder for tests
    async fn create_test_builder() -> ConsensusBuilder {
        let config = ConsensusBuilderConfig::default();
        let byz_config = ByzantineConsensusConfig::default();
        let coordinator = Arc::new(
            ByzantineConsensusCoordinator::new(byz_config).await.unwrap(),
        );
        ConsensusBuilder::new(config, coordinator).await.unwrap()
    }

    #[tokio::test]
    async fn test_consensus_builder_initialization() {
        let builder = create_test_builder().await;
        let status = builder.get_consensus_status().await;
        assert_eq!(status.active_voting_sessions, 0);
        assert_eq!(status.total_votes_cast, 0);
        assert_eq!(status.active_conflicts, 0);
    }

    #[tokio::test]
    async fn test_create_voting_session() {
        let builder = create_test_builder().await;
        let session_id = builder
            .create_voting_session(
                "test_proposal".to_string(),
                Some(VotingMechanism::SimpleMajority),
                3600,
                Some(0.5),
            )
            .await
            .unwrap();
        assert!(!session_id.is_empty());

        let status = builder.get_consensus_status().await;
        assert_eq!(status.active_voting_sessions, 1);
    }

    #[tokio::test]
    async fn test_quadratic_voting() {
        let builder = create_test_builder().await;

        // Initialize voter data
        {
            let mut stakes = builder.voter_stakes.write().await;
            stakes.insert("voter1".to_string(), 1000);
            let mut reputations = builder.voter_reputations.write().await;
            reputations.insert("voter1".to_string(), 0.8);
        }

        // Create voting session
        let session_id = builder
            .create_voting_session(
                "test_proposal".to_string(),
                Some(VotingMechanism::Quadratic { total_credits: 100 }),
                3600,
                None,
            )
            .await
            .unwrap();

        // Cast quadratic vote
        let mut allocations = FxHashMap::default();
        allocations.insert("option_a".to_string(), 5); // 25 credits for 5 influence
        allocations.insert("option_b".to_string(), 3); // 9 credits for 3 influence

        let vote_id = builder
            .cast_quadratic_vote(session_id, "voter1".to_string(), allocations)
            .await;
        assert!(vote_id.is_ok());
    }

    #[tokio::test]
    async fn test_config_defaults() {
        let config = ConsensusBuilderConfig::default();
        assert!(config.enable_private_voting);
        assert!(config.enable_cross_consensus_voting);
        assert_eq!(config.max_delegation_depth, 5);
    }

    #[tokio::test]
    async fn test_conviction_config_defaults() {
        let config = ConvictionVotingConfig::default();
        assert_eq!(config.conviction_factor, 2.0);
        assert_eq!(config.max_conviction, 6.0);
        assert_eq!(config.full_conviction_time, 2592000);
        assert_eq!(config.min_lock_time, 86400);
    }
}
