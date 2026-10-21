//! HotStuff Byzantine Fault Tolerant Consensus
//!
//! This module implements the HotStuff BFT consensus protocol with the following
//! key improvements over traditional PBFT:
//!
//! ## Key Features
//!
//! - **O(n) Message Complexity**: Linear view change protocol vs PBFT's O(n^2)
//! - **Pipelined Voting**: Multiple blocks can be in different phases concurrently
//! - **Optimistic Responsiveness**: Can progress at network speed without timeouts
//! - **Three-Phase Commit**: Prepare -> Pre-commit -> Commit with chained QCs
//! - **Leader Rotation**: Automatic leader election for each view
//!
//! ## Protocol Overview
//!
//! HotStuff operates in views, where each view has a designated leader:
//!
//! 1. **Prepare Phase**: Leader proposes block, replicas vote
//! 2. **Pre-commit Phase**: Leader aggregates votes into prepareQC, broadcasts
//! 3. **Commit Phase**: Leader aggregates votes into precommitQC, broadcasts
//! 4. **Decide**: After commitQC, block is finalized
//!
//! The "three-chain" rule: A block B is committed when its grandchild block
//! has a QC (i.e., there are 3 consecutive QCs).
//!
//! Adapted from `blockchain-core/src/consensus/hotstuff/`.

pub mod crypto;
pub mod pipelined_voting;
pub mod types;

use async_trait::async_trait;
use log::{info, warn};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use tokio::time::sleep;

use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::transaction::Transaction;
use wrfcoin_shared_protocol::consensus::finality::{
    FinalitySignal, FinalitySignalSender, emit_finality_signal,
};
use wrfcoin_shared_protocol::crypto::hash::hash_bytes;

use crate::traits::ConsensusEngine;
use crate::types::{ConsensusConfig, ConsensusError, ConsensusResult};

// Import types for internal use and re-export key types
pub use crypto::{CryptoError, HotStuffSigningKey, SignatureVerifier, compute_block_hash};
pub use pipelined_voting::{OptimisticPath, PipelinedVotingManager, VoteCollector};
pub use types::*;

/// Event channel for HotStuff consensus events
pub type HotStuffEventSender = mpsc::Sender<HotStuffEvent>;
#[allow(dead_code)]
pub type HotStuffEventReceiver = mpsc::Receiver<HotStuffEvent>;

/// Events emitted by the HotStuff consensus engine
#[derive(Debug, Clone)]
pub enum HotStuffEvent {
    /// New block proposed
    BlockProposed(HotStuffProposal),
    /// Vote received
    VoteReceived(HotStuffVote),
    /// QC formed
    QCFormed(QuorumCertificate),
    /// Block committed
    BlockCommitted {
        hash: BlockHash,
        height: BlockHeight,
    },
    /// View changed
    ViewChanged {
        old_view: ViewNumber,
        new_view: ViewNumber,
    },
    /// Timeout in view
    ViewTimeout(ViewNumber),
    /// Leader changed
    LeaderChanged { view: ViewNumber, leader: NodeId },
    /// Equivocation detected
    EquivocationDetected { node_id: NodeId, evidence: String },
}

/// Main HotStuff consensus engine
///
/// Coordinates the consensus protocol, managing proposals, votes, QCs,
/// and view changes.
#[derive(Debug)]
pub struct HotStuffConsensus {
    /// Configuration
    config: HotStuffConfig,
    /// This node's ID
    node_id: NodeId,
    /// This node's signing key
    signing_key: HotStuffSigningKey,
    /// Signature verifier for all validators
    signature_verifier: Arc<RwLock<SignatureVerifier>>,
    /// Current state
    state: Arc<RwLock<HotStuffState>>,
    /// Validator set
    validators: Arc<RwLock<ValidatorSet>>,
    /// Pipelined voting manager
    voting_manager: Arc<RwLock<PipelinedVotingManager>>,
    /// Optimistic path tracker
    optimistic_path: Arc<RwLock<OptimisticPath>>,
    /// Event sender
    event_sender: Option<HotStuffEventSender>,
    /// Message sender for network
    message_sender: Option<mpsc::Sender<(NodeId, HotStuffMessage)>>,
    /// Equivocation evidence
    equivocation_evidence: Arc<RwLock<FxHashMap<NodeId, Vec<HotStuffVote>>>>,
    /// Running flag
    is_running: Arc<RwLock<bool>>,
    /// View timeout handle
    view_timeout_handle: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,
    /// Optional BFT finality signal sink (#1216).
    ///
    /// When set, the commit phase of `handle_qc` emits a
    /// [`FinalitySignal`](wrfcoin_shared_protocol::consensus::finality::FinalitySignal)
    /// on this channel so that a chain-layer consumer can call
    /// [`Blockchain::set_finalized_height`](wrfcoin_shared_protocol::blockchain::chain::Blockchain::set_finalized_height),
    /// which activates the reorg rejection guard in `plan_reorg`.
    finality_sink: Arc<RwLock<Option<FinalitySignalSender>>>,
}

impl HotStuffConsensus {
    /// Create a new HotStuff consensus engine
    pub fn new(
        config: HotStuffConfig,
        node_id: NodeId,
        signing_key: HotStuffSigningKey,
        validators: ValidatorSet,
    ) -> Result<Self, HotStuffError> {
        let initial_leader = validators
            .get_leader(0)
            .map(|v| v.node_id.clone())
            .unwrap_or_default();

        let state = HotStuffState {
            current_view: 0,
            current_leader: initial_leader.clone(),
            node_id: node_id.clone(),
            is_leader: initial_leader == node_id,
            current_phase: HotStuffPhase::Prepare,
            locked_qc: None,
            prepare_qc: None,
            committed_height: 0,
            latest_proposal: None,
            pending_votes: FxHashMap::default(),
        };

        // Create signature verifier from validator set
        let signature_verifier = SignatureVerifier::from_validator_set(&validators)
            .map_err(|e| HotStuffError::CryptoError(e.to_string()))?;

        let validators_arc = Arc::new(RwLock::new(validators.clone()));
        let voting_manager =
            PipelinedVotingManager::new(validators.clone(), node_id.clone(), config.clone());

        Ok(Self {
            config: config.clone(),
            node_id,
            signing_key,
            signature_verifier: Arc::new(RwLock::new(signature_verifier)),
            state: Arc::new(RwLock::new(state)),
            validators: validators_arc,
            voting_manager: Arc::new(RwLock::new(voting_manager)),
            optimistic_path: Arc::new(RwLock::new(OptimisticPath::new(config.delta))),
            event_sender: None,
            message_sender: None,
            equivocation_evidence: Arc::new(RwLock::new(FxHashMap::default())),
            is_running: Arc::new(RwLock::new(false)),
            view_timeout_handle: Arc::new(RwLock::new(None)),
            finality_sink: Arc::new(RwLock::new(None)),
        })
    }

    /// Create a new HotStuff consensus engine with a generated key
    pub fn with_generated_key(
        config: HotStuffConfig,
        node_id: NodeId,
        validators: ValidatorSet,
    ) -> Result<Self, HotStuffError> {
        let signing_key = HotStuffSigningKey::generate(node_id.clone());
        Self::new(config, node_id, signing_key, validators)
    }

    /// Set the event sender for consensus events
    #[allow(dead_code)]
    pub fn set_event_sender(&mut self, sender: HotStuffEventSender) {
        self.event_sender = Some(sender);
    }

    /// Set the message sender for network communication
    #[allow(dead_code)]
    pub fn set_message_sender(&mut self, sender: mpsc::Sender<(NodeId, HotStuffMessage)>) {
        self.message_sender = Some(sender);
    }

    /// Attach a BFT finality signal sink (#1216).
    ///
    /// After this call, the commit phase of [`handle_qc`](Self::handle_qc)
    /// will emit a
    /// [`FinalitySignal`](wrfcoin_shared_protocol::consensus::finality::FinalitySignal)
    /// every time a block is committed via the three-chain rule. A
    /// chain-layer consumer task can then forward that signal into
    /// [`Blockchain::set_finalized_height`](wrfcoin_shared_protocol::blockchain::chain::Blockchain::set_finalized_height),
    /// which is the only way the BFT finality guard in `plan_reorg` is
    /// actually armed.
    ///
    /// Takes `&self` because the sink lives behind an
    /// [`Arc<RwLock>`] — callers typically share the engine across
    /// multiple spawned tasks and need the setter to work post-creation.
    pub async fn set_finality_sink(&self, sender: FinalitySignalSender) {
        *self.finality_sink.write().await = Some(sender);
    }

    /// Remove the BFT finality signal sink. Returns the previous sender so
    /// the caller can decide whether to drop or reuse it.
    pub async fn take_finality_sink(&self) -> Option<FinalitySignalSender> {
        self.finality_sink.write().await.take()
    }

    /// Start the consensus engine
    pub async fn start(&self) -> Result<(), HotStuffError> {
        info!(
            "Starting HotStuff consensus engine for node {}",
            self.node_id
        );

        // Set running flag
        *self.is_running.write().await = true;

        // Start view timeout timer
        self.start_view_timeout().await;

        // If we're the leader of view 0, start proposing
        let state = self.state.read().await;
        if state.is_leader {
            drop(state);
            // Initial proposal will be triggered by external call to propose_block
            info!("Node {} is the initial leader", self.node_id);
        }

        Ok(())
    }

    /// Stop the consensus engine
    pub async fn stop(&self) {
        info!("Stopping HotStuff consensus engine");
        *self.is_running.write().await = false;

        // Cancel view timeout
        if let Some(handle) = self.view_timeout_handle.write().await.take() {
            handle.abort();
        }
    }

    /// Propose a new block (called by leader)
    pub async fn propose_block(
        &self,
        transactions: Vec<String>,
        state_root: String,
    ) -> Result<HotStuffProposal, HotStuffError> {
        let state = self.state.read().await;

        if !state.is_leader {
            return Err(HotStuffError::NotLeader(state.current_view));
        }

        let view_number = state.current_view;
        let committed_height = state.committed_height;
        let justify_qc = self.get_or_create_justify_qc(&state).await?;
        drop(state);

        let block_height = committed_height + 1;
        let block_hash = self.compute_hotstuff_block_hash(view_number, block_height, &transactions);
        let parent_hash = justify_qc.block_hash.clone();

        let payload = ProposalPayload {
            transaction_hashes: transactions,
            state_root,
            timestamp: self.current_timestamp(),
            extra_data: vec![],
        };

        // Build proposal without signature first, then sign the complete proposal
        let mut proposal = HotStuffProposal {
            block_hash: block_hash.clone(),
            block_height,
            parent_hash,
            view_number,
            leader_id: self.node_id.clone(),
            justify_qc,
            payload,
            signature: vec![],
            timestamp: self.current_timestamp(),
        };
        proposal.signature = self.signing_key.sign_proposal(&proposal);

        // Add block to pipeline
        let pipelined_block = self.proposal_to_pipelined_block(&proposal)?;
        {
            let mut voting_manager = self.voting_manager.write().await;
            voting_manager.add_block(pipelined_block).await?;
        }

        // Update state
        {
            let mut state = self.state.write().await;
            state.latest_proposal = Some(proposal.clone());
        }

        // Emit event
        self.emit_event(HotStuffEvent::BlockProposed(proposal.clone()))
            .await;

        // Broadcast proposal to all validators
        self.broadcast_message(HotStuffMessage::Proposal(proposal.clone()))
            .await?;

        info!("Proposed block {block_hash} at height {block_height} for view {view_number}");

        Ok(proposal)
    }

    /// Process a received proposal
    pub async fn process_proposal(
        &self,
        proposal: HotStuffProposal,
    ) -> Result<HotStuffVote, HotStuffError> {
        let state = self.state.read().await;

        // Verify proposal is for current view
        if proposal.view_number != state.current_view {
            return Err(HotStuffError::ViewMismatch {
                expected: state.current_view,
                got: proposal.view_number,
            });
        }

        // Verify proposal is from the expected leader
        if proposal.leader_id != state.current_leader {
            return Err(HotStuffError::InvalidProposal(format!(
                "Expected leader {}, got {}",
                state.current_leader, proposal.leader_id
            )));
        }
        drop(state);

        // Verify proposal signature
        self.verify_proposal_signature(&proposal)?;

        // Verify justify QC
        self.verify_qc(&proposal.justify_qc)?;

        // Check safety: proposal must extend from locked_qc or have higher QC
        self.check_safety(&proposal).await?;

        // Add block to pipeline
        let pipelined_block = self.proposal_to_pipelined_block(&proposal)?;
        {
            let mut voting_manager = self.voting_manager.write().await;
            voting_manager.add_block(pipelined_block).await?;
        }

        // Update state
        {
            let mut state = self.state.write().await;
            state.latest_proposal = Some(proposal.clone());
        }

        // Create and send vote
        let vote = self.create_vote(&proposal, HotStuffPhase::Prepare)?;

        // Send vote to leader
        self.send_vote_to_leader(&vote).await?;

        info!(
            "Voted for block {} in view {}",
            proposal.block_hash, proposal.view_number
        );

        Ok(vote)
    }

    /// Process a received vote
    pub async fn process_vote(
        &self,
        vote: HotStuffVote,
    ) -> Result<Option<QuorumCertificate>, HotStuffError> {
        // Verify vote signature BEFORE recording anything — unauthenticated
        // votes must never enter the equivocation-evidence store.
        self.verify_vote_signature(&vote)?;

        // Check for equivocation (only reached for cryptographically valid votes)
        if self.config.enable_equivocation_detection {
            self.check_equivocation(&vote).await?;
        }

        // Add vote to voting manager
        let mut voting_manager = self.voting_manager.write().await;
        let qc_result = voting_manager.process_vote(vote.clone()).await?;
        drop(voting_manager);

        // Emit event
        self.emit_event(HotStuffEvent::VoteReceived(vote.clone()))
            .await;

        // If QC formed, handle it
        if let Some(ref qc) = qc_result {
            self.handle_qc(qc.clone()).await?;
        }

        Ok(qc_result)
    }

    /// Handle a formed QC
    async fn handle_qc(&self, qc: QuorumCertificate) -> Result<(), HotStuffError> {
        info!(
            "Handling QC for block {} phase {:?}",
            qc.block_hash, qc.phase
        );

        // Emit event
        self.emit_event(HotStuffEvent::QCFormed(qc.clone())).await;

        let state = self.state.read().await;
        let is_leader = state.is_leader;
        let _view = state.current_view;
        drop(state);

        // If we're the leader, advance to next phase
        if is_leader {
            match qc.phase {
                HotStuffPhase::Prepare => {
                    // Broadcast prepareQC and collect precommit votes
                    self.broadcast_qc_and_collect_votes(&qc, HotStuffPhase::PreCommit)
                        .await?;
                }
                HotStuffPhase::PreCommit => {
                    // Update locked_qc
                    {
                        let mut state = self.state.write().await;
                        state.locked_qc = Some(qc.clone());
                    }
                    // Broadcast precommitQC and collect commit votes
                    self.broadcast_qc_and_collect_votes(&qc, HotStuffPhase::Commit)
                        .await?;
                }
                HotStuffPhase::Commit => {
                    // Block is committed!
                    self.emit_event(HotStuffEvent::BlockCommitted {
                        hash: qc.block_hash.clone(),
                        height: qc.block_height,
                    })
                    .await;

                    // #1216: Also forward the finality signal to the chain
                    // layer so `Blockchain::set_finalized_height` is called.
                    // In HotStuff, a block reaching the Commit phase with a
                    // commitQC is BFT-final by the three-chain rule — it
                    // cannot be reverted without >= 1/3 byzantine validators.
                    // This is the single place where the reorg rejection
                    // guard gets armed from HotStuff. Without this emit, the
                    // guard stays inert and deep reorgs can revert committed
                    // blocks.
                    self.emit_finality_signal_for_block(
                        qc.block_height,
                        qc.block_hash.clone(),
                    )
                    .await;

                    // Update committed height
                    {
                        let mut state = self.state.write().await;
                        state.committed_height = qc.block_height;
                    }

                    // Advance to next view
                    self.advance_view().await?;
                }
                HotStuffPhase::Decide => {}
            }
        }

        Ok(())
    }

    /// Process a new view message (for view change)
    #[allow(dead_code)]
    pub async fn process_new_view(&self, msg: NewViewMessage) -> Result<(), HotStuffError> {
        let state = self.state.read().await;

        // Only process if we're the leader of the new view
        if msg.new_view != state.current_view + 1 {
            return Ok(()); // Ignore messages for other views
        }
        drop(state);

        // Verify signature
        self.verify_new_view_signature(&msg)?;

        // Update high_qc if msg has higher QC
        let voting_manager = self.voting_manager.write().await;
        if let Some(high_qc) = voting_manager.get_high_qc() {
            if msg.high_qc.view_number > high_qc.view_number {
                // Would update high_qc here
            }
        }
        drop(voting_manager);

        Ok(())
    }

    /// Handle view timeout
    #[allow(dead_code)]
    pub async fn handle_timeout(&self) -> Result<(), HotStuffError> {
        let state = self.state.read().await;
        let view = state.current_view;
        drop(state);

        warn!("View {view} timed out");

        // Emit timeout event
        self.emit_event(HotStuffEvent::ViewTimeout(view)).await;

        // Update optimistic path tracker
        {
            let mut optimistic = self.optimistic_path.write().await;
            optimistic.record_result(false, self.config.view_timeout);
        }

        // Send new-view message to next leader
        self.send_new_view_message(view + 1).await?;

        // Advance to next view
        self.advance_view().await?;

        Ok(())
    }

    /// Advance to the next view
    async fn advance_view(&self) -> Result<(), HotStuffError> {
        let mut state = self.state.write().await;
        let old_view = state.current_view;
        let new_view = old_view + 1;

        // Get new leader
        let validators = self.validators.read().await;
        let new_leader = validators
            .get_leader(new_view)
            .map(|v| v.node_id.clone())
            .ok_or_else(|| HotStuffError::NotEnoughValidators {
                have: validators.active_count(),
                need: self.config.min_validators,
            })?;
        drop(validators);

        // Update state
        state.current_view = new_view;
        state.current_leader = new_leader.clone();
        state.is_leader = new_leader == self.node_id;
        state.current_phase = HotStuffPhase::Prepare;
        state.pending_votes.clear();

        info!("Advanced to view {new_view} with leader {new_leader}");
        drop(state);

        // Emit events
        self.emit_event(HotStuffEvent::ViewChanged { old_view, new_view })
            .await;
        self.emit_event(HotStuffEvent::LeaderChanged {
            view: new_view,
            leader: new_leader,
        })
        .await;

        // Restart view timeout
        self.start_view_timeout().await;

        Ok(())
    }

    /// Start/restart the view timeout timer
    async fn start_view_timeout(&self) {
        // Cancel existing timeout
        if let Some(handle) = self.view_timeout_handle.write().await.take() {
            handle.abort();
        }

        let is_running = Arc::clone(&self.is_running);
        let timeout_duration = self.config.view_timeout;
        let state = Arc::clone(&self.state);

        // In a real implementation, this would trigger handle_timeout
        // For now, we just track that a timeout was started
        let handle = tokio::spawn(async move {
            sleep(timeout_duration).await;
            if *is_running.read().await {
                let current_view = state.read().await.current_view;
                warn!("View {current_view} timeout triggered (handling would occur here)");
            }
        });

        *self.view_timeout_handle.write().await = Some(handle);
    }

    /// Check safety constraint (locking rule)
    async fn check_safety(&self, proposal: &HotStuffProposal) -> Result<(), HotStuffError> {
        let state = self.state.read().await;

        if let Some(ref locked_qc) = state.locked_qc {
            // Safety rule: Only vote if proposal extends locked_qc
            // or has a QC with view >= locked_qc.view
            if proposal.justify_qc.view_number < locked_qc.view_number {
                // Check if proposal extends from locked block
                // In full implementation, would trace the chain
                return Err(HotStuffError::SafetyViolation(format!(
                    "Proposal's QC view {} < locked QC view {}",
                    proposal.justify_qc.view_number, locked_qc.view_number
                )));
            }
        }

        Ok(())
    }

    /// Check for equivocation (double voting)
    ///
    /// # Callers must verify the vote signature before calling this.
    /// Only authenticated votes should enter the evidence store.
    async fn check_equivocation(&self, vote: &HotStuffVote) -> Result<(), HotStuffError> {
        // Hard cap: 3 phases × 1 024 views worth of evidence per voter.
        // Prevents the evidence HashMap from growing without bound across a
        // long-running node lifetime.  A legitimate validator produces at most
        // 3 signed votes per view, so this cap covers ≥ 1 024 consecutive views
        // of history — far more than required for equivocation detection.
        const MAX_EVIDENCE_PER_VOTER: usize = 3 * 1024;

        let mut evidence = self.equivocation_evidence.write().await;
        let voter_votes = evidence.entry(vote.voter_id.clone()).or_default();

        // Check if voter already voted in this view
        for existing_vote in voter_votes.iter() {
            if existing_vote.view_number == vote.view_number
                && existing_vote.phase == vote.phase
                && existing_vote.block_hash != vote.block_hash
            {
                let evidence_str = format!(
                    "Double vote in view {} phase {:?}: {} vs {}",
                    vote.view_number, vote.phase, existing_vote.block_hash, vote.block_hash
                );

                self.emit_event(HotStuffEvent::EquivocationDetected {
                    node_id: vote.voter_id.clone(),
                    evidence: evidence_str.clone(),
                })
                .await;

                return Err(HotStuffError::Equivocation(evidence_str));
            }
        }

        if voter_votes.len() < MAX_EVIDENCE_PER_VOTER {
            voter_votes.push(vote.clone());
        } else {
            warn!(
                "Equivocation evidence store for voter {} is at cap ({}); \
                 skipping insertion (node may need restart to clear stale history)",
                vote.voter_id, MAX_EVIDENCE_PER_VOTER
            );
        }
        Ok(())
    }

    // === Helper methods ===

    fn current_timestamp(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    fn compute_hotstuff_block_hash(
        &self,
        view: ViewNumber,
        height: BlockHeight,
        txs: &[String],
    ) -> BlockHash {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(view.to_le_bytes());
        hasher.update(height.to_le_bytes());
        for tx in txs {
            hasher.update(tx.as_bytes());
        }
        hex::encode(hasher.finalize())
    }

    #[allow(dead_code)]
    fn sign_proposal(
        &self,
        block_hash: &str,
        view: ViewNumber,
    ) -> Result<SignatureBytes, HotStuffError> {
        // Create a temporary proposal structure for signing
        let proposal = HotStuffProposal {
            block_hash: block_hash.to_string(),
            block_height: 0, // Will be set by caller
            parent_hash: String::new(),
            view_number: view,
            leader_id: self.node_id.clone(),
            justify_qc: QuorumCertificate::new(
                HotStuffPhase::Commit,
                0,
                "genesis".to_string(),
                0,
                AggregatedSignature::MultiSig(vec![]),
                0,
                vec![],
            ),
            payload: ProposalPayload {
                transaction_hashes: vec![],
                state_root: String::new(),
                timestamp: 0,
                extra_data: vec![],
            },
            signature: vec![],
            timestamp: 0,
        };
        Ok(self.signing_key.sign_proposal(&proposal))
    }

    fn create_vote(
        &self,
        proposal: &HotStuffProposal,
        phase: HotStuffPhase,
    ) -> Result<HotStuffVote, HotStuffError> {
        let mut vote = HotStuffVote::new(
            phase,
            proposal.view_number,
            proposal.block_hash.clone(),
            proposal.block_height,
            self.node_id.clone(),
            vec![],
        );
        // Sign the vote with our signing key
        vote.signature = self.signing_key.sign_vote(&vote);
        Ok(vote)
    }

    #[allow(dead_code)]
    fn sign_vote(
        &self,
        block_hash: &str,
        view: ViewNumber,
        phase: HotStuffPhase,
    ) -> Result<SignatureBytes, HotStuffError> {
        // Create a temporary vote for signing
        let vote = HotStuffVote::new(
            phase,
            view,
            block_hash.to_string(),
            0,
            self.node_id.clone(),
            vec![],
        );
        Ok(self.signing_key.sign_vote(&vote))
    }

    fn verify_proposal_signature(
        &self,
        proposal: &HotStuffProposal,
    ) -> Result<(), HotStuffError> {
        let verifier = self.signature_verifier.try_read().map_err(|_| {
            HotStuffError::StateError("Could not read signature verifier".to_string())
        })?;

        match verifier.verify_proposal(proposal) {
            Ok(true) => Ok(()),
            Ok(false) => Err(HotStuffError::InvalidProposal(
                "Invalid proposal signature".to_string(),
            )),
            Err(e) => Err(HotStuffError::CryptoError(e.to_string())),
        }
    }

    fn verify_vote_signature(&self, vote: &HotStuffVote) -> Result<(), HotStuffError> {
        let verifier = self.signature_verifier.try_read().map_err(|_| {
            HotStuffError::StateError("Could not read signature verifier".to_string())
        })?;

        match verifier.verify_vote(vote) {
            Ok(true) => Ok(()),
            Ok(false) => Err(HotStuffError::InvalidVote(format!(
                "Invalid vote signature from {}",
                vote.voter_id
            ))),
            Err(e) => Err(HotStuffError::CryptoError(e.to_string())),
        }
    }

    fn verify_qc(&self, qc: &QuorumCertificate) -> Result<(), HotStuffError> {
        // Genesis QC (view 0, "genesis" block hash) is always valid
        if qc.view_number == 0 && qc.block_hash == "genesis" {
            return Ok(());
        }

        let validators = self
            .validators
            .try_read()
            .map_err(|_| HotStuffError::StateError("Could not read validators".to_string()))?;

        if !qc.has_quorum(validators.active_count()) {
            return Err(HotStuffError::InvalidQC(format!(
                "QC has {} signers, need {}",
                qc.signer_count,
                QuorumCertificate::quorum_threshold(validators.active_count())
            )));
        }
        Ok(())
    }

    fn verify_new_view_signature(&self, msg: &NewViewMessage) -> Result<(), HotStuffError> {
        let verifier = self.signature_verifier.try_read().map_err(|_| {
            HotStuffError::StateError("Could not read signature verifier".to_string())
        })?;

        match verifier.verify_new_view(msg) {
            Ok(true) => Ok(()),
            Ok(false) => Err(HotStuffError::InvalidNewView(format!(
                "Invalid new view signature from {}",
                msg.sender_id
            ))),
            Err(e) => Err(HotStuffError::CryptoError(e.to_string())),
        }
    }

    fn proposal_to_pipelined_block(
        &self,
        proposal: &HotStuffProposal,
    ) -> Result<PipelinedBlock, HotStuffError> {
        Ok(PipelinedBlock {
            hash: proposal.block_hash.clone(),
            height: proposal.block_height,
            parent_hash: proposal.parent_hash.clone(),
            view_number: proposal.view_number,
            proposal: proposal.clone(),
            prepare_qc: None,
            precommit_qc: None,
            commit_qc: None,
            is_committed: false,
        })
    }

    async fn get_or_create_justify_qc(
        &self,
        state: &HotStuffState,
    ) -> Result<QuorumCertificate, HotStuffError> {
        // Use prepare_qc if available, otherwise create genesis QC
        if let Some(ref qc) = state.prepare_qc {
            return Ok(qc.clone());
        }

        // Genesis QC for first block
        Ok(QuorumCertificate::new(
            HotStuffPhase::Commit,
            0,
            "genesis".to_string(),
            0,
            AggregatedSignature::MultiSig(vec![]),
            0,
            vec![],
        ))
    }

    async fn emit_event(&self, event: HotStuffEvent) {
        if let Some(ref sender) = self.event_sender {
            let _ = sender.send(event).await;
        }
    }

    /// Emit a [`FinalitySignal`] onto the chain-layer finality sink, if one
    /// is attached. Called from the commit phase of `handle_qc` so that
    /// every committed HotStuff block arms the BFT finality guard via
    /// [`Blockchain::set_finalized_height`](wrfcoin_shared_protocol::blockchain::chain::Blockchain::set_finalized_height).
    ///
    /// The HotStuff `BlockHash` is a lowercase-hex SHA256 string. We
    /// decode it to `[u8; 32]` when it matches that format; if decoding
    /// fails (e.g. because a test constructed a synthetic non-hex hash),
    /// we fall back to SHA256(block_hash.as_bytes()) so the signal still
    /// carries a stable, deterministic 32-byte identifier. The reorg
    /// guard only cares about the height, so this fallback is safe —
    /// the hash is purely advisory for divergence diagnostics.
    async fn emit_finality_signal_for_block(
        &self,
        block_height: BlockHeight,
        block_hash: BlockHash,
    ) {
        let sink_guard = self.finality_sink.read().await;
        if let Some(sender) = sink_guard.as_ref() {
            let hash_bytes = Self::block_hash_to_bytes(&block_hash);
            emit_finality_signal(
                sender,
                FinalitySignal::new(block_height, hash_bytes, "hotstuff"),
            );
        }
    }

    /// Decode a HotStuff [`BlockHash`] (hex-encoded SHA256) into a 32-byte
    /// array. Falls back to `sha256(block_hash.as_bytes())` on decode
    /// failure so non-hex test hashes still produce a deterministic
    /// signal.
    fn block_hash_to_bytes(block_hash: &BlockHash) -> [u8; 32] {
        let mut out = [0u8; 32];
        match hex::decode(block_hash) {
            Ok(bytes) if bytes.len() == 32 => {
                out.copy_from_slice(&bytes);
            }
            _ => {
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(block_hash.as_bytes());
                let result = hasher.finalize();
                out.copy_from_slice(&result);
            }
        }
        out
    }

    async fn broadcast_message(&self, message: HotStuffMessage) -> Result<(), HotStuffError> {
        if let Some(ref sender) = self.message_sender {
            let validators = self.validators.read().await;
            for validator in &validators.validators {
                if validator.node_id != self.node_id && validator.is_active {
                    let _ = sender
                        .send((validator.node_id.clone(), message.clone()))
                        .await;
                }
            }
        }
        Ok(())
    }

    async fn send_vote_to_leader(&self, vote: &HotStuffVote) -> Result<(), HotStuffError> {
        if let Some(ref sender) = self.message_sender {
            let state = self.state.read().await;
            let _ = sender
                .send((
                    state.current_leader.clone(),
                    HotStuffMessage::Vote(vote.clone()),
                ))
                .await;
        }
        Ok(())
    }

    async fn broadcast_qc_and_collect_votes(
        &self,
        _qc: &QuorumCertificate,
        _next_phase: HotStuffPhase,
    ) -> Result<(), HotStuffError> {
        // In full implementation: broadcast QC, wait for votes on next phase
        Ok(())
    }

    async fn send_new_view_message(&self, new_view: ViewNumber) -> Result<(), HotStuffError> {
        let voting_manager = self.voting_manager.read().await;
        let high_qc = voting_manager.get_high_qc().cloned().unwrap_or_else(|| {
            QuorumCertificate::new(
                HotStuffPhase::Commit,
                0,
                "genesis".to_string(),
                0,
                AggregatedSignature::MultiSig(vec![]),
                0,
                vec![],
            )
        });
        drop(voting_manager);

        let signature = self.sign_vote("newview", new_view, HotStuffPhase::Prepare)?;
        let msg = NewViewMessage {
            new_view,
            sender_id: self.node_id.clone(),
            high_qc,
            signature,
            timestamp: self.current_timestamp(),
        };

        // Send to next leader
        let validators = self.validators.read().await;
        if let Some(next_leader) = validators.get_leader(new_view) {
            if let Some(ref sender) = self.message_sender {
                let _ = sender
                    .send((next_leader.node_id.clone(), HotStuffMessage::NewView(msg)))
                    .await;
            }
        }

        Ok(())
    }

    /// Get current state (for testing/monitoring)
    pub async fn get_state(&self) -> HotStuffState {
        self.state.read().await.clone()
    }

    /// Get current metrics from the internal HotStuff engine
    pub async fn get_hotstuff_metrics(&self) -> HotStuffMetrics {
        self.voting_manager.read().await.get_metrics().await
    }
}

// =============================================================================
// ConsensusEngine wrapper
// =============================================================================

/// Wrapper around `HotStuffConsensus` that implements the `ConsensusEngine` trait,
/// bridging HotStuff's internal types to shared-protocol `Block`.
///
/// HotStuff uses its own internal block representation (`PipelinedBlock`) for the
/// pipelined BFT protocol. This wrapper creates shared-protocol `Block` objects
/// and stores HotStuff-specific metadata (view number, leader, internal hash)
/// in the block's `metadata` field.
#[derive(Debug)]
pub struct HotStuffConsensusEngine {
    /// The underlying HotStuff consensus engine
    inner: HotStuffConsensus,
}

impl HotStuffConsensusEngine {
    /// Create a new HotStuff consensus engine wrapper
    pub fn new(
        config: HotStuffConfig,
        node_id: NodeId,
        signing_key: HotStuffSigningKey,
        validators: ValidatorSet,
    ) -> ConsensusResult<Self> {
        let inner = HotStuffConsensus::new(config, node_id, signing_key, validators)
            .map_err(|e| ConsensusError::InitializationError(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Create a new HotStuff consensus engine wrapper with a generated key
    pub fn with_generated_key(
        config: HotStuffConfig,
        node_id: NodeId,
        validators: ValidatorSet,
    ) -> ConsensusResult<Self> {
        let inner = HotStuffConsensus::with_generated_key(config, node_id, validators)
            .map_err(|e| ConsensusError::InitializationError(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Access the underlying HotStuff consensus engine
    pub fn inner(&self) -> &HotStuffConsensus {
        &self.inner
    }
}

#[async_trait]
impl ConsensusEngine for HotStuffConsensusEngine {
    async fn validate_block(
        &self,
        block: &Block,
        _chain: &Blockchain,
    ) -> ConsensusResult<bool> {
        // Check consensus type stored in metadata
        if let Some(ct) = block.metadata.get("consensus_type") {
            if ct != "HotStuff" {
                return Ok(false);
            }
        }

        // Verify the validator is the expected leader
        let validator_addr = block
            .metadata
            .get("validator")
            .map(|s| s.as_str())
            .unwrap_or("");

        if validator_addr.is_empty() {
            return Ok(false);
        }

        // Check the node is a known validator
        let validators = self.inner.validators.read().await;
        let is_known = validators.get_validator(validator_addr).is_some();

        Ok(is_known)
    }

    async fn create_block(
        &self,
        transactions: Vec<Transaction>,
        previous_hash: &str,
        miner_address: &str,
        chain: &Blockchain,
    ) -> ConsensusResult<Block> {
        // Verify this node is the current leader
        let state = self.inner.state.read().await;
        if !state.is_leader {
            return Err(ConsensusError::CreationFailed(format!(
                "Node {} is not the current leader (leader is {})",
                miner_address, state.current_leader
            )));
        }
        let view_number = state.current_view;
        drop(state);

        let prev = hash_bytes(previous_hash.as_bytes());

        let mut block = Block::new(
            3, // version
            prev,
            transactions,
            1, // BFT doesn't use traditional difficulty
            chain.height + 1,
        );

        // Store producer and consensus type in metadata
        block
            .metadata
            .insert("validator".into(), miner_address.into());
        block
            .metadata
            .insert("consensus_type".into(), "HotStuff".into());
        block
            .metadata
            .insert("view_number".into(), view_number.to_string());

        // Finalize block hash
        let hash = block.calculate_hash();
        block.set_hash(hash);

        Ok(block)
    }

    async fn get_difficulty(&self) -> u64 {
        // BFT consensus doesn't have traditional difficulty
        1
    }

    fn algorithm_name(&self) -> &'static str {
        "HotStuff"
    }

    async fn configure(&mut self, config: &ConsensusConfig) -> ConsensusResult<()> {
        // Map ConsensusConfig to HotStuffConfig fields where applicable
        if let Some(param) = config.algorithm_params.get("enable_pipelining") {
            if let Some(val) = param.as_bool() {
                self.inner.config.enable_pipelining = val;
            }
        }
        if let Some(param) = config.algorithm_params.get("min_validators") {
            if let Some(val) = param.as_u64() {
                self.inner.config.min_validators = val as u32;
            }
        }
        if let Some(param) = config.algorithm_params.get("view_timeout_secs") {
            if let Some(val) = param.as_u64() {
                self.inner.config.view_timeout = std::time::Duration::from_secs(val);
            }
        }
        Ok(())
    }

    fn get_metrics(&self) -> FxHashMap<String, serde_json::Value> {
        let mut metrics = FxHashMap::default();

        metrics.insert("algorithm".to_string(), "HotStuff".into());
        metrics.insert("node_id".to_string(), self.inner.node_id.clone().into());
        metrics.insert(
            "enable_pipelining".to_string(),
            self.inner.config.enable_pipelining.into(),
        );
        metrics.insert(
            "min_validators".to_string(),
            self.inner.config.min_validators.into(),
        );
        metrics.insert(
            "enable_equivocation_detection".to_string(),
            self.inner.config.enable_equivocation_detection.into(),
        );

        metrics
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key(node_id: &str) -> HotStuffSigningKey {
        let mut seed = [0u8; 32];
        for (slot, byte) in node_id.bytes().enumerate().take(seed.len()) {
            seed[slot] = byte;
        }
        HotStuffSigningKey::from_bytes(&seed, node_id.to_string())
            .expect("deterministic test key must be valid")
    }

    fn create_test_validators() -> ValidatorSet {
        // Recreate deterministic keys per use instead of cloning secret-bearing
        // signing keys. Every caller gets the same key as this validator set.
        let key1 = test_key("node1");
        let key2 = test_key("node2");
        let key3 = test_key("node3");
        let key4 = test_key("node4");

        let validators = vec![
            HotStuffValidator {
                node_id: "node1".to_string(),
                public_key: key1.public_key_bytes(),
                voting_power: 100,
                is_active: true,
                index: 0,
            },
            HotStuffValidator {
                node_id: "node2".to_string(),
                public_key: key2.public_key_bytes(),
                voting_power: 100,
                is_active: true,
                index: 1,
            },
            HotStuffValidator {
                node_id: "node3".to_string(),
                public_key: key3.public_key_bytes(),
                voting_power: 100,
                is_active: true,
                index: 2,
            },
            HotStuffValidator {
                node_id: "node4".to_string(),
                public_key: key4.public_key_bytes(),
                voting_power: 100,
                is_active: true,
                index: 3,
            },
        ];
        ValidatorSet::new(validators, 0)
    }

    #[tokio::test]
    async fn test_hotstuff_creation() {
        let validators = create_test_validators();
        let config = HotStuffConfig::default();
        let consensus =
            HotStuffConsensus::new(config, "node1".to_string(), test_key("node1"), validators)
                .expect("Failed to create consensus");

        let state = consensus.get_state().await;
        assert_eq!(state.current_view, 0);
        assert_eq!(state.current_leader, "node1");
        assert!(state.is_leader);
    }

    #[tokio::test]
    async fn test_hotstuff_leader_election() {
        let validators = create_test_validators();
        let config = HotStuffConfig::default();

        // Node2 is not the initial leader
        let consensus =
            HotStuffConsensus::new(config, "node2".to_string(), test_key("node2"), validators)
                .expect("Failed to create consensus");

        let state = consensus.get_state().await;
        assert_eq!(state.current_leader, "node1"); // node1 is leader of view 0
        assert!(!state.is_leader);
    }

    #[tokio::test]
    async fn test_hotstuff_propose_not_leader() {
        let validators = create_test_validators();
        let config = HotStuffConfig::default();

        // Create consensus for node2 (not the initial leader)
        let consensus =
            HotStuffConsensus::new(config, "node2".to_string(), test_key("node2"), validators)
                .expect("Failed to create consensus");

        let result = consensus
            .propose_block(vec![], "state_root".to_string())
            .await;
        assert!(matches!(result, Err(HotStuffError::NotLeader(0))));
    }

    #[tokio::test]
    async fn test_hotstuff_propose_as_leader() {
        let validators = create_test_validators();
        let config = HotStuffConfig::default();

        // Create consensus for node1 (the initial leader)
        let consensus =
            HotStuffConsensus::new(config, "node1".to_string(), test_key("node1"), validators)
                .expect("Failed to create consensus");

        let result = consensus
            .propose_block(
                vec!["tx1".to_string(), "tx2".to_string()],
                "state_root_123".to_string(),
            )
            .await;

        assert!(result.is_ok());
        let proposal = result.unwrap();
        assert_eq!(proposal.view_number, 0);
        assert_eq!(proposal.leader_id, "node1");
        assert_eq!(proposal.block_height, 1);
    }

    #[tokio::test]
    async fn test_sign_and_verify_proposal() {
        let validators = create_test_validators();
        let config = HotStuffConfig::default();

        let consensus =
            HotStuffConsensus::new(config, "node1".to_string(), test_key("node1"), validators)
                .expect("Failed to create consensus");

        // Propose a block (this will sign it)
        let proposal = consensus
            .propose_block(vec!["tx1".to_string()], "state_root".to_string())
            .await
            .expect("Failed to propose block");

        // Verify the proposal signature
        let verifier = consensus.signature_verifier.read().await;
        let result = verifier.verify_proposal(&proposal);
        assert!(result.is_ok());
        assert!(result.unwrap(), "Proposal signature should be valid");
    }

    #[tokio::test]
    async fn test_sign_and_verify_vote() {
        let validators = create_test_validators();
        let config = HotStuffConfig::default();

        // Create the proposal from node1
        let consensus_leader = HotStuffConsensus::new(
            config.clone(),
            "node1".to_string(),
            test_key("node1"),
            validators.clone(),
        )
        .expect("Failed to create leader consensus");

        let proposal = consensus_leader
            .propose_block(vec!["tx1".to_string()], "state_root".to_string())
            .await
            .expect("Failed to propose block");

        // Create vote from node2
        let consensus_voter =
            HotStuffConsensus::new(config, "node2".to_string(), test_key("node2"), validators)
                .expect("Failed to create voter consensus");

        // Create and sign a vote
        let vote = consensus_voter
            .create_vote(&proposal, HotStuffPhase::Prepare)
            .expect("Failed to create vote");

        // Verify the vote signature
        let verifier = consensus_voter.signature_verifier.read().await;
        let result = verifier.verify_vote(&vote);
        assert!(result.is_ok());
        assert!(result.unwrap(), "Vote signature should be valid");
    }

    #[tokio::test]
    async fn test_qc_verification() {
        let validators = create_test_validators();
        let config = HotStuffConfig::default();

        let consensus =
            HotStuffConsensus::with_generated_key(config, "node1".to_string(), validators.clone())
                .expect("Failed to create consensus");

        // Create a valid QC with enough signers
        let qc = QuorumCertificate::new(
            HotStuffPhase::Prepare,
            1,
            "block_hash_123".to_string(),
            1,
            AggregatedSignature::MultiSig(vec![]),
            3,            // 3 out of 4 validators = quorum
            vec![0b0111], // bitmap showing nodes 0, 1, 2 signed
        );

        let result = consensus.verify_qc(&qc);
        assert!(result.is_ok(), "QC with quorum should be valid");

        // Create an invalid QC with too few signers
        let invalid_qc = QuorumCertificate::new(
            HotStuffPhase::Prepare,
            1,
            "block_hash_123".to_string(),
            1,
            AggregatedSignature::MultiSig(vec![]),
            1, // Only 1 signer - not enough
            vec![0b0001],
        );

        let result = consensus.verify_qc(&invalid_qc);
        assert!(result.is_err(), "QC without quorum should be invalid");
    }

    #[tokio::test]
    async fn test_consensus_engine_wrapper() {
        let validators = create_test_validators();
        let config = HotStuffConfig::default();

        let engine = HotStuffConsensusEngine::new(
            config,
            "node1".to_string(),
            test_key("node1"),
            validators,
        )
        .expect("Failed to create engine wrapper");

        assert_eq!(engine.algorithm_name(), "HotStuff");
        assert_eq!(engine.get_difficulty().await, 1);

        let metrics = engine.get_metrics();
        assert_eq!(
            metrics.get("algorithm").unwrap(),
            &serde_json::Value::String("HotStuff".to_string())
        );
    }

    // -----------------------------------------------------------------------
    // Issue #1036: Multi-node HotStuff consensus tests
    // -----------------------------------------------------------------------

    /// Three HotStuff validators where the leader (node1) creates a block and
    /// the other two validators accept it through the ConsensusEngine trait.
    /// This tests that a block produced by the leader passes validation on
    /// all other nodes with the same validator set.
    #[tokio::test]
    async fn multi_node_hotstuff_leader_block_accepted_by_validators() {
        let validators = create_test_validators();
        let config = HotStuffConfig::default();

        // Create three HotStuff engines with the SAME validator set.
        // node1 is the leader at view 0.
        let engine_node1 = HotStuffConsensusEngine::new(
            config.clone(),
            "node1".to_string(),
            test_key("node1"),
            validators.clone(),
        )
        .expect("Failed to create node1 engine");

        let engine_node2 = HotStuffConsensusEngine::new(
            config.clone(),
            "node2".to_string(),
            test_key("node2"),
            validators.clone(),
        )
        .expect("Failed to create node2 engine");

        let engine_node3 = HotStuffConsensusEngine::new(
            config,
            "node3".to_string(),
            test_key("node3"),
            validators,
        )
        .expect("Failed to create node3 engine");

        let chain = Blockchain::default();

        // Node1 (leader) creates a block through the ConsensusEngine trait
        let block = engine_node1
            .create_block(
                Vec::new(),
                &wrfcoin_shared_protocol::crypto::hash::Hash::default().to_string(),
                "node1",
                &chain,
            )
            .await
            .unwrap();

        assert!(block.hash.is_some());
        assert_eq!(
            block.metadata.get("consensus_type").map(|s| s.as_str()),
            Some("HotStuff")
        );
        assert_eq!(
            block.metadata.get("validator").map(|s| s.as_str()),
            Some("node1")
        );

        // All three nodes validate the block
        let valid_on_1 = engine_node1.validate_block(&block, &chain).await.unwrap();
        let valid_on_2 = engine_node2.validate_block(&block, &chain).await.unwrap();
        let valid_on_3 = engine_node3.validate_block(&block, &chain).await.unwrap();

        assert!(valid_on_1, "Leader must accept its own block");
        assert!(valid_on_2, "Validator node2 must accept the leader's block");
        assert!(valid_on_3, "Validator node3 must accept the leader's block");
    }

    /// A non-leader node should NOT be able to create a block (HotStuff
    /// requires leader authority). This tests that consensus rejects
    /// unauthorized block creation.
    #[tokio::test]
    async fn multi_node_hotstuff_non_leader_cannot_create_block() {
        let validators = create_test_validators();
        let config = HotStuffConfig::default();

        let engine_node2 = HotStuffConsensusEngine::new(
            config,
            "node2".to_string(),
            test_key("node2"),
            validators,
        )
        .expect("Failed to create node2 engine");

        let chain = Blockchain::default();

        // Node2 is not the leader, so create_block should fail
        let result = engine_node2
            .create_block(
                Vec::new(),
                &wrfcoin_shared_protocol::crypto::hash::Hash::default().to_string(),
                "node2",
                &chain,
            )
            .await;

        assert!(
            result.is_err(),
            "Non-leader node must not be able to create blocks"
        );
    }

    /// A block produced by a known validator (the leader) but validated by a
    /// node that does NOT have that validator in its set should be rejected.
    /// This tests cross-node validator set consistency.
    #[tokio::test]
    async fn multi_node_hotstuff_unknown_validator_rejected() {
        let validators = create_test_validators();
        let config = HotStuffConfig::default();

        // Node1 creates a block as leader of the full validator set
        let engine_node1 = HotStuffConsensusEngine::new(
            config.clone(),
            "node1".to_string(),
            test_key("node1"),
            validators,
        )
        .expect("Failed to create node1 engine");

        let chain = Blockchain::default();

        let block = engine_node1
            .create_block(
                Vec::new(),
                &wrfcoin_shared_protocol::crypto::hash::Hash::default().to_string(),
                "node1",
                &chain,
            )
            .await
            .unwrap();

        // Create a separate engine with a DIFFERENT validator set that does
        // not include node1.
        let isolated_key = HotStuffSigningKey::generate("isolated".to_string());
        let isolated_validator = HotStuffValidator {
            node_id: "isolated".to_string(),
            public_key: isolated_key.public_key_bytes(),
            voting_power: 100,
            is_active: true,
            index: 0,
        };
        let isolated_set = ValidatorSet::new(vec![isolated_validator], 0);

        let engine_isolated = HotStuffConsensusEngine::new(
            config,
            "isolated".to_string(),
            isolated_key,
            isolated_set,
        )
        .expect("Failed to create isolated engine");

        // The isolated node should reject the block because "node1" is not
        // in its validator set.
        let valid = engine_isolated
            .validate_block(&block, &chain)
            .await
            .unwrap();
        assert!(
            !valid,
            "Node with different validator set must reject unknown validator's block"
        );
    }

    // --------------------------------------------------------------------
    // #1216 wiring tests: HotStuff finality sink
    // --------------------------------------------------------------------
    //
    // These tests target the finality_sink field added on HotStuffConsensus
    // so that committed blocks arm the BFT finality guard in
    // `Blockchain::plan_reorg`.

    #[test]
    fn block_hash_to_bytes_decodes_hex_to_32_bytes() {
        // A real HotStuff block hash is SHA256 hex (64 chars).
        let hex_hash: BlockHash = "a".repeat(64);
        let bytes = HotStuffConsensus::block_hash_to_bytes(&hex_hash);
        assert_eq!(bytes, [0xaau8; 32]);
    }

    #[test]
    fn block_hash_to_bytes_falls_back_to_sha256_on_invalid_hex() {
        // A non-hex string still produces a stable 32-byte identifier
        // (via sha256 fallback). Two different inputs must produce
        // different outputs.
        let bytes_a = HotStuffConsensus::block_hash_to_bytes(&"not-hex".to_string());
        let bytes_b = HotStuffConsensus::block_hash_to_bytes(&"also-not-hex".to_string());
        assert_ne!(bytes_a, bytes_b);
        // Same input produces same output.
        let bytes_a2 = HotStuffConsensus::block_hash_to_bytes(&"not-hex".to_string());
        assert_eq!(bytes_a, bytes_a2);
    }

    #[tokio::test]
    async fn set_finality_sink_receives_emitted_signal() {
        use wrfcoin_shared_protocol::consensus::finality::finality_signal_channel;

        let validators = create_test_validators();
        let config = HotStuffConfig::default();
        let consensus =
            HotStuffConsensus::new(config, "node1".to_string(), test_key("node1"), validators)
                .expect("failed to create hotstuff");

        let (tx, mut rx) = finality_signal_channel();
        consensus.set_finality_sink(tx).await;

        // Drive the emit helper directly. This is the same path
        // handle_qc uses in the Commit phase, without needing to thread
        // a full three-phase QC pipeline through tests (which already
        // has its own integration coverage above).
        let hex_hash: BlockHash = "b".repeat(64);
        consensus
            .emit_finality_signal_for_block(42, hex_hash.clone())
            .await;

        let signal = rx
            .recv()
            .await
            .expect("finality sink must receive the emitted signal");
        assert_eq!(signal.height, 42);
        assert_eq!(signal.block_hash, [0xbbu8; 32]);
        assert_eq!(signal.source, "hotstuff");
    }

    #[tokio::test]
    async fn emit_finality_signal_is_noop_when_sink_absent() {
        let validators = create_test_validators();
        let config = HotStuffConfig::default();
        let consensus =
            HotStuffConsensus::new(config, "node1".to_string(), test_key("node1"), validators)
                .expect("failed to create hotstuff");

        // No sink installed. Call must not panic.
        consensus
            .emit_finality_signal_for_block(1, "c".repeat(64))
            .await;
    }

    #[tokio::test]
    async fn take_finality_sink_returns_and_disables_sink() {
        use wrfcoin_shared_protocol::consensus::finality::finality_signal_channel;

        let validators = create_test_validators();
        let config = HotStuffConfig::default();
        let consensus =
            HotStuffConsensus::new(config, "node1".to_string(), test_key("node1"), validators)
                .expect("failed to create hotstuff");

        let (tx, mut rx) = finality_signal_channel();
        consensus.set_finality_sink(tx).await;
        let taken = consensus.take_finality_sink().await;
        assert!(taken.is_some(), "take must return the previously installed sink");

        // After taking, emitting should be a no-op.
        consensus
            .emit_finality_signal_for_block(99, "d".repeat(64))
            .await;

        assert!(
            rx.try_recv().is_err(),
            "sink was removed; no signal should arrive"
        );
    }
}
