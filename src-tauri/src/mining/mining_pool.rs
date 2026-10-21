use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::collections::{HashMap, HashSet};
use tokio::sync::{Mutex, mpsc};
use tokio::time::{Duration, Instant};
use log::{info, warn, error};
use serde::{Serialize, Deserialize};
use std::net::SocketAddr;

use crate::blockchain::block::Block;
use crate::blockchain::chain::Blockchain;
use crate::blockchain::transaction::{Transaction, TransactionType};
use crate::blockchain::nft::{NFT, NFTRegistry};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinerShare {
    pub miner_address: String,
    pub nonce: u64,
    pub timestamp: u64,
    pub difficulty: u32,
    pub share_difficulty: u32,
    pub block_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinerStats {
    pub address: String,
    pub shares: u64,
    pub invalid_shares: u64,
    pub hashrate: f64,
    pub blocks_found: u64,
    pub last_share_time: u64,
    pub rewards: u64, // in base units (1 WRF = 10^8 units)
    pub nft_contributions: HashMap<String, f64>, // NFT ID -> contribution percentage
}

pub struct MiningPool {
    pub running: Arc<AtomicBool>,
    pub miners: HashMap<String, MinerStats>,
    pub current_block: Arc<Mutex<Block>>,
    pub blockchain: Arc<Mutex<Blockchain>>,
    pub pool_address: String,
    pub pool_fee: f64, // percentage
    pub minimum_share_difficulty: u32,
    pub nft_registry: Arc<Mutex<NFTRegistry>>,
    pub computational_nfts: HashMap<String, f64>, // NFT ID -> hashpower contribution
}

impl MiningPool {
    pub fn new(
        blockchain: Arc<Mutex<Blockchain>>,
        pool_address: String,
        nft_registry: Arc<Mutex<NFTRegistry>>,
        pool_fee: f64,
    ) -> Self {
        MiningPool {
            running: Arc::new(AtomicBool::new(false)),
            miners: HashMap::new(),
            current_block: Arc::new(Mutex::new(Block::genesis())), // Placeholder, will be updated
            blockchain,
            pool_address,
            pool_fee,
            minimum_share_difficulty: 1,
            nft_registry,
            computational_nfts: HashMap::new(),
        }
    }
    
    pub async fn start(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.running.store(true, Ordering::SeqCst);
        
        // Initialize with a new block to mine
        self.update_current_block().await?;
        
        info!("Mining pool started");
        
        Ok(())
    }
    
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        info!("Mining pool stopped");
    }
    
    pub async fn update_current_block(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut bc = self.blockchain.lock().await;
        let mut new_block = bc.create_block()?;
        
        // Set coinbase transaction to pool address
        if let Some(coinbase) = new_block.transactions.first_mut() {
            if let Some(output) = coinbase.outputs.first_mut() {
                output.address = self.pool_address.clone();
            }
        }
        
        // Release blockchain lock before acquiring current_block lock to avoid deadlock
        drop(bc);
        
        let mut current = self.current_block.lock().await;
        *current = new_block;
        
        Ok(())
    }
    
    pub async fn process_share(&mut self, share: MinerShare) -> Result<bool, &'static str> {
        // Validate share
        if !self.validate_share(&share).await? {
            // Track invalid share
            if let Some(stats) = self.miners.get_mut(&share.miner_address) {
                stats.invalid_shares += 1;
            }
            return Ok(false);
        }
        
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        // Update miner stats
        let miner_stats = self.miners.entry(share.miner_address.clone())
            .or_insert_with(|| MinerStats {
                address: share.miner_address.clone(),
                shares: 0,
                invalid_shares: 0,
                hashrate: 0.0,
                blocks_found: 0,
                last_share_time: now,
                rewards: 0,
                nft_contributions: HashMap::new(),
            });
        
        miner_stats.shares += 1;
        miner_stats.last_share_time = now;
        
        // Calculate hashrate (simplified)
        // A more sophisticated implementation would use a sliding window
        let share_difficulty_factor = 2_u64.pow(share.share_difficulty) as f64;
        miner_stats.hashrate = share_difficulty_factor / 30.0; // Assuming one share per 30 seconds on average
        
        // Check if the share solves the block
        if self.is_block_solution(&share).await? {
            // Create a copy of the current block with the winning nonce
            let mut block = self.current_block.lock().await.clone();
            block.nonce = share.nonce;
            block.hash = block.calculate_hash();
            
            // Verify block meets chain difficulty
            let target = (1 << (256 - block.difficulty)) - 1;
            let hash_value = u128::from_str_radix(&block.hash[0..32], 16).unwrap_or(u128::MAX);
            
            if hash_value <= target.into() {
                // Submit block to blockchain
                let mut bc = self.blockchain.lock().await;
                if let Err(e) = bc.add_block(block.clone()) {
                    error!("Failed to add block to blockchain: {}", e);
                    return Err("Failed to add block to blockchain");
                }
                
                // Update miner stats
                miner_stats.blocks_found += 1;
                
                // Calculate and distribute rewards
                self.distribute_rewards(block.clone()).await?;
                
                // Create a new block to mine
                drop(bc); // Release blockchain lock
                self.update_current_block().await.map_err(|_| "Failed to update current block")?;
            }
        }
        
        Ok(true)
    }
    
    async fn validate_share(&self, share: &MinerShare) -> Result<bool, &'static str> {
        // Check if share meets minimum difficulty
        if share.share_difficulty < self.minimum_share_difficulty {
            return Ok(false);
        }
        
        // Verify block hash
        let current_block = self.current_block.lock().await;
        
        // Recreate block with provided nonce
        let mut block_copy = current_block.clone();
        block_copy.nonce = share.nonce;
        let hash = block_copy.calculate_hash();
        
        // Check if provided hash matches calculated hash
        if hash != share.block_hash {
            return Ok(false);
        }
        
        // Check if hash meets share difficulty
        let share_target = (1 << (256 - share.share_difficulty)) - 1;
        let hash_value = u128::from_str_radix(&hash[0..32], 16).unwrap_or(u128::MAX);
        
        Ok(hash_value <= share_target.into())
    }
    
    async fn is_block_solution(&self, share: &MinerShare) -> Result<bool, &'static str> {
        // Check if hash meets block difficulty
        let target = (1 << (256 - share.difficulty)) - 1;
        let hash_value = u128::from_str_radix(&share.block_hash[0..32], 16).unwrap_or(u128::MAX);
        
        Ok(hash_value <= target.into())
    }
    
    async fn distribute_rewards(&mut self, block: Block) -> Result<(), &'static str> {
        // Get block reward from coinbase transaction
        let reward = if let Some(coinbase) = block.transactions.first() {
            if let Some(output) = coinbase.outputs.first() {
                output.amount
            } else {
                return Err("Invalid coinbase transaction");
            }
        } else {
            return Err("No coinbase transaction found");
        };
        
        // Calculate total shares
        let total_shares: u64 = self.miners.values().map(|stats| stats.shares).sum();
        
        if total_shares == 0 {
            return Ok(());
        }
        
        // Pool fee (integer arithmetic to avoid rounding)
        let pool_fee_amount = (reward as f64 * (self.pool_fee / 100.0)) as u64;
        let distributable_reward = reward.saturating_sub(pool_fee_amount);

        // Distribute rewards proportionally to shares
        for (_, stats) in self.miners.iter_mut() {
            let miner_reward = (distributable_reward as f64 * stats.shares as f64 / total_shares as f64) as u64;
            stats.rewards = stats.rewards.saturating_add(miner_reward);
        }
        
        // Reset shares for next round
        for (_, stats) in self.miners.iter_mut() {
            stats.shares = 0;
        }
        
        Ok(())
    }
    
    pub async fn register_computational_nft(&mut self, nft_id: String) -> Result<(), &'static str> {
        let nft_registry = self.nft_registry.lock().await;
        
        if let Some(nft) = nft_registry.get_nft(&nft_id) {
            if !nft.is_computational() {
                return Err("Not a computational NFT");
            }
            
            // Calculate hashpower contribution based on NFT attributes
            let compute_power = match nft.metadata.compute_power {
                Some(power) => power,
                None => return Err("NFT has no compute power specified"),
            };
            
            // Add to computational NFTs
            self.computational_nfts.insert(nft_id, compute_power);
            
            Ok(())
        } else {
            Err("NFT not found")
        }
    }
    
    pub async fn associate_miner_with_nft(
        &mut self,
        miner_address: String,
        nft_id: String,
        contribution_percentage: f64,
    ) -> Result<(), &'static str> {
        // Verify NFT exists and is a computational NFT
        if !self.computational_nfts.contains_key(&nft_id) {
            return Err("NFT not registered for mining");
        }
        
        // Add NFT to miner's contributions
        if let Some(stats) = self.miners.get_mut(&miner_address) {
            stats.nft_contributions.insert(nft_id, contribution_percentage);
            Ok(())
        } else {
            // Create new miner stats if they don't exist yet
            let mut nft_contributions = HashMap::new();
            nft_contributions.insert(nft_id, contribution_percentage);
            
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            
            self.miners.insert(miner_address.clone(), MinerStats {
                address: miner_address,
                shares: 0,
                invalid_shares: 0,
                hashrate: 0.0,
                blocks_found: 0,
                last_share_time: now,
                rewards: 0,
                nft_contributions,
            });
            
            Ok(())
        }
    }
    
    pub fn get_miner_stats(&self, miner_address: &str) -> Option<MinerStats> {
        self.miners.get(miner_address).cloned()
    }
    
    pub fn get_pool_stats(&self) -> (u64, f64, u64) {
        // Returns (total miners, total hashrate, total blocks found)
        let total_miners = self.miners.len() as u64;
        let total_hashrate: f64 = self.miners.values().map(|stats| stats.hashrate).sum();
        let total_blocks: u64 = self.miners.values().map(|stats| stats.blocks_found).sum();
        
        (total_miners, total_hashrate, total_blocks)
    }
}
