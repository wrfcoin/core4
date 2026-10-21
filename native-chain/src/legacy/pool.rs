use crate::blockchain::transaction::{Transaction, TransactionType};
use crate::config::settings::Config;
use crate::crypto::hash::Hash;
use anyhow::{Result, anyhow};
use log::debug;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

const MAX_COMMIT_REVEAL_PAYLOAD_BYTES: usize = 64 * 1024;
const MAX_ORACLE_ATTESTATION_PAYLOAD_BYTES: usize = 32 * 1024;

/// Transaction priority for mempool ordering
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TxPriority {
    /// High priority transactions
    High,

    /// Medium priority transactions
    Medium,

    /// Low priority transactions
    Low,
}

/// In-memory pool of unconfirmed transactions
pub struct Mempool {
    /// Unconfirmed transactions by ID
    transactions: HashMap<Hash, Transaction>,

    /// Transaction ordering by fee rate
    tx_by_fee_rate: Vec<(Hash, u64)>, // (txid, fee_rate)

    /// Transaction priority overrides
    priorities: HashMap<Hash, TxPriority>,

    /// Set of transactions that depend on other transactions
    dependencies: HashMap<Hash, HashSet<Hash>>, // txid -> set of txids it depends on

    /// Set of double-spend attempts (transactions spending the same input)
    double_spends: HashMap<(Hash, u32), Vec<Hash>>, // (prev_txid, prev_vout) -> vec of txids spending it

    /// When transactions were added to mempool
    timestamps: HashMap<Hash, Instant>,

    /// Maximum time to keep a transaction in mempool
    max_age: Duration,

    /// Maximum size of the mempool in bytes
    max_size: usize,

    /// Current size of the mempool in bytes
    current_size: usize,

    /// Configuration
    config: Config,

    /// Replay keys for launch-critical protocol payloads.
    protocol_replay_keys: HashSet<String>,
}

impl Mempool {
    /// Creates a new mempool
    pub fn new(config: &Config) -> Self {
        Self {
            transactions: HashMap::new(),
            tx_by_fee_rate: Vec::new(),
            priorities: HashMap::new(),
            dependencies: HashMap::new(),
            double_spends: HashMap::new(),
            timestamps: HashMap::new(),
            max_age: Duration::from_secs(72 * 3600), // 3 days
            max_size: 300_000_000,                   // 300MB
            current_size: 0,
            config: config.clone(),
            protocol_replay_keys: HashSet::new(),
        }
    }

    /// Adds a transaction to the mempool
    pub fn add_transaction(&mut self, tx: Transaction) -> Result<()> {
        // Ensure transaction has an ID
        let tx_id = tx.id.ok_or_else(|| anyhow!("Transaction has no ID"))?;

        // Check if transaction is already in mempool
        if self.transactions.contains_key(&tx_id) {
            return Err(anyhow!("Transaction already in mempool"));
        }

        // Basic validation
        if !tx.validate_structure() {
            return Err(anyhow!("Invalid transaction structure"));
        }

        // Check transaction size
        let tx_size = tx.size();
        if tx_size > self.config.max_transaction_size {
            return Err(anyhow!("Transaction exceeds maximum size"));
        }

        // Commit/reveal payloads are constrained separately from full transaction size.
        if matches!(
            tx.tx_type,
            TransactionType::ContributionCommit
                | TransactionType::ContributionReveal
                | TransactionType::ForecastCommit
                | TransactionType::ForecastReveal
        ) {
            let payload_size = tx.data.as_ref().map(|d| d.len()).unwrap_or(0);
            if payload_size > MAX_COMMIT_REVEAL_PAYLOAD_BYTES {
                return Err(anyhow!(
                    "Commit/reveal payload exceeds {MAX_COMMIT_REVEAL_PAYLOAD_BYTES} bytes"
                ));
            }
        }
        if tx.tx_type == TransactionType::OracleAttestation {
            let payload_size = tx.data.as_ref().map(|d| d.len()).unwrap_or(0);
            if payload_size > MAX_ORACLE_ATTESTATION_PAYLOAD_BYTES {
                return Err(anyhow!(
                    "Oracle attestation payload exceeds {MAX_ORACLE_ATTESTATION_PAYLOAD_BYTES} bytes"
                ));
            }
        }

        let protocol_replay_key = self.validate_launch_payload_and_replay_key(&tx)?;
        if let Some(key) = &protocol_replay_key {
            if self.protocol_replay_keys.contains(key) {
                return Err(anyhow!("Launch protocol replay detected"));
            }
        }

        // Check if adding this transaction would exceed mempool size
        if self.current_size + tx_size > self.max_size {
            // Remove low fee transactions to make room
            self.evict_transactions(tx_size)?;
        }

        // Check for double spends within the mempool
        let mut depends_on = HashSet::new();
        for input in &tx.inputs {
            let outpoint = (input.txid, input.vout);

            // If this input is already spent, it's a double spend
            if let Some(spending_txs) = self.double_spends.get(&outpoint) {
                if !spending_txs.is_empty() {
                    return Err(anyhow!("Double spend detected in mempool"));
                }
            }

            // If the input references a transaction in the mempool, add it as a dependency
            if self.transactions.contains_key(&input.txid) {
                depends_on.insert(input.txid);
            }

            // Track this input as spent
            self.double_spends.entry(outpoint).or_default().push(tx_id);
        }

        // Rough estimate of fee rate (fee/size) - would need full UTXO set to be accurate
        let fee_rate = 1000; // Default placeholder

        // Store transaction
        self.transactions.insert(tx_id, tx);
        self.timestamps.insert(tx_id, Instant::now());
        self.current_size += tx_size;
        if let Some(key) = protocol_replay_key {
            self.protocol_replay_keys.insert(key);
        }

        // Store fee rate for ordering
        self.tx_by_fee_rate.push((tx_id, fee_rate));

        // Sort by fee rate (highest first)
        self.tx_by_fee_rate.sort_by(|a, b| b.1.cmp(&a.1));

        // Store dependencies
        if !depends_on.is_empty() {
            self.dependencies.insert(tx_id, depends_on);
        }

        debug!(
            "Added transaction to mempool: {tx_id} (size: {tx_size} bytes, fee rate: {fee_rate})"
        );

        Ok(())
    }

    /// Removes a transaction from the mempool
    pub fn remove_transaction(&mut self, tx_id: &Hash) -> bool {
        if let Some(tx) = self.transactions.remove(tx_id) {
            // Update mempool size
            self.current_size = self.current_size.saturating_sub(tx.size());

            // Remove from fee rate ordering
            self.tx_by_fee_rate.retain(|(id, _)| id != tx_id);

            // Remove from priorities
            self.priorities.remove(tx_id);

            // Remove timestamp
            self.timestamps.remove(tx_id);

            // Remove outpoints from double spends
            for input in &tx.inputs {
                let outpoint = (input.txid, input.vout);
                if let Some(spending_txs) = self.double_spends.get_mut(&outpoint) {
                    spending_txs.retain(|id| id != tx_id);
                    if spending_txs.is_empty() {
                        self.double_spends.remove(&outpoint);
                    }
                }
            }

            // Remove from dependencies
            self.dependencies.remove(tx_id);

            if let Ok(Some(key)) = self.launch_replay_key(&tx) {
                self.protocol_replay_keys.remove(&key);
            }

            // Remove this transaction from all dependency lists
            for deps in self.dependencies.values_mut() {
                deps.remove(tx_id);
            }

            debug!("Removed transaction from mempool: {tx_id}");
            true
        } else {
            false
        }
    }

    /// Gets a transaction from the mempool by ID
    pub fn get_transaction(&self, tx_id: &Hash) -> Option<Transaction> {
        self.transactions.get(tx_id).cloned()
    }

    /// Gets all transactions in the mempool
    pub fn get_all_transactions(&self) -> Vec<Transaction> {
        self.transactions.values().cloned().collect()
    }

    /// Gets transactions of a specific type, up to the specified limit
    pub fn get_transactions_by_type(
        &self,
        tx_type: TransactionType,
        limit: usize,
    ) -> Vec<Transaction> {
        self.transactions
            .values()
            .filter(|tx| tx.tx_type == tx_type)
            .take(limit)
            .cloned()
            .collect()
    }

    /// Gets all transactions from the mempool with no size limit
    /// This is mainly used by API handlers
    pub fn get_transactions(&self) -> Vec<Transaction> {
        self.get_all_transactions()
    }

    /// Gets transactions for a block, up to the specified size limit
    /// This version is used by the consensus modules
    pub fn get_transactions_with_limit(&self, size_limit: usize) -> Vec<Transaction> {
        let mut result = Vec::new();
        let mut total_size = 0;
        let mut included_ids = HashSet::new();

        // First pass: include transactions in fee rate order, respecting dependencies
        for (tx_id, _) in &self.tx_by_fee_rate {
            if self.can_include_transaction(tx_id, &included_ids) {
                if let Some(tx) = self.transactions.get(tx_id) {
                    let tx_size = tx.size();

                    // Check if adding this would exceed the size limit
                    if total_size + tx_size > size_limit {
                        continue;
                    }

                    result.push(tx.clone());
                    included_ids.insert(*tx_id);
                    total_size += tx_size;
                }
            }
        }

        result
    }

    /// Returns the number of transactions in the mempool
    pub fn size(&self) -> usize {
        self.transactions.len()
    }

    /// Returns the total size of the mempool in bytes
    pub fn total_size(&self) -> usize {
        self.current_size
    }

    /// Returns the count of pending transactions in the mempool
    pub async fn get_pending_transactions_count(&self) -> Result<usize> {
        Ok(self.transactions.len())
    }

    /// Sets the priority of a transaction
    pub fn set_priority(&mut self, tx_id: &Hash, priority: TxPriority) {
        self.priorities.insert(*tx_id, priority);
    }

    /// Cleans up old transactions from the mempool
    pub fn clean_old_transactions(&mut self) -> usize {
        let now = Instant::now();
        let mut to_remove = Vec::new();

        for (tx_id, timestamp) in &self.timestamps {
            if now.duration_since(*timestamp) > self.max_age {
                to_remove.push(*tx_id);
            }
        }

        let count = to_remove.len();
        for tx_id in to_remove {
            self.remove_transaction(&tx_id);
        }

        count
    }

    /// Evicts transactions to make room for a new one of the given size
    fn evict_transactions(&mut self, needed_size: usize) -> Result<()> {
        // If mempool is empty, we're done
        if self.transactions.is_empty() {
            return Ok(());
        }

        // If needed size is greater than max size, we can't fit it
        if needed_size > self.max_size {
            return Err(anyhow!("Transaction too large for mempool"));
        }

        // First, remove expired transactions
        self.clean_old_transactions();

        // If we have enough space now, we're done
        if self.current_size + needed_size <= self.max_size {
            return Ok(());
        }

        // Otherwise, start evicting from lowest fee rate
        let mut to_remove = Vec::new();
        let mut freed_size = 0;

        // Sort by fee rate, lowest first
        let mut tx_by_fee_rate = self.tx_by_fee_rate.clone();
        tx_by_fee_rate.sort_by(|a, b| a.1.cmp(&b.1));

        for (tx_id, _) in tx_by_fee_rate {
            if let Some(tx) = self.transactions.get(&tx_id) {
                // Skip high priority transactions
                if let Some(TxPriority::High) = self.priorities.get(&tx_id) {
                    continue;
                }

                // Mark for removal
                to_remove.push(tx_id);
                freed_size += tx.size();

                // If we've freed enough space, stop
                if freed_size >= needed_size {
                    break;
                }
            }
        }

        // Remove the transactions
        for tx_id in to_remove {
            self.remove_transaction(&tx_id);
        }

        // Check if we've freed enough space
        if self.current_size + needed_size <= self.max_size {
            Ok(())
        } else {
            Err(anyhow!("Unable to make room for transaction"))
        }
    }

    /// Checks if a transaction can be included based on dependencies
    fn can_include_transaction(&self, tx_id: &Hash, included: &HashSet<Hash>) -> bool {
        // Check if it has dependencies
        if let Some(deps) = self.dependencies.get(tx_id) {
            // All dependencies must already be included
            for dep_id in deps {
                if !included.contains(dep_id) {
                    return false;
                }
            }
        }

        true
    }

    fn validate_launch_payload_and_replay_key(&self, tx: &Transaction) -> Result<Option<String>> {
        match tx.tx_type {
            TransactionType::ContributionCommit | TransactionType::ForecastCommit => {
                let payload = tx
                    .parse_commit_payload()
                    .ok_or_else(|| anyhow!("Malformed commit payload"))?;
                if payload.commit_id.trim().is_empty() {
                    return Err(anyhow!("commit_id is required"));
                }
                if payload.station_id.trim().is_empty() {
                    return Err(anyhow!("station_id is required"));
                }
                if payload.contribution_type.trim().is_empty() {
                    return Err(anyhow!("contribution_type is required"));
                }
                Ok(Some(format!("commit:{}", payload.commit_id)))
            },
            TransactionType::ContributionReveal | TransactionType::ForecastReveal => {
                let payload = tx
                    .parse_reveal_payload()
                    .ok_or_else(|| anyhow!("Malformed reveal payload"))?;
                if payload.commit_id.trim().is_empty() {
                    return Err(anyhow!("commit_id is required"));
                }
                if payload.station_id.trim().is_empty() {
                    return Err(anyhow!("station_id is required"));
                }
                if payload.nonce.trim().is_empty() {
                    return Err(anyhow!("nonce is required"));
                }
                if !tx.has_valid_reveal_commit() {
                    return Err(anyhow!("Reveal payload does not match commit hash"));
                }
                Ok(Some(format!("reveal:{}", payload.commit_id)))
            },
            TransactionType::OracleAttestation => {
                let payload = tx
                    .parse_oracle_attestation_payload()
                    .ok_or_else(|| anyhow!("Malformed oracle attestation payload"))?;
                if payload.commit_id.trim().is_empty() {
                    return Err(anyhow!("commit_id is required"));
                }
                if payload.oracle_id.trim().is_empty() {
                    return Err(anyhow!("oracle_id is required"));
                }
                let signature_hex = payload
                    .signature
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .ok_or_else(|| anyhow!("signature is required"))?;
                if signature_hex.is_empty() {
                    return Err(anyhow!("signature is required"));
                }
                if !payload.has_valid_signature() {
                    return Err(anyhow!("oracle attestation signature verification failed"));
                }
                if let Some(confidence) = payload.confidence {
                    if !(0.0..=1.0).contains(&confidence) {
                        return Err(anyhow!("confidence must be between 0 and 1"));
                    }
                }
                Ok(Some(format!(
                    "attestation:{}:{}",
                    payload.commit_id, payload.oracle_id
                )))
            },
            _ => Ok(None),
        }
    }

    fn launch_replay_key(&self, tx: &Transaction) -> Result<Option<String>> {
        match tx.tx_type {
            TransactionType::ContributionCommit | TransactionType::ForecastCommit => tx
                .parse_commit_payload()
                .map(|payload| format!("commit:{}", payload.commit_id))
                .map(Some)
                .ok_or_else(|| anyhow!("Malformed commit payload")),
            TransactionType::ContributionReveal | TransactionType::ForecastReveal => tx
                .parse_reveal_payload()
                .map(|payload| format!("reveal:{}", payload.commit_id))
                .map(Some)
                .ok_or_else(|| anyhow!("Malformed reveal payload")),
            TransactionType::OracleAttestation => tx
                .parse_oracle_attestation_payload()
                .map(|payload| format!("attestation:{}:{}", payload.commit_id, payload.oracle_id))
                .map(Some)
                .ok_or_else(|| anyhow!("Malformed oracle attestation payload")),
            _ => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockchain::transaction::{
        OracleAttestationPayload, TransactionInput, TransactionOutput,
    };
    use crate::crypto::hash::hash_string;
    use crate::crypto::signature::{PrivateKey, PublicKey};
    use rand::{SeedableRng, rngs::StdRng};

    fn build_launch_tx(tx_type: TransactionType, payload: String, tag: &str) -> Transaction {
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
        tx.data = Some(payload);
        tx.set_id();
        tx
    }

    fn build_valid_payload(tx_type: TransactionType, padding_len: usize, tag: &str) -> String {
        match tx_type {
            TransactionType::ContributionCommit | TransactionType::ForecastCommit => {
                let contribution_type = if tx_type == TransactionType::ContributionCommit {
                    "contribution"
                } else {
                    "forecast"
                };
                serde_json::json!({
                    "commit_id": format!("commit-{tag}"),
                    "station_id": format!("station-{}{}", tag, "a".repeat(padding_len)),
                    "timestamp": 1_700_000_000_i64,
                    "contribution_type": contribution_type
                })
                .to_string()
            },
            TransactionType::ContributionReveal | TransactionType::ForecastReveal => {
                let contribution_type = if tx_type == TransactionType::ContributionReveal {
                    "contribution"
                } else {
                    "forecast"
                };
                let station_id = format!("station-{tag}");
                let nonce = format!("nonce-{tag}");
                let data = serde_json::json!({ "blob": "a".repeat(padding_len) });
                let commit_id = Transaction::compute_commit_id(
                    &data,
                    &nonce,
                    &station_id,
                    1_700_000_000_i64,
                    contribution_type,
                );
                serde_json::json!({
                    "commit_id": commit_id,
                    "station_id": station_id,
                    "timestamp": 1_700_000_000_i64,
                    "contribution_type": contribution_type,
                    "data": data,
                    "nonce": nonce
                })
                .to_string()
            },
            TransactionType::OracleAttestation => {
                let mut rng = StdRng::seed_from_u64(
                    10_000
                        + tag
                            .bytes()
                            .fold(0u64, |acc, b| acc.saturating_add(b as u64)),
                );
                let private_key = PrivateKey::generate(&mut rng);
                let oracle_id = PublicKey::from_private_key(&private_key).to_string();
                let commit_id = format!("commit-{tag}");
                let approved = true;
                let timestamp = 1_700_000_000_i64;
                let message = OracleAttestationPayload {
                    commit_id: commit_id.clone(),
                    oracle_id: oracle_id.clone(),
                    approved,
                    signature: None,
                    confidence: Some(0.9),
                    timestamp,
                    nonce: None,
                }
                .canonical_signing_message();
                let signature = private_key.sign(message.as_bytes()).to_string();
                serde_json::json!({
                    "commit_id": commit_id,
                    "oracle_id": oracle_id,
                    "approved": approved,
                    "signature": signature,
                    "confidence": 0.9,
                    "timestamp": timestamp,
                    "padding": "a".repeat(padding_len)
                })
                .to_string()
            },
            _ => unreachable!("invalid launch tx type for test payload"),
        }
    }

    #[test]
    fn allows_valid_launch_payloads_below_limits() {
        let mut mempool = Mempool::new(&Config::default());
        let tx_types = [
            TransactionType::ContributionCommit,
            TransactionType::ContributionReveal,
            TransactionType::ForecastCommit,
            TransactionType::ForecastReveal,
            TransactionType::OracleAttestation,
        ];

        for (idx, tx_type) in tx_types.iter().enumerate() {
            let tx = build_launch_tx(
                *tx_type,
                build_valid_payload(*tx_type, 1024, &format!("limit-{idx}")),
                &format!("limit-{idx}"),
            );
            let result = mempool.add_transaction(tx);
            assert!(
                result.is_ok(),
                "expected valid {tx_type:?} payload to be accepted, got {result:?}"
            );
        }
    }

    #[test]
    fn rejects_launch_payloads_above_limit() {
        let mut mempool = Mempool::new(&Config::default());
        let tx_types = [
            TransactionType::ContributionCommit,
            TransactionType::ContributionReveal,
            TransactionType::ForecastCommit,
            TransactionType::ForecastReveal,
            TransactionType::OracleAttestation,
        ];

        for (idx, tx_type) in tx_types.iter().enumerate() {
            let oversized_payload_len = if *tx_type == TransactionType::OracleAttestation {
                MAX_ORACLE_ATTESTATION_PAYLOAD_BYTES + 1
            } else {
                MAX_COMMIT_REVEAL_PAYLOAD_BYTES + 1
            };
            let tx = build_launch_tx(
                *tx_type,
                "a".repeat(oversized_payload_len),
                &format!("overflow-{idx}"),
            );
            let err = mempool
                .add_transaction(tx)
                .expect_err("payload above limit must be rejected");
            let message = err.to_string();
            assert!(
                message.contains("payload exceeds"),
                "unexpected error message for {tx_type:?}: {message}"
            );
        }
    }

    #[test]
    fn rejects_malformed_launch_payloads() {
        let mut mempool = Mempool::new(&Config::default());
        let tx_types = [
            TransactionType::ContributionCommit,
            TransactionType::ContributionReveal,
            TransactionType::ForecastCommit,
            TransactionType::ForecastReveal,
            TransactionType::OracleAttestation,
        ];

        for (idx, tx_type) in tx_types.iter().enumerate() {
            let tx = build_launch_tx(
                *tx_type,
                "not-json".to_string(),
                &format!("malformed-{idx}"),
            );
            let err = mempool
                .add_transaction(tx)
                .expect_err("malformed payload must be rejected");
            assert!(
                err.to_string().to_lowercase().contains("malformed"),
                "expected malformed payload error for {tx_type:?}, got {err}"
            );
        }
    }

    #[test]
    fn rejects_replay_oracle_attestation_by_commit_and_oracle() {
        let mut mempool = Mempool::new(&Config::default());
        let first = build_launch_tx(
            TransactionType::OracleAttestation,
            build_valid_payload(TransactionType::OracleAttestation, 0, "dup"),
            "dup-1",
        );
        let second = build_launch_tx(
            TransactionType::OracleAttestation,
            build_valid_payload(TransactionType::OracleAttestation, 0, "dup"),
            "dup-2",
        );

        mempool.add_transaction(first).unwrap();
        let err = mempool
            .add_transaction(second)
            .expect_err("duplicate attestation replay key must be rejected");
        assert!(err.to_string().contains("Launch protocol replay detected"));
    }

    #[test]
    fn rejects_oracle_attestation_without_signature() {
        let mut mempool = Mempool::new(&Config::default());
        let mut rng = StdRng::seed_from_u64(99);
        let private_key = PrivateKey::generate(&mut rng);
        let oracle_id = PublicKey::from_private_key(&private_key).to_string();
        let payload = serde_json::json!({
            "commit_id": "commit-missing-sig",
            "oracle_id": oracle_id,
            "approved": true,
            "signature": null,
            "confidence": 0.9,
            "timestamp": 1_700_000_000_i64
        })
        .to_string();

        let tx = build_launch_tx(TransactionType::OracleAttestation, payload, "missing-sig");
        let err = mempool
            .add_transaction(tx)
            .expect_err("unsigned oracle attestation must be rejected");
        assert!(err.to_string().contains("signature is required"));
    }
}
