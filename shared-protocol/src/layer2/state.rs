use crate::blockchain::transaction::Transaction;
use crate::common::types::Address;
use crate::crypto::hash::Hash;
use anyhow::Result;
use log::warn;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// Layer 2 account state
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer2Account {
    /// Account address
    pub address: Address,

    /// Account balance
    pub balance: u64,

    /// Account nonce
    pub nonce: u64,

    /// Storage slots (key -> value)
    pub storage: HashMap<String, Vec<u8>>,

    /// Code (for smart contract accounts)
    pub code: Option<Vec<u8>>,

    /// Last update timestamp (unix seconds)
    pub last_update: u64,
}

#[allow(dead_code)]
impl Layer2Account {
    /// Create a new Layer 2 account
    pub fn new(address: Address) -> Self {
        Self {
            address,
            balance: 0,
            nonce: 0,
            storage: HashMap::new(),
            code: None,
            last_update: 0,
        }
    }

    /// Get storage value
    pub fn get_storage(&self, key: &str) -> Option<&Vec<u8>> {
        self.storage.get(key)
    }

    /// Set storage value
    pub fn set_storage(&mut self, key: String, value: Vec<u8>) {
        self.storage.insert(key, value);
    }

    /// Calculate account hash
    pub fn hash(&self) -> Hash {
        let mut hasher = Sha256::new();

        hasher.update(self.address.as_bytes());
        hasher.update(self.balance.to_be_bytes());
        hasher.update(self.nonce.to_be_bytes());

        let mut storage_keys: Vec<String> = self.storage.keys().cloned().collect();
        storage_keys.sort();

        for key in storage_keys {
            hasher.update(key.as_bytes());
            if let Some(value) = self.storage.get(&key) {
                hasher.update(value);
            }
        }

        if let Some(code) = &self.code {
            hasher.update(code);
        }

        let mut h = [0u8; 32];
        h.copy_from_slice(&hasher.finalize());
        Hash::from_bytes(h)
    }
}

/// State proof for verifying state transitions
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateProof {
    /// Proof type
    pub proof_type: StateProofType,

    /// Proof data
    pub proof_data: Vec<u8>,

    /// Merkle proof nodes
    pub merkle_proof: Vec<Hash>,

    /// Timestamp of proof creation (unix seconds)
    pub timestamp: u64,

    /// Verifier signature
    pub verifier_signature: Option<Vec<u8>>,
}

impl Default for StateProof {
    fn default() -> Self {
        Self {
            proof_type: StateProofType::MerkleProof,
            proof_data: Vec::new(),
            merkle_proof: Vec::new(),
            timestamp: 0,
            verifier_signature: None,
        }
    }
}

/// Types of state proofs
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StateProofType {
    /// Merkle proof
    MerkleProof,

    /// Zero-knowledge proof
    ZKProof,

    /// Fraud proof (for optimistic rollups)
    FraudProof,

    /// Validity proof
    ValidityProof,

    /// Data availability proof
    DataAvailabilityProof,
}

/// State transition in Layer 2
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateTransition {
    /// Batch ID for this transition
    pub batch_id: u64,

    /// Previous state root
    pub from_state_root: Hash,

    /// New state root
    pub to_state_root: Hash,

    /// Number of transactions in this transition
    pub transaction_count: u64,

    /// Timestamp of this transition (unix seconds)
    pub timestamp: u64,

    /// Proof of the state transition
    pub proof: StateProof,
}

/// Layer 2 state tree implementation
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer2State {
    /// Accounts by address
    pub accounts: HashMap<Address, Layer2Account>,

    /// Current state root
    pub state_root: Hash,

    /// Block number
    pub block_number: u64,

    /// Last update timestamp (unix seconds)
    pub last_update: u64,
}

#[allow(dead_code)]
impl Layer2State {
    /// Create a new Layer 2 state
    pub fn new() -> Self {
        Self {
            accounts: HashMap::new(),
            state_root: Hash::default(),
            block_number: 0,
            last_update: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }

    /// Get account state
    pub fn get_account(&self, address: &Address) -> Option<&Layer2Account> {
        self.accounts.get(address)
    }

    /// Read the sender's nonce without cloning the full account (#1414).
    ///
    /// Returns `0` for unknown addresses, mirroring the default produced by
    /// `Layer2Account::new` so behavior is identical to the previous
    /// `get_account(...).cloned().unwrap_or_else(|| Layer2Account::new(...))`
    /// pattern when only the scalar nonce is needed.
    pub fn get_nonce(&self, address: &Address) -> u64 {
        self.accounts.get(address).map(|a| a.nonce).unwrap_or(0)
    }

    /// Read the account's balance without cloning the full account (#1414).
    ///
    /// Returns `0` for unknown addresses, mirroring the default produced by
    /// `Layer2Account::new`.
    pub fn get_balance(&self, address: &Address) -> u64 {
        self.accounts.get(address).map(|a| a.balance).unwrap_or(0)
    }

    /// Create or get account state
    pub fn get_or_create_account(&mut self, address: &Address) -> &mut Layer2Account {
        self.accounts
            .entry(*address)
            .or_insert_with(|| Layer2Account::new(*address))
    }

    /// Apply a transaction to the state.
    ///
    /// Validates the sender's nonce and balance via scalar accessors on
    /// `Layer2State` (`get_nonce`, `get_balance`) instead of cloning the
    /// full `Layer2Account` (#1414). The clone is unnecessary because
    /// validation only needs the two scalar fields, and Layer2Account's
    /// `storage: HashMap<String, Vec<u8>>` and optional `code: Vec<u8>`
    /// can be arbitrarily large per account.
    pub async fn apply_transaction(&mut self, transaction: &Transaction) -> Result<()> {
        let sender = transaction.sender_address;
        // #1414: read scalar nonce/balance directly from the accounts map
        // instead of cloning the full Layer2Account (which carries the
        // unbounded `storage: HashMap<String, Vec<u8>>` and optional
        // `code: Vec<u8>`). For unknown senders the accessors return 0,
        // matching the previous `Layer2Account::new(sender)` default.
        let sender_nonce = self.get_nonce(&sender);
        let sender_balance = self.get_balance(&sender);
        let debit = transaction
            .amount
            .checked_add(transaction.fee)
            .ok_or_else(|| anyhow::anyhow!("amount+fee overflow"))?;
        let last_update = transaction.timestamp.timestamp() as u64;

        if transaction.nonce != sender_nonce {
            return Err(anyhow::anyhow!(
                "Invalid nonce: expected {}, got {}",
                sender_nonce,
                transaction.nonce
            ));
        }

        if sender_balance < debit {
            return Err(anyhow::anyhow!(
                "Insufficient balance: {} < {}",
                sender_balance,
                debit
            ));
        }

        // Mirror the existing reserve-before-mutate pattern used for nonce
        // overflow hardening: validate all arithmetic before mutating any
        // account field so rejected transactions leave state unchanged.
        let next_nonce = sender_nonce
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("nonce increment would overflow u64::MAX"))?;
        let sender_new_balance = sender_balance
            .checked_sub(debit)
            .ok_or_else(|| anyhow::anyhow!("balance underflow"))?;
        let recipient_new_balance = transaction
            .recipient_address
            .map(|recipient| {
                let recipient_base_balance = if recipient == sender {
                    sender_new_balance
                } else {
                    self.get_account(&recipient)
                        .map(|account| account.balance)
                        .unwrap_or(0)
                };

                recipient_base_balance
                    .checked_add(transaction.amount)
                    .ok_or_else(|| anyhow::anyhow!("recipient balance overflow"))
                    .map(|balance| (recipient, balance))
            })
            .transpose()?;

        {
            let sender_account = self.get_or_create_account(&sender);
            sender_account.balance = sender_new_balance;
            sender_account.nonce = next_nonce;
            sender_account.last_update = last_update;
        }

        if let Some((recipient, balance)) = recipient_new_balance {
            if recipient == sender {
                let sender_account = self.get_or_create_account(&sender);
                sender_account.balance = balance;
            } else {
                let recipient_account = self.get_or_create_account(&recipient);
                recipient_account.balance = balance;
                recipient_account.last_update = last_update;
            }
        }

        self.update_state_root();

        self.last_update = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Ok(())
    }

    /// Get the current state root
    pub fn get_state_root(&self) -> Hash {
        self.state_root
    }

    /// Update the state root
    fn update_state_root(&mut self) {
        let mut hasher = Sha256::new();

        let mut accounts: Vec<&Address> = self.accounts.keys().collect();
        accounts.sort_by_key(|a| a.0);

        for address in accounts {
            if let Some(account) = self.accounts.get(address) {
                let account_hash = account.hash();
                hasher.update(account_hash.as_bytes());
            }
        }

        let mut new_root = [0u8; 32];
        new_root.copy_from_slice(&hasher.finalize());
        self.state_root = Hash::from_bytes(new_root);
    }

    /// Generate a Merkle proof for an account
    pub fn generate_account_proof(&self, address: &Address) -> Option<StateProof> {
        let account = self.get_account(address)?;
        let account_hash = account.hash();

        Some(StateProof {
            proof_type: StateProofType::MerkleProof,
            proof_data: account_hash.as_bytes().to_vec(),
            merkle_proof: vec![self.state_root],
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            verifier_signature: None,
        })
    }

    /// Verify a state transition
    pub fn verify_state_transition(&self, transition: &StateTransition) -> Result<bool> {
        if transition.from_state_root != self.state_root {
            return Ok(false);
        }

        Ok(true)
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

    #[test]
    fn new_state_starts_empty_with_default_root_and_zero_block() {
        let state = Layer2State::new();
        assert!(state.accounts.is_empty());
        assert_eq!(state.block_number, 0);
        assert_eq!(state.state_root, Hash::default());
    }

    #[test]
    fn verify_state_transition_accepts_matching_from_root() {
        let mut state = Layer2State::new();
        // Add an account so the state root becomes non-default.
        let a = addr(0x01);
        state.get_or_create_account(&a).balance = 100;
        state.update_state_root();
        let current_root = state.get_state_root();

        let transition = StateTransition {
            batch_id: 1,
            from_state_root: current_root,
            to_state_root: Hash::from_bytes([0xffu8; 32]),
            transaction_count: 1,
            timestamp: 0,
            proof: StateProof::default(),
        };

        assert!(state.verify_state_transition(&transition).unwrap());
    }

    #[test]
    fn verify_state_transition_rejects_tampered_from_root() {
        let state = Layer2State::new();
        // default root is Hash::default(); any other value must be rejected
        let wrong_root = Hash::from_bytes([0xabu8; 32]);

        let transition = StateTransition {
            batch_id: 2,
            from_state_root: wrong_root,
            to_state_root: Hash::from_bytes([0xcdu8; 32]),
            transaction_count: 0,
            timestamp: 0,
            proof: StateProof::default(),
        };

        assert!(!state.verify_state_transition(&transition).unwrap());
    }

    #[tokio::test]
    async fn apply_transaction_rejects_amount_fee_overflow_without_mutating_state() {
        let sender = addr(0x11);
        let recipient = addr(0x22);
        let mut state = Layer2State::new();
        state.get_or_create_account(&sender).balance = 500;
        state.get_or_create_account(&recipient).balance = 25;

        let tx = transfer_tx(sender, recipient, u64::MAX, 1, 0);
        let err = state
            .apply_transaction(&tx)
            .await
            .expect_err("amount+fee overflow must be rejected cleanly");

        assert!(
            err.to_string().contains("amount+fee overflow"),
            "expected amount+fee overflow error, got {err:?}"
        );
        assert_eq!(state.get_account(&sender).unwrap().balance, 500);
        assert_eq!(state.get_account(&sender).unwrap().nonce, 0);
        assert_eq!(state.get_account(&recipient).unwrap().balance, 25);
    }

    #[tokio::test]
    async fn apply_transaction_rejects_recipient_balance_overflow_without_mutating_sender() {
        let sender = addr(0x33);
        let recipient = addr(0x44);
        let mut state = Layer2State::new();
        state.get_or_create_account(&sender).balance = u64::MAX;
        state.get_or_create_account(&recipient).balance = 1;

        let tx = transfer_tx(sender, recipient, u64::MAX, 0, 0);
        let err = state
            .apply_transaction(&tx)
            .await
            .expect_err("recipient overflow must be rejected cleanly");

        assert!(
            err.to_string().contains("recipient balance overflow"),
            "expected recipient overflow error, got {err:?}"
        );
        assert_eq!(state.get_account(&sender).unwrap().balance, u64::MAX);
        assert_eq!(state.get_account(&sender).unwrap().nonce, 0);
        assert_eq!(state.get_account(&recipient).unwrap().balance, 1);
    }
}
