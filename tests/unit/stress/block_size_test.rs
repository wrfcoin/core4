//! Block Size Stress Test for Wrfcoin
//!
//! This module implements tests to evaluate the blockchain's performance
//! with different block sizes and transaction densities.

use anyhow::{Context, Result};
use futures::stream::{self, StreamExt};
use log::{debug, error, info, warn};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::time;

/// Configuration for block size tests
#[derive(Debug, Clone)]
pub struct BlockSizeTestConfig {
    /// API URL of the target node
    pub api_url: String,
    
    /// Block sizes to test (number of transactions)
    pub block_sizes: Vec<usize>,
    
    /// Number of test blocks per size
    pub blocks_per_size: usize,
    
    /// Mining address to use
    pub miner_address: String,
    
    /// Delay between blocks (seconds)
    pub inter_block_delay_seconds: u64,
    
    /// Whether to automatically increase difficulty
    pub auto_adjust_difficulty: bool,
}

impl Default for BlockSizeTestConfig {
    fn default() -> Self {
        Self {
            api_url: std::env::var("WRFCOIN_TEST_API_URL")
                .unwrap_or_else(|_| "http://localhost:8556".to_string()),
            block_sizes: vec![10, 50, 100, 250, 500, 1000],
            blocks_per_size: 3,
            miner_address: "miner_address".to_string(),
            inter_block_delay_seconds: 5,
            auto_adjust_difficulty: true,
        }
    }
}

/// Block metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockMetrics {
    /// Block hash
    pub hash: String,
    
    /// Block height
    pub height: u64,
    
    /// Number of transactions
    pub transaction_count: usize,
    
    /// Block size in bytes
    pub size_bytes: usize,
    
    /// Time to mine the block in milliseconds
    pub mining_time_ms: u64,
    
    /// Time to propagate the block in milliseconds
    pub propagation_time_ms: Option<u64>,
    
    /// Block timestamp
    pub timestamp: u64,
    
    /// Difficulty
    pub difficulty: f64,
}

/// Block size test results
#[derive(Debug, Clone)]
pub struct BlockSizeTestResults {
    /// Test configuration
    pub config: BlockSizeTestConfig,
    
    /// Metrics for each block
    pub block_metrics: Vec<BlockMetrics>,
    
    /// Test duration
    pub duration_seconds: f64,
    
    /// Average time to create blocks by size
    pub avg_creation_times: HashMap<usize, f64>,
    
    /// Average propagation times by size
    pub avg_propagation_times: HashMap<usize, f64>,
    
    /// Transactions per second by block size
    pub tps_by_size: HashMap<usize, f64>,
}

impl BlockSizeTestResults {
    /// Analyze the results
    pub fn analyze(&self) {
        println!("=== Block Size Test Analysis ===");
        println!("Total blocks tested: {}", self.block_metrics.len());
        println!("Test duration: {:.2} seconds", self.duration_seconds);
        
        // Print metrics for each block size
        for &size in &self.config.block_sizes {
            if let Some(&avg_creation) = self.avg_creation_times.get(&size) {
                println!("\nBlock size: {} transactions", size);
                println!("  Average creation time: {:.2} ms", avg_creation);
                
                if let Some(&avg_prop) = self.avg_propagation_times.get(&size) {
                    println!("  Average propagation time: {:.2} ms", avg_prop);
                }
                
                if let Some(&tps) = self.tps_by_size.get(&size) {
                    println!("  Transactions per second: {:.2}", tps);
                }
            }
        }
        
        // Find optimal block size
        if let Some((optimal_size, _)) = self.tps_by_size.iter()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)) {
            
            println!("\nOptimal block size: {} transactions", optimal_size);
            println!("  TPS: {:.2}", self.tps_by_size[optimal_size]);
            println!("  Average creation time: {:.2} ms", self.avg_creation_times[optimal_size]);
            
            if self.avg_propagation_times.contains_key(optimal_size) {
                println!("  Average propagation time: {:.2} ms", self.avg_propagation_times[optimal_size]);
            }
        }
        
        println!("===============================");
    }
}

/// Block size test runner
pub struct BlockSizeTest {
    config: BlockSizeTestConfig,
    client: Client,
}

impl BlockSizeTest {
    /// Create a new block size test
    pub fn new(config: BlockSizeTestConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("Failed to create HTTP client");
            
        Self { config, client }
    }
    
    /// Run the block size test
    pub async fn run(&self) -> Result<BlockSizeTestResults> {
        info!("Starting block size test");
        info!("Testing block sizes: {:?}", self.config.block_sizes);
        
        let start_time = Instant::now();
        let mut block_metrics = Vec::new();
        let mut current_difficulty = self.get_current_difficulty().await?;
        
        // Test each block size
        for &size in &self.config.block_sizes {
            info!("Testing blocks with {} transactions", size);
            
            for i in 0..self.config.blocks_per_size {
                info!("Creating block {}/{} with {} transactions", i+1, self.config.blocks_per_size, size);
                
                // Prepare transactions
                let txs = self.prepare_transactions(size).await?;
                info!("Prepared {} transactions", txs.len());
                
                // Submit transactions
                self.submit_transactions(&txs).await?;
                info!("Submitted transactions to mempool");
                
                // Mine the block and measure time
                let mining_start = Instant::now();
                let block_hash = self.mine_block().await?;
                let mining_time = mining_start.elapsed();
                
                info!("Mined block in {:.2} ms", mining_time.as_millis());
                
                // Get block details
                let block = self.get_block_details(&block_hash).await?;
                
                // Measure propagation time (if multi-node)
                let propagation_time = self.measure_propagation_time(&block_hash).await.ok();
                
                // Record metrics
                let metrics = BlockMetrics {
                    hash: block_hash,
                    height: block.height,
                    transaction_count: block.transaction_count,
                    size_bytes: block.size_bytes,
                    mining_time_ms: mining_time.as_millis() as u64,
                    propagation_time_ms: propagation_time,
                    timestamp: block.timestamp,
                    difficulty: current_difficulty,
                };
                
                block_metrics.push(metrics);
                
                // Auto-adjust difficulty if enabled
                if self.config.auto_adjust_difficulty {
                    if mining_time.as_millis() < 500 {
                        current_difficulty *= 1.2;
                        info!("Increased difficulty to {}", current_difficulty);
                        self.set_difficulty(current_difficulty).await?;
                    } else if mining_time.as_millis() > 10000 {
                        current_difficulty *= 0.8;
                        info!("Decreased difficulty to {}", current_difficulty);
                        self.set_difficulty(current_difficulty).await?;
                    }
                }
                
                // Delay between blocks
                if i < self.config.blocks_per_size - 1 {
                    time::sleep(Duration::from_secs(self.config.inter_block_delay_seconds)).await;
                }
            }
        }
        
        let duration_seconds = start_time.elapsed().as_secs_f64();
        
        // Calculate statistics
        let mut avg_creation_times: HashMap<usize, f64> = HashMap::new();
        let mut avg_propagation_times: HashMap<usize, f64> = HashMap::new();
        let mut tps_by_size: HashMap<usize, f64> = HashMap::new();
        
        for &size in &self.config.block_sizes {
            let size_metrics: Vec<_> = block_metrics.iter()
                .filter(|m| m.transaction_count == size)
                .collect();
                
            if !size_metrics.is_empty() {
                // Average creation time
                let total_creation_time: u64 = size_metrics.iter()
                    .map(|m| m.mining_time_ms)
                    .sum();
                    
                let avg_creation = total_creation_time as f64 / size_metrics.len() as f64;
                avg_creation_times.insert(size, avg_creation);
                
                // Average propagation time (if available)
                let prop_metrics: Vec<_> = size_metrics.iter()
                    .filter_map(|m| m.propagation_time_ms)
                    .collect();
                    
                if !prop_metrics.is_empty() {
                    let total_prop_time: u64 = prop_metrics.iter().sum();
                    let avg_propagation = total_prop_time as f64 / prop_metrics.len() as f64;
                    avg_propagation_times.insert(size, avg_propagation);
                }
                
                // Transactions per second
                if avg_creation > 0.0 {
                    let tps = (size as f64) * 1000.0 / avg_creation;
                    tps_by_size.insert(size, tps);
                }
            }
        }
        
        let results = BlockSizeTestResults {
            config: self.config.clone(),
            block_metrics,
            duration_seconds,
            avg_creation_times,
            avg_propagation_times,
            tps_by_size,
        };
        
        results.analyze();
        
        Ok(results)
    }
    
    /// Get current blockchain difficulty
    async fn get_current_difficulty(&self) -> Result<f64> {
        let endpoint = format!("{}/status", self.config.api_url);
        
        let response = self.client
            .get(&endpoint)
            .send()
            .await
            .context("Failed to get blockchain status")?;
            
        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to get blockchain status"));
        }
        
        let status: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse status response")?;
            
        if let Some(difficulty) = status
            .get("current_difficulty")
            .and_then(|d| d.as_f64()) {
            Ok(difficulty)
        } else {
            // Default difficulty if not available
            Ok(1.0)
        }
    }
    
    /// Set blockchain difficulty
    async fn set_difficulty(&self, difficulty: f64) -> Result<()> {
        let endpoint = format!("{}/testing/set-difficulty", self.config.api_url);
        
        let body = serde_json::json!({ "difficulty": difficulty });
        
        let response = self.client
            .post(&endpoint)
            .json(&body)
            .send()
            .await
            .context("Failed to set difficulty")?;
            
        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to set difficulty"));
        }
        
        Ok(())
    }
    
    /// Prepare a batch of test transactions
    async fn prepare_transactions(&self, count: usize) -> Result<Vec<serde_json::Value>> {
        let mut transactions = Vec::with_capacity(count);
        
        // Create test addresses
        let addresses: Vec<String> = (0..10)
            .map(|i| format!("test_address_{}", i))
            .collect();
            
        // Generate transactions
        for i in 0..count {
            let sender_idx = i % 10;
            let recipient_idx = (i + 5) % 10;
            
            let transaction = serde_json::json!({
                "sender": addresses[sender_idx],
                "recipient": addresses[recipient_idx],
                "amount": 0.1,
                "fee": 0.001,
                "data": format!("block-test-{}", i),
            });
            
            transactions.push(transaction);
        }
        
        Ok(transactions)
    }
    
    /// Submit transactions to the mempool
    async fn submit_transactions(&self, transactions: &[serde_json::Value]) -> Result<()> {
        let endpoint = format!("{}/transactions", self.config.api_url);
        
        // Submit in parallel with rate limiting
        let futures = stream::iter(transactions.iter().enumerate())
            .map(|(i, tx)| {
                let client = self.client.clone();
                let endpoint = endpoint.clone();
                let tx = tx.clone();
                
                async move {
                    // Add a small delay to prevent overwhelming the node
                    if i > 0 && i % 50 == 0 {
                        time::sleep(Duration::from_millis(100)).await;
                    }
                    
                    match client.post(&endpoint).json(&tx).send().await {
                        Ok(response) => {
                            if response.status().is_success() {
                                Ok(())
                            } else {
                                let status = response.status();
                                let error = response.text().await.unwrap_or_default();
                                Err(anyhow::anyhow!("Failed to submit transaction: {} - {}", status, error))
                            }
                        }
                        Err(e) => Err(anyhow::anyhow!("Request error: {}", e)),
                    }
                }
            })
            .buffer_unordered(10) // Submit 10 at a time
            .collect::<Vec<Result<()>>>();
            
        let results = futures.await;
        
        let errors: Vec<_> = results.iter()
            .filter_map(|r| r.as_ref().err())
            .collect();
            
        if !errors.is_empty() {
            warn!("Failed to submit {} out of {} transactions", errors.len(), transactions.len());
            for (i, error) in errors.iter().enumerate().take(5) {
                warn!("Error {}: {}", i+1, error);
            }
            
            if errors.len() > 5 {
                warn!("... and {} more errors", errors.len() - 5);
            }
        }
        
        Ok(())
    }
    
    /// Mine a block
    async fn mine_block(&self) -> Result<String> {
        let endpoint = format!("{}/mine", self.config.api_url);
        
        let body = serde_json::json!({
            "miner": self.config.miner_address,
        });
        
        let response = self.client
            .post(&endpoint)
            .json(&body)
            .send()
            .await
            .context("Failed to mine block")?;
            
        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to mine block"));
        }
        
        let result: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse mining response")?;
            
        if let Some(hash) = result.get("block_hash").and_then(|h| h.as_str()) {
            Ok(hash.to_string())
        } else {
            Err(anyhow::anyhow!("Mining response did not contain block hash"))
        }
    }
    
    /// Get block details
    async fn get_block_details(&self, block_hash: &str) -> Result<BlockDetails> {
        let endpoint = format!("{}/blocks/{}", self.config.api_url, block_hash);
        
        let response = self.client
            .get(&endpoint)
            .send()
            .await
            .context("Failed to get block details")?;
            
        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to get block details"));
        }
        
        let block: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse block details")?;
            
        let height = block.get("height")
            .and_then(|h| h.as_u64())
            .context("Missing block height")?;
            
        let transaction_count = block.get("transactions")
            .and_then(|txs| txs.as_array())
            .map(|txs| txs.len())
            .context("Missing transactions array")?;
            
        let size_bytes = block.get("size")
            .and_then(|s| s.as_u64())
            .unwrap_or(0) as usize;
            
        let timestamp = block.get("timestamp")
            .and_then(|t| t.as_u64())
            .context("Missing block timestamp")?;
            
        Ok(BlockDetails {
            height,
            transaction_count,
            size_bytes,
            timestamp,
        })
    }
    
    /// Measure block propagation time across the network
    async fn measure_propagation_time(&self, block_hash: &str) -> Result<u64> {
        // Get list of network nodes
        let nodes = self.get_network_nodes().await?;
        
        if nodes.len() <= 1 {
            // Can't measure propagation with just one node
            return Err(anyhow::anyhow!("Insufficient nodes to measure propagation"));
        }
        
        let start_time = Instant::now();
        let timeout = Duration::from_secs(30);
        let mut all_nodes_have_block = false;
        
        // Poll nodes until all have the block or timeout
        while start_time.elapsed() < timeout {
            let mut nodes_with_block = 0;
            
            for node in &nodes {
                if let Ok(true) = self.node_has_block(node, block_hash).await {
                    nodes_with_block += 1;
                }
            }
            
            if nodes_with_block == nodes.len() {
                all_nodes_have_block = true;
                break;
            }
            
            // Wait a bit before checking again
            time::sleep(Duration::from_millis(100)).await;
        }
        
        if all_nodes_have_block {
            Ok(start_time.elapsed().as_millis() as u64)
        } else {
            Err(anyhow::anyhow!("Timeout waiting for block propagation"))
        }
    }
    
    /// Get list of network nodes
    async fn get_network_nodes(&self) -> Result<Vec<String>> {
        let endpoint = format!("{}/nodes", self.config.api_url);
        
        let response = self.client
            .get(&endpoint)
            .send()
            .await
            .context("Failed to get network nodes")?;
            
        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to get network nodes"));
        }
        
        let nodes: Vec<serde_json::Value> = response
            .json()
            .await
            .context("Failed to parse network nodes response")?;
            
        let node_urls = nodes.iter()
            .filter_map(|node| {
                node.get("api_url").and_then(|url| url.as_str()).map(String::from)
            })
            .collect();
            
        Ok(node_urls)
    }
    
    /// Check if a node has a specific block
    async fn node_has_block(&self, node_url: &str, block_hash: &str) -> Result<bool> {
        let endpoint = format!("{}/blocks/{}", node_url, block_hash);
        
        let response = self.client
            .get(&endpoint)
            .send()
            .await
            .context("Failed to check block on node")?;
            
        Ok(response.status().is_success())
    }
}

/// Block details
#[derive(Debug, Clone)]
pub struct BlockDetails {
    pub height: u64,
    pub transaction_count: usize,
    pub size_bytes: usize,
    pub timestamp: u64,
}

/// Run a block size test
pub async fn run_block_size_test(
    api_url: &str,
    block_sizes: Vec<usize>,
    blocks_per_size: usize,
) -> Result<()> {
    info!("Starting block size test");
    
    let config = BlockSizeTestConfig {
        api_url: api_url.to_string(),
        block_sizes,
        blocks_per_size,
        miner_address: "test_miner".to_string(),
        inter_block_delay_seconds: 5,
        auto_adjust_difficulty: true,
    };
    
    let test = BlockSizeTest::new(config);
    let results = test.run().await?;
    
    info!(
        "Block size test completed with {} total blocks",
        results.block_metrics.len()
    );
    
    // Calculate average TPS
    let total_txs: usize = results.block_metrics.iter()
        .map(|m| m.transaction_count)
        .sum();
        
    let avg_tps = total_txs as f64 / results.duration_seconds;
    
    info!(
        "Overall average TPS: {:.2} ({} transactions in {:.2} seconds)",
        avg_tps,
        total_txs,
        results.duration_seconds
    );
    
    Ok(())
}