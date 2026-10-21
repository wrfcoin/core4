//! Example to test the consensus module functionality
//!
//! This example demonstrates blockchain consensus features including
//! chain reorganization, difficulty adjustment, and fork handling.
//!
//! Usage:
//! ```
//! cargo run --example consensus_test
//! ```

use anyhow::Result;
use chrono::Utc;
use colored::*;
use env_logger::Env;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

// Import from the main crate
use wrfcoin_cli::consensus::{
    BlockHeader, ConsensusAlgorithm, ConsensusEngine, ConsensusParameters,
};
use wrfcoin_cli::security::{CheckpointBlock, SecurityService};

fn main() -> Result<()> {
    // Initialize logger
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    // Display header
    println!("{}", "Wrfcoin Consensus Module Test".bold().green());
    println!("{}", "============================".green());

    // Create security service
    let security_service = Arc::new(Mutex::new(SecurityService::default()));

    // Create consensus parameters
    let parameters = ConsensusParameters {
        algorithm: ConsensusAlgorithm::ProofOfWork,
        target_block_time: 15, // 15 seconds
        block_reward: 50.0,
        reward_halving_interval: Some(210000),
        max_supply: Some(21000000.0),
        initial_difficulty: 1,
        difficulty_adjustment_interval: 10, // Small for testing
        max_difficulty_adjustment_factor: 0.25,
        max_future_block_time: 120,
        max_block_size: 1000000,
        min_confirmations: 6,
    };

    // Create genesis block
    let genesis_block = BlockHeader {
        hash: "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f".to_string(),
        height: 0,
        prev_hash: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
        timestamp: Utc::now().to_rfc3339(),
        merkle_root: "4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b".to_string(),
        difficulty: 1,
        nonce: 0,
        version: 1,
        block_type: "Genesis".to_string(),
    };

    // Create consensus engine
    let consensus = ConsensusEngine::new(security_service.clone(), parameters, genesis_block);

    // Test 1: Build a main chain
    println!("\n{}", "Test 1: Building Main Chain".yellow());

    println!("Current blockchain height: {}", consensus.get_height());
    println!("Current difficulty: {}", consensus.get_difficulty());

    // Add some blocks to the main chain
    let mut prev_hash =
        "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f".to_string();

    for height in 1..=15 {
        let block = BlockHeader {
            hash: format!("main_block_hash_{}", height),
            height,
            prev_hash: prev_hash.clone(),
            timestamp: Utc::now().to_rfc3339(),
            merkle_root: format!("merkle_root_{}", height),
            difficulty: consensus.get_difficulty(),
            nonce: 12345 + height,
            version: 1,
            block_type: "Standard".to_string(),
        };

        match consensus.process_block(block.clone())? {
            wrfcoin_cli::security::ConsensusResult::Valid => {
                println!("Added block {} to main chain", height);
                prev_hash = block.hash.clone();
            },
            wrfcoin_cli::security::ConsensusResult::Invalid(reason) => {
                println!("Failed to add block {}: {}", height, reason);
            },
            wrfcoin_cli::security::ConsensusResult::Indeterminate(reason) => {
                println!("Block {} status indeterminate: {}", height, reason);
            },
        }

        // Small delay to ensure blocks have different timestamps
        thread::sleep(Duration::from_millis(10));
    }

    println!("Main chain built with height: {}", consensus.get_height());
    println!("Current difficulty: {}", consensus.get_difficulty());

    // Test 2: Orphaned blocks
    println!("\n{}", "Test 2: Orphaned Blocks".yellow());

    // Create an orphaned block (doesn't connect to known chain)
    let orphan_block = BlockHeader {
        hash: "orphan_block_hash".to_string(),
        height: 20, // Gap from the main chain
        prev_hash: "unknown_prev_hash".to_string(),
        timestamp: Utc::now().to_rfc3339(),
        merkle_root: "orphan_merkle_root".to_string(),
        difficulty: consensus.get_difficulty(),
        nonce: 54321,
        version: 1,
        block_type: "Standard".to_string(),
    };

    match consensus.process_block(orphan_block)? {
        wrfcoin_cli::security::ConsensusResult::Valid => {
            println!("Orphan block was unexpectedly accepted");
        },
        wrfcoin_cli::security::ConsensusResult::Invalid(reason) => {
            println!("Orphan block rejected: {}", reason);
        },
        wrfcoin_cli::security::ConsensusResult::Indeterminate(reason) => {
            println!("Orphan block status indeterminate (expected): {}", reason);
        },
    }

    // Test 3: Create a fork
    println!("\n{}", "Test 3: Blockchain Fork".yellow());

    // Get a block to fork from (block 5)
    let fork_start_block = consensus.get_block_at_height(5).unwrap();
    println!(
        "Starting fork from block {} ({})",
        fork_start_block.height, fork_start_block.hash
    );

    // Create a competing chain (fork)
    let mut fork_prev_hash = fork_start_block.hash.clone();

    // Create branches of the fork (fork blocks 6 to 10)
    for height in 6..=10 {
        let block = BlockHeader {
            hash: format!("fork_block_hash_{}", height),
            height,
            prev_hash: fork_prev_hash.clone(),
            timestamp: Utc::now().to_rfc3339(),
            merkle_root: format!("fork_merkle_root_{}", height),
            difficulty: consensus.get_difficulty(),
            nonce: 98765 - height,
            version: 1,
            block_type: "Standard".to_string(),
        };

        match consensus.process_block(block.clone())? {
            wrfcoin_cli::security::ConsensusResult::Valid => {
                println!("Added block {} to fork", height);
                fork_prev_hash = block.hash.clone();
            },
            wrfcoin_cli::security::ConsensusResult::Invalid(reason) => {
                println!("Failed to add block {} to fork: {}", height, reason);
            },
            wrfcoin_cli::security::ConsensusResult::Indeterminate(reason) => {
                println!("Fork block {} status indeterminate: {}", height, reason);
            },
        }

        // Small delay to ensure blocks have different timestamps
        thread::sleep(Duration::from_millis(10));
    }

    println!(
        "Current chain height after fork attempt: {}",
        consensus.get_height()
    );

    // Test 4: Create a longer fork with higher difficulty to trigger reorganization
    println!("\n{}", "Test 4: Chain Reorganization".yellow());

    // Get a block to fork from (block 7)
    let reorg_fork_start_block = consensus.get_block_at_height(7).unwrap();
    println!(
        "Starting reorganization fork from block {} ({})",
        reorg_fork_start_block.height, reorg_fork_start_block.hash
    );

    // Create a competing chain with higher difficulty
    let mut reorg_fork_prev_hash = reorg_fork_start_block.hash.clone();

    // Create blocks for this fork with higher difficulty
    for height in 8..=20 {
        let block = BlockHeader {
            hash: format!("reorg_fork_block_hash_{}", height),
            height,
            prev_hash: reorg_fork_prev_hash.clone(),
            timestamp: Utc::now().to_rfc3339(),
            merkle_root: format!("reorg_fork_merkle_root_{}", height),
            difficulty: consensus.get_difficulty() * 5, // Higher difficulty
            nonce: 54321 + height,
            version: 1,
            block_type: "Standard".to_string(),
        };

        match consensus.process_block(block.clone())? {
            wrfcoin_cli::security::ConsensusResult::Valid => {
                println!("Added block {} to reorganization fork", height);
                reorg_fork_prev_hash = block.hash.clone();
            },
            wrfcoin_cli::security::ConsensusResult::Invalid(reason) => {
                println!(
                    "Failed to add block {} to reorganization fork: {}",
                    height, reason
                );
            },
            wrfcoin_cli::security::ConsensusResult::Indeterminate(reason) => {
                println!(
                    "Reorganization fork block {} status indeterminate: {}",
                    height, reason
                );
            },
        }

        // Small delay to ensure blocks have different timestamps
        thread::sleep(Duration::from_millis(10));
    }

    println!(
        "Chain height after reorganization attempt: {}",
        consensus.get_height()
    );
    if let Some(latest_block) = consensus.get_latest_block() {
        println!("Latest block hash: {}", latest_block.hash);

        // Check if reorganization was successful
        if latest_block.hash.starts_with("reorg_fork_block_hash") {
            println!("Chain reorganization was successful!");
        } else {
            println!("Chain was not reorganized");
        }
    }

    // Test 5: Block Rewards
    println!("\n{}", "Test 5: Block Rewards".yellow());

    for height in 0..=5 {
        let reward = consensus.calculate_block_reward(height * 210000);
        println!("Block reward at height {}: {} WRF", height * 210000, reward);
    }

    // Test 6: Checkpoints
    println!("\n{}", "Test 6: Checkpoint Blocks".yellow());

    // Add a checkpoint at the current chain tip
    let latest_height = consensus.get_height();
    let latest_block = consensus.get_latest_block().unwrap();

    let checkpoint = CheckpointBlock {
        height: latest_height,
        hash: latest_block.hash.clone(),
        timestamp: Utc::now().to_rfc3339(),
    };

    // Add the checkpoint to the security service
    security_service.lock().unwrap().add_checkpoint(checkpoint);
    println!(
        "Added checkpoint at height {} with hash {}",
        latest_height, latest_block.hash
    );

    // Try to create a fork that would reorganize past the checkpoint
    println!("Attempting to create a fork that would reorganize past the checkpoint (should fail)");

    // Get a block to fork from before the checkpoint
    let pre_checkpoint_block = consensus.get_block_at_height(latest_height - 5).unwrap();

    // Create competing chain
    let mut invalid_fork_prev_hash = pre_checkpoint_block.hash.clone();

    // This fork should be rejected since it would go past a checkpoint
    for height in (latest_height - 4)..=(latest_height + 5) {
        let block = BlockHeader {
            hash: format!("invalid_fork_block_hash_{}", height),
            height,
            prev_hash: invalid_fork_prev_hash.clone(),
            timestamp: Utc::now().to_rfc3339(),
            merkle_root: format!("invalid_fork_merkle_root_{}", height),
            difficulty: consensus.get_difficulty() * 10, // Very high difficulty
            nonce: 12345 + height,
            version: 1,
            block_type: "Standard".to_string(),
        };

        match consensus.process_block(block.clone())? {
            wrfcoin_cli::security::ConsensusResult::Valid => {
                println!("Added block {} to invalid fork (unexpected!)", height);
                invalid_fork_prev_hash = block.hash.clone();
            },
            wrfcoin_cli::security::ConsensusResult::Invalid(reason) => {
                println!(
                    "Failed to add block {} to invalid fork (expected): {}",
                    height, reason
                );
                break;
            },
            wrfcoin_cli::security::ConsensusResult::Indeterminate(reason) => {
                println!(
                    "Invalid fork block {} status indeterminate: {}",
                    height, reason
                );
                invalid_fork_prev_hash = block.hash.clone();
            },
        }

        thread::sleep(Duration::from_millis(10));
    }

    println!(
        "Final chain height after all tests: {}",
        consensus.get_height()
    );
    if let Some(final_block) = consensus.get_latest_block() {
        println!("Final block hash: {}", final_block.hash);
    }

    println!(
        "\n{}",
        "Consensus testing completed successfully!".green().bold()
    );
    Ok(())
}
