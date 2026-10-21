use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use log::{info, warn, error};
use serde::{Serialize, Deserialize};
use chrono::Utc;

use crate::blockchain::block::Block;
use crate::blockchain::transaction::{Transaction, TransactionType};
use crate::storage::database::Database;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockchainInfo {
    pub height: u64,
    pub latest_hash: String,
    pub difficulty: u32,
    pub total_transactions: usize,
    pub mining_reward: u64, // in base units (1 WRF = 10^8 units)
    pub average_block_time: f64, // in seconds
    pub network_hashrate: f64,   // in hashes per second
}

pub struct Blockchain {
    pub chain: Vec<Block>,
    pub pending_transactions: Vec<Transaction>,
    pub utxo_set: HashMap<String, Vec<Transaction>>,
    pub utxo_indices: HashMap<String, Vec<usize>>,
    pub mining_reward: u64, // in base units (1 WRF = 10^8 units)
    pub difficulty: u32,
    pub database: Arc<Mutex<Database>>,
    // Lookup indices for O(1) block/transaction retrieval (issue #449)
    block_hash_index: HashMap<String, usize>,   // block hash -> position in self.chain
    block_height_index: HashMap<u64, usize>,     // block index -> position in self.chain
    transaction_index: HashMap<String, usize>,   // tx id -> position in self.chain (block containing tx)
}

impl Blockchain {
    pub fn new(database: Arc<Mutex<Database>>) -> Result<Self, Box<dyn std::error::Error>> {
        let mut blockchain = Blockchain {
            chain: Vec::new(),
            pending_transactions: Vec::new(),
            utxo_set: HashMap::new(),
            utxo_indices: HashMap::new(),
            mining_reward: 50 * 100_000_000, // Initial mining reward: 50 WRF in base units
            difficulty: 4,       // Initial difficulty
            database,
            block_hash_index: HashMap::new(),
            block_height_index: HashMap::new(),
            transaction_index: HashMap::new(),
        };
        
        // Initialize blockchain from database or create genesis block
        blockchain.initialize()?;
        
        Ok(blockchain)
    }
    
    async fn initialize(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Try to load blockchain from database
        let db = self.database.lock().await;
        let blocks = db.get_all_blocks()?;
        
        if blocks.is_empty() {
            // No blocks in database, create genesis block
            let genesis = Block::genesis();
            self.add_block_to_chain(genesis)?;
        } else {
            // Load blocks from database
            self.chain = blocks;

            // Rebuild UTXO set
            self.rebuild_utxo_set()?;

            // Rebuild lookup indices
            self.rebuild_lookup_indices();
        }
        
        Ok(())
    }
    
    fn rebuild_utxo_set(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.utxo_set.clear();
        self.utxo_indices.clear();
        
        // Process each block in the chain
        for block in &self.chain {
            for transaction in &block.transactions {
                // Remove spent outputs
                for input in &transaction.inputs {
                    if let Some(outputs) = self.utxo_set.get_mut(&input.transaction_id) {
                        if input.output_index < outputs.len() {
                            outputs.remove(input.output_index);
                        }
                    }
                    
                    // Update indices
                    if let Some(indices) = self.utxo_indices.get_mut(&input.transaction_id) {
                        indices.retain(|&idx| idx != input.output_index);
                    }
                }
                
                // Add new outputs
                let tx_id = transaction.id.clone();
                self.utxo_set.insert(tx_id.clone(), transaction.outputs.clone());
                
                let indices: Vec<usize> = (0..transaction.outputs.len()).collect();
                self.utxo_indices.insert(tx_id, indices);
            }
        }
        
        Ok(())
    }

    fn rebuild_lookup_indices(&mut self) {
        self.block_hash_index.clear();
        self.block_height_index.clear();
        self.transaction_index.clear();

        for (pos, block) in self.chain.iter().enumerate() {
            self.block_hash_index.insert(block.hash.clone(), pos);
            self.block_height_index.insert(block.index, pos);
            for tx in &block.transactions {
                self.transaction_index.insert(tx.id.clone(), pos);
            }
        }
    }

    pub fn create_block(&mut self) -> Result<Block, Box<dyn std::error::Error>> {
        // Get the latest block
        let latest_block = self.get_latest_block()?;
        
        // Prepare pending transactions
        let mut transactions = Vec::new();
        
        // Add coinbase transaction
        let coinbase = Transaction::coinbase(
            format!("Block reward for height {}", latest_block.index + 1),
            "miner_address".to_string(), // This will be replaced with actual miner address
            self.mining_reward,
        );
        
        transactions.push(coinbase);
        
        // Add pending transactions (up to a limit)
        let max_transactions = 1000; // Arbitrary limit
        let transaction_count = std::cmp::min(self.pending_transactions.len(), max_transactions);
        
        for i in 0..transaction_count {
            transactions.push(self.pending_transactions[i].clone());
        }
        
        // Create new block
        let new_block = Block::new(
            latest_block.index + 1,
            Utc::now(),
            transactions,
            latest_block.hash.clone(),
            self.difficulty,
        );
        
        Ok(new_block)
    }
    
    pub fn add_block(&mut self, mut block: Block) -> Result<(), Box<dyn std::error::Error>> {
        // Verify block
        if !self.is_valid_block(&block)? {
            return Err("Invalid block".into());
        }
        
        // Compute the hash for the block if it's not already set
        if block.hash.is_empty() {
            block.hash = block.calculate_hash();
        }
        
        // Add the block to the chain
        self.add_block_to_chain(block)?;
        
        // Remove processed transactions from pending list
        self.update_pending_transactions();
        
        // Adjust difficulty if needed
        self.adjust_difficulty();
        
        Ok(())
    }
    
    fn add_block_to_chain(&mut self, block: Block) -> Result<(), Box<dyn std::error::Error>> {
        // Update UTXO set
        for transaction in &block.transactions {
            // Remove spent outputs
            for input in &transaction.inputs {
                if let Some(outputs) = self.utxo_set.get_mut(&input.transaction_id) {
                    if input.output_index < outputs.len() {
                        outputs.remove(input.output_index);
                    }
                }
                
                // Update indices
                if let Some(indices) = self.utxo_indices.get_mut(&input.transaction_id) {
                    indices.retain(|&idx| idx != input.output_index);
                }
            }
            
            // Add new outputs
            let tx_id = transaction.id.clone();
            self.utxo_set.insert(tx_id.clone(), transaction.outputs.clone());
            
            let indices: Vec<usize> = (0..transaction.outputs.len()).collect();
            self.utxo_indices.insert(tx_id, indices);
        }
        
        // Add block to chain
        self.chain.push(block.clone());

        // Update lookup indices
        let pos = self.chain.len() - 1;
        let pushed_block = &self.chain[pos];
        self.block_hash_index.insert(pushed_block.hash.clone(), pos);
        self.block_height_index.insert(pushed_block.index, pos);
        for tx in &pushed_block.transactions {
            self.transaction_index.insert(tx.id.clone(), pos);
        }

        // Save block to database
        let db = self.database.lock().await?;
        db.save_block(&block)?;
        
        info!("Added block {} to chain", block.index);
        
        Ok(())
    }
    
    fn update_pending_transactions(&mut self) {
        // Get all transaction IDs from the latest block
        let latest_block = match self.get_latest_block() {
            Ok(block) => block,
            Err(_) => return,
        };
        
        let processed_tx_ids: Vec<String> = latest_block.transactions
            .iter()
            .map(|tx| tx.id.clone())
            .collect();
        
        // Remove processed transactions from pending list
        self.pending_transactions.retain(|tx| !processed_tx_ids.contains(&tx.id));
    }
    
    fn adjust_difficulty(&mut self) {
        // Only adjust difficulty every 2016 blocks (like Bitcoin)
        if self.chain.len() % 2016 == 0 && self.chain.len() >= 2016 {
            let latest_block = match self.get_latest_block() {
                Ok(block) => block,
                Err(_) => return,
            };
            
            let previous_adjustment_block = &self.chain[self.chain.len() - 2016];
            
            // Calculate the time it took to mine 2016 blocks
            let time_diff = (latest_block.timestamp - previous_adjustment_block.timestamp).num_seconds();
            
            // Target time for 2016 blocks is 2 weeks (1,209,600 seconds)
            const TARGET_TIME_SECONDS: i64 = 1_209_600;
            
            // Adjust difficulty
            if time_diff < TARGET_TIME_SECONDS / 4 {
                // Too fast, increase difficulty (max +4)
                self.difficulty += 1.min(4);
            } else if time_diff > TARGET_TIME_SECONDS * 4 {
                // Too slow, decrease difficulty (min 1)
                self.difficulty = (self.difficulty as i32 - 1).max(1) as u32;
            }
            
            info!("Adjusted mining difficulty to {}", self.difficulty);
        }
    }
    
    pub fn add_transaction(&mut self, transaction: Transaction) -> Result<(), Box<dyn std::error::Error>> {
        // Verify transaction
        if !self.is_valid_transaction(&transaction)? {
            return Err("Invalid transaction".into());
        }
        
        // Add to pending transactions
        self.pending_transactions.push(transaction.clone());
        
        info!("Added transaction {} to pending pool", transaction.id);
        
        Ok(())
    }
    
    pub fn get_latest_block(&self) -> Result<Block, Box<dyn std::error::Error>> {
        if self.chain.is_empty() {
            return Err("Blockchain is empty".into());
        }
        
        Ok(self.chain.last().unwrap().clone())
    }
    
    pub fn get_block_by_hash(&self, hash: &str) -> Option<Block> {
        self.block_hash_index.get(hash)
            .and_then(|&pos| self.chain.get(pos))
            .cloned()
    }

    pub fn get_block_by_index(&self, index: u64) -> Option<Block> {
        self.block_height_index.get(&index)
            .and_then(|&pos| self.chain.get(pos))
            .cloned()
    }

    pub fn get_transaction_by_id(&self, id: &str) -> Option<Transaction> {
        // Check indexed blocks first (O(1) block lookup + scan within block)
        if let Some(&block_pos) = self.transaction_index.get(id) {
            if let Some(block) = self.chain.get(block_pos) {
                for tx in &block.transactions {
                    if tx.id == id {
                        return Some(tx.clone());
                    }
                }
            }
        }
        // Fall back to pending transactions (small pool, linear scan is fine)
        self.pending_transactions.iter()
            .find(|tx| tx.id == id)
            .cloned()
    }
    
    fn is_valid_block(&self, block: &Block) -> Result<bool, Box<dyn std::error::Error>> {
        // Get previous block
        let previous_block = if block.index > 0 {
            match self.get_block_by_index(block.index - 1) {
                Some(prev) => prev,
                None => return Err("Previous block not found".into()),
            }
        } else {
            // Genesis block validation
            if block.index != 0 {
                return Err("Invalid block index for genesis block".into());
            }
            
            return Ok(block.is_valid());
        };
        
        // Check block index
        if block.index != previous_block.index + 1 {
            return Ok(false);
        }
        
        // Check previous hash
        if block.previous_hash != previous_block.hash {
            return Ok(false);
        }
        
        // Check block hash meets difficulty requirement
        let target = (1 << (256 - block.difficulty)) - 1;
        let hash_value = u128::from_str_radix(&block.hash[0..32], 16).unwrap_or(u128::MAX);
        
        if hash_value > target.into() {
            return Ok(false);
        }
        
        // Validate transactions
        for transaction in &block.transactions {
            if !self.is_valid_transaction(transaction)? {
                return Ok(false);
            }
        }
        
        // Check coinbase transaction
        if let Some(tx) = block.transactions.first() {
            if !matches!(tx.transaction_type, TransactionType::Coinbase) {
                return Ok(false);
            }
            
            if tx.get_output_amount() > self.mining_reward {
                return Ok(false);
            }
        } else {
            return Ok(false); // Block must have at least one transaction (coinbase)
        }
        
        Ok(block.is_valid())
    }
    
    fn is_valid_transaction(&self, transaction: &Transaction) -> Result<bool, Box<dyn std::error::Error>> {
        // Coinbase transactions are validated differently
        if matches!(transaction.transaction_type, TransactionType::Coinbase) {
            return Ok(transaction.is_valid());
        }
        
        // Check if transaction exists already
        if self.get_transaction_by_id(&transaction.id).is_some() {
            return Ok(false);
        }
        
        // Verify transaction signatures and format
        if !transaction.is_valid() {
            return Ok(false);
        }
        
        // Check if inputs exist and are unspent
        for input in &transaction.inputs {
            let tx_id = &input.transaction_id;
            let output_idx = input.output_index;
            
            // Check if the referenced transaction exists
            if let Some(outputs) = self.utxo_set.get(tx_id) {
                // Check if the referenced output exists and is unspent
                if output_idx >= outputs.len() {
                    return Ok(false);
                }
            } else {
                return Ok(false); // Referenced transaction not found
            }
        }
        
        // Calculate input amount
        let input_amount = transaction.get_input_amount(&self.utxo_set);
        
        // Calculate output amount
        let output_amount = transaction.get_output_amount();
        
        // Check if inputs >= outputs
        if input_amount < output_amount {
            return Ok(false);
        }
        
        Ok(true)
    }
    
    pub fn get_blockchain_info(&self) -> BlockchainInfo {
        let latest_block = match self.get_latest_block() {
            Ok(block) => block,
            Err(_) => return BlockchainInfo {
                height: 0,
                latest_hash: String::new(),
                difficulty: self.difficulty,
                total_transactions: 0,
                mining_reward: self.mining_reward,
                average_block_time: 0.0,
                network_hashrate: 0.0,
            },
        };
        
        // Calculate total transactions
        let total_transactions = self.chain
            .iter()
            .map(|block| block.transactions.len())
            .sum();
        
        // Calculate average block time (if there are enough blocks)
        let average_block_time = if self.chain.len() > 10 {
            let latest_blocks = &self.chain[(self.chain.len() - 10)..];
            let mut sum = 0.0;
            
            for i in 1..latest_blocks.len() {
                let time_diff = (latest_blocks[i].timestamp - latest_blocks[i-1].timestamp).num_seconds();
                sum += time_diff as f64;
            }
            
            sum / (latest_blocks.len() - 1) as f64
        } else {
            0.0
        };
        
        // Estimate network hashrate
        let hashrate = if average_block_time > 0.0 {
            // Each hash has a 1/2^difficulty chance of being valid
            // So on average, 2^difficulty hashes are needed per block
            (2_u64.pow(latest_block.difficulty) as f64) / average_block_time
        } else {
            0.0
        };
        
        BlockchainInfo {
            height: latest_block.index,
            latest_hash: latest_block.hash,
            difficulty: self.difficulty,
            total_transactions,
            mining_reward: self.mining_reward,
            average_block_time,
            network_hashrate: hashrate,
        }
    }
}
