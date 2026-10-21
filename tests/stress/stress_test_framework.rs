//! Stress Testing Framework for Wrfcoin
//!
//! This module provides tools and utilities for stress testing the Wrfcoin blockchain.
//! It includes functionality for generating high transaction volumes, measuring performance,
//! and analyzing system behavior under load.

use anyhow::{Context, Result};
use futures::stream::{self, StreamExt};
use log::{debug, error, info, warn};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::time;

// Configuration for stress tests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StressTestConfig {
    // Target node API URL
    pub api_url: String,
    
    // Number of concurrent clients
    pub concurrent_clients: usize,
    
    // Number of transactions per client
    pub transactions_per_client: usize,
    
    // Delay between batches (milliseconds)
    pub batch_delay_ms: u64,
    
    // Batch size (transactions per batch)
    pub batch_size: usize,
    
    // Transaction types to use
    pub transaction_types: Vec<TransactionType>,
    
    // Random seed for reproducible tests
    pub random_seed: Option<u64>,
    
    // Test duration (seconds, 0 means unlimited)
    pub duration_seconds: u64,
    
    // Log interval (seconds)
    pub log_interval_seconds: u64,
    
    // Whether to collect detailed metrics
    pub detailed_metrics: bool,
}

impl Default for StressTestConfig {
    fn default() -> Self {
        Self {
            api_url: std::env::var("WRFCOIN_TEST_API_URL")
                .unwrap_or_else(|_| "http://localhost:8556".to_string()),
            concurrent_clients: 10,
            transactions_per_client: 100,
            batch_delay_ms: 100,
            batch_size: 10,
            transaction_types: vec![TransactionType::Transfer],
            random_seed: None,
            duration_seconds: 60,
            log_interval_seconds: 5,
            detailed_metrics: true,
        }
    }
}

// Type of transaction to generate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransactionType {
    // Regular token transfer
    Transfer,
    
    // NFT creation
    CreateNft,
    
    // NFT verification
    VerifyNft,
    
    // Complex transaction with multiple inputs/outputs
    Complex,
}

// Stats collected during stress test
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct StressTestStats {
    // Total transactions submitted
    pub transactions_submitted: usize,
    
    // Successful transactions
    pub transactions_successful: usize,
    
    // Failed transactions
    pub transactions_failed: usize,
    
    // Transaction latencies (milliseconds)
    pub latencies_ms: Vec<u64>,
    
    // Test duration
    pub duration_ms: u64,
    
    // Transactions per second
    pub tps: f64,
    
    // Memory usage before test
    pub memory_before_mb: f64,
    
    // Memory usage after test
    pub memory_after_mb: f64,
    
    // CPU usage percentage
    pub cpu_usage_percent: f64,
    
    // Confirmation times (if measured)
    pub confirmation_times_ms: Vec<u64>,
    
    // Block sizes during test
    pub block_sizes: Vec<usize>,
    
    // Mempool sizes during test
    pub mempool_sizes: Vec<usize>,
}

impl StressTestStats {
    // Calculate and update derived metrics
    pub fn calculate_derived_metrics(&mut self) {
        if self.duration_ms > 0 {
            self.tps = (self.transactions_successful as f64) * 1000.0 / (self.duration_ms as f64);
        }
    }
    
    // Print a summary of the stats
    pub fn print_summary(&self) {
        println!("=== Stress Test Results ===");
        println!("Transactions submitted: {}", self.transactions_submitted);
        println!("Transactions successful: {}", self.transactions_successful);
        println!("Transactions failed: {}", self.transactions_failed);
        println!("Success rate: {:.2}%", 
            (self.transactions_successful as f64) / 
            (self.transactions_submitted as f64) * 100.0);
        println!("Test duration: {:.2} seconds", self.duration_ms as f64 / 1000.0);
        println!("Transactions per second: {:.2}", self.tps);
        
        if !self.latencies_ms.is_empty() {
            let avg_latency = self.latencies_ms.iter().sum::<u64>() as f64 / self.latencies_ms.len() as f64;
            println!("Average latency: {:.2} ms", avg_latency);
            
            let mut sorted_latencies = self.latencies_ms.clone();
            sorted_latencies.sort_unstable();
            
            let p50 = sorted_latencies[sorted_latencies.len() * 50 / 100];
            let p95 = sorted_latencies[sorted_latencies.len() * 95 / 100];
            let p99 = sorted_latencies[sorted_latencies.len() * 99 / 100];
            
            println!("Latency p50: {} ms", p50);
            println!("Latency p95: {} ms", p95);
            println!("Latency p99: {} ms", p99);
        }
        
        if !self.confirmation_times_ms.is_empty() {
            let avg_confirmation = self.confirmation_times_ms.iter().sum::<u64>() as f64 / 
                self.confirmation_times_ms.len() as f64;
            println!("Average confirmation time: {:.2} ms", avg_confirmation);
        }
        
        if !self.mempool_sizes.is_empty() {
            let avg_mempool = self.mempool_sizes.iter().sum::<usize>() as f64 / 
                self.mempool_sizes.len() as f64;
            println!("Average mempool size: {:.2} txs", avg_mempool);
        }
        
        println!("Memory usage: {:.2} MB → {:.2} MB", self.memory_before_mb, self.memory_after_mb);
        println!("CPU usage: {:.2}%", self.cpu_usage_percent);
        println!("===========================");
    }
}

// Transaction template for generating test transactions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionTemplate {
    // Optional sender address
    pub sender: Option<String>,
    
    // Optional recipient address
    pub recipient: Option<String>,
    
    // Transaction amount
    pub amount: f64,
    
    // Transaction type
    pub tx_type: TransactionType,
    
    // Additional data (for NFTs, etc.)
    pub additional_data: Option<serde_json::Value>,
}

// Main stress test runner
pub struct StressTestRunner {
    config: StressTestConfig,
    client: Client,
    stats: Arc<Mutex<StressTestStats>>,
    running: Arc<AtomicBool>,
    counter: Arc<AtomicUsize>,
}

impl StressTestRunner {
    // Create a new stress test runner
    pub fn new(config: StressTestConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");
            
        Self {
            config,
            client,
            stats: Arc::new(Mutex::new(StressTestStats::default())),
            running: Arc::new(AtomicBool::new(false)),
            counter: Arc::new(AtomicUsize::new(0)),
        }
    }
    
    // Run the stress test
    pub async fn run(&self) -> Result<StressTestStats> {
        info!("Starting stress test with {} concurrent clients", self.config.concurrent_clients);
        
        // Get initial system metrics
        let memory_before = self.get_memory_usage().await?;
        
        // Set running flag
        self.running.store(true, Ordering::SeqCst);
        
        // Start metrics collection
        let metrics_handle = self.start_metrics_collection();
        
        // Record start time
        let start_time = Instant::now();
        
        // Create concurrent clients
        let mut handles = Vec::new();
        
        for client_id in 0..self.config.concurrent_clients {
            let client_clone = self.client.clone();
            let config_clone = self.config.clone();
            let stats_clone = self.stats.clone();
            let running_clone = self.running.clone();
            let counter_clone = self.counter.clone();
            
            let handle = tokio::spawn(async move {
                Self::client_worker(
                    client_id,
                    client_clone,
                    config_clone,
                    stats_clone,
                    running_clone,
                    counter_clone,
                ).await
            });
            
            handles.push(handle);
        }
        
        // Set up duration timer if needed
        if self.config.duration_seconds > 0 {
            let running_clone = self.running.clone();
            let duration_seconds = self.config.duration_seconds;
            tokio::spawn(async move {
                time::sleep(Duration::from_secs(duration_seconds)).await;
                running_clone.store(false, Ordering::SeqCst);
                info!("Test duration reached, stopping test");
            });
        }
        
        // Wait for all clients to complete
        for handle in handles {
            if let Err(e) = handle.await {
                error!("Client thread panicked: {}", e);
            }
        }
        
        // Calculate test duration
        let duration = start_time.elapsed();
        
        // Stop metrics collection
        if let Some(handle) = metrics_handle {
            handle.abort();
        }
        
        // Get final system metrics
        let memory_after = self.get_memory_usage().await?;
        
        // Update stats
        let mut stats = self.stats.lock().await;
        stats.duration_ms = duration.as_millis() as u64;
        stats.memory_before_mb = memory_before;
        stats.memory_after_mb = memory_after;
        stats.calculate_derived_metrics();
        
        info!("Stress test completed in {:.2} seconds", duration.as_secs_f64());
        stats.print_summary();
        
        Ok(stats.clone())
    }
    
    // Client worker function
    async fn client_worker(
        client_id: usize,
        client: Client,
        config: StressTestConfig,
        stats: Arc<Mutex<StressTestStats>>,
        running: Arc<AtomicBool>,
        counter: Arc<AtomicUsize>,
    ) -> Result<()> {
        debug!("Client {} starting", client_id);
        
        let mut transaction_count = 0;
        
        while running.load(Ordering::SeqCst) && 
              (config.transactions_per_client == 0 || 
               transaction_count < config.transactions_per_client) {
            
            // Create a batch of transactions
            let batch_size = std::cmp::min(
                config.batch_size,
                if config.transactions_per_client > 0 {
                    config.transactions_per_client - transaction_count
                } else {
                    config.batch_size
                }
            );
            
            if batch_size == 0 {
                break;
            }
            
            // Process a batch of transactions concurrently
            let futures = stream::iter((0..batch_size).map(|i| {
                let tx_counter = counter.fetch_add(1, Ordering::SeqCst);
                let client = client.clone();
                let config = config.clone();
                let stats = stats.clone();
                
                async move {
                    let template = Self::generate_transaction_template(
                        client_id, 
                        tx_counter, 
                        &config
                    );
                    
                    let result = Self::submit_transaction(&client, &config, template).await;
                    
                    // Update stats
                    let mut stats_lock = stats.lock().await;
                    stats_lock.transactions_submitted += 1;
                    
                    match result {
                        Ok(latency) => {
                            stats_lock.transactions_successful += 1;
                            stats_lock.latencies_ms.push(latency);
                        }
                        Err(_) => {
                            stats_lock.transactions_failed += 1;
                        }
                    }
                    
                    drop(stats_lock);
                    
                    result
                }
            }))
            .buffer_unordered(batch_size)
            .collect::<Vec<_>>();
            
            // Wait for batch completion
            let _results = futures.await;
            
            transaction_count += batch_size;
            
            // Apply batch delay
            if config.batch_delay_ms > 0 {
                time::sleep(Duration::from_millis(config.batch_delay_ms)).await;
            }
        }
        
        debug!("Client {} completed {} transactions", client_id, transaction_count);
        
        Ok(())
    }
    
    // Generate a transaction template
    fn generate_transaction_template(
        client_id: usize,
        tx_counter: usize,
        config: &StressTestConfig,
    ) -> TransactionTemplate {
        // Choose a transaction type
        let tx_type = if config.transaction_types.is_empty() {
            TransactionType::Transfer
        } else {
            config.transaction_types[tx_counter % config.transaction_types.len()].clone()
        };
        
        // Generate addresses based on client_id and counter
        let sender = format!("client{}_wallet", client_id);
        let recipient = format!("recipient{}", tx_counter % 100); // Cycle through 100 recipients
        
        match tx_type {
            TransactionType::Transfer => {
                TransactionTemplate {
                    sender: Some(sender),
                    recipient: Some(recipient),
                    amount: 1.0,
                    tx_type: TransactionType::Transfer,
                    additional_data: None,
                }
            }
            TransactionType::CreateNft => {
                let nft_data = serde_json::json!({
                    "name": format!("StressTest NFT #{}", tx_counter),
                    "description": "NFT created during stress test",
                    "creator": sender,
                    "metadata": {
                        "test_id": tx_counter,
                        "client_id": client_id,
                    }
                });
                
                TransactionTemplate {
                    sender: Some(sender),
                    recipient: None,
                    amount: 0.0,
                    tx_type: TransactionType::CreateNft,
                    additional_data: Some(nft_data),
                }
            }
            TransactionType::VerifyNft => {
                let nft_data = serde_json::json!({
                    "nft_id": format!("nft-{}", tx_counter % 1000), // Assume these NFTs exist
                    "verifier": sender,
                    "verification_data": {
                        "timestamp": chrono::Utc::now().timestamp(),
                        "score": tx_counter % 100,
                    }
                });
                
                TransactionTemplate {
                    sender: Some(sender),
                    recipient: None,
                    amount: 0.0,
                    tx_type: TransactionType::VerifyNft,
                    additional_data: Some(nft_data),
                }
            }
            TransactionType::Complex => {
                // Generate a complex transaction with multiple inputs/outputs
                let complex_data = serde_json::json!({
                    "inputs": [
                        { "address": sender.clone(), "amount": 1.0 },
                        { "address": format!("client{}_wallet", (client_id + 1) % 10), "amount": 0.5 }
                    ],
                    "outputs": [
                        { "address": recipient.clone(), "amount": 0.8 },
                        { "address": format!("recipient{}", (tx_counter + 1) % 100), "amount": 0.4 },
                        { "address": format!("recipient{}", (tx_counter + 2) % 100), "amount": 0.3 }
                    ]
                });
                
                TransactionTemplate {
                    sender: Some(sender),
                    recipient: Some(recipient),
                    amount: 1.5, // Total input amount
                    tx_type: TransactionType::Complex,
                    additional_data: Some(complex_data),
                }
            }
        }
    }
    
    // Submit a transaction to the API
    async fn submit_transaction(
        client: &Client,
        config: &StressTestConfig,
        template: TransactionTemplate,
    ) -> Result<u64> {
        let start_time = Instant::now();
        
        // Convert template to actual transaction based on type
        let transaction = match template.tx_type {
            TransactionType::Transfer => {
                serde_json::json!({
                    "sender": template.sender.unwrap_or_default(),
                    "recipient": template.recipient.unwrap_or_default(),
                    "amount": template.amount,
                    "type": "TRANSFER"
                })
            }
            TransactionType::CreateNft => {
                let mut data = serde_json::json!({
                    "creator": template.sender.unwrap_or_default(),
                    "type": "CREATE_NFT"
                });
                
                if let Some(additional) = template.additional_data {
                    // Merge additional data
                    if let Some(data_obj) = data.as_object_mut() {
                        if let Some(additional_obj) = additional.as_object() {
                            for (key, value) in additional_obj {
                                data_obj.insert(key.clone(), value.clone());
                            }
                        }
                    }
                }
                
                data
            }
            TransactionType::VerifyNft => {
                let mut data = serde_json::json!({
                    "verifier": template.sender.unwrap_or_default(),
                    "type": "VERIFY_NFT"
                });
                
                if let Some(additional) = template.additional_data {
                    // Merge additional data
                    if let Some(data_obj) = data.as_object_mut() {
                        if let Some(additional_obj) = additional.as_object() {
                            for (key, value) in additional_obj {
                                data_obj.insert(key.clone(), value.clone());
                            }
                        }
                    }
                }
                
                data
            }
            TransactionType::Complex => {
                // For complex transactions, use the additional data directly
                template.additional_data.unwrap_or(serde_json::json!({}))
            }
        };
        
        // Submit transaction to API
        let endpoint = format!("{}/transactions", config.api_url);
        
        let response = client
            .post(&endpoint)
            .json(&transaction)
            .send()
            .await
            .context("Failed to send transaction request")?;
            
        let status = response.status();
        
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Transaction failed with status {}: {}",
                status,
                error_text
            ));
        }
        
        // Calculate latency
        let latency = start_time.elapsed().as_millis() as u64;
        
        Ok(latency)
    }
    
    // Start metrics collection
    fn start_metrics_collection(&self) -> Option<tokio::task::JoinHandle<()>> {
        if !self.config.detailed_metrics {
            return None;
        }
        
        let api_url = self.config.api_url.clone();
        let client = self.client.clone();
        let stats = self.stats.clone();
        let running = self.running.clone();
        let interval = self.config.log_interval_seconds;
        
        let handle = tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(interval));
            
            while running.load(Ordering::SeqCst) {
                interval.tick().await;
                
                // Collect mempool size
                if let Ok(mempool_size) = Self::get_mempool_size(&client, &api_url).await {
                    let mut stats_lock = stats.lock().await;
                    stats_lock.mempool_sizes.push(mempool_size);
                }
                
                // Collect latest block size
                if let Ok(block_size) = Self::get_latest_block_size(&client, &api_url).await {
                    let mut stats_lock = stats.lock().await;
                    stats_lock.block_sizes.push(block_size);
                }
                
                // Log current progress
                let stats_lock = stats.lock().await;
                info!(
                    "Progress: {} submitted, {} successful, {} failed, {:.2} TPS",
                    stats_lock.transactions_submitted,
                    stats_lock.transactions_successful,
                    stats_lock.transactions_failed,
                    (stats_lock.transactions_successful as f64) / 
                        (stats_lock.duration_ms as f64 / 1000.0).max(0.001)
                );
            }
        });
        
        Some(handle)
    }
    
    // Get current memory usage
    async fn get_memory_usage(&self) -> Result<f64> {
        // In a real implementation, this would query the system for memory usage
        // For now, return a placeholder value
        Ok(100.0)
    }
    
    // Get current mempool size
    async fn get_mempool_size(client: &Client, api_url: &str) -> Result<usize> {
        let endpoint = format!("{}/transactions/pending", api_url);
        
        let response = client
            .get(&endpoint)
            .send()
            .await
            .context("Failed to get mempool size")?;
            
        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to get mempool size"));
        }
        
        let transactions: Vec<serde_json::Value> = response
            .json()
            .await
            .context("Failed to parse mempool response")?;
            
        Ok(transactions.len())
    }
    
    // Get latest block size
    async fn get_latest_block_size(client: &Client, api_url: &str) -> Result<usize> {
        let endpoint = format!("{}/blocks", api_url);
        
        let response = client
            .get(&endpoint)
            .send()
            .await
            .context("Failed to get blocks")?;
            
        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to get blocks"));
        }
        
        let blocks: Vec<serde_json::Value> = response
            .json()
            .await
            .context("Failed to parse blocks response")?;
            
        // Get the last block
        if let Some(last_block) = blocks.last() {
            if let Some(transactions) = last_block.get("transactions").and_then(|t| t.as_array()) {
                return Ok(transactions.len());
            }
        }
        
        Ok(0)
    }
}