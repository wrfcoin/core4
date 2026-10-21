use std::collections::HashMap;

use crate::blockchain::transaction::Transaction;
use crate::common::types::Address;
use crate::crypto::hash::Hash;
use crate::layer2::proof::{ZKProof, ZKVerifier, derive_proof_digest};
use crate::layer2::state::{Layer2Account, Layer2State, StateProof, StateTransition};
use anyhow::Result;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

/// Types of Layer 2 rollups
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RollupType {
    /// Optimistic rollup (fraud proofs)
    Optimistic,

    /// Zero-knowledge rollup (validity proofs)
    ZeroKnowledge,

    /// Validium (data availability off-chain)
    Validium,

    /// Volition (hybrid approach)
    Volition,
}

impl std::fmt::Display for RollupType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RollupType::Optimistic => write!(f, "Optimistic Rollup"),
            RollupType::ZeroKnowledge => write!(f, "ZK Rollup"),
            RollupType::Validium => write!(f, "Validium"),
            RollupType::Volition => write!(f, "Volition"),
        }
    }
}

/// Rollup configuration
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollupConfig {
    /// Rollup ID
    pub id: String,

    /// Rollup name
    pub name: String,

    /// Rollup type
    pub rollup_type: RollupType,

    /// Rollup operator address
    pub operator_address: Address,

    /// Batch size (number of transactions per batch)
    pub batch_size: usize,

    /// Maximum number of batches per block
    pub max_batches_per_block: usize,

    /// Challenge period in blocks (for optimistic rollups)
    pub challenge_period_blocks: u64,

    /// L1 gas limit per batch
    pub l1_gas_limit_per_batch: u64,

    /// Verification contract address
    pub verification_contract_address: Option<Address>,

    /// Data availability committee (if using)
    pub data_availability_committee: Option<Vec<Address>>,

    /// Data storage mode
    pub data_storage_mode: DataStorageMode,

    /// Fee token address (if custom token used for fees)
    pub fee_token_address: Option<Address>,
}

impl Default for RollupConfig {
    fn default() -> Self {
        Self {
            id: "wrfcoin-zkrollup".to_string(),
            name: "Wrfcoin ZK Rollup".to_string(),
            rollup_type: RollupType::ZeroKnowledge,
            operator_address: Address([0; 20]),
            batch_size: 100,
            max_batches_per_block: 10,
            challenge_period_blocks: 100,
            l1_gas_limit_per_batch: 1_000_000,
            verification_contract_address: None,
            data_availability_committee: None,
            data_storage_mode: DataStorageMode::OnChain,
            fee_token_address: None,
        }
    }
}

/// Data storage mode for the rollup
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataStorageMode {
    /// Store all data on-chain
    OnChain,

    /// Store transaction data off-chain
    OffChain,

    /// Store only state transitions on-chain
    StateTransitionOnly,

    /// Hybrid approach (configurable per transaction)
    Hybrid,
}

/// Batch status
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BatchStatus {
    /// Batch is being assembled
    Assembling,

    /// Batch is being executed
    Executing,

    /// Batch is being proved
    Proving,

    /// Batch is ready for submission
    ReadyForSubmission,

    /// Batch has been submitted to L1
    Submitted,

    /// Batch has been finalized on L1
    Finalized,

    /// Batch has been challenged (optimistic rollups only)
    Challenged,

    /// Batch has been rejected
    Rejected,
}

/// L1 receipt event observed for a submitted batch.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum L1BatchReceiptEvent {
    /// Batch transaction was included in an L1 block.
    InclusionObserved {
        l1_block_number: u64,
        l1_block_timestamp: u64,
    },
    /// Batch has enough confirmations to be considered finalized.
    FinalityObserved {
        observed_l1_block: u64,
        observed_timestamp: u64,
    },
    /// Batch was challenged in optimistic mode.
    ChallengeObserved { reason: String },
}

/// Canonical L1 receipt payload used to drive rollup state transitions.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L1BatchReceipt {
    pub batch_id: u64,
    pub l1_transaction_hash: Hash,
    pub event: L1BatchReceiptEvent,
}

/// Transaction batch for the rollup
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionBatch {
    /// Batch ID
    pub id: u64,

    /// Transactions in this batch
    pub transactions: Vec<Transaction>,

    /// Batch timestamp (unix seconds)
    pub timestamp: u64,

    /// Previous state root
    pub previous_state_root: Hash,

    /// New state root after applying transactions
    pub new_state_root: Hash,

    /// Zero-knowledge proof (for ZK rollups)
    pub zkproof: Option<ZKProof>,

    /// Batch status
    pub status: BatchStatus,

    /// Batch submitter address
    pub submitter: Address,

    /// L1 transaction hash (once submitted)
    pub l1_transaction_hash: Option<Hash>,

    /// L1 block number (once included)
    pub l1_block_number: Option<u64>,

    /// L1 block timestamp (once included)
    pub l1_block_timestamp: Option<u64>,

    /// L1 block at which this batch became finalized
    pub l1_finality_block_number: Option<u64>,

    /// L1 timestamp when finality was observed
    pub l1_finality_timestamp: Option<u64>,

    /// Batch fees collected
    pub fees_collected: u64,
}

/// Rollup-specific errors that callers may want to inspect.
#[derive(Debug, Error)]
pub enum RollupError {
    #[error("direct rollup funding is only allowed before the first batch starts")]
    FundingAfterBatchStart,
}

/// Layer 2 Rollup
#[allow(dead_code)]
pub struct Rollup {
    /// Rollup configuration
    config: RollupConfig,

    /// Current state
    state: Arc<RwLock<Layer2State>>,

    /// Transaction batches
    batches: Arc<RwLock<Vec<TransactionBatch>>>,

    /// Current batch being assembled
    current_batch: Arc<RwLock<Option<TransactionBatch>>>,

    /// Batch counter
    batch_counter: Arc<RwLock<u64>>,

    /// State transitions
    state_transitions: Arc<RwLock<Vec<StateTransition>>>,

    /// ZK verifier
    zk_verifier: Option<Arc<ZKVerifier>>,

    /// Pending transactions
    pending_transactions: Arc<RwLock<Vec<Transaction>>>,
}

#[allow(dead_code)]
impl Rollup {
    /// Create a new rollup
    pub async fn new(config: RollupConfig) -> Result<Self> {
        info!("Creating {} with ID {}", config.rollup_type, config.id);

        let state = Arc::new(RwLock::new(Layer2State::new()));
        let batches = Arc::new(RwLock::new(Vec::new()));
        let current_batch = Arc::new(RwLock::new(None));
        let batch_counter = Arc::new(RwLock::new(0));
        let state_transitions = Arc::new(RwLock::new(Vec::new()));
        let pending_transactions = Arc::new(RwLock::new(Vec::new()));

        let zk_verifier = if config.rollup_type == RollupType::ZeroKnowledge
            || config.rollup_type == RollupType::Validium
        {
            Some(Arc::new(ZKVerifier::new()))
        } else {
            None
        };

        Ok(Self {
            config,
            state,
            batches,
            current_batch,
            batch_counter,
            state_transitions,
            zk_verifier,
            pending_transactions,
        })
    }

    /// Seed a single account balance before the first batch starts.
    pub async fn fund_account(&self, account: Address, balance: u64) -> Result<()> {
        self.fund_accounts([(account, balance)]).await
    }

    /// Seed multiple account balances before the first batch starts.
    pub async fn fund_accounts<I>(&self, entries: I) -> Result<()>
    where
        I: IntoIterator<Item = (Address, u64)>,
    {
        if *self.batch_counter.read().await > 0 {
            return Err(RollupError::FundingAfterBatchStart.into());
        }

        let mut state = self.state.write().await;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        for (address, balance) in entries {
            let account_state = state
                .accounts
                .entry(address)
                .or_insert_with(|| Layer2Account {
                    address,
                    balance: 0,
                    nonce: 0,
                    storage: HashMap::new(),
                    code: None,
                    last_update: 0,
                });
            account_state.balance = balance;
            account_state.last_update = now;
        }

        let mut hasher = Sha256::new();
        let mut addresses: Vec<&Address> = state.accounts.keys().collect();
        addresses.sort_by_key(|address| address.0);
        for address in addresses {
            if let Some(account) = state.accounts.get(address) {
                hasher.update(account.hash().as_bytes());
            }
        }

        let mut new_root = [0u8; 32];
        new_root.copy_from_slice(&hasher.finalize());
        state.state_root = Hash::from_bytes(new_root);
        state.last_update = now;

        Ok(())
    }

    /// Start a new batch
    pub async fn start_new_batch(&self) -> Result<u64> {
        let mut current_batch_write = self.current_batch.write().await;

        if current_batch_write.is_some() {
            return Err(anyhow::anyhow!("A batch is already in progress"));
        }

        let mut batch_counter = self.batch_counter.write().await;
        let batch_id = *batch_counter;
        *batch_counter += 1;

        let state_root = {
            let state_read = self.state.read().await;
            state_read.get_state_root()
        };

        let new_batch = TransactionBatch {
            id: batch_id,
            transactions: Vec::with_capacity(self.config.batch_size),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            previous_state_root: state_root,
            new_state_root: state_root,
            zkproof: None,
            status: BatchStatus::Assembling,
            submitter: self.config.operator_address,
            l1_transaction_hash: None,
            l1_block_number: None,
            l1_block_timestamp: None,
            l1_finality_block_number: None,
            l1_finality_timestamp: None,
            fees_collected: 0,
        };

        *current_batch_write = Some(new_batch);

        info!("Started new rollup batch {}", batch_id);

        Ok(batch_id)
    }

    /// Add a transaction to the current batch
    pub async fn add_transaction(&self, transaction: Transaction) -> Result<()> {
        let mut current_batch_write = self.current_batch.write().await;

        if let Some(batch) = &mut *current_batch_write {
            if batch.transactions.len() >= self.config.batch_size {
                self.pending_transactions.write().await.push(transaction);
                return Err(anyhow::anyhow!("Current batch is full, added to pending"));
            }

            Self::validate_transaction_for_batch(batch, &transaction)?;

            batch.fees_collected = batch
                .fees_collected
                .checked_add(transaction.fee)
                .ok_or_else(|| anyhow::anyhow!("batch fee total overflow"))?;
            batch.transactions.push(transaction);

            Ok(())
        } else {
            self.pending_transactions.write().await.push(transaction);
            Err(anyhow::anyhow!("No batch in progress, added to pending"))
        }
    }

    fn validate_transaction_for_batch(
        batch: &TransactionBatch,
        transaction: &Transaction,
    ) -> Result<()> {
        if transaction.recipient_address.is_none() {
            return Err(anyhow::anyhow!(
                "Rollup transaction must include recipient_address"
            ));
        }

        if transaction.amount == 0 && transaction.fee == 0 {
            return Err(anyhow::anyhow!(
                "Rollup transaction must transfer value or pay fee"
            ));
        }

        let tx_id = transaction.id.unwrap_or_default();

        if batch
            .transactions
            .iter()
            .any(|existing| existing.id.unwrap_or_default() == tx_id && tx_id != Hash::default())
        {
            return Err(anyhow::anyhow!("Duplicate transaction hash in batch"));
        }

        if batch.transactions.iter().any(|existing| {
            existing.sender_address == transaction.sender_address
                && existing.nonce == transaction.nonce
        }) {
            return Err(anyhow::anyhow!(
                "Duplicate sender/nonce pair detected in batch"
            ));
        }

        if let Some(recipient) = transaction.recipient_address {
            if recipient == transaction.sender_address {
                return Err(anyhow::anyhow!(
                    "Sender and recipient must be different in rollup transaction"
                ));
            }
        }

        Ok(())
    }

    /// Finalize the current batch
    pub async fn finalize_batch(&self) -> Result<u64> {
        let mut current_batch_write = self.current_batch.write().await;

        if let Some(mut batch) = current_batch_write.take() {
            let mut state_write = self.state.write().await;

            for tx in &batch.transactions {
                state_write.apply_transaction(tx).await?;
            }

            let new_state_root = state_write.get_state_root();
            batch.new_state_root = new_state_root;

            let state_transition = StateTransition {
                batch_id: batch.id,
                from_state_root: batch.previous_state_root,
                to_state_root: new_state_root,
                transaction_count: batch.transactions.len() as u64,
                timestamp: batch.timestamp,
                proof: StateProof::default(),
            };

            self.state_transitions.write().await.push(state_transition);

            if self.config.rollup_type == RollupType::ZeroKnowledge {
                batch.status = BatchStatus::Proving;

                let mut proof_material = Vec::new();
                proof_material.extend_from_slice(batch.previous_state_root.as_bytes());
                proof_material.extend_from_slice(batch.new_state_root.as_bytes());
                proof_material.extend_from_slice(&(batch.transactions.len() as u64).to_be_bytes());
                for tx in &batch.transactions {
                    proof_material.extend_from_slice(tx.id.unwrap_or_default().as_bytes());
                }
                let tx_commitment = Sha256::digest(&proof_material);
                let public_inputs = [
                    batch.previous_state_root.as_bytes().as_slice(),
                    batch.new_state_root.as_bytes().as_slice(),
                    tx_commitment.as_slice(),
                ]
                .concat();
                let verification_key = Sha256::digest(format!("{}:vk", self.config.id).as_bytes());
                let proof_digest = derive_proof_digest(&public_inputs, verification_key.as_slice());

                batch.zkproof = Some(ZKProof {
                    proof_data: proof_digest.to_vec(),
                    public_inputs,
                    verification_key: verification_key.to_vec(),
                });

                batch.status = BatchStatus::ReadyForSubmission;
            } else {
                batch.status = BatchStatus::ReadyForSubmission;
            }

            let batch_id = batch.id;
            self.batches.write().await.push(batch);

            info!("Finalized rollup batch {}", batch_id);

            drop(current_batch_write);
            let pending = self.pending_transactions.read().await.clone();
            if !pending.is_empty() {
                info!("Processing {} pending transactions", pending.len());

                if let Ok(_new_batch_id) = self.start_new_batch().await {
                    for tx in pending {
                        let _ = self.add_transaction(tx).await;
                    }
                    self.pending_transactions.write().await.clear();
                }
            }

            Ok(batch_id)
        } else {
            Err(anyhow::anyhow!("No batch in progress"))
        }
    }

    /// Submit a batch to L1
    pub async fn submit_batch(&self, batch_id: u64) -> Result<Hash> {
        let mut batches = self.batches.write().await;

        let batch_index = batches
            .iter()
            .position(|b| b.id == batch_id)
            .ok_or_else(|| anyhow::anyhow!("Batch not found"))?;

        let batch = &mut batches[batch_index];

        if batch.status != BatchStatus::ReadyForSubmission {
            return Err(anyhow::anyhow!(
                "Batch not ready for submission: {:?}",
                batch.status
            ));
        }

        let mut hasher = Sha256::new();
        hasher.update(self.config.id.as_bytes());
        hasher.update(batch.id.to_be_bytes());
        hasher.update(batch.previous_state_root.as_bytes());
        hasher.update(batch.new_state_root.as_bytes());
        for tx in &batch.transactions {
            hasher.update(tx.id.unwrap_or_default().as_bytes());
        }
        let mut h = [0u8; 32];
        h.copy_from_slice(&hasher.finalize());
        let tx_hash = Hash::from_bytes(h);

        batch.status = BatchStatus::Submitted;
        batch.l1_transaction_hash = Some(tx_hash);
        batch.l1_block_number = None;
        batch.l1_block_timestamp = None;
        batch.l1_finality_block_number = None;
        batch.l1_finality_timestamp = None;

        info!(
            "Submitted rollup batch {} to L1, tx hash: {} (awaiting inclusion/finality)",
            batch_id, tx_hash
        );

        Ok(tx_hash)
    }

    /// Ingest an L1 receipt and apply the corresponding batch transition.
    pub async fn ingest_l1_receipt(&self, receipt: L1BatchReceipt) -> Result<()> {
        let batch = self
            .get_batch(receipt.batch_id)
            .await
            .ok_or_else(|| anyhow::anyhow!("Batch not found"))?;

        let expected_hash = batch
            .l1_transaction_hash
            .ok_or_else(|| anyhow::anyhow!("Batch has not been submitted to L1"))?;
        if expected_hash != receipt.l1_transaction_hash {
            return Err(anyhow::anyhow!(
                "L1 receipt hash mismatch for batch {}",
                receipt.batch_id
            ));
        }

        match receipt.event {
            L1BatchReceiptEvent::InclusionObserved {
                l1_block_number,
                l1_block_timestamp,
            } => {
                self.record_l1_inclusion(receipt.batch_id, l1_block_number, l1_block_timestamp)
                    .await
            },
            L1BatchReceiptEvent::FinalityObserved {
                observed_l1_block,
                observed_timestamp,
            } => {
                self.finalize_submitted_batch(
                    receipt.batch_id,
                    observed_l1_block,
                    observed_timestamp,
                )
                .await
            },
            L1BatchReceiptEvent::ChallengeObserved { reason } => {
                self.challenge_submitted_batch(receipt.batch_id, &reason)
                    .await
            },
        }
    }

    /// Record L1 inclusion metadata for a submitted batch.
    pub async fn record_l1_inclusion(
        &self,
        batch_id: u64,
        l1_block_number: u64,
        l1_block_timestamp: u64,
    ) -> Result<()> {
        let mut batches = self.batches.write().await;
        let batch = batches
            .iter_mut()
            .find(|b| b.id == batch_id)
            .ok_or_else(|| anyhow::anyhow!("Batch not found"))?;

        if batch.status != BatchStatus::Submitted {
            return Err(anyhow::anyhow!(
                "Batch must be submitted before inclusion is recorded"
            ));
        }

        batch.l1_block_number = Some(l1_block_number);
        batch.l1_block_timestamp = Some(l1_block_timestamp);
        Ok(())
    }

    /// Mark a submitted batch as challenged.
    pub async fn challenge_submitted_batch(&self, batch_id: u64, reason: &str) -> Result<()> {
        let mut batches = self.batches.write().await;
        let batch = batches
            .iter_mut()
            .find(|b| b.id == batch_id)
            .ok_or_else(|| anyhow::anyhow!("Batch not found"))?;

        if self.config.rollup_type != RollupType::Optimistic {
            return Err(anyhow::anyhow!(
                "Challenge flow is only available for optimistic rollups"
            ));
        }
        if batch.status != BatchStatus::Submitted {
            return Err(anyhow::anyhow!(
                "Only submitted batches can be challenged, current status: {:?}",
                batch.status
            ));
        }

        warn!("Batch {} challenged: {}", batch_id, reason);
        batch.status = BatchStatus::Challenged;
        Ok(())
    }

    /// Finalize a submitted L2 batch once L1 finality conditions are met.
    pub async fn finalize_submitted_batch(
        &self,
        batch_id: u64,
        observed_l1_block: u64,
        observed_timestamp: u64,
    ) -> Result<()> {
        let mut batches = self.batches.write().await;
        let batch = batches
            .iter_mut()
            .find(|b| b.id == batch_id)
            .ok_or_else(|| anyhow::anyhow!("Batch not found"))?;

        if batch.status != BatchStatus::Submitted {
            return Err(anyhow::anyhow!(
                "Batch must be submitted to finalize, current status: {:?}",
                batch.status
            ));
        }

        let inclusion_block = batch
            .l1_block_number
            .ok_or_else(|| anyhow::anyhow!("L1 inclusion block is missing"))?;

        let confirmations_required = match self.config.rollup_type {
            RollupType::Optimistic => self.config.challenge_period_blocks.max(1),
            _ => 5,
        };
        let finality_block = inclusion_block.saturating_add(confirmations_required);
        if observed_l1_block < finality_block {
            return Err(anyhow::anyhow!(
                "Insufficient confirmations: observed L1 block {} but finality requires at least {}",
                observed_l1_block,
                finality_block
            ));
        }

        batch.status = BatchStatus::Finalized;
        batch.l1_finality_block_number = Some(observed_l1_block);
        batch.l1_finality_timestamp = Some(observed_timestamp);
        Ok(())
    }

    /// Get the rollup configuration
    pub fn get_config(&self) -> &RollupConfig {
        &self.config
    }

    /// Get current state
    pub async fn get_state(&self) -> Layer2State {
        self.state.read().await.clone()
    }

    /// Get a batch by ID
    pub async fn get_batch(&self, batch_id: u64) -> Option<TransactionBatch> {
        let batches = self.batches.read().await;
        batches.iter().find(|b| b.id == batch_id).cloned()
    }

    /// Get all batches
    pub async fn get_batches(&self) -> Vec<TransactionBatch> {
        self.batches.read().await.clone()
    }

    /// Get the current batch
    pub async fn get_current_batch(&self) -> Option<TransactionBatch> {
        self.current_batch.read().await.clone()
    }

    /// Get pending transactions
    pub async fn get_pending_transactions(&self) -> Vec<Transaction> {
        self.pending_transactions.read().await.clone()
    }

    /// Get batch count
    pub async fn get_batch_count(&self) -> usize {
        self.batches.read().await.len()
    }

    /// Get transaction count
    pub async fn get_transaction_count(&self) -> usize {
        let batches = self.batches.read().await;
        // Safety: this sums resident Vec lengths for metrics only; each term is
        // bounded by `batch_size` and does not aggregate attacker-controlled
        // balances or fees.
        batches.iter().map(|b| b.transactions.len()).sum()
    }

    /// Verify a batch
    pub async fn verify_batch(&self, batch_id: u64) -> Result<bool> {
        let batches = self.batches.read().await;

        let batch = batches
            .iter()
            .find(|b| b.id == batch_id)
            .ok_or_else(|| anyhow::anyhow!("Batch not found"))?;

        if self.config.rollup_type == RollupType::ZeroKnowledge {
            if let Some(zkproof) = &batch.zkproof {
                if let Some(verifier) = &self.zk_verifier {
                    let result = verifier
                        .verify(zkproof, batch.previous_state_root, batch.new_state_root)
                        .await?;

                    return Ok(result);
                }
            }

            return Err(anyhow::anyhow!("No ZK proof or verifier available"));
        } else {
            let transitions = self.state_transitions.read().await;
            let transition = transitions
                .iter()
                .find(|t| t.batch_id == batch_id)
                .ok_or_else(|| anyhow::anyhow!("No state transition recorded for batch"))?;
            let roots_match = transition.from_state_root == batch.previous_state_root
                && transition.to_state_root == batch.new_state_root;
            let tx_count_matches =
                transition.transaction_count as usize == batch.transactions.len();
            return Ok(roots_match && tx_count_matches);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockchain::transaction::Transaction;

    fn addr(byte: u8) -> Address {
        Address([byte; 20])
    }

    fn transfer_tx(
        sender: Address,
        recipient: Address,
        amount: u64,
        fee: u64,
        nonce: u64,
    ) -> Transaction {
        let mut tx = Transaction::new(vec![], vec![]);
        tx.sender_address = sender;
        tx.recipient_address = Some(recipient);
        tx.amount = amount;
        tx.fee = fee;
        tx.nonce = nonce;
        tx.set_id();
        tx
    }

    #[tokio::test]
    async fn submitted_batch_requires_inclusion_and_finality() {
        let rollup = Rollup::new(RollupConfig::default())
            .await
            .expect("rollup init");

        let batch_id = rollup.start_new_batch().await.expect("start batch");
        rollup.finalize_batch().await.expect("finalize batch");
        rollup.submit_batch(batch_id).await.expect("submit batch");

        assert!(
            rollup
                .finalize_submitted_batch(batch_id, 100, 1_000_000)
                .await
                .is_err()
        );

        rollup
            .record_l1_inclusion(batch_id, 100, 1_000_000)
            .await
            .expect("record inclusion");

        assert!(
            rollup
                .finalize_submitted_batch(batch_id, 104, 1_000_010)
                .await
                .is_err()
        );
        rollup
            .finalize_submitted_batch(batch_id, 105, 1_000_020)
            .await
            .expect("finalize batch");

        let batch = rollup.get_batch(batch_id).await.expect("batch exists");
        assert_eq!(batch.status, BatchStatus::Finalized);
        assert_eq!(batch.l1_finality_block_number, Some(105));
    }

    #[tokio::test]
    async fn optimistic_batch_can_be_challenged() {
        let config = RollupConfig {
            rollup_type: RollupType::Optimistic,
            challenge_period_blocks: 10,
            ..RollupConfig::default()
        };
        let rollup = Rollup::new(config).await.expect("rollup init");

        let batch_id = rollup.start_new_batch().await.expect("start batch");
        rollup.finalize_batch().await.expect("finalize batch");
        rollup.submit_batch(batch_id).await.expect("submit batch");
        rollup
            .record_l1_inclusion(batch_id, 200, 2_000_000)
            .await
            .expect("record inclusion");

        rollup
            .challenge_submitted_batch(batch_id, "fraud proof pending")
            .await
            .expect("challenge");
        let batch = rollup.get_batch(batch_id).await.expect("batch exists");
        assert_eq!(batch.status, BatchStatus::Challenged);

        assert!(
            rollup
                .finalize_submitted_batch(batch_id, 220, 2_000_100)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn l1_receipt_ingestion_drives_finality_flow() {
        let rollup = Rollup::new(RollupConfig::default())
            .await
            .expect("rollup init");

        let batch_id = rollup.start_new_batch().await.expect("start batch");
        rollup.finalize_batch().await.expect("finalize batch");
        let l1_hash = rollup.submit_batch(batch_id).await.expect("submit batch");

        rollup
            .ingest_l1_receipt(L1BatchReceipt {
                batch_id,
                l1_transaction_hash: l1_hash,
                event: L1BatchReceiptEvent::InclusionObserved {
                    l1_block_number: 100,
                    l1_block_timestamp: 1_000_000,
                },
            })
            .await
            .expect("ingest inclusion");

        rollup
            .ingest_l1_receipt(L1BatchReceipt {
                batch_id,
                l1_transaction_hash: l1_hash,
                event: L1BatchReceiptEvent::FinalityObserved {
                    observed_l1_block: 105,
                    observed_timestamp: 1_000_050,
                },
            })
            .await
            .expect("ingest finality");

        let batch = rollup.get_batch(batch_id).await.expect("batch exists");
        assert_eq!(batch.status, BatchStatus::Finalized);
        assert_eq!(batch.l1_finality_block_number, Some(105));
    }

    #[tokio::test]
    async fn l1_receipt_ingestion_rejects_tx_hash_mismatch() {
        let rollup = Rollup::new(RollupConfig::default())
            .await
            .expect("rollup init");

        let batch_id = rollup.start_new_batch().await.expect("start batch");
        rollup.finalize_batch().await.expect("finalize batch");
        let _submitted_hash = rollup.submit_batch(batch_id).await.expect("submit batch");
        let wrong_hash = Hash::from_bytes([9u8; 32]);

        assert!(
            rollup
                .ingest_l1_receipt(L1BatchReceipt {
                    batch_id,
                    l1_transaction_hash: wrong_hash,
                    event: L1BatchReceiptEvent::InclusionObserved {
                        l1_block_number: 100,
                        l1_block_timestamp: 1_000_000,
                    },
                })
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn add_transaction_rejects_fee_total_overflow_without_panicking() {
        let rollup = Rollup::new(RollupConfig::default())
            .await
            .expect("rollup init");
        let alice = addr(0xa1);
        let bob = addr(0xb2);

        rollup.start_new_batch().await.expect("start batch");
        rollup
            .add_transaction(transfer_tx(alice, bob, 1, u64::MAX, 0))
            .await
            .expect("first fee should fit");

        let err = rollup
            .add_transaction(transfer_tx(alice, addr(0xc3), 1, 1, 1))
            .await
            .expect_err("overflowing fee totals must be rejected cleanly");

        assert!(
            err.to_string().contains("batch fee total overflow"),
            "expected batch fee total overflow error, got {err:?}"
        );
    }
}
