use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;

use crate::blockchain::block::Block;
use crate::blockchain::chain::Blockchain;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConsensusAlgorithm {
    ProofOfWork,
    ProofOfStake,
    DelegatedProofOfStake,
}

pub struct Consensus {
    pub algorithm: ConsensusAlgorithm,
    pub difficulty: u32,
    pub target_block_time: u64, // in seconds
    pub stake_threshold: u64,   // minimum amount to stake for PoS (in base units)
}

impl Consensus {
    pub fn new(algorithm: ConsensusAlgorithm) -> Self {
        match algorithm {
            ConsensusAlgorithm::ProofOfWork => Consensus {
                algorithm,
                difficulty: 4,
                target_block_time: 600, // 10 minutes like Bitcoin
                stake_threshold: 0,
            },
            ConsensusAlgorithm::ProofOfStake => Consensus {
                algorithm,
                difficulty: 2,
                target_block_time: 60, // 1 minute
                stake_threshold: 100 * 100_000_000, // 100 WRF in base units
            },
            ConsensusAlgorithm::DelegatedProofOfStake => Consensus {
                algorithm,
                difficulty: 1,
                target_block_time: 30, // 30 seconds
                stake_threshold: 1_000 * 100_000_000, // 1000 WRF in base units
            },
        }
    }
    
    pub fn is_valid_block(&self, block: &Block, blockchain: &Blockchain) -> bool {
        match self.algorithm {
            ConsensusAlgorithm::ProofOfWork => self.verify_pow(block),
            ConsensusAlgorithm::ProofOfStake => self.verify_pos(block, blockchain),
            ConsensusAlgorithm::DelegatedProofOfStake => self.verify_dpos(block, blockchain),
        }
    }
    
    pub fn verify_pow(&self, block: &Block) -> bool {
        // Verify block hash meets difficulty requirement
        if block.hash != block.calculate_hash() {
            return false;
        }
        
        let target = (1 << (256 - block.difficulty)) - 1;
        let hash_value = u128::from_str_radix(&block.hash[0..32], 16).unwrap_or(u128::MAX);
        
        hash_value <= target.into()
    }
    
    pub fn verify_pos(&self, block: &Block, blockchain: &Blockchain) -> bool {
        // In PoS, the miner (validator) must have coins staked
        if let Some(coinbase) = block.transactions.first() {
            let miner_address = match coinbase.outputs.first() {
                Some(output) => &output.address,
                None => return false,
            };
            
            // Calculate miner's stake (in base units)
            let mut stake: u64 = 0;

            for (tx_id, outputs) in &blockchain.utxo_set {
                for (i, output) in outputs.iter().enumerate() {
                    if output.address == *miner_address {
                        // Ensure this UTXO is mature enough (>100 blocks old)
                        if let Some(tx) = blockchain.get_transaction_by_id(tx_id) {
                            // For simplicity, we don't actually check transaction maturity here
                            stake = stake.saturating_add(output.amount);
                        }
                    }
                }
            }

            // Check if miner has minimum stake
            if stake < self.stake_threshold {
                return false;
            }

            // In PoS, the hash difficulty is lower but must meet a target that considers stake
            // For simplicity, we'll use a basic formula: target = base_target * stake / max_stake
            let base_target = (1 << (256 - block.difficulty)) - 1;
            // Cap at 1000 WRF (1000 * 10^8 base units) for max advantage
            let max_stake: u64 = 1_000 * 100_000_000;
            let stake_factor = (stake.min(max_stake) as f64) / (max_stake as f64);
            let adjusted_target = ((base_target as f64) * stake_factor) as u128;
            
            let hash_value = u128::from_str_radix(&block.hash[0..32], 16).unwrap_or(u128::MAX);
            
            hash_value <= adjusted_target
        } else {
            false
        }
    }
    
    pub fn verify_dpos(&self, block: &Block, blockchain: &Blockchain) -> bool {
        // In DPoS, only elected delegates can create blocks
        // For simplicity, we'll assume the top 10 addresses by stake are the delegates
        
        // Get miner address
        if let Some(coinbase) = block.transactions.first() {
            let miner_address = match coinbase.outputs.first() {
                Some(output) => &output.address,
                None => return false,
            };
            
            // Calculate stakes for all addresses (in base units)
            let mut stakes: HashMap<String, u64> = HashMap::new();

            for (tx_id, outputs) in &blockchain.utxo_set {
                for (i, output) in outputs.iter().enumerate() {
                    *stakes.entry(output.address.clone()).or_insert(0) += output.amount;
                }
            }

            // Sort addresses by stake
            let mut addresses: Vec<(&String, &u64)> = stakes.iter().collect();
            addresses.sort_by(|a, b| b.1.cmp(a.1));
            
            // Check if miner is in top 10 delegates
            let is_delegate = addresses
                .iter()
                .take(10)
                .any(|(addr, _)| **addr == *miner_address);
            
            if !is_delegate {
                return false;
            }
            
            // Verify block timestamp - in DPoS, each delegate has a specific time slot
            // We'll use a simple rule: timestamp % (num_delegates * slot_duration) 
            // should match delegate's position * slot_duration
            
            // For simplicity, this check is omitted here
            
            // Finally check basic block validity as in PoW
            self.verify_pow(block)
        } else {
            false
        }
    }
    
    pub fn adjust_difficulty(&mut self, blockchain: &Blockchain) {
        // Adjust difficulty based on recent block times
        if blockchain.chain.len() < 10 {
            return; // Not enough blocks to adjust
        }
        
        let last_10_blocks = &blockchain.chain[(blockchain.chain.len() - 10)..];
        
        // Calculate average block time
        let mut times = Vec::with_capacity(9);
        for i in 1..10 {
            let time_diff = (last_10_blocks[i].timestamp - last_10_blocks[i-1].timestamp).num_seconds();
            times.push(time_diff as u64);
        }
        
        // Remove outliers (extreme values)
        times.sort();
        let times = &times[1..8]; // Remove highest and lowest values
        
        let avg_time = times.iter().sum::<u64>() / times.len() as u64;
        
        // Adjust difficulty based on deviation from target time
        let deviation = avg_time as f64 / self.target_block_time as f64;
        
        if deviation < 0.5 {
            // Blocks are being mined too quickly, increase difficulty
            self.difficulty += 1;
        } else if deviation > 2.0 {
            // Blocks are being mined too slowly, decrease difficulty
            self.difficulty = (self.difficulty as i32 - 1).max(1) as u32;
        }
    }
}
