use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use log::{info, warn, error};
use rayon::prelude::*;
use serde::{Serialize, Deserialize};

use crate::blockchain::block::Block;
use crate::blockchain::chain::Blockchain;
use crate::blockchain::transaction::Transaction;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuMiningStats {
    pub is_mining: bool,
    pub cores_used: usize,
    pub hashrate: f64,       // hashes per second
    pub blocks_mined: u64,
    pub time_mining: u64,    // seconds
    pub last_block_time: Option<u64>, // seconds
    pub rewards_earned: u64, // in base units (1 WRF = 10^8 units)
}

pub struct CpuMiner {
    pub stats: CpuMiningStats,
    pub is_mining: Arc<AtomicBool>,
    pub miner_address: String,
    pub threads: usize,
}

impl CpuMiner {
    pub fn new(miner_address: String, threads: Option<usize>) -> Self {
        let num_threads = threads.unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|p| p.get())
                .unwrap_or(1)
        });
        
        CpuMiner {
            stats: CpuMiningStats {
                is_mining: false,
                cores_used: num_threads,
                hashrate: 0.0,
                blocks_mined: 0,
                time_mining: 0,
                last_block_time: None,
                rewards_earned: 0,
            },
            is_mining: Arc::new(AtomicBool::new(false)),
            miner_address,
            threads: num_threads,
        }
    }
    
    pub fn set_threads(&mut self, threads: usize) {
        self.threads = threads;
        self.stats.cores_used = threads;
    }
    
    pub fn start(&mut self) {
        self.is_mining.store(true, Ordering::SeqCst);
        self.stats.is_mining = true;
    }
    
    pub fn stop(&mut self) {
        self.is_mining.store(false, Ordering::SeqCst);
        self.stats.is_mining = false;
    }
    
    pub async fn mine_block(
        &mut self,
        blockchain: Arc<Mutex<Blockchain>>,
    ) -> Result<Block, Box<dyn std::error::Error>> {
        let start_time = Instant::now();
        
        // Get a block to mine
        let mut block = {
            let mut bc = blockchain.lock().await;
            let mut block = bc.create_block()?;
            
            // Update coinbase transaction with miner's address
            if let Some(coinbase) = block.transactions.first_mut() {
                if let Some(output) = coinbase.outputs.first_mut() {
                    output.address = self.miner_address.clone();
                }
            }
            
            block
        };
        
        // Mine the block
        let is_mining = self.is_mining.clone();
        let mining_result = tokio::task::spawn_blocking(move || {
            let target = (1 << (256 - block.difficulty)) - 1;
            let mut start_nonce = 0;
            let mut hashrate_counter = 0;
            let mut last_hashrate_update = Instant::now();
            let mut hashrate = 0.0;
            
            // Find a valid hash
            while is_mining.load(Ordering::SeqCst) {
                let result = (start_nonce..start_nonce + 100_000)
                    .into_par_iter()
                    .find_any(|&nonce| {
                        if !is_mining.load(Ordering::SeqCst) {
                            return false;
                        }
                        
                        block.nonce = nonce;
                        let hash = block.calculate_hash();
                        
                        // Check if hash meets target
                        let hash_value = u128::from_str_radix(&hash[0..32], 16).unwrap_or(u128::MAX);
                        hash_value <= target.into()
                    });
                
                start_nonce += 100_000;
                hashrate_counter += 100_000;
                
                // Update hashrate every second
                let now = Instant::now();
                if now.duration_since(last_hashrate_update).as_secs() >= 1 {
                    let elapsed = now.duration_since(last_hashrate_update).as_secs_f64();
                    hashrate = hashrate_counter as f64 / elapsed;
                    hashrate_counter = 0;
                    last_hashrate_update = now;
                }
                
                if let Some(nonce) = result {
                    block.nonce = nonce;
                    block.hash = block.calculate_hash();
                    return (block, hashrate);
                }
            }
            
            (block, hashrate)
        }).await?;
        
        let (mined_block, hashrate) = mining_result;
        
        // Update stats
        let elapsed = start_time.elapsed();
        self.stats.hashrate = hashrate;
        self.stats.blocks_mined += 1;
        self.stats.time_mining += elapsed.as_secs();
        self.stats.last_block_time = Some(elapsed.as_secs());
        
        // Update rewards (coinbase transaction amount)
        if let Some(coinbase) = mined_block.transactions.first() {
            if let Some(output) = coinbase.outputs.first() {
                self.stats.rewards_earned += output.amount;
            }
        }
        
        Ok(mined_block)
    }
    
    pub async fn run(
        &mut self,
        blockchain: Arc<Mutex<Blockchain>>,
    ) {
        info!("Starting CPU miner with {} threads", self.threads);
        self.start();
        
        // Set Rayon global thread pool with desired number of threads
        rayon::ThreadPoolBuilder::new()
            .num_threads(self.threads)
            .build_global()
            .unwrap_or_else(|e| {
                error!("Failed to build global thread pool: {}", e);
            });
        
        while self.is_mining.load(Ordering::SeqCst) {
            match self.mine_block(blockchain.clone()).await {
                Ok(mined_block) => {
                    // Submit mined block to blockchain
                    info!("Mined a new block with hash: {}", mined_block.hash);
                    
                    let mut bc = blockchain.lock().await;
                    if let Err(e) = bc.add_block(mined_block) {
                        error!("Failed to add mined block: {}", e);
                    }
                },
                Err(e) => {
                    error!("Mining error: {}", e);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
        
        info!("CPU miner stopped");
    }
}
