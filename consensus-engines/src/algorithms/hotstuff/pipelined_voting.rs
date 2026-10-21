//! Pipelined Voting for HotStuff BFT
//!
//! This module implements pipelined voting which allows multiple consensus phases
//! to be processed concurrently, improving throughput without sacrificing safety.
//!
//! Adapted from `blockchain-core/src/consensus/hotstuff/pipelined_voting.rs`.
//!
//! ## Key Features
//!
//! - **Concurrent Phases**: Multiple blocks can be in different phases simultaneously
//! - **O(n) Message Complexity**: Linear communication through leader-centric voting
//! - **Chained QCs**: Each QC references the previous, forming a certification chain
//! - **Three-phase finality**: Blocks finalize after 3 consecutive QCs

use log::{debug, info, warn};
use rustc_hash::FxHashMap;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::types::*;

/// Maximum number of concurrent pipelined blocks
const MAX_PIPELINE_DEPTH: usize = 10;

/// Vote collection state for a single block
#[derive(Debug, Clone)]
pub struct VoteCollector {
    /// Block being voted on
    pub block_hash: BlockHash,
    /// Block height
    pub block_height: BlockHeight,
    /// View number
    pub view_number: ViewNumber,
    /// Current phase
    pub phase: HotStuffPhase,
    /// Collected votes by voter ID
    pub votes: FxHashMap<NodeId, HotStuffVote>,
    /// Total voting power collected
    pub collected_voting_power: u128,
    /// Required voting power for quorum
    pub required_voting_power: u128,
    /// Whether quorum was reached
    pub quorum_reached: bool,
    /// Formed QC (once quorum is reached)
    pub formed_qc: Option<QuorumCertificate>,
}

impl VoteCollector {
    /// Create a new vote collector for a block
    pub fn new(
        block_hash: BlockHash,
        block_height: BlockHeight,
        view_number: ViewNumber,
        phase: HotStuffPhase,
        required_voting_power: u128,
    ) -> Self {
        Self {
            block_hash,
            block_height,
            view_number,
            phase,
            votes: FxHashMap::default(),
            collected_voting_power: 0,
            required_voting_power,
            quorum_reached: false,
            formed_qc: None,
        }
    }

    /// Add a vote to the collector
    /// Returns true if this vote caused quorum to be reached
    pub fn add_vote(&mut self, vote: HotStuffVote, voting_power: u64) -> bool {
        // Check vote is for the correct block and phase
        if vote.block_hash != self.block_hash || vote.phase != self.phase {
            warn!(
                "Vote mismatch: expected {} phase {:?}, got {} phase {:?}",
                self.block_hash, self.phase, vote.block_hash, vote.phase
            );
            return false;
        }

        // Check for duplicate vote
        if self.votes.contains_key(&vote.voter_id) {
            debug!(
                "Duplicate vote from {} for block {}",
                vote.voter_id, self.block_hash
            );
            return false;
        }

        // Add vote
        self.votes.insert(vote.voter_id.clone(), vote);
        self.collected_voting_power += u128::from(voting_power);

        // Check if quorum reached
        if !self.quorum_reached && self.collected_voting_power >= self.required_voting_power {
            self.quorum_reached = true;
            debug!(
                "Quorum reached for block {} phase {:?}",
                self.block_hash, self.phase
            );
            return true;
        }

        false
    }

    /// Create a QC from the collected votes
    pub fn create_qc(&mut self, validators: &ValidatorSet) -> Option<QuorumCertificate> {
        if !self.quorum_reached {
            return None;
        }

        if self.formed_qc.is_some() {
            return self.formed_qc.clone();
        }

        // Create signer bitmap
        let signer_bitmap_len = usize::try_from(validators.active_count().div_ceil(8))
            .expect("active validator count fits in usize");
        let mut signer_bitmap = vec![0u8; signer_bitmap_len];
        let mut signatures = Vec::new();

        for vote in self.votes.values() {
            if let Some(validator) = validators.get_validator(&vote.voter_id) {
                // Set bit in bitmap
                let byte_idx =
                    usize::try_from(validator.index / 8).expect("validator index fits in usize");
                let bit_idx = validator.index % 8;
                if byte_idx < signer_bitmap.len() {
                    signer_bitmap[byte_idx] |= 1 << bit_idx;
                }

                signatures.push(IndividualSignature {
                    signer_id: vote.voter_id.clone(),
                    signature: vote.signature.clone(),
                    public_key: validator.public_key.clone(),
                });
            }
        }

        let qc = QuorumCertificate::new(
            self.phase,
            self.view_number,
            self.block_hash.clone(),
            self.block_height,
            AggregatedSignature::MultiSig(signatures),
            u32::try_from(self.votes.len()).expect("signer count fits in u32"),
            signer_bitmap,
        );

        self.formed_qc = Some(qc.clone());
        Some(qc)
    }
}

/// Pipeline slot representing a block in progress
#[derive(Debug)]
pub struct PipelineSlot {
    /// The block being processed
    pub block: PipelinedBlock,
    /// Vote collectors for each phase
    pub collectors: FxHashMap<HotStuffPhase, VoteCollector>,
    /// When this slot was created
    pub created_at: std::time::Instant,
    /// Phases that have completed
    pub completed_phases: Vec<HotStuffPhase>,
}

impl PipelineSlot {
    /// Create a new pipeline slot for a block
    pub fn new(block: PipelinedBlock, required_voting_power: u128) -> Self {
        let mut collectors = FxHashMap::default();

        // Create collectors for all voting phases
        for phase in [
            HotStuffPhase::Prepare,
            HotStuffPhase::PreCommit,
            HotStuffPhase::Commit,
        ] {
            collectors.insert(
                phase,
                VoteCollector::new(
                    block.hash.clone(),
                    block.height,
                    block.view_number,
                    phase,
                    required_voting_power,
                ),
            );
        }

        Self {
            block,
            collectors,
            created_at: std::time::Instant::now(),
            completed_phases: Vec::new(),
        }
    }

    /// Get collector for a phase
    pub fn get_collector(&mut self, phase: HotStuffPhase) -> Option<&mut VoteCollector> {
        self.collectors.get_mut(&phase)
    }

    /// Check if all phases are complete
    pub fn is_fully_committed(&self) -> bool {
        self.completed_phases.contains(&HotStuffPhase::Commit)
    }

    /// Mark a phase as complete
    pub fn complete_phase(&mut self, phase: HotStuffPhase, qc: QuorumCertificate) {
        if !self.completed_phases.contains(&phase) {
            self.completed_phases.push(phase);
        }

        // Update block with QC
        match phase {
            HotStuffPhase::Prepare => self.block.prepare_qc = Some(qc),
            HotStuffPhase::PreCommit => self.block.precommit_qc = Some(qc),
            HotStuffPhase::Commit => {
                self.block.commit_qc = Some(qc);
                self.block.is_committed = true;
            },
            HotStuffPhase::Decide => {},
        }
    }
}

/// Pipelined voting manager
///
/// Manages concurrent voting phases for multiple blocks in flight.
/// Implements the "chained" HotStuff variant where QCs are pipelined.
#[derive(Debug)]
#[allow(dead_code)]
pub struct PipelinedVotingManager {
    /// Pipeline slots by block hash
    slots: FxHashMap<BlockHash, PipelineSlot>,
    /// Order of blocks in pipeline (for commitment order)
    pipeline_order: VecDeque<BlockHash>,
    /// Maximum pipeline depth
    max_depth: usize,
    /// Validator set
    validators: ValidatorSet,
    /// This node's ID
    node_id: NodeId,
    /// Configuration
    config: HotStuffConfig,
    /// Metrics
    metrics: Arc<RwLock<HotStuffMetrics>>,
    /// Last committed height
    last_committed_height: BlockHeight,
    /// Highest QC seen
    high_qc: Option<QuorumCertificate>,
    /// Locked QC (safety lock)
    locked_qc: Option<QuorumCertificate>,
}

impl PipelinedVotingManager {
    /// Create a new pipelined voting manager
    pub fn new(validators: ValidatorSet, node_id: NodeId, config: HotStuffConfig) -> Self {
        Self {
            slots: FxHashMap::default(),
            pipeline_order: VecDeque::new(),
            max_depth: MAX_PIPELINE_DEPTH,
            validators,
            node_id,
            config,
            metrics: Arc::new(RwLock::new(HotStuffMetrics::default())),
            last_committed_height: 0,
            high_qc: None,
            locked_qc: None,
        }
    }

    /// Add a new block to the pipeline
    pub async fn add_block(&mut self, block: PipelinedBlock) -> Result<(), HotStuffError> {
        // Check pipeline capacity
        if self.slots.len() >= self.max_depth {
            // Try to clean up committed blocks
            self.cleanup_committed_blocks().await;

            if self.slots.len() >= self.max_depth {
                return Err(HotStuffError::LivenessIssue(
                    "Pipeline full, cannot add new block".to_string(),
                ));
            }
        }

        // Verify block is valid for the pipeline
        self.verify_block_for_pipeline(&block)?;

        // Create slot
        let required_voting_power = self.validators.quorum_threshold();
        let slot = PipelineSlot::new(block.clone(), required_voting_power);

        // Add to pipeline
        self.slots.insert(block.hash.clone(), slot);
        self.pipeline_order.push_back(block.hash.clone());

        info!(
            "Added block {} height {} to pipeline",
            block.hash, block.height
        );
        Ok(())
    }

    /// Process an incoming vote
    pub async fn process_vote(
        &mut self,
        vote: HotStuffVote,
    ) -> Result<Option<QuorumCertificate>, HotStuffError> {
        // Verify vote signature
        self.verify_vote_signature(&vote)?;

        // Get the slot for this vote
        let slot = self.slots.get_mut(&vote.block_hash).ok_or_else(|| {
            HotStuffError::InvalidProposal(format!(
                "No pipeline slot for block {}",
                vote.block_hash
            ))
        })?;

        // Get voter's voting power
        let voting_power = self
            .validators
            .get_validator(&vote.voter_id)
            .map(|v| v.voting_power)
            .unwrap_or(0);

        if voting_power == 0 {
            return Err(HotStuffError::InvalidSignature(format!(
                "Unknown voter: {}",
                vote.voter_id
            )));
        }

        // Add vote to collector
        let collector = slot.get_collector(vote.phase).ok_or_else(|| {
            HotStuffError::InvalidProposal(format!("Invalid phase {:?}", vote.phase))
        })?;

        let quorum_reached = collector.add_vote(vote.clone(), voting_power);

        // Update metrics
        {
            let mut metrics = self.metrics.write().await;
            metrics.votes_received += 1;
        }

        // If quorum reached, create QC and advance phase
        if quorum_reached {
            let qc = collector.create_qc(&self.validators);
            if let Some(ref qc) = qc {
                self.handle_qc_formed(qc.clone()).await?;
                return Ok(qc.clone().into());
            }
        }

        Ok(None)
    }

    /// Handle a QC being formed
    async fn handle_qc_formed(&mut self, qc: QuorumCertificate) -> Result<(), HotStuffError> {
        info!("QC formed for block {} phase {:?}", qc.block_hash, qc.phase);

        // Update metrics
        {
            let mut metrics = self.metrics.write().await;
            metrics.qcs_formed += 1;
        }

        // Update high_qc if this is newer
        self.update_high_qc(&qc);

        // Update slot
        if let Some(slot) = self.slots.get_mut(&qc.block_hash) {
            slot.complete_phase(qc.phase, qc.clone());

            // Check for commit (three-chain rule)
            if qc.phase == HotStuffPhase::Commit {
                self.try_commit_blocks().await?;
            }
        }

        // Update locked_qc if this is a precommit QC
        if qc.phase == HotStuffPhase::PreCommit {
            self.update_locked_qc(&qc);
        }

        Ok(())
    }

    /// Try to commit blocks based on the three-chain rule
    async fn try_commit_blocks(&mut self) -> Result<(), HotStuffError> {
        // Find blocks that can be committed
        let mut blocks_to_commit = Vec::new();

        for hash in &self.pipeline_order {
            if let Some(slot) = self.slots.get(hash) {
                if slot.is_fully_committed() && slot.block.height > self.last_committed_height {
                    blocks_to_commit.push((hash.clone(), slot.block.height));
                }
            }
        }

        // Commit in order
        blocks_to_commit.sort_by_key(|(_, height)| *height);

        for (hash, height) in blocks_to_commit {
            if self
                .last_committed_height
                .checked_add(1)
                .is_some_and(|next_height| height == next_height)
            {
                self.commit_block(&hash).await?;
            }
        }

        Ok(())
    }

    /// Commit a single block
    async fn commit_block(&mut self, block_hash: &BlockHash) -> Result<(), HotStuffError> {
        if let Some(slot) = self.slots.get(block_hash) {
            info!(
                "Committing block {} height {}",
                block_hash, slot.block.height
            );

            self.last_committed_height = slot.block.height;

            // Update metrics
            {
                let mut metrics = self.metrics.write().await;
                metrics.blocks_committed += 1;

                // Calculate commit latency
                let latency = slot.created_at.elapsed().as_millis() as f64;
                let total_latency = metrics.avg_commit_latency_ms * metrics.blocks_committed as f64;
                metrics.avg_commit_latency_ms =
                    (total_latency + latency) / (metrics.blocks_committed + 1) as f64;
            }
        }

        Ok(())
    }

    /// Cleanup committed blocks from the pipeline
    async fn cleanup_committed_blocks(&mut self) {
        let mut to_remove = Vec::new();

        for hash in &self.pipeline_order {
            if let Some(slot) = self.slots.get(hash) {
                if slot.block.height <= self.last_committed_height {
                    to_remove.push(hash.clone());
                }
            }
        }

        for hash in to_remove {
            self.slots.remove(&hash);
            self.pipeline_order.retain(|h| h != &hash);
        }
    }

    /// Verify a block is valid for adding to the pipeline
    fn verify_block_for_pipeline(&self, block: &PipelinedBlock) -> Result<(), HotStuffError> {
        // Check block height
        if block.height <= self.last_committed_height {
            return Err(HotStuffError::InvalidProposal(format!(
                "Block height {} already committed",
                block.height
            )));
        }

        // Check parent exists or is the last committed block
        let needs_parent_in_pipeline = self
            .last_committed_height
            .checked_add(1)
            .is_none_or(|next_height| block.height > next_height);
        if needs_parent_in_pipeline {
            // Parent should be in pipeline
            if !self.slots.contains_key(&block.parent_hash) {
                return Err(HotStuffError::InvalidProposal(format!(
                    "Parent block {} not in pipeline",
                    block.parent_hash
                )));
            }
        }

        // Check justification QC
        self.verify_qc(&block.proposal.justify_qc)?;

        Ok(())
    }

    /// Verify a vote signature
    fn verify_vote_signature(&self, vote: &HotStuffVote) -> Result<(), HotStuffError> {
        // Get voter's public key
        let _validator = self
            .validators
            .get_validator(&vote.voter_id)
            .ok_or_else(|| {
                HotStuffError::InvalidSignature(format!("Unknown voter: {}", vote.voter_id))
            })?;

        // In production, verify the signature here using the validator's public key
        // For now, we trust signatures (signature verification is in crypto.rs)

        Ok(())
    }

    /// Verify a QC
    fn verify_qc(&self, qc: &QuorumCertificate) -> Result<(), HotStuffError> {
        // Genesis QC (view 0, "genesis" block hash) is always valid
        if qc.view_number == 0 && qc.block_hash == "genesis" {
            return Ok(());
        }

        // Check QC has enough signers
        if !qc.has_quorum(self.validators.active_count()) {
            return Err(HotStuffError::InvalidQC(format!(
                "QC has {} signers, need {}",
                qc.signer_count,
                QuorumCertificate::quorum_threshold(self.validators.active_count())
            )));
        }

        // In production, verify aggregated signature

        Ok(())
    }

    /// Update high_qc if the given QC is higher
    fn update_high_qc(&mut self, qc: &QuorumCertificate) {
        let should_update = match &self.high_qc {
            None => true,
            Some(current) => qc.view_number > current.view_number,
        };

        if should_update {
            self.high_qc = Some(qc.clone());
        }
    }

    /// Update locked_qc if the given QC is higher
    fn update_locked_qc(&mut self, qc: &QuorumCertificate) {
        let should_update = match &self.locked_qc {
            None => true,
            Some(current) => qc.view_number > current.view_number,
        };

        if should_update {
            self.locked_qc = Some(qc.clone());
        }
    }

    /// Get the current high QC
    pub fn get_high_qc(&self) -> Option<&QuorumCertificate> {
        self.high_qc.as_ref()
    }

    /// Get the locked QC
    #[allow(dead_code)]
    pub fn get_locked_qc(&self) -> Option<&QuorumCertificate> {
        self.locked_qc.as_ref()
    }

    /// Get the last committed height
    #[allow(dead_code)]
    pub fn get_last_committed_height(&self) -> BlockHeight {
        self.last_committed_height
    }

    /// Get current pipeline depth
    #[allow(dead_code)]
    pub fn pipeline_depth(&self) -> usize {
        self.slots.len()
    }

    /// Get metrics
    pub async fn get_metrics(&self) -> HotStuffMetrics {
        self.metrics.read().await.clone()
    }

    /// Check if a block is in the pipeline
    #[allow(dead_code)]
    pub fn has_block(&self, block_hash: &BlockHash) -> bool {
        self.slots.contains_key(block_hash)
    }

    /// Get a block from the pipeline
    #[allow(dead_code)]
    pub fn get_block(&self, block_hash: &BlockHash) -> Option<&PipelinedBlock> {
        self.slots.get(block_hash).map(|s| &s.block)
    }
}

/// Optimistic fast path for HotStuff
///
/// When network conditions are good, HotStuff can progress without waiting
/// for timeouts, achieving "optimistic responsiveness".
#[derive(Debug)]
#[allow(dead_code)]
pub struct OptimisticPath {
    /// Expected round trip time
    expected_rtt: std::time::Duration,
    /// Fast path threshold (votes before timeout)
    fast_path_threshold: f64,
    /// Current network delay estimate
    current_delay_estimate: std::time::Duration,
    /// Successful fast paths
    fast_path_successes: u64,
    /// Failed fast paths (fell back to timeout)
    fast_path_failures: u64,
}

impl OptimisticPath {
    /// Create a new optimistic path tracker
    pub fn new(expected_rtt: std::time::Duration) -> Self {
        Self {
            expected_rtt,
            fast_path_threshold: 0.9, // 90% of votes needed for fast path
            current_delay_estimate: expected_rtt,
            fast_path_successes: 0,
            fast_path_failures: 0,
        }
    }

    /// Check if we should wait for fast path
    #[allow(dead_code)]
    pub fn should_wait_for_fast_path(&self) -> bool {
        // Only use fast path if success rate is good
        let total = self.fast_path_successes + self.fast_path_failures;
        if total < 10 {
            return true; // Try fast path by default initially
        }

        let success_rate = self.fast_path_successes as f64 / total as f64;
        success_rate > 0.5 // Continue with fast path if >50% success
    }

    /// Record a fast path attempt result
    pub fn record_result(&mut self, success: bool, actual_delay: std::time::Duration) {
        if success {
            self.fast_path_successes += 1;
        } else {
            self.fast_path_failures += 1;
        }

        // Update delay estimate with exponential moving average
        let alpha = 0.1;
        let current_ms = self.current_delay_estimate.as_millis() as f64;
        let actual_ms = actual_delay.as_millis() as f64;
        let new_estimate = (1.0 - alpha) * current_ms + alpha * actual_ms;
        self.current_delay_estimate = std::time::Duration::from_millis(new_estimate as u64);
    }

    /// Get the suggested timeout for optimistic responsiveness
    #[allow(dead_code)]
    pub fn get_optimistic_timeout(&self) -> std::time::Duration {
        // 2*delta is the theoretical bound for optimistic responsiveness
        self.current_delay_estimate * 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_validators() -> ValidatorSet {
        let validators = vec![
            HotStuffValidator {
                node_id: "node1".to_string(),
                public_key: vec![1, 2, 3],
                voting_power: 100,
                is_active: true,
                index: 0,
            },
            HotStuffValidator {
                node_id: "node2".to_string(),
                public_key: vec![4, 5, 6],
                voting_power: 100,
                is_active: true,
                index: 1,
            },
            HotStuffValidator {
                node_id: "node3".to_string(),
                public_key: vec![7, 8, 9],
                voting_power: 100,
                is_active: true,
                index: 2,
            },
            HotStuffValidator {
                node_id: "node4".to_string(),
                public_key: vec![10, 11, 12],
                voting_power: 100,
                is_active: true,
                index: 3,
            },
        ];
        ValidatorSet::new(validators, 0)
    }

    #[allow(dead_code)]
    fn create_test_qc(view: ViewNumber, phase: HotStuffPhase) -> QuorumCertificate {
        QuorumCertificate::new(
            phase,
            view,
            format!("blockhash{}", view),
            view,
            AggregatedSignature::MultiSig(vec![]),
            3,
            vec![0b1111],
        )
    }

    #[test]
    fn test_quorum_threshold() {
        // For n=4 validators (3f+1 where f=1), quorum should be 3 (2f+1)
        assert_eq!(QuorumCertificate::quorum_threshold(4), 3);
        // For n=7 validators (3f+1 where f=2), quorum should be 5 (2f+1)
        assert_eq!(QuorumCertificate::quorum_threshold(7), 5);
        // For n=10 validators (3f+1 where f=3), quorum should be 7 (2f+1)
        assert_eq!(QuorumCertificate::quorum_threshold(10), 7);
    }

    #[test]
    fn test_vote_collector() {
        let validators = create_test_validators();
        let mut collector = VoteCollector::new(
            "block1".to_string(),
            1,
            1,
            HotStuffPhase::Prepare,
            validators.quorum_threshold(), // 267 for 400 total power
        );

        // Add first vote
        let vote1 = HotStuffVote::new(
            HotStuffPhase::Prepare,
            1,
            "block1".to_string(),
            1,
            "node1".to_string(),
            vec![1, 2, 3],
        );
        assert!(!collector.add_vote(vote1, 100)); // Not yet quorum

        // Add second vote
        let vote2 = HotStuffVote::new(
            HotStuffPhase::Prepare,
            1,
            "block1".to_string(),
            1,
            "node2".to_string(),
            vec![4, 5, 6],
        );
        assert!(!collector.add_vote(vote2, 100)); // Still not quorum

        // Add third vote - should reach quorum
        let vote3 = HotStuffVote::new(
            HotStuffPhase::Prepare,
            1,
            "block1".to_string(),
            1,
            "node3".to_string(),
            vec![7, 8, 9],
        );
        assert!(collector.add_vote(vote3, 100)); // Quorum reached!

        assert!(collector.quorum_reached);
        let qc = collector.create_qc(&validators);
        assert!(qc.is_some());
    }

    #[test]
    fn validator_set_handles_total_voting_power_above_u64() {
        let validators = vec![
            HotStuffValidator {
                node_id: "node1".to_string(),
                public_key: vec![1, 2, 3],
                voting_power: u64::MAX - 1,
                is_active: true,
                index: 0,
            },
            HotStuffValidator {
                node_id: "node2".to_string(),
                public_key: vec![4, 5, 6],
                voting_power: 10,
                is_active: true,
                index: 1,
            },
        ];
        let validator_set = ValidatorSet::new(validators, 0);
        let expected_total = u128::from(u64::MAX - 1) + 10;
        assert_eq!(validator_set.total_voting_power, expected_total);
        assert_eq!(
            validator_set.quorum_threshold(),
            (expected_total * 2).div_ceil(3)
        );
    }

    #[test]
    fn vote_collector_handles_large_voting_power_without_wrapping() {
        let validators = vec![
            HotStuffValidator {
                node_id: "node1".to_string(),
                public_key: vec![1, 2, 3],
                voting_power: u64::MAX,
                is_active: true,
                index: 0,
            },
            HotStuffValidator {
                node_id: "node2".to_string(),
                public_key: vec![4, 5, 6],
                voting_power: 10,
                is_active: true,
                index: 1,
            },
        ];
        let validator_set = ValidatorSet::new(validators, 0);
        let mut collector = VoteCollector::new(
            "block-overflow".to_string(),
            1,
            1,
            HotStuffPhase::Prepare,
            validator_set.quorum_threshold(),
        );

        let vote1 = HotStuffVote::new(
            HotStuffPhase::Prepare,
            1,
            "block-overflow".to_string(),
            1,
            "node1".to_string(),
            vec![1, 2, 3],
        );
        assert!(collector.add_vote(vote1, u64::MAX));
        assert_eq!(collector.collected_voting_power, u128::from(u64::MAX));
    }

    #[test]
    fn test_phase_progression() {
        assert_eq!(
            HotStuffPhase::Prepare.next(),
            Some(HotStuffPhase::PreCommit)
        );
        assert_eq!(HotStuffPhase::PreCommit.next(), Some(HotStuffPhase::Commit));
        assert_eq!(HotStuffPhase::Commit.next(), Some(HotStuffPhase::Decide));
        assert_eq!(HotStuffPhase::Decide.next(), None);
    }

    #[test]
    fn test_validator_set_leader_election() {
        let validators = create_test_validators();

        // Leader should rotate based on view number
        let leader0 = validators.get_leader(0).unwrap();
        let leader1 = validators.get_leader(1).unwrap();
        let leader4 = validators.get_leader(4).unwrap();

        assert_eq!(leader0.node_id, "node1");
        assert_eq!(leader1.node_id, "node2");
        assert_eq!(leader4.node_id, "node1"); // Should wrap around
    }

    #[test]
    fn test_optimistic_path() {
        let mut optimistic = OptimisticPath::new(std::time::Duration::from_millis(100));

        // Initially should use fast path
        assert!(optimistic.should_wait_for_fast_path());

        // Record some successes
        for _ in 0..10 {
            optimistic.record_result(true, std::time::Duration::from_millis(90));
        }

        // Should still use fast path
        assert!(optimistic.should_wait_for_fast_path());

        // Record many failures
        for _ in 0..20 {
            optimistic.record_result(false, std::time::Duration::from_millis(500));
        }

        // Should no longer use fast path
        assert!(!optimistic.should_wait_for_fast_path());
    }
}
