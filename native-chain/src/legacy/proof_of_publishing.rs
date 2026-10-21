use crate::blockchain::block::Block;
use crate::blockchain::chain::Blockchain;
use crate::blockchain::state::StateTransition;
use crate::blockchain::token_emission::TokenEmissionManager;
use crate::blockchain::transaction::{
    CalibrationAttestationPayload, RevealPayload, Transaction, TransactionType,
};
use crate::common::constants::DEFAULT_ORACLE_QUORUM;
use crate::common::types::{Address, Balance};
use crate::contributions::commit_reveal::CommitRevealState;
use crate::crypto::hash::{Hash, hash_bytes};
use crate::mempool::pool::Mempool;
use crate::nft::computational_nft::ComputationalNFT;
use anyhow::{Context, Result, anyhow};
use log::{debug, info, warn};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

/// Represents different roles in the Proof-of-Publishing ecosystem
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PublishingRole {
    /// Collects raw data from sensors, IoT devices, or web sources
    Collector,

    /// Analyzes data to produce valuable insights or forecasts
    Analyzer,

    /// Stores and validates data for long-term access
    Keeper,

    /// Purchases carbon offsets or performs environmental actions
    Offsetter,
}

impl PublishingRole {
    /// Convert a role from string representation
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "collector" => Some(Self::Collector),
            "analyzer" => Some(Self::Analyzer),
            "keeper" => Some(Self::Keeper),
            "offsetter" => Some(Self::Offsetter),
            _ => None,
        }
    }

    /// Convert a role to string representation
    pub fn to_str(&self) -> &'static str {
        match self {
            Self::Collector => "collector",
            Self::Analyzer => "analyzer",
            Self::Keeper => "keeper",
            Self::Offsetter => "offsetter",
        }
    }
}

/// Implements the Proof-of-Publishing (PoP) consensus algorithm
/// This extends the PoUA (Proof of Useful Analysis) with additional
/// publishing-specific validation and token distribution logic
pub struct ProofOfPublishing {
    /// Current validation difficulty (for fallback when no analysis is available)
    pub difficulty: u32,

    /// Target block time in seconds
    pub target_block_time: u64,

    /// Required publishing score threshold for accepting a block
    pub min_publishing_score: f64,

    /// Maximum number of transactions to include in a block
    pub max_tx_per_block: usize,

    /// Time of the last difficulty adjustment
    last_adjustment_time: Option<Instant>,

    /// Number of blocks since last difficulty adjustment
    blocks_since_adjustment: u64,

    /// Role-based rewards distribution percentages
    role_reward_percentages: HashMap<PublishingRole, f64>,

    /// Region scarcity weighting factor (1.0 is neutral)
    region_weights: HashMap<String, f64>,

    /// Accuracy adjustment factor (increases rewards for highly accurate forecasts)
    accuracy_multiplier: f64,

    /// Reputation adjustment factor (increases rewards for publishers with good reputation)
    reputation_multiplier: f64,

    /// Submission timing rules (in seconds)
    submission_time_window: u64,
}

impl ProofOfPublishing {
    fn verify_attestation_signature(
        &self,
        attestation: &crate::blockchain::transaction::OracleAttestationPayload,
    ) -> bool {
        attestation.has_valid_signature()
    }

    /// Creates a new ProofOfPublishing instance
    pub fn new(
        initial_difficulty: u32,
        target_block_time: u64,
        min_publishing_score: f64,
        max_tx_per_block: usize,
    ) -> Self {
        // Default role distribution: 35% for analyzers, 30% for collectors,
        // 25% for keepers, and 10% for offsetters
        let mut role_reward_percentages = HashMap::new();
        role_reward_percentages.insert(PublishingRole::Analyzer, 0.35);
        role_reward_percentages.insert(PublishingRole::Collector, 0.30);
        role_reward_percentages.insert(PublishingRole::Keeper, 0.25);
        role_reward_percentages.insert(PublishingRole::Offsetter, 0.10);

        Self {
            difficulty: initial_difficulty,
            target_block_time,
            min_publishing_score,
            max_tx_per_block,
            last_adjustment_time: None,
            blocks_since_adjustment: 0,
            role_reward_percentages,
            region_weights: HashMap::new(),
            accuracy_multiplier: 1.0,
            reputation_multiplier: 1.0,
            submission_time_window: 3600, // Default 1 hour
        }
    }

    /// Set the reward distribution percentages for different roles
    pub fn set_role_reward_percentages(
        &mut self,
        percentages: HashMap<PublishingRole, f64>,
    ) -> Result<()> {
        // Validate that percentages add up to 1.0
        let total: f64 = percentages.values().sum();
        if (total - 1.0).abs() > 0.001 {
            return Err(anyhow!(
                "Role reward percentages must add up to 1.0, got {total}"
            ));
        }

        self.role_reward_percentages = percentages;
        Ok(())
    }

    /// Set the region scarcity weights
    pub fn set_region_weights(&mut self, weights: HashMap<String, f64>) {
        self.region_weights = weights;
    }

    /// Set the accuracy multiplier
    pub fn set_accuracy_multiplier(&mut self, multiplier: f64) {
        self.accuracy_multiplier = multiplier.max(0.0);
    }

    /// Set the reputation multiplier
    pub fn set_reputation_multiplier(&mut self, multiplier: f64) {
        self.reputation_multiplier = multiplier.max(0.0);
    }

    /// Set the submission time window
    pub fn set_submission_time_window(&mut self, window_seconds: u64) {
        self.submission_time_window = window_seconds;
    }

    /// Creates a new block with verified publications.
    ///
    /// Three-phase lock pattern (#974):
    /// - Phase A: Snapshot chain/mempool state under short read locks
    /// - Phase B: Build block with no locks held (assembly, hashing, mining)
    /// - Phase C: Commit under write lock with stale-tip check
    pub async fn create_next_block(
        &mut self,
        blockchain: Arc<RwLock<Blockchain>>,
        mempool: Arc<RwLock<Mempool>>,
        miner_address: &str,
    ) -> Result<Block> {
        // Start timing for difficulty adjustment
        let start_time = Instant::now();
        if self.last_adjustment_time.is_none() {
            self.last_adjustment_time = Some(start_time);
        }

        // ── Phase A: Snapshot under short read locks ──────────────────
        let (snapshot_tip, snapshot_height, block_reward) = {
            let chain = blockchain.read().await;
            (
                chain.tip,
                chain.height,
                chain.calculate_block_reward(chain.height + 1),
            )
        };

        let (
            contribution_reveal_transactions,
            forecast_reveal_transactions,
            regular_transactions,
        ) = {
            let mempool_guard = mempool.read().await;

            let contrib = mempool_guard.get_transactions_by_type(
                TransactionType::ContributionReveal,
                (self.max_tx_per_block / 2).max(1),
            );
            let forecast = mempool_guard.get_transactions_by_type(
                TransactionType::ForecastReveal,
                (self.max_tx_per_block / 2).max(1),
            );
            let priority_tx_count = contrib.len() + forecast.len() + 1;
            let remaining_space = self.max_tx_per_block.saturating_sub(priority_tx_count);
            let regular = if remaining_space > 0 {
                mempool_guard.get_transactions_with_limit(remaining_space)
            } else {
                vec![]
            };

            (contrib, forecast, regular)
        };
        // Both read locks released here.

        // ── Phase B: Build block with no locks held ───────────────────
        let coinbase =
            Transaction::new_coinbase(miner_address, block_reward, snapshot_height + 1);

        let contribution_reveal_owned = contribution_reveal_transactions
            .iter()
            .map(|tx| tx.as_ref().clone())
            .collect::<Vec<_>>();
        let forecast_reveal_owned = forecast_reveal_transactions
            .iter()
            .map(|tx| tx.as_ref().clone())
            .collect::<Vec<_>>();
        let regular_transactions_owned =
            regular_transactions.iter().cloned().collect::<Vec<_>>();

        let mut transactions = self.assemble_block_transactions(
            coinbase,
            contribution_reveal_owned,
            forecast_reveal_owned,
            regular_transactions_owned,
        );
        self.filter_by_submission_timing(&mut transactions);

        let publishing_score = self.calculate_block_publishing_score(&transactions);

        let mut block = Block::new(
            3, // version 3 for Proof-of-Publishing
            snapshot_tip,
            transactions,
            self.difficulty,
            snapshot_height + 1,
        );

        block
            .metadata
            .insert("publishing_score".to_string(), publishing_score.to_string());

        let role_stats = self.calculate_role_statistics(&block.transactions);
        for (role, count) in &role_stats {
            let key = format!("role_{}_count", role.to_str());
            block.metadata.insert(key, count.to_string());
        }

        info!(
            "Creating new PoP block at height {} with {} transactions (publishing score: {:.2})",
            block.height,
            block.transactions.len(),
            publishing_score
        );

        if publishing_score >= self.min_publishing_score {
            let hash = block.calculate_hash();
            block.set_hash(hash);
            info!(
                "Block has sufficient publishing score ({publishing_score:.2}), accepting without mining"
            );
        } else {
            warn!("Insufficient publishing score ({publishing_score:.2}), falling back to PoW");
            let hash = self.mine_block(&mut block)?;
            block.set_hash(hash);
        }

        self.apply_role_based_rewards(&mut block);

        // ── Phase C: Commit under write lock with stale-tip check ─────
        let mut chain = blockchain.write().await;

        // Guard against TOCTOU race: if a P2P block arrived between Phase A
        // and now, the chain tip will have advanced and our block is stale.
        if chain.tip != snapshot_tip || chain.height != snapshot_height {
            return Err(anyhow::anyhow!(
                "stale tip: chain advanced during block production \
                 (expected height={}, got={})",
                snapshot_height,
                chain.height
            ));
        }

        self.apply_governance_transitions(&mut chain, &block)?;
        self.apply_contribution_rewards(&mut chain, &mut block)?;
        chain.add_block(block.clone())?;

        // Remove processed transactions from mempool
        let mut mempool_guard = mempool.write().await;
        for tx in &contribution_reveal_transactions {
            if let Some(tx_id) = &tx.id {
                mempool_guard.remove_transaction(tx_id);
            }
        }
        for tx in &forecast_reveal_transactions {
            if let Some(tx_id) = &tx.id {
                mempool_guard.remove_transaction(tx_id);
            }
        }
        for tx in &regular_transactions {
            if let Some(tx_id) = &tx.id {
                mempool_guard.remove_transaction(tx_id);
            }
        }

        // Track block for difficulty adjustment
        self.blocks_since_adjustment += 1;
        self.adjust_difficulty();

        info!(
            "Successfully created PoP block at height {}, hash: {}",
            block.height,
            block.hash.as_ref().unwrap()
        );

        Ok(block)
    }

    /// Assemble transactions with reveal-first ordering for block construction.
    fn assemble_block_transactions(
        &self,
        coinbase: Transaction,
        contribution_reveals: Vec<Transaction>,
        forecast_reveals: Vec<Transaction>,
        regular_transactions: Vec<Transaction>,
    ) -> Vec<Transaction> {
        let mut transactions = vec![coinbase];
        transactions.extend(contribution_reveals);
        transactions.extend(forecast_reveals);
        transactions.extend(regular_transactions);
        transactions
    }

    /// Calculate the publishing score for a block based on its transactions
    fn calculate_block_publishing_score(&self, transactions: &[Transaction]) -> f64 {
        let mut total_score = 0.0;
        let mut publishing_tx_count = 0;

        let valid_reveal_commit_ids = self.collect_valid_reveal_commit_ids(transactions);
        let attested_commit_ids = self.collect_attested_commit_ids(
            transactions,
            &valid_reveal_commit_ids,
            DEFAULT_ORACLE_QUORUM.max(1),
            None,
        );

        for tx in transactions {
            let tx_score = match tx.tx_type {
                // Commits are protocol inputs only; they are never directly rewarded.
                TransactionType::ContributionCommit | TransactionType::ForecastCommit => 0.0,
                TransactionType::ContributionReveal => {
                    let Some(commit_id) = tx.commit_id() else {
                        continue;
                    };
                    // Phase 1 gating: contribution reveals are scored only when oracle-attested.
                    if valid_reveal_commit_ids.contains(&commit_id)
                        && attested_commit_ids.contains(&commit_id)
                    {
                        self.score_data_collection(tx)
                    } else {
                        0.0
                    }
                },
                TransactionType::ForecastReveal => {
                    let Some(commit_id) = tx.commit_id() else {
                        continue;
                    };
                    // PR2 rule: score forecast reveals only when commit-reveal verification passed.
                    if valid_reveal_commit_ids.contains(&commit_id) {
                        self.score_data_analysis(tx)
                    } else {
                        0.0
                    }
                },
                TransactionType::OracleAttestation => {
                    self.score_oracle_attestation(tx, &attested_commit_ids)
                },
                _ => 0.0, // Regular transactions don't contribute to publishing score
            };

            if tx_score > 0.0 {
                total_score += tx_score;
                publishing_tx_count += 1;
            }
        }

        // Return average score, or 0 if no publishing transactions
        if publishing_tx_count > 0 {
            total_score / publishing_tx_count as f64
        } else {
            0.0
        }
    }

    /// Build the set of reveal commit IDs that pass commit-reveal verification.
    fn collect_valid_reveal_commit_ids(&self, transactions: &[Transaction]) -> HashSet<String> {
        let mut state = CommitRevealState::default();
        let mut valid_commit_ids = HashSet::new();

        // Register all commits first so reveal validation can reference prior-registered state.
        for (idx, tx) in transactions.iter().enumerate() {
            match tx.tx_type {
                TransactionType::ContributionCommit | TransactionType::ForecastCommit => {
                    if let Some(payload) = tx.parse_commit_payload() {
                        let _ = state.register_commit(&payload, idx as u64);
                    }
                },
                _ => {},
            }
        }

        for (idx, tx) in transactions.iter().enumerate() {
            match tx.tx_type {
                TransactionType::ContributionReveal | TransactionType::ForecastReveal => {
                    if let Some(payload) = tx.parse_reveal_payload() {
                        if state.apply_reveal(&payload, idx as u64).is_ok() {
                            valid_commit_ids.insert(payload.commit_id);
                        }
                    }
                },
                _ => {},
            }
            state.prune_expired(idx as u64);
        }

        valid_commit_ids
    }

    /// Collect commit IDs that have reached the required quorum of *distinct* oracle approvals.
    ///
    /// Each oracle is counted at most once per commit ID to prevent a single oracle
    /// from inflating the approval count by submitting duplicate attestation transactions.
    fn collect_attested_commit_ids(
        &self,
        transactions: &[Transaction],
        valid_reveal_commit_ids: &HashSet<String>,
        required_quorum: usize,
        allowed_oracles: Option<&HashSet<String>>,
    ) -> HashSet<String> {
        // commit_id -> set of distinct oracle_ids that approved it
        let mut approvals: HashMap<String, HashSet<String>> = HashMap::new();
        for tx in transactions {
            if tx.tx_type != TransactionType::OracleAttestation {
                continue;
            }
            if let Some(attestation) = tx.parse_oracle_attestation_payload() {
                if !attestation.approved {
                    continue;
                }
                if !self.verify_attestation_signature(&attestation) {
                    continue;
                }
                if !valid_reveal_commit_ids.contains(&attestation.commit_id) {
                    continue;
                }
                if let Some(oracles) = allowed_oracles {
                    if !oracles.is_empty() && !oracles.contains(&attestation.oracle_id) {
                        continue;
                    }
                }
                approvals
                    .entry(attestation.commit_id)
                    .or_default()
                    .insert(attestation.oracle_id);
            }
        }
        approvals
            .into_iter()
            .filter_map(|(commit_id, unique_oracles)| {
                (unique_oracles.len() >= required_quorum).then_some(commit_id)
            })
            .collect()
    }

    /// Score oracle attestations that approve commit IDs with valid reveals.
    fn score_oracle_attestation(
        &self,
        tx: &Transaction,
        valid_reveal_commit_ids: &HashSet<String>,
    ) -> f64 {
        let Some(attestation) = tx.parse_oracle_attestation_payload() else {
            return 0.0;
        };
        if !attestation.approved || !valid_reveal_commit_ids.contains(&attestation.commit_id) {
            return 0.0;
        }

        let confidence = attestation.confidence.unwrap_or(1.0).clamp(0.0, 1.0);
        2.0 * confidence
    }

    fn apply_contribution_rewards(&self, chain: &mut Blockchain, block: &mut Block) -> Result<()> {
        let revealed_commit_ids = self.apply_launch_state_transitions(chain, block);
        let attested_commit_ids: HashSet<String> = revealed_commit_ids
            .iter()
            .filter_map(|commit_id| {
                let has_quorum = chain
                    .state
                    .attestations
                    .get(commit_id)
                    .map(|set| set.has_quorum(chain.state.oracle_quorum.max(1)))
                    .unwrap_or(false);
                has_quorum.then_some(commit_id.clone())
            })
            .collect();

        let consensus_reward = chain.calculate_block_reward(block.height).max(1);
        let mut emission = TokenEmissionManager::default();
        emission.set_max_emission_per_block(consensus_reward);

        let mut minted_count = 0u64;
        let mut minted_contribution_count = 0u64;
        let mut minted_forecast_count = 0u64;
        let mut minted_total = 0u128;
        let mut skipped_calibration_count = 0u64;
        let mut skipped_fraud_count = 0u64;
        let mut skipped_unattested_forecast_count = 0u64;
        let mut seen_contribution_payloads = HashMap::<String, String>::new();
        let emission_override = Some(chain.state.governance.params.emission_rate);

        for tx in &block.transactions {
            let Some(commit_id) = tx.commit_id() else {
                continue;
            };

            let recipient = tx
                .outputs
                .first()
                .map(|o| o.address.clone())
                .unwrap_or_else(|| "unknown".to_string());
            let recipient_address = self.resolve_recipient_address(&recipient, &tx.sender_address);

            match tx.tx_type {
                TransactionType::ContributionReveal => {
                    if !attested_commit_ids.contains(&commit_id) {
                        continue;
                    }
                    let Some(reveal_payload) = tx.parse_reveal_payload() else {
                        continue;
                    };
                    if !chain.state.has_active_calibration(
                        &reveal_payload.station_id,
                        reveal_payload.timestamp,
                    ) {
                        skipped_calibration_count = skipped_calibration_count.saturating_add(1);
                        continue;
                    }
                    if !self.passes_contribution_integrity_checks(
                        &reveal_payload,
                        block,
                        &mut seen_contribution_payloads,
                    ) {
                        skipped_fraud_count = skipped_fraud_count.saturating_add(1);
                        continue;
                    }

                    let contribution_score = self.score_data_collection(tx);
                    let oracle_quality = chain
                        .state
                        .attestations
                        .get(&commit_id)
                        .map(|s| s.average_confidence())
                        .unwrap_or(1.0);
                    let reward = emission.calculate_contribution_reward(
                        consensus_reward,
                        contribution_score,
                        oracle_quality,
                        emission_override,
                    ) as u128;

                    if reward == 0 {
                        continue;
                    }

                    chain.apply_state_transition_no_root(StateTransition::ApplyReward {
                        address: recipient_address,
                        amount: Balance::new(reward),
                        reason: format!("contribution:{commit_id}"),
                    })?;
                    minted_count = minted_count.saturating_add(1);
                    minted_contribution_count = minted_contribution_count.saturating_add(1);
                    minted_total = minted_total.saturating_add(reward);
                },
                TransactionType::ForecastReveal => {
                    if !attested_commit_ids.contains(&commit_id) {
                        skipped_unattested_forecast_count =
                            skipped_unattested_forecast_count.saturating_add(1);
                        continue;
                    }
                    let Some(forecast_skill) = self.calculate_forecast_skill_score(tx) else {
                        continue;
                    };
                    let oracle_quality = chain
                        .state
                        .attestations
                        .get(&commit_id)
                        .map(|s| s.average_confidence())
                        .unwrap_or(1.0);
                    let reward = emission.calculate_forecast_skill_reward(
                        consensus_reward,
                        forecast_skill,
                        oracle_quality,
                        emission_override,
                    ) as u128;
                    if reward == 0 {
                        continue;
                    }
                    chain.apply_state_transition_no_root(StateTransition::ApplyReward {
                        address: recipient_address,
                        amount: Balance::new(reward),
                        reason: format!("forecast_skill:{commit_id}:{forecast_skill:.6}"),
                    })?;
                    minted_count = minted_count.saturating_add(1);
                    minted_forecast_count = minted_forecast_count.saturating_add(1);
                    minted_total = minted_total.saturating_add(reward);
                },
                _ => {},
            }
        }

        block
            .metadata
            .insert("rewards_minted_count".to_string(), minted_count.to_string());
        block.metadata.insert(
            "rewards_minted_contribution_count".to_string(),
            minted_contribution_count.to_string(),
        );
        block.metadata.insert(
            "rewards_minted_forecast_count".to_string(),
            minted_forecast_count.to_string(),
        );
        block.metadata.insert(
            "rewards_skipped_calibration_count".to_string(),
            skipped_calibration_count.to_string(),
        );
        block.metadata.insert(
            "rewards_skipped_fraud_count".to_string(),
            skipped_fraud_count.to_string(),
        );
        block.metadata.insert(
            "rewards_skipped_unattested_forecast_count".to_string(),
            skipped_unattested_forecast_count.to_string(),
        );
        block
            .metadata
            .insert("rewards_minted_total".to_string(), minted_total.to_string());

        // Single state root recomputation for all transitions applied during
        // launch state transitions + reward distribution (instead of per-transition).
        chain.state.recalculate_state_root();

        Ok(())
    }

    fn apply_launch_state_transitions(
        &self,
        chain: &mut Blockchain,
        block: &Block,
    ) -> HashSet<String> {
        let mut revealed_commit_ids = HashSet::new();

        for tx in &block.transactions {
            match tx.tx_type {
                TransactionType::ContributionCommit | TransactionType::ForecastCommit => {
                    if let Some(payload) = tx.parse_commit_payload() {
                        if let Err(e) =
                            chain.apply_state_transition_no_root(StateTransition::RegisterCommit {
                                payload,
                                block_height: block.height,
                            })
                        {
                            warn!("Ignoring invalid commit during reward pass: {e}");
                        }
                    }
                },
                TransactionType::ContributionReveal | TransactionType::ForecastReveal => {
                    if let Some(payload) = tx.parse_reveal_payload() {
                        let commit_id = payload.commit_id.clone();
                        match chain.apply_state_transition_no_root(StateTransition::ApplyReveal {
                            payload: payload.clone(),
                            block_height: block.height,
                            block_timestamp: block.timestamp.timestamp(),
                        }) {
                            Ok(_) => {
                                if let Some(calibration_attestation) =
                                    Self::extract_calibration_attestation(&payload)
                                {
                                    if let Err(err) = chain.apply_state_transition_no_root(
                                        StateTransition::RecordCalibrationAttestation {
                                            payload: calibration_attestation,
                                        },
                                    ) {
                                        warn!(
                                            "Ignoring invalid calibration attestation during reward pass: {err}"
                                        );
                                    }
                                }
                                revealed_commit_ids.insert(commit_id);
                            },
                            Err(e) => warn!("Ignoring invalid reveal during reward pass: {e}"),
                        }
                    }
                },
                _ => {},
            }
        }

        for tx in &block.transactions {
            if tx.tx_type != TransactionType::OracleAttestation {
                continue;
            }
            if let Some(payload) = tx.parse_oracle_attestation_payload() {
                if let Err(e) =
                    chain.apply_state_transition_no_root(StateTransition::RecordOracleAttestation {
                        payload,
                        current_block: block.height,
                    })
                {
                    warn!("Ignoring invalid oracle attestation during reward pass: {e}");
                }
            }
        }

        revealed_commit_ids
    }

    fn apply_governance_transitions(&self, chain: &mut Blockchain, block: &Block) -> Result<()> {
        let transitions = chain.state.governance_transitions_for_block(
            &block.transactions,
            block.height,
            block.timestamp.timestamp(),
        );
        chain.apply_state_transitions_batch(transitions)?;
        Ok(())
    }

    fn resolve_recipient_address(&self, recipient: &str, sender_address: &Address) -> Address {
        if let Ok(addr) = recipient.parse::<Address>() {
            return addr;
        }
        if *sender_address != Address([0; 20]) {
            return *sender_address;
        }
        let hash = hash_bytes(recipient.as_bytes());
        let hash_bytes = hash.as_bytes();
        let mut bytes = [0u8; 20];
        bytes.copy_from_slice(&hash_bytes[..20]);
        Address(bytes)
    }

    /// Score an NFT execution transaction
    fn score_nft_execution(&self, tx: &Transaction) -> f64 {
        if let Some(data) = &tx.data {
            if let Ok(nft) = ComputationalNFT::from_json(data) {
                // Higher scores for NFTs with more complexity and verification
                let verification_bonus = if nft.is_verified { 1.5 } else { 1.0 };
                let complexity_factor = (nft.computation_complexity as f64).min(10.0) / 10.0;

                // Apply region weight if available
                let region_weight = nft
                    .region
                    .as_ref()
                    .and_then(|region| self.region_weights.get(region))
                    .copied()
                    .unwrap_or(1.0);

                // Apply accuracy multiplier if accuracy is available
                let accuracy_factor = nft
                    .accuracy
                    .map(|acc| 1.0 + (acc - 0.5).max(0.0) * self.accuracy_multiplier)
                    .unwrap_or(1.0);

                // Apply reputation multiplier if reputation is available
                let reputation_factor = nft
                    .creator_reputation
                    .map(|rep| 1.0 + (rep - 0.5).max(0.0) * self.reputation_multiplier)
                    .unwrap_or(1.0);

                // Score based on NFT properties
                let nft_score = 10.0
                    * verification_bonus
                    * complexity_factor
                    * region_weight
                    * accuracy_factor
                    * reputation_factor;

                return nft_score;
            }
        }
        0.0
    }

    /// Score a data analysis transaction
    fn score_data_analysis(&self, tx: &Transaction) -> f64 {
        let Some(metrics) = self.extract_metrics(tx) else {
            return 0.0;
        };

        let accuracy = metrics.get("accuracy").unwrap_or(&0.5);
        let complexity = metrics.get("complexity").unwrap_or(&0.5);
        let usefulness = metrics.get("usefulness").unwrap_or(&0.5);
        let region = metrics.get("region").copied();

        // Apply region weight if available
        let region_weight = region
            .and_then(|region_id| {
                let region_name = format!("region_{}", region_id as u32);
                self.region_weights.get(&region_name)
            })
            .copied()
            .unwrap_or(1.0);

        // Apply accuracy multiplier
        let accuracy_factor = 1.0 + (accuracy - 0.5).max(0.0) * self.accuracy_multiplier;

        // Score formula that rewards accuracy and complexity
        8.0 * (accuracy * 0.6 + complexity * 0.2 + usefulness * 0.2)
            * region_weight
            * accuracy_factor
    }

    /// Score a data collection transaction
    fn score_data_collection(&self, tx: &Transaction) -> f64 {
        let Some(metrics) = self.extract_metrics(tx) else {
            return 0.0;
        };

        let quality = metrics.get("quality").unwrap_or(&0.5);
        let volume = metrics.get("volume").unwrap_or(&0.5);
        let uniqueness = metrics.get("uniqueness").unwrap_or(&0.5);
        let region = metrics.get("region").copied();

        // Apply region weight if available
        let region_weight = region
            .and_then(|region_id| {
                let region_name = format!("region_{}", region_id as u32);
                self.region_weights.get(&region_name)
            })
            .copied()
            .unwrap_or(1.0);

        // Score formula that rewards quality and uniqueness
        6.0 * (quality * 0.5 + volume * 0.2 + uniqueness * 0.3) * region_weight
    }

    fn extract_calibration_attestation(
        reveal_payload: &RevealPayload,
    ) -> Option<CalibrationAttestationPayload> {
        reveal_payload
            .data
            .get("calibration_attestation")
            .cloned()
            .and_then(|value| serde_json::from_value::<CalibrationAttestationPayload>(value).ok())
    }

    fn passes_contribution_integrity_checks(
        &self,
        reveal_payload: &RevealPayload,
        block: &Block,
        seen_payload_hashes: &mut HashMap<String, String>,
    ) -> bool {
        let block_ts = block.timestamp.timestamp();
        if let Some(observed_at) =
            Self::data_field_as_f64(&reveal_payload.data, &["observed_at", "observed_timestamp"])
        {
            if (block_ts - observed_at as i64).abs() > 6 * 60 * 60 {
                return false;
            }
        }

        let latitude = Self::data_field_as_f64(&reveal_payload.data, &["latitude", "lat"]);
        let longitude = Self::data_field_as_f64(&reveal_payload.data, &["longitude", "lon", "lng"]);
        if latitude.is_some_and(|value| !(-90.0..=90.0).contains(&value))
            || longitude.is_some_and(|value| !(-180.0..=180.0).contains(&value))
        {
            return false;
        }

        let temperature =
            Self::data_field_as_f64(&reveal_payload.data, &["temperature_c", "temperature"]);
        if temperature.is_some_and(|value| !(-95.0..=65.0).contains(&value)) {
            return false;
        }

        let humidity = Self::data_field_as_f64(&reveal_payload.data, &["humidity"]);
        if humidity.is_some_and(|value| !(0.0..=100.0).contains(&value)) {
            return false;
        }

        let pressure = Self::data_field_as_f64(&reveal_payload.data, &["pressure_hpa", "pressure"]);
        if pressure.is_some_and(|value| !(300.0..=1200.0).contains(&value)) {
            return false;
        }

        let payload_hash = Self::reveal_payload_hash_for_fraud(reveal_payload);
        if let Some(existing_station) = seen_payload_hashes.get(&payload_hash) {
            if existing_station != &reveal_payload.station_id {
                return false;
            }
        } else {
            seen_payload_hashes.insert(payload_hash, reveal_payload.station_id.clone());
        }

        true
    }

    fn reveal_payload_hash_for_fraud(reveal_payload: &RevealPayload) -> String {
        let mut normalized = reveal_payload.data.clone();
        if let Some(object) = normalized.as_object_mut() {
            object.remove("calibration_attestation");
        }
        hash_bytes(
            serde_json::to_string(&normalized)
                .unwrap_or_default()
                .as_bytes(),
        )
        .to_string()
    }

    fn data_field_as_f64(data: &serde_json::Value, keys: &[&str]) -> Option<f64> {
        let object = data.as_object()?;
        keys.iter()
            .find_map(|key| object.get(*key).and_then(|value| value.as_f64()))
    }

    fn calculate_forecast_skill_score(&self, tx: &Transaction) -> Option<f64> {
        let reveal = tx.parse_reveal_payload()?;
        let mut components = Vec::new();

        if let Some(brier) = Self::data_field_as_f64(&reveal.data, &["brier_score", "brier"]) {
            components.push((1.0 - brier.clamp(0.0, 1.0)).clamp(0.0, 1.0));
        } else if let (Some(probability), Some(outcome)) = (
            Self::data_field_as_f64(&reveal.data, &["forecast_probability", "probability"]),
            Self::extract_binary_outcome(&reveal.data, "observed_outcome"),
        ) {
            let brier_score = (probability.clamp(0.0, 1.0) - outcome).powi(2);
            components.push((1.0 - brier_score).clamp(0.0, 1.0));
        }

        if let Some(crps) = Self::data_field_as_f64(&reveal.data, &["crps_score", "crps"]) {
            components.push((1.0 - crps.clamp(0.0, 1.0)).clamp(0.0, 1.0));
        }

        if components.is_empty() {
            return None;
        }

        Some(components.iter().sum::<f64>() / components.len() as f64)
    }

    fn extract_binary_outcome(data: &serde_json::Value, key: &str) -> Option<f64> {
        let value = data.get(key)?;
        if let Some(boolean) = value.as_bool() {
            return Some(if boolean { 1.0 } else { 0.0 });
        }
        value
            .as_f64()
            .map(|numeric| if numeric >= 0.5 { 1.0 } else { 0.0 })
    }

    fn extract_metrics(&self, tx: &Transaction) -> Option<HashMap<String, f64>> {
        if let Some(reveal_payload) = tx.parse_reveal_payload() {
            let reveal_data = serde_json::to_string(&reveal_payload.data).ok()?;
            return self.parse_metrics(&reveal_data).ok();
        }
        if let Some(data) = &tx.data {
            return self.parse_metrics(data).ok();
        }
        None
    }

    /// Score an environmental offset transaction
    fn score_environmental_offset(&self, tx: &Transaction) -> f64 {
        if let Some(data) = &tx.data {
            // Parse data to get offset metrics
            if let Ok(metrics) = self.parse_metrics(data) {
                let amount = metrics.get("amount").unwrap_or(&0.5);
                let verification = metrics.get("verification").unwrap_or(&0.5);
                let impact = metrics.get("impact").unwrap_or(&0.5);

                // Score formula that rewards verified and impactful offsets
                let offset_score = 5.0 * (amount * 0.3 + verification * 0.4 + impact * 0.3);

                return offset_score;
            }
        }
        0.0
    }

    /// Parse metrics from JSON data
    fn parse_metrics(&self, data: &str) -> Result<HashMap<String, f64>> {
        let parsed: serde_json::Value =
            serde_json::from_str(data).context("Failed to parse metrics")?;
        let object = parsed
            .as_object()
            .ok_or_else(|| anyhow!("metrics payload must be a JSON object"))?;
        let mut metrics = HashMap::new();
        for (key, value) in object {
            if let Some(number) = value.as_f64() {
                metrics.insert(key.clone(), number);
            }
        }
        Ok(metrics)
    }

    /// Calculate statistics about roles in the block
    fn calculate_role_statistics(
        &self,
        transactions: &[Transaction],
    ) -> HashMap<PublishingRole, usize> {
        let mut role_counts = HashMap::new();
        role_counts.insert(PublishingRole::Analyzer, 0);
        role_counts.insert(PublishingRole::Collector, 0);
        role_counts.insert(PublishingRole::Keeper, 0);
        role_counts.insert(PublishingRole::Offsetter, 0);

        for tx in transactions {
            match tx.tx_type {
                TransactionType::ForecastReveal => {
                    *role_counts.entry(PublishingRole::Analyzer).or_insert(0) += 1;
                },
                TransactionType::ContributionReveal => {
                    *role_counts.entry(PublishingRole::Collector).or_insert(0) += 1;
                },
                TransactionType::DataStorage => {
                    *role_counts.entry(PublishingRole::Keeper).or_insert(0) += 1;
                },
                TransactionType::EnvironmentalOffset => {
                    *role_counts.entry(PublishingRole::Offsetter).or_insert(0) += 1;
                },
                _ => {},
            }
        }

        role_counts
    }

    /// Apply role-based rewards to the transactions in a block
    fn apply_role_based_rewards(&self, block: &mut Block) {
        // Skip if no transactions
        if block.transactions.is_empty() {
            return;
        }

        // Get coinbase transaction for the total block reward
        let coinbase = &block.transactions[0];
        let total_reward = coinbase.outputs.first().map(|o| o.amount).unwrap_or(0);

        // Count transactions by role (could be used for future dynamic adjustments)
        let _role_stats = self.calculate_role_statistics(&block.transactions);

        // Calculate rewards for each role
        let mut role_rewards = HashMap::new();
        for (role, percentage) in &self.role_reward_percentages {
            let role_reward = (total_reward as f64 * percentage).round() as u64;
            role_rewards.insert(*role, role_reward);

            // Store role reward in block metadata
            let key = format!("reward_{}", role.to_str());
            block.metadata.insert(key, role_reward.to_string());
        }

        // Store additional reward metadata
        block
            .metadata
            .insert("reward_total".to_string(), total_reward.to_string());
    }

    /// Filter transactions by submission timing rules
    fn filter_by_submission_timing(&self, transactions: &mut Vec<Transaction>) {
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Remove transactions that are outside the submission window
        transactions.retain(|tx| {
            // Convert DateTime<Utc> to Unix timestamp (seconds since epoch)
            let tx_time = tx.timestamp.timestamp() as u64;
            let time_diff = current_time.abs_diff(tx_time);

            // Keep transaction if it's within the submission window
            time_diff <= self.submission_time_window
        });
    }

    /// Mine a block by finding a valid nonce (fallback method when analysis is insufficient)
    fn mine_block(&self, block: &mut Block) -> Result<Hash> {
        let target = self.calculate_target(self.difficulty);

        // Start with nonce = 0
        block.nonce = 0;

        let start_time = Instant::now();
        let mut hash_count = 0;

        loop {
            let hash = block.calculate_hash();
            hash_count += 1;

            // Check if hash meets the difficulty target
            if self.meets_target(&hash, &target) {
                let elapsed = start_time.elapsed();
                let hash_rate = if elapsed.as_secs() > 0 {
                    hash_count / elapsed.as_secs()
                } else {
                    hash_count
                };

                info!(
                    "Mining successful: {hash_count} hashes in {elapsed:?} ({hash_rate} hashes/sec)"
                );

                return Ok(hash);
            }

            // Log progress occasionally
            if hash_count % 100_000 == 0 {
                debug!("Mining in progress: {hash_count} hashes calculated");
            }

            // Check if we should give up (e.g., new block received from network)
            if hash_count % 10_000 == 0 && self.should_stop_mining() {
                return Err(anyhow!("Mining interrupted"));
            }

            // Increment nonce and try again
            block.nonce += 1;
        }
    }

    /// Adjusts the mining difficulty based on block times
    fn adjust_difficulty(&mut self) {
        const ADJUSTMENT_INTERVAL: u64 = 2016; // Adjust every ~2 weeks (like Bitcoin)

        if self.blocks_since_adjustment >= ADJUSTMENT_INTERVAL {
            if let Some(last_time) = self.last_adjustment_time {
                let now = Instant::now();
                let elapsed = now.duration_since(last_time);
                let elapsed_secs = elapsed.as_secs();

                // Expected time for these blocks at target block time
                let expected_secs = self.target_block_time * ADJUSTMENT_INTERVAL;

                // Calculate the ratio of actual time to expected time
                let ratio = elapsed_secs as f64 / expected_secs as f64;

                // Adjust difficulty
                if ratio < 0.25 {
                    // Too fast - increase difficulty by a maximum of 4x
                    self.difficulty = self.difficulty.saturating_mul(4);
                } else if ratio < 0.5 {
                    // Somewhat fast - increase difficulty by 2x
                    self.difficulty = self.difficulty.saturating_mul(2);
                } else if ratio > 4.0 {
                    // Too slow - decrease difficulty by 4x
                    self.difficulty = self.difficulty.saturating_div(4);
                } else if ratio > 2.0 {
                    // Somewhat slow - decrease difficulty by 2x
                    self.difficulty = self.difficulty.saturating_div(2);
                } else {
                    // Within acceptable range - fine tuning
                    let new_difficulty = (self.difficulty as f64 / ratio).round() as u32;
                    self.difficulty = new_difficulty.max(1); // Never go below 1
                }

                // Also adjust publishing score threshold based on block time
                // If blocks are too fast, increase the minimum score requirement
                if ratio < 0.8 {
                    self.min_publishing_score = (self.min_publishing_score * 1.05).min(9.5);
                } else if ratio > 1.2 {
                    self.min_publishing_score = (self.min_publishing_score * 0.95).max(1.0);
                }

                info!(
                    "Adjusted difficulty to {} and min publishing score to {:.2} (ratio: {:.2})",
                    self.difficulty, self.min_publishing_score, ratio
                );

                // Reset for next adjustment period
                self.last_adjustment_time = Some(now);
                self.blocks_since_adjustment = 0;
            }
        }
    }

    /// Calculates a target hash based on difficulty
    fn calculate_target(&self, difficulty: u32) -> [u8; 32] {
        let mut target = [0xFF; 32]; // Start with max value

        // Increase leading zeros based on difficulty
        let zeros = (difficulty / 8) as usize;
        let remainder = difficulty % 8;

        // Set complete zero bytes
        for i in 0..zeros {
            if i < target.len() {
                target[i] = 0;
            }
        }

        // Set partial zero byte if needed
        if zeros < target.len() && remainder > 0 {
            target[zeros] = 0xFF >> remainder;
        }

        target
    }

    /// Checks if a hash meets the target difficulty
    fn meets_target(&self, hash: &Hash, target: &[u8; 32]) -> bool {
        let hash_bytes = hash.as_bytes();

        for i in 0..32 {
            if hash_bytes[i] < target[i] {
                return true;
            } else if hash_bytes[i] > target[i] {
                return false;
            }
        }

        // If we get here, the hash exactly equals the target
        true
    }

    /// Check if mining should be stopped (e.g., due to new block received)
    fn should_stop_mining(&self) -> bool {
        // This is a placeholder for network integration
        // In a real implementation, this would check if a new block was received
        false
    }

    /// Verifies that a block contains valid publications
    pub async fn verify_block(&self, block: &Block) -> Result<bool> {
        // Get the publishing score from block metadata
        let publishing_score = block
            .metadata
            .get("publishing_score")
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);

        // If the score is below the threshold, fallback to PoW verification
        if publishing_score < self.min_publishing_score {
            debug!(
                "Block publishing score ({publishing_score:.2}) below threshold, verifying with PoW"
            );
            return self.verify_block_pow(block);
        }

        // Verify that the publishing score matches the transactions
        let calculated_score = self.calculate_block_publishing_score(&block.transactions);

        // Allow small tolerance due to potential floating point differences
        const SCORE_TOLERANCE: f64 = 0.001;

        if (calculated_score - publishing_score).abs() > SCORE_TOLERANCE {
            warn!(
                "Block publishing score mismatch: claimed {publishing_score:.4}, calculated {calculated_score:.4}"
            );
            return Ok(false);
        }

        // Apply submission timing rules
        let timestamp_valid = self.verify_submission_timing(block);
        if !timestamp_valid {
            warn!("Block contains transactions outside the submission window");
            return Ok(false);
        }

        // Verify role-based rewards
        let rewards_valid = self.verify_role_based_rewards(block);
        if !rewards_valid {
            warn!("Block contains invalid role-based rewards");
            return Ok(false);
        }

        // All verification passed
        Ok(true)
    }

    /// Verify submission timing of transactions in a block
    fn verify_submission_timing(&self, block: &Block) -> bool {
        // Convert block timestamp to Unix timestamp (seconds since epoch)
        let block_time = block.timestamp.timestamp() as u64;

        for (i, tx) in block.transactions.iter().enumerate() {
            // Skip coinbase transaction
            if i == 0 {
                continue;
            }

            // Convert tx timestamp to Unix timestamp
            let tx_time = tx.timestamp.timestamp() as u64;
            let time_diff = block_time.abs_diff(tx_time);

            // Transaction is too old or from the future
            if time_diff > self.submission_time_window {
                return false;
            }
        }

        true
    }

    /// Verify role-based rewards in a block
    fn verify_role_based_rewards(&self, block: &Block) -> bool {
        // Skip if no transactions
        if block.transactions.is_empty() {
            return true;
        }

        // Get coinbase transaction for the total block reward
        let coinbase = &block.transactions[0];
        let total_reward = coinbase.outputs.first().map(|o| o.amount).unwrap_or(0);

        // Verify role rewards if they're specified in metadata
        for (role, percentage) in &self.role_reward_percentages {
            let key = format!("reward_{}", role.to_str());

            if let Some(reward_str) = block.metadata.get(&key) {
                if let Ok(reward) = reward_str.parse::<u64>() {
                    let expected_reward = (total_reward as f64 * percentage).round() as u64;

                    // Allow small rounding errors
                    if (reward as i64 - expected_reward as i64).abs() > 1 {
                        warn!(
                            "Invalid reward for role {}: expected {}, got {}",
                            role.to_str(),
                            expected_reward,
                            reward
                        );
                        return false;
                    }
                }
            }
        }

        true
    }

    /// Verify a block using proof-of-work (fallback verification method)
    fn verify_block_pow(&self, block: &Block) -> Result<bool> {
        // Check if block hash meets difficulty
        if let Some(hash) = &block.hash {
            let target = self.calculate_target(block.difficulty);
            return Ok(self.meets_target(hash, &target));
        }

        Err(anyhow!("Block has no hash"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockchain::block::Block;
    use crate::blockchain::chain::Blockchain;
    use crate::blockchain::state::StateTransition;
    use crate::blockchain::transaction::{
        CalibrationAttestationPayload, CommitPayload, ForecastArtifactManifest,
        OracleAttestationPayload, RevealPayload, TransactionInput, TransactionOutput,
    };
    use crate::common::types::{Address, Balance};
    use crate::crypto::hash::hash_string;
    use crate::crypto::signature::{PrivateKey, PublicKey};
    use rand::{SeedableRng, rngs::StdRng};

    fn build_tx(tx_type: TransactionType, data: Option<&str>, tag: &str) -> Transaction {
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: hash_string(&format!("input-{tag}")),
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 1,
                address: format!("wrf-{tag}"),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        tx.tx_type = tx_type;
        tx.data = data.map(|v| v.to_string());
        tx.set_id();
        tx
    }

    fn build_commit_and_reveal(
        reveal_type: TransactionType,
        station_id: &str,
        contribution_type: &str,
        timestamp: i64,
        reveal_metrics: serde_json::Value,
        tag: &str,
    ) -> (Transaction, Transaction, String) {
        let nonce = format!("nonce-{tag}");
        let commit_id = Transaction::compute_commit_id(
            &reveal_metrics,
            &nonce,
            station_id,
            timestamp,
            contribution_type,
        );

        let commit_tx_type = match reveal_type {
            TransactionType::ContributionReveal => TransactionType::ContributionCommit,
            TransactionType::ForecastReveal => TransactionType::ForecastCommit,
            _ => panic!("invalid reveal type"),
        };

        let commit_payload = CommitPayload {
            commit_id: commit_id.clone(),
            station_id: station_id.to_string(),
            timestamp,
            contribution_type: contribution_type.to_string(),
            auth_pubkey: None,
            auth_signature: None,
        };

        let manifest = if contribution_type == "forecast" {
            Some(ForecastArtifactManifest {
                schema_version: 1,
                model_id: "test-model".to_string(),
                input_hash: "a".repeat(64),
                output_hash: "b".repeat(64),
                produced_at: timestamp,
                region: "us-east".to_string(),
                horizon_hours: 48,
                input_variables: vec![
                    "humidity".to_string(),
                    "pressure".to_string(),
                    "temperature".to_string(),
                ],
                output_variables: vec!["precipitation".to_string(), "temperature".to_string()],
            })
        } else {
            None
        };

        let reveal_payload = RevealPayload {
            commit_id: commit_id.clone(),
            station_id: station_id.to_string(),
            timestamp,
            contribution_type: contribution_type.to_string(),
            data: reveal_metrics,
            nonce,
            data_cid: None,
            data_hash: None,
            storage_provider: None,
            provenance: None,
            manifest,
            verification_receipt: None,
            compute_receipt: None,
            quality_tier: None,
        };

        let mut commit_tx = build_tx(commit_tx_type, None, &format!("commit-{tag}"));
        commit_tx.data = Some(serde_json::to_string(&commit_payload).unwrap());
        commit_tx.set_id();

        let mut reveal_tx = build_tx(reveal_type, None, &format!("reveal-{tag}"));
        reveal_tx.data = Some(serde_json::to_string(&reveal_payload).unwrap());
        reveal_tx.set_id();

        (commit_tx, reveal_tx, commit_id)
    }

    fn build_attestation_for_oracle(
        commit_id: &str,
        oracle_id: &str,
        approved: bool,
        timestamp: i64,
        signature: Option<String>,
        tag: &str,
    ) -> Transaction {
        let payload = OracleAttestationPayload {
            commit_id: commit_id.to_string(),
            oracle_id: oracle_id.to_string(),
            approved,
            signature,
            confidence: Some(1.0),
            timestamp,
            nonce: Some(format!("nonce-{tag}")),
        };
        let mut tx = build_tx(
            TransactionType::OracleAttestation,
            None,
            &format!("att-{tag}"),
        );
        tx.data = Some(serde_json::to_string(&payload).unwrap());
        tx.set_id();
        tx
    }

    fn build_attestation(commit_id: &str, approved: bool, tag: &str) -> Transaction {
        let seed = 1000
            + tag
                .bytes()
                .fold(0u64, |acc, b| acc.saturating_add(b as u64));
        let (private_key, oracle_id) = oracle_identity(seed);
        signed_attestation_tx(
            commit_id,
            &private_key,
            &oracle_id,
            approved,
            1_700_000_010,
            tag,
        )
    }

    fn addr(byte: u8) -> Address {
        Address([byte; 20])
    }

    fn oracle_identity(seed: u64) -> (PrivateKey, String) {
        let mut rng = StdRng::seed_from_u64(seed);
        let private_key = PrivateKey::generate(&mut rng);
        let oracle_id = PublicKey::from_private_key(&private_key).to_string();
        (private_key, oracle_id)
    }

    fn signed_attestation_tx(
        commit_id: &str,
        private_key: &PrivateKey,
        oracle_id: &str,
        approved: bool,
        timestamp: i64,
        tag: &str,
    ) -> Transaction {
        let message = OracleAttestationPayload {
            commit_id: commit_id.to_string(),
            oracle_id: oracle_id.to_string(),
            approved,
            signature: None,
            confidence: Some(1.0),
            timestamp,
            nonce: Some(format!("nonce-{tag}")),
        }
        .canonical_signing_message();
        let signature = private_key.sign(message.as_bytes()).to_string();
        build_attestation_for_oracle(
            commit_id,
            oracle_id,
            approved,
            timestamp,
            Some(signature),
            tag,
        )
    }

    fn calibration_attestation_value(
        station_id: &str,
        issued_at: i64,
        expires_at: i64,
        tag: &str,
    ) -> serde_json::Value {
        let seed = 9_000
            + tag
                .bytes()
                .fold(0u64, |acc, b| acc.saturating_add(b as u64));
        let (calibrator_key, calibrator_id) = oracle_identity(seed);
        let mut calibration = CalibrationAttestationPayload {
            station_id: station_id.to_string(),
            calibrator_id,
            issued_at,
            expires_at,
            signature: None,
            nonce: Some(format!("cal-{tag}")),
            calibrator_key_epoch: None,
        };
        let message = calibration.canonical_signing_message();
        calibration.signature = Some(calibrator_key.sign(message.as_bytes()).to_string());
        serde_json::to_value(calibration).expect("calibration serialization")
    }

    #[test]
    fn publishing_score_requires_attested_contribution_reveal() {
        let pop = ProofOfPublishing::new(1, 60, 0.0, 10);

        let (contribution_commit, contribution_reveal, commit_id) = build_commit_and_reveal(
            TransactionType::ContributionReveal,
            "station-1",
            "contribution",
            1_700_000_000,
            serde_json::json!({"quality":1.0,"volume":1.0,"uniqueness":1.0}),
            "contrib",
        );
        let attestation_a = build_attestation(&commit_id, true, "contrib-a");
        let attestation_b = build_attestation(&commit_id, true, "contrib-b");

        let score = pop.calculate_block_publishing_score(&[
            contribution_commit,
            contribution_reveal,
            attestation_a,
            attestation_b,
        ]);

        // Contribution reveal score 6.0, two attestations at 2.0 each => average 10/3.
        assert!(
            (score - (10.0 / 3.0)).abs() < 1e-9,
            "expected quorum-attested contribution score average of 10/3, got {score}"
        );
    }

    #[test]
    fn publishing_score_ignores_unattested_contribution_reveal() {
        let pop = ProofOfPublishing::new(1, 60, 0.0, 10);
        let (contribution_commit, contribution_reveal, _commit_id) = build_commit_and_reveal(
            TransactionType::ContributionReveal,
            "station-2",
            "contribution",
            1_700_000_001,
            serde_json::json!({"quality":1.0,"volume":1.0,"uniqueness":1.0}),
            "contrib-no-attest",
        );

        let score =
            pop.calculate_block_publishing_score(&[contribution_commit, contribution_reveal]);
        assert!(
            (score - 0.0).abs() < 1e-9,
            "expected unattested contribution reveal score to be 0, got {score}"
        );
    }

    #[test]
    fn publishing_score_counts_valid_forecast_reveal_without_attestation() {
        let pop = ProofOfPublishing::new(1, 60, 0.0, 10);
        let (forecast_commit, forecast_reveal, _) = build_commit_and_reveal(
            TransactionType::ForecastReveal,
            "model-1",
            "forecast",
            1_700_000_002,
            serde_json::json!({"accuracy":1.0,"complexity":1.0,"usefulness":1.0}),
            "forecast",
        );

        let score = pop.calculate_block_publishing_score(&[forecast_commit, forecast_reveal]);
        assert!(
            (score - 12.0).abs() < 1e-9,
            "expected valid forecast reveal score to be 12.0, got {score}"
        );
    }

    #[test]
    fn assemble_block_transactions_prioritizes_reveals_before_regular() {
        let pop = ProofOfPublishing::new(1, 60, 0.0, 10);

        let coinbase = Transaction::new_coinbase("miner1", 50, 1);
        let contribution_reveal = build_tx(
            TransactionType::ContributionReveal,
            Some(r#"{"quality":0.8,"volume":0.8,"uniqueness":0.8}"#),
            "priority-contrib",
        );
        let forecast_reveal = build_tx(
            TransactionType::ForecastReveal,
            Some(r#"{"accuracy":0.8,"complexity":0.8,"usefulness":0.8}"#),
            "priority-forecast",
        );
        let regular_a = build_tx(TransactionType::Regular, None, "regular-a");
        let regular_b = build_tx(TransactionType::Regular, None, "regular-b");

        let assembled = pop.assemble_block_transactions(
            coinbase,
            vec![contribution_reveal.clone()],
            vec![forecast_reveal.clone()],
            vec![regular_a.clone(), regular_b.clone()],
        );

        assert_eq!(assembled.len(), 5);
        assert_eq!(assembled[0].tx_type, TransactionType::Coinbase);
        assert_eq!(assembled[1].tx_type, TransactionType::ContributionReveal);
        assert_eq!(assembled[2].tx_type, TransactionType::ForecastReveal);
        assert_eq!(assembled[3].tx_type, TransactionType::Regular);
        assert_eq!(assembled[4].tx_type, TransactionType::Regular);

        assert_eq!(assembled[1].id, contribution_reveal.id);
        assert_eq!(assembled[2].id, forecast_reveal.id);
        assert_eq!(assembled[3].id, regular_a.id);
        assert_eq!(assembled[4].id, regular_b.id);
    }

    #[test]
    fn rewards_require_oracle_quorum() {
        let pop = ProofOfPublishing::new(1, 60, 0.0, 10);
        let mut chain = Blockchain::default();
        let (oracle_q1_sk, oracle_q1_id) = oracle_identity(201);
        let (_oracle_q2_sk, oracle_q2_id) = oracle_identity(202);
        chain
            .apply_state_transition(StateTransition::SetOracleConfig {
                oracle_set: vec![oracle_q1_id.clone(), oracle_q2_id],
                oracle_quorum: 2,
            })
            .unwrap();
        let recipient = addr(7);
        chain.state.create_account(recipient, Balance(0));

        let (commit, mut reveal, commit_id) = build_commit_and_reveal(
            TransactionType::ContributionReveal,
            "station-q",
            "contribution",
            1_700_000_010,
            serde_json::json!({"quality":1.0,"volume":1.0,"uniqueness":1.0}),
            "q",
        );
        reveal.sender_address = recipient;
        let att1 = signed_attestation_tx(
            &commit_id,
            &oracle_q1_sk,
            &oracle_q1_id,
            true,
            1_700_000_011,
            "q1",
        );

        let mut block = Block::new(3, chain.tip, vec![commit, reveal, att1], 1, 1);
        pop.apply_contribution_rewards(&mut chain, &mut block)
            .unwrap();

        let account = chain.state.get_account(&recipient).unwrap();
        assert_eq!(account.earned_balance_total, 0);
        assert_eq!(
            block
                .metadata
                .get("rewards_minted_count")
                .map(String::as_str),
            Some("0")
        );
    }

    #[test]
    fn rewards_mint_when_quorum_met() {
        let pop = ProofOfPublishing::new(1, 60, 0.0, 10);
        let mut chain = Blockchain::default();
        let (oracle_r1_sk, oracle_r1_id) = oracle_identity(211);
        let (oracle_r2_sk, oracle_r2_id) = oracle_identity(212);
        chain
            .apply_state_transition(StateTransition::SetOracleConfig {
                oracle_set: vec![oracle_r1_id.clone(), oracle_r2_id.clone()],
                oracle_quorum: 2,
            })
            .unwrap();
        let recipient = addr(8);
        chain.state.create_account(recipient, Balance(0));

        let (commit, mut reveal, commit_id) = build_commit_and_reveal(
            TransactionType::ContributionReveal,
            "station-r",
            "contribution",
            1_700_000_020,
            serde_json::json!({
                "quality":1.0,
                "volume":1.0,
                "uniqueness":1.0,
                "calibration_attestation": calibration_attestation_value(
                    "station-r",
                    1_700_000_000,
                    1_700_003_600,
                    "r-cal"
                )
            }),
            "r",
        );
        reveal.sender_address = recipient;
        let att1 = signed_attestation_tx(
            &commit_id,
            &oracle_r1_sk,
            &oracle_r1_id,
            true,
            1_700_000_021,
            "r1",
        );
        let att2 = signed_attestation_tx(
            &commit_id,
            &oracle_r2_sk,
            &oracle_r2_id,
            true,
            1_700_000_022,
            "r2",
        );

        let mut block = Block::new(3, chain.tip, vec![commit, reveal, att1, att2], 1, 1);
        pop.apply_contribution_rewards(&mut chain, &mut block)
            .unwrap();

        let account = chain.state.get_account(&recipient).unwrap();
        assert!(account.earned_balance_total > Balance(0));
        assert_eq!(
            block
                .metadata
                .get("rewards_minted_count")
                .map(String::as_str),
            Some("1")
        );
    }

    #[test]
    fn rewards_require_calibration_attestation() {
        let pop = ProofOfPublishing::new(1, 60, 0.0, 10);
        let mut chain = Blockchain::default();
        let (oracle_a_sk, oracle_a_id) = oracle_identity(241);
        let (oracle_b_sk, oracle_b_id) = oracle_identity(242);
        chain
            .apply_state_transition(StateTransition::SetOracleConfig {
                oracle_set: vec![oracle_a_id.clone(), oracle_b_id.clone()],
                oracle_quorum: 2,
            })
            .unwrap();
        let recipient = addr(11);
        chain.state.create_account(recipient, Balance(0));

        let (commit, mut reveal, commit_id) = build_commit_and_reveal(
            TransactionType::ContributionReveal,
            "station-cal-required",
            "contribution",
            1_700_000_025,
            serde_json::json!({"quality":1.0,"volume":1.0,"uniqueness":1.0}),
            "cal-required",
        );
        reveal.sender_address = recipient;
        let att1 = signed_attestation_tx(
            &commit_id,
            &oracle_a_sk,
            &oracle_a_id,
            true,
            1_700_000_026,
            "cal-required-a",
        );
        let att2 = signed_attestation_tx(
            &commit_id,
            &oracle_b_sk,
            &oracle_b_id,
            true,
            1_700_000_027,
            "cal-required-b",
        );

        let mut block = Block::new(3, chain.tip, vec![commit, reveal, att1, att2], 1, 1);
        pop.apply_contribution_rewards(&mut chain, &mut block)
            .unwrap();

        let account = chain.state.get_account(&recipient).unwrap();
        assert_eq!(account.earned_balance_total, 0);
        assert_eq!(
            block
                .metadata
                .get("rewards_skipped_calibration_count")
                .map(String::as_str),
            Some("1")
        );
    }

    #[test]
    fn forecast_rewards_use_skill_score() {
        let pop = ProofOfPublishing::new(1, 60, 0.0, 10);
        let mut chain = Blockchain::default();
        let (oracle_sk, oracle_id) = oracle_identity(252);
        chain
            .apply_state_transition(StateTransition::SetOracleConfig {
                oracle_set: vec![oracle_id.clone()],
                oracle_quorum: 1,
            })
            .unwrap();
        let recipient = addr(12);
        chain.state.create_account(recipient, Balance(0));

        let (commit, mut reveal, commit_id) = build_commit_and_reveal(
            TransactionType::ForecastReveal,
            "model-skill-1",
            "forecast",
            1_700_000_050,
            serde_json::json!({
                "forecast_probability": 0.95,
                "observed_outcome": 1.0,
                "crps_score": 0.05
            }),
            "forecast-skill",
        );
        reveal.sender_address = recipient;
        let attestation = signed_attestation_tx(
            &commit_id,
            &oracle_sk,
            &oracle_id,
            true,
            1_700_000_051,
            "forecast-skill-att",
        );

        let mut block = Block::new(3, chain.tip, vec![commit, reveal, attestation], 1, 1);
        pop.apply_contribution_rewards(&mut chain, &mut block)
            .unwrap();

        let account = chain.state.get_account(&recipient).unwrap();
        assert!(account.earned_balance_total > Balance(0));
        assert_eq!(
            block
                .metadata
                .get("rewards_minted_forecast_count")
                .map(String::as_str),
            Some("1")
        );
    }

    #[test]
    fn forecast_rewards_require_oracle_quorum() {
        let pop = ProofOfPublishing::new(1, 60, 0.0, 10);
        let mut chain = Blockchain::default();
        let recipient = addr(15);
        chain.state.create_account(recipient, Balance(0));

        let (commit, mut reveal, _commit_id) = build_commit_and_reveal(
            TransactionType::ForecastReveal,
            "model-skill-2",
            "forecast",
            1_700_000_070,
            serde_json::json!({
                "forecast_probability": 0.9,
                "observed_outcome": 1.0,
                "crps_score": 0.08
            }),
            "forecast-skill-no-attestation",
        );
        reveal.sender_address = recipient;

        let mut block = Block::new(3, chain.tip, vec![commit, reveal], 1, 1);
        pop.apply_contribution_rewards(&mut chain, &mut block)
            .unwrap();

        let account = chain.state.get_account(&recipient).unwrap();
        assert_eq!(account.earned_balance_total, 0);
        assert_eq!(
            block
                .metadata
                .get("rewards_skipped_unattested_forecast_count")
                .map(String::as_str),
            Some("1")
        );
    }

    #[test]
    fn copied_contribution_payloads_trigger_fraud_penalty() {
        let pop = ProofOfPublishing::new(1, 60, 0.0, 20);
        let mut chain = Blockchain::default();
        let (oracle_sk, oracle_id) = oracle_identity(251);
        chain
            .apply_state_transition(StateTransition::SetOracleConfig {
                oracle_set: vec![oracle_id.clone()],
                oracle_quorum: 1,
            })
            .unwrap();
        let recipient_a = addr(13);
        let recipient_b = addr(14);
        chain.state.create_account(recipient_a, Balance(0));
        chain.state.create_account(recipient_b, Balance(0));

        let shared_data = serde_json::json!({
            "quality":1.0,
            "volume":1.0,
            "uniqueness":1.0,
            "latitude": 41.0,
            "longitude": -71.0
        });
        let (commit_a, mut reveal_a, commit_id_a) = build_commit_and_reveal(
            TransactionType::ContributionReveal,
            "station-fraud-a",
            "contribution",
            1_700_000_060,
            serde_json::json!({
                "quality": shared_data["quality"],
                "volume": shared_data["volume"],
                "uniqueness": shared_data["uniqueness"],
                "latitude": shared_data["latitude"],
                "longitude": shared_data["longitude"],
                "calibration_attestation": calibration_attestation_value(
                    "station-fraud-a",
                    1_700_000_000,
                    1_700_003_600,
                    "fraud-cal-a",
                )
            }),
            "fraud-a",
        );
        let (commit_b, mut reveal_b, commit_id_b) = build_commit_and_reveal(
            TransactionType::ContributionReveal,
            "station-fraud-b",
            "contribution",
            1_700_000_061,
            serde_json::json!({
                "quality": shared_data["quality"],
                "volume": shared_data["volume"],
                "uniqueness": shared_data["uniqueness"],
                "latitude": shared_data["latitude"],
                "longitude": shared_data["longitude"],
                "calibration_attestation": calibration_attestation_value(
                    "station-fraud-b",
                    1_700_000_000,
                    1_700_003_600,
                    "fraud-cal-b",
                )
            }),
            "fraud-b",
        );
        reveal_a.sender_address = recipient_a;
        reveal_b.sender_address = recipient_b;

        let att_a = signed_attestation_tx(
            &commit_id_a,
            &oracle_sk,
            &oracle_id,
            true,
            1_700_000_062,
            "fraud-att-a",
        );
        let att_b = signed_attestation_tx(
            &commit_id_b,
            &oracle_sk,
            &oracle_id,
            true,
            1_700_000_063,
            "fraud-att-b",
        );

        let mut block = Block::new(
            3,
            chain.tip,
            vec![commit_a, reveal_a, commit_b, reveal_b, att_a, att_b],
            1,
            1,
        );
        pop.apply_contribution_rewards(&mut chain, &mut block)
            .unwrap();

        let account_a = chain.state.get_account(&recipient_a).unwrap();
        let account_b = chain.state.get_account(&recipient_b).unwrap();
        assert!(account_a.earned_balance_total + account_b.earned_balance_total > Balance(0));
        assert_eq!(
            block
                .metadata
                .get("rewards_skipped_fraud_count")
                .map(String::as_str),
            Some("1")
        );
    }

    #[test]
    fn rewards_ignore_duplicate_attestations_from_same_oracle() {
        let pop = ProofOfPublishing::new(1, 60, 0.0, 10);
        let mut chain = Blockchain::default();
        let (oracle_dup_sk, oracle_dup_id) = oracle_identity(221);
        let (_oracle_other_sk, oracle_other_id) = oracle_identity(222);
        chain
            .apply_state_transition(StateTransition::SetOracleConfig {
                oracle_set: vec![oracle_dup_id.clone(), oracle_other_id],
                oracle_quorum: 2,
            })
            .unwrap();
        let recipient = addr(9);
        chain.state.create_account(recipient, Balance(0));

        let (commit, mut reveal, commit_id) = build_commit_and_reveal(
            TransactionType::ContributionReveal,
            "station-dup",
            "contribution",
            1_700_000_030,
            serde_json::json!({"quality":1.0,"volume":1.0,"uniqueness":1.0}),
            "dup",
        );
        reveal.sender_address = recipient;
        let att1 = signed_attestation_tx(
            &commit_id,
            &oracle_dup_sk,
            &oracle_dup_id,
            true,
            1_700_000_031,
            "dup-a",
        );
        let att2 = signed_attestation_tx(
            &commit_id,
            &oracle_dup_sk,
            &oracle_dup_id,
            true,
            1_700_000_032,
            "dup-b",
        );

        let mut block = Block::new(3, chain.tip, vec![commit, reveal, att1, att2], 1, 1);
        pop.apply_contribution_rewards(&mut chain, &mut block)
            .unwrap();

        let account = chain.state.get_account(&recipient).unwrap();
        assert_eq!(account.earned_balance_total, 0);
        assert_eq!(
            block
                .metadata
                .get("rewards_minted_count")
                .map(String::as_str),
            Some("0")
        );
    }

    #[test]
    fn rewards_ignore_attestations_with_invalid_signature() {
        let pop = ProofOfPublishing::new(1, 60, 0.0, 10);
        let mut chain = Blockchain::default();
        let (_oracle_bad_sk, oracle_bad_id) = oracle_identity(231);
        let (_oracle_good_sk, oracle_good_id) = oracle_identity(232);
        chain
            .apply_state_transition(StateTransition::SetOracleConfig {
                oracle_set: vec![oracle_bad_id.clone(), oracle_good_id],
                oracle_quorum: 1,
            })
            .unwrap();
        let recipient = addr(10);
        chain.state.create_account(recipient, Balance(0));

        let (commit, mut reveal, commit_id) = build_commit_and_reveal(
            TransactionType::ContributionReveal,
            "station-bad-sig",
            "contribution",
            1_700_000_040,
            serde_json::json!({"quality":1.0,"volume":1.0,"uniqueness":1.0}),
            "bad-sig",
        );
        reveal.sender_address = recipient;

        let invalid_payload = OracleAttestationPayload {
            commit_id,
            oracle_id: oracle_bad_id,
            approved: true,
            signature: Some("definitely-not-a-valid-signature".to_string()),
            confidence: Some(1.0),
            timestamp: 1_700_000_041,
            nonce: None,
        };
        let mut bad_att = build_tx(TransactionType::OracleAttestation, None, "bad-sig-att");
        bad_att.data = Some(serde_json::to_string(&invalid_payload).unwrap());
        bad_att.set_id();

        let mut block = Block::new(3, chain.tip, vec![commit, reveal, bad_att], 1, 1);
        pop.apply_contribution_rewards(&mut chain, &mut block)
            .unwrap();

        let account = chain.state.get_account(&recipient).unwrap();
        assert_eq!(account.earned_balance_total, 0);
        assert_eq!(
            block
                .metadata
                .get("rewards_minted_count")
                .map(String::as_str),
            Some("0")
        );
    }
}
