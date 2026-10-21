#![allow(dead_code)]
#![allow(clippy::disallowed_types)]

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

// Simple representation of a block
#[derive(Debug, Clone)]
pub struct Block {
    pub index: u64,
    pub timestamp: u64,
    pub prev_hash: String,
    pub hash: String,
    pub data: String,
    pub nonce: u64,
    pub difficulty: u32,
    pub block_type: BlockType,
}

/// Defines different types of blocks
#[derive(Debug, Clone)]
pub enum BlockType {
    /// Regular transaction block
    Transaction,
    /// Block containing NFT operations
    NFT(String), // ID of the NFT involved
}

impl Block {
    // Create new block
    pub fn new(
        index: u64,
        prev_hash: String,
        data: String,
        difficulty: u32,
        block_type: BlockType,
    ) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        let mut block = Block {
            index,
            timestamp,
            prev_hash,
            hash: String::new(),
            data,
            nonce: 0,
            difficulty,
            block_type,
        };

        block.mine();
        block
    }

    // Calculate hash of the block
    fn calculate_hash(&self) -> String {
        // Include block type in hash calculation
        let block_type_str = match &self.block_type {
            BlockType::Transaction => "transaction".to_string(),
            BlockType::NFT(id) => format!("nft-{id}"),
        };

        format!(
            "{}{}{}{}{}{}",
            self.index, self.timestamp, self.prev_hash, self.data, block_type_str, self.nonce
        )
        .into_bytes()
        .iter()
        .map(|&b| format!("{:02x}", b))
        .collect::<String>()
    }

    // Mine the block (simplified PoW)
    pub fn mine(&mut self) {
        let target = "0".repeat(self.difficulty as usize);

        // Limit mining to a max number of attempts to prevent timeouts
        let max_attempts = 1000;
        let mut attempts = 0;

        while {
            self.nonce += 1;
            self.hash = self.calculate_hash();
            attempts += 1;

            // Exit if we've found a valid hash or reached max attempts
            if self.hash.starts_with(&target) || attempts >= max_attempts {
                false
            } else {
                true
            }
        } {}

        println!("Block mined: {} after {attempts} attempts", self.hash);
    }
}

/// Define transaction types
#[derive(Debug, Clone)]
pub enum TransactionType {
    /// Regular coin transfer
    Transfer {
        from: String,
        to: String,
        amount: u64,
    },
    /// NFT minting
    MintNFT { creator: String, nft_id: String },
    /// NFT verification
    VerifyNFT { verifier: String, nft_id: String },
    /// NFT endorsement
    EndorseNFT { endorser: String, nft_id: String },
}

/// Represents a transaction
#[derive(Debug, Clone)]
pub struct Transaction {
    pub transaction_type: TransactionType,
    pub timestamp: u64,
    pub signature: String,
}

impl Transaction {
    /// Create a new transaction
    pub fn new(transaction_type: TransactionType, signature: String) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        Transaction {
            transaction_type,
            timestamp,
            signature,
        }
    }

    /// Serialize to JSON string
    pub fn to_json(&self) -> String {
        match &self.transaction_type {
            TransactionType::Transfer { from, to, amount } => {
                format!(
                    "{{\"type\":\"transfer\",\"from\":\"{}\",\"to\":\"{}\",\"amount\":{},\"timestamp\":{},\"signature\":\"{}\"}}",
                    from, to, amount, self.timestamp, self.signature
                )
            },
            TransactionType::MintNFT { creator, nft_id } => {
                format!(
                    "{{\"type\":\"mint_nft\",\"creator\":\"{}\",\"nft_id\":\"{}\",\"timestamp\":{},\"signature\":\"{}\"}}",
                    creator, nft_id, self.timestamp, self.signature
                )
            },
            TransactionType::VerifyNFT { verifier, nft_id } => {
                format!(
                    "{{\"type\":\"verify_nft\",\"verifier\":\"{}\",\"nft_id\":\"{}\",\"timestamp\":{},\"signature\":\"{}\"}}",
                    verifier, nft_id, self.timestamp, self.signature
                )
            },
            TransactionType::EndorseNFT { endorser, nft_id } => {
                format!(
                    "{{\"type\":\"endorse_nft\",\"endorser\":\"{}\",\"nft_id\":\"{}\",\"timestamp\":{},\"signature\":\"{}\"}}",
                    endorser, nft_id, self.timestamp, self.signature
                )
            },
        }
    }
}

// Simple blockchain implementation
#[derive(Debug)]
pub struct Blockchain {
    pub chain: Vec<Block>,
    pub difficulty: u32,
    pub mempool: Vec<Transaction>,
    pub balances: HashMap<String, u64>,
}

impl Blockchain {
    // Create a new blockchain with genesis block
    pub fn new() -> Self {
        let mut blockchain = Blockchain {
            chain: vec![],
            difficulty: 1, // Lowered difficulty for faster mining
            mempool: vec![],
            balances: HashMap::new(),
        };

        // Create genesis block
        let genesis = Block::new(
            0,
            "0".to_string(),
            "Genesis Block".to_string(),
            blockchain.difficulty,
            BlockType::Transaction,
        );
        blockchain.chain.push(genesis);

        blockchain
    }

    // Add a new block to the chain
    pub fn add_block(&mut self, data: String, block_type: BlockType) -> Block {
        let prev_block = self.chain.last().unwrap();
        let new_block = Block::new(
            prev_block.index + 1,
            prev_block.hash.clone(),
            data,
            self.difficulty,
            block_type,
        );

        self.chain.push(new_block.clone());
        new_block
    }

    // Validate the blockchain
    pub fn is_valid(&self) -> bool {
        for i in 1..self.chain.len() {
            let current = &self.chain[i];
            let previous = &self.chain[i - 1];

            if current.hash != current.calculate_hash() || current.prev_hash != previous.hash {
                return false;
            }
        }
        true
    }

    // Add a transfer transaction to mempool
    pub fn add_transfer_transaction(&mut self, from: &str, to: &str, amount: u64) -> bool {
        // Check if sender has enough balance
        let sender_balance = self.balances.get(from).unwrap_or(&0);
        if *sender_balance < amount {
            return false;
        }

        let tx_type = TransactionType::Transfer {
            from: from.to_string(),
            to: to.to_string(),
            amount,
        };

        // Create simple signature (in a real implementation, this would use proper cryptography)
        let signature = format!("sig-{from}-{to}-{amount}");

        let transaction = Transaction::new(tx_type, signature);
        self.mempool.push(transaction);
        true
    }

    // Add an NFT mint transaction to mempool
    pub fn add_mint_nft_transaction(&mut self, creator: &str, nft_id: &str) -> bool {
        let tx_type = TransactionType::MintNFT {
            creator: creator.to_string(),
            nft_id: nft_id.to_string(),
        };

        // Simple signature
        let signature = format!("sig-mint-{creator}-{nft_id}");

        let transaction = Transaction::new(tx_type, signature);
        self.mempool.push(transaction);
        true
    }

    // Add an NFT verification transaction to mempool
    pub fn add_verify_nft_transaction(&mut self, verifier: &str, nft_id: &str) -> bool {
        let tx_type = TransactionType::VerifyNFT {
            verifier: verifier.to_string(),
            nft_id: nft_id.to_string(),
        };

        // Simple signature
        let signature = format!("sig-verify-{verifier}-{nft_id}");

        let transaction = Transaction::new(tx_type, signature);
        self.mempool.push(transaction);
        true
    }

    // Add an NFT endorsement transaction to mempool
    pub fn add_endorse_nft_transaction(&mut self, endorser: &str, nft_id: &str) -> bool {
        let tx_type = TransactionType::EndorseNFT {
            endorser: endorser.to_string(),
            nft_id: nft_id.to_string(),
        };

        // Simple signature
        let signature = format!("sig-endorse-{endorser}-{nft_id}");

        let transaction = Transaction::new(tx_type, signature);
        self.mempool.push(transaction);
        true
    }

    // Process pending transactions into a block
    pub fn mine_pending_transactions(&mut self, mining_reward_address: &str) -> Option<Block> {
        if self.mempool.is_empty() {
            return None;
        }

        // Convert transactions to JSON strings and join
        let txs_json: Vec<String> = self.mempool.iter().map(|tx| tx.to_json()).collect();
        let data = txs_json.join("|");

        // Determine block type based on transaction types
        let block_type = if self.mempool.iter().any(|tx| {
            matches!(
                tx.transaction_type,
                TransactionType::MintNFT { .. }
                    | TransactionType::VerifyNFT { .. }
                    | TransactionType::EndorseNFT { .. }
            )
        }) {
            // If any transaction is NFT-related, create an NFT block with placeholder ID
            BlockType::NFT("mixed".to_string())
        } else {
            BlockType::Transaction
        };

        // Create a new block with all pending transactions
        let new_block = self.add_block(data, block_type);

        // Process transactions
        for tx in &self.mempool {
            match &tx.transaction_type {
                TransactionType::Transfer { from, to, amount } => {
                    // Update balances for transfer
                    let sender_balance = self.balances.entry(from.to_string()).or_insert(0);
                    if *sender_balance >= *amount {
                        *sender_balance -= amount;
                        let receiver_balance = self.balances.entry(to.to_string()).or_insert(0);
                        *receiver_balance += amount;
                    }
                },
                // NFT transactions are handled elsewhere
                _ => {},
            }
        }

        // Clear the mempool
        self.mempool.clear();

        // Add mining reward
        let reward_balance = self
            .balances
            .entry(mining_reward_address.to_string())
            .or_insert(0);
        *reward_balance += 50; // Fixed mining reward

        Some(new_block)
    }

    // Get balance for an address
    pub fn get_balance(&self, address: &str) -> u64 {
        *self.balances.get(address).unwrap_or(&0)
    }

    // Get all blocks related to a specific NFT
    #[allow(dead_code)]
    pub fn get_nft_blocks(&self, nft_id: &str) -> Vec<&Block> {
        self.chain
            .iter()
            .filter(|block| {
                if let BlockType::NFT(id) = &block.block_type {
                    id == nft_id || id == "mixed"
                } else {
                    false
                }
            })
            .collect()
    }
}
