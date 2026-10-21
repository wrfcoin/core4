//! Network Partition Test for Wrfcoin
//!
//! This module implements tests to evaluate the blockchain's resilience to network partitions
//! and how it recovers from them. It simulates split-brain scenarios and measures
//! consensus recovery time.

use anyhow::{Context, Result};
use log::{debug, error, info, warn};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::{Duration, Instant};
use tokio::time;

/// Network node information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub id: String,
    pub address: String,
    pub peers: Vec<String>,
    pub region: Option<String>,
    pub node_type: String,
}

/// Partition configuration
#[derive(Debug, Clone)]
pub struct PartitionConfig {
    /// API URL of the control node
    pub control_node_url: String,
    
    /// Duration of the partition (seconds)
    pub partition_duration_seconds: u64,
    
    /// Duration of recovery period to monitor (seconds)
    pub recovery_duration_seconds: u64,
    
    /// Percentage of nodes to include in partition A (0-100)
    pub partition_a_percentage: u8,
    
    /// Whether to make partition A and B completely isolated
    pub complete_isolation: bool,
    
    /// Number of test transactions to send during partition
    pub test_transactions: usize,
}

impl Default for PartitionConfig {
    fn default() -> Self {
        Self {
            control_node_url: std::env::var("WRFCOIN_TEST_API_URL")
                .unwrap_or_else(|_| "http://localhost:8556".to_string()),
            partition_duration_seconds: 60,
            recovery_duration_seconds: 120,
            partition_a_percentage: 50,
            complete_isolation: true,
            test_transactions: 100,
        }
    }
}

/// Network partition test runner
pub struct NetworkPartitionTest {
    config: PartitionConfig,
    client: Client,
}

impl NetworkPartitionTest {
    /// Create a new network partition test
    pub fn new(config: PartitionConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");
            
        Self { config, client }
    }
    
    /// Run the network partition test
    pub async fn run(&self) -> Result<()> {
        info!("Starting network partition test");
        
        // Get list of network nodes
        let nodes = self.get_network_nodes().await?;
        
        if nodes.is_empty() {
            return Err(anyhow::anyhow!("No network nodes found"));
        }
        
        info!("Found {} nodes in the network", nodes.len());
        
        // Determine partition groups
        let (partition_a, partition_b) = self.create_partitions(&nodes)?;
        
        info!(
            "Created partitions: A ({} nodes), B ({} nodes)",
            partition_a.len(),
            partition_b.len()
        );
        
        // Collect initial metrics
        let initial_metrics = self.collect_network_metrics().await?;
        info!("Collected initial network metrics");
        
        // Create network partition
        self.create_network_partition(&partition_a, &partition_b).await?;
        info!("Network partition created");
        
        // Record partition start time
        let partition_start = Instant::now();
        
        // Send test transactions to both partitions
        let sender_a = partition_a.first().map(|n| n.id.clone()).unwrap_or_default();
        let sender_b = partition_b.first().map(|n| n.id.clone()).unwrap_or_default();
        let receiver_a = partition_a.last().map(|n| n.id.clone()).unwrap_or_default();
        let receiver_b = partition_b.last().map(|n| n.id.clone()).unwrap_or_default();
        
        info!("Sending test transactions during partition");
        
        // Send transactions to partition A
        let txs_a = self.send_test_transactions(
            &partition_a,
            sender_a,
            receiver_a,
            self.config.test_transactions / 2,
        ).await?;
        
        // Send transactions to partition B
        let txs_b = self.send_test_transactions(
            &partition_b,
            sender_b,
            receiver_b,
            self.config.test_transactions / 2,
        ).await?;
        
        info!(
            "Sent {} transactions to partition A and {} to partition B",
            txs_a.len(),
            txs_b.len()
        );
        
        // Wait for partition duration
        let wait_time = self.config.partition_duration_seconds as i64 - 
            partition_start.elapsed().as_secs() as i64;
            
        if wait_time > 0 {
            info!(
                "Waiting {} seconds to maintain partition",
                wait_time
            );
            time::sleep(Duration::from_secs(wait_time as u64)).await;
        }
        
        // Collect metrics during partition
        let partition_metrics = self.collect_network_metrics().await?;
        
        // Heal the network partition
        self.heal_network_partition(&partition_a, &partition_b).await?;
        info!("Network partition healed, monitoring recovery");
        
        // Record recovery start time
        let recovery_start = Instant::now();
        
        // Wait for recovery period while periodically checking consensus
        let mut recovery_metrics = Vec::new();
        let recovery_check_interval = Duration::from_secs(15); // Check every 15 seconds
        let total_checks = self.config.recovery_duration_seconds / 15;
        
        for i in 0..total_checks {
            time::sleep(recovery_check_interval).await;
            
            info!("Recovery monitoring: check {}/{}", i+1, total_checks);
            
            // Collect metrics during recovery
            let metrics = self.collect_network_metrics().await?;
            recovery_metrics.push(metrics);
            
            // Check transaction status in all nodes
            let tx_status = self.check_transaction_consensus(&txs_a, &txs_b).await?;
            info!(
                "Transaction consensus: {:.2}% agreement",
                tx_status.consensus_percentage
            );
            
            if tx_status.full_consensus {
                info!(
                    "Full consensus achieved after {} seconds",
                    recovery_start.elapsed().as_secs()
                );
                break;
            }
        }
        
        // Determine recovery time based on blockchain height convergence
        if let Some(recovery_time) = self.calculate_recovery_time(&recovery_metrics) {
            info!(
                "Network recovered from partition after approximately {} seconds",
                recovery_time.as_secs()
            );
        } else {
            warn!("Could not determine exact recovery time, network may still be recovering");
        }
        
        // Analyze and report results
        self.analyze_partition_results(
            initial_metrics,
            partition_metrics,
            recovery_metrics.last().cloned().unwrap_or_default(),
            txs_a,
            txs_b,
        ).await?;
        
        info!("Network partition test completed");
        
        Ok(())
    }
    
    /// Get all network nodes
    async fn get_network_nodes(&self) -> Result<Vec<NodeInfo>> {
        let endpoint = format!("{}/nodes", self.config.control_node_url);
        
        let response = self.client
            .get(&endpoint)
            .send()
            .await
            .context("Failed to get network nodes")?;
            
        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to get network nodes"));
        }
        
        let nodes: Vec<NodeInfo> = response
            .json()
            .await
            .context("Failed to parse network nodes response")?;
            
        Ok(nodes)
    }
    
    /// Create network partitions
    fn create_partitions(&self, nodes: &[NodeInfo]) -> Result<(Vec<NodeInfo>, Vec<NodeInfo>)> {
        let total_nodes = nodes.len();
        let partition_a_size = (total_nodes * self.config.partition_a_percentage as usize) / 100;
        
        // Ensure we have at least one node in each partition
        let partition_a_size = partition_a_size.max(1).min(total_nodes - 1);
        
        // Create the two partitions
        let partition_a = nodes[0..partition_a_size].to_vec();
        let partition_b = nodes[partition_a_size..].to_vec();
        
        Ok((partition_a, partition_b))
    }
    
    /// Create a network partition by blocking communication between partitions
    async fn create_network_partition(
        &self,
        partition_a: &[NodeInfo],
        partition_b: &[NodeInfo],
    ) -> Result<()> {
        // In a real implementation, this would configure the network to create
        // a partition between the nodes. For this test implementation, we'll
        // simulate the partition by calling a special API endpoint.
        
        let endpoint = format!("{}/testing/create-partition", self.config.control_node_url);
        
        let partition_request = serde_json::json!({
            "partition_a": partition_a.iter().map(|n| n.id.clone()).collect::<Vec<_>>(),
            "partition_b": partition_b.iter().map(|n| n.id.clone()).collect::<Vec<_>>(),
            "complete_isolation": self.config.complete_isolation,
        });
        
        let response = self.client
            .post(&endpoint)
            .json(&partition_request)
            .send()
            .await
            .context("Failed to create network partition")?;
            
        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to create network partition"));
        }
        
        Ok(())
    }
    
    /// Heal the network partition
    async fn heal_network_partition(
        &self,
        partition_a: &[NodeInfo],
        partition_b: &[NodeInfo],
    ) -> Result<()> {
        // Simulate healing the partition by calling a special API endpoint
        
        let endpoint = format!("{}/testing/heal-partition", self.config.control_node_url);
        
        let heal_request = serde_json::json!({
            "partition_a": partition_a.iter().map(|n| n.id.clone()).collect::<Vec<_>>(),
            "partition_b": partition_b.iter().map(|n| n.id.clone()).collect::<Vec<_>>(),
        });
        
        let response = self.client
            .post(&endpoint)
            .json(&heal_request)
            .send()
            .await
            .context("Failed to heal network partition")?;
            
        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to heal network partition"));
        }
        
        Ok(())
    }
    
    /// Send test transactions within a partition
    async fn send_test_transactions(
        &self,
        partition: &[NodeInfo],
        sender: String,
        recipient: String,
        count: usize,
    ) -> Result<Vec<String>> {
        let mut transaction_ids = Vec::with_capacity(count);
        
        // If partition is empty, return empty vector
        if partition.is_empty() {
            return Ok(transaction_ids);
        }
        
        // Choose a node from the partition to send transactions
        let node = &partition[0];
        let endpoint = format!("{}/transactions", node.address);
        
        for i in 0..count {
            let transaction = serde_json::json!({
                "sender": sender,
                "recipient": recipient,
                "amount": 0.001,
                "data": format!("partition-test-{}", i),
            });
            
            let response = self.client
                .post(&endpoint)
                .json(&transaction)
                .send()
                .await
                .context("Failed to send test transaction")?;
                
            if !response.status().is_success() {
                warn!("Failed to send test transaction {}", i);
                continue;
            }
            
            let result: serde_json::Value = response
                .json()
                .await
                .context("Failed to parse transaction response")?;
                
            if let Some(tx_id) = result.get("id").and_then(|id| id.as_str()) {
                transaction_ids.push(tx_id.to_string());
            }
        }
        
        Ok(transaction_ids)
    }
    
    /// Collect network metrics
    async fn collect_network_metrics(&self) -> Result<NetworkMetrics> {
        let endpoint = format!("{}/nodes/metrics", self.config.control_node_url);
        
        let response = self.client
            .get(&endpoint)
            .send()
            .await
            .context("Failed to get network metrics")?;
            
        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to get network metrics"));
        }
        
        let metrics: NetworkMetrics = response
            .json()
            .await
            .context("Failed to parse network metrics response")?;
            
        Ok(metrics)
    }
    
    /// Check transaction consensus across all nodes
    async fn check_transaction_consensus(
        &self,
        txs_a: &[String],
        txs_b: &[String],
    ) -> Result<TransactionConsensusStatus> {
        // Get all nodes
        let nodes = self.get_network_nodes().await?;
        
        // Combine all transaction IDs
        let mut all_txs = txs_a.to_vec();
        all_txs.extend(txs_b.iter().cloned());
        
        let mut tx_statuses = Vec::new();
        
        // Check each transaction in each node
        for tx_id in &all_txs {
            let mut confirmations = 0;
            let mut rejections = 0;
            let mut pending = 0;
            let mut unknown = 0;
            
            for node in &nodes {
                let endpoint = format!("{}/transactions/{}", node.address, tx_id);
                
                match self.client.get(&endpoint).send().await {
                    Ok(response) => {
                        if response.status().is_success() {
                            let tx_info: serde_json::Value = match response.json().await {
                                Ok(info) => info,
                                Err(_) => {
                                    unknown += 1;
                                    continue;
                                }
                            };
                            
                            if let Some(status) = tx_info.get("status").and_then(|s| s.as_str()) {
                                match status.to_lowercase().as_str() {
                                    "confirmed" => confirmations += 1,
                                    "rejected" => rejections += 1,
                                    "pending" => pending += 1,
                                    _ => unknown += 1,
                                }
                            } else {
                                unknown += 1;
                            }
                        } else {
                            unknown += 1;
                        }
                    }
                    Err(_) => {
                        unknown += 1;
                    }
                }
            }
            
            tx_statuses.push(TransactionStatus {
                id: tx_id.clone(),
                confirmations,
                rejections,
                pending,
                unknown,
                total_nodes: nodes.len(),
            });
        }
        
        // Calculate consensus metrics
        let total_transactions = tx_statuses.len();
        if total_transactions == 0 {
            return Ok(TransactionConsensusStatus {
                full_consensus: true,
                consensus_percentage: 100.0,
                transaction_statuses: Vec::new(),
            });
        }
        
        let mut full_consensus_count = 0;
        
        for status in &tx_statuses {
            // We consider "full consensus" when all nodes agree on the transaction status
            if status.confirmations == status.total_nodes || 
               status.rejections == status.total_nodes {
                full_consensus_count += 1;
            }
        }
        
        let consensus_percentage = 
            (full_consensus_count as f64 / total_transactions as f64) * 100.0;
            
        let full_consensus = full_consensus_count == total_transactions;
        
        Ok(TransactionConsensusStatus {
            full_consensus,
            consensus_percentage,
            transaction_statuses: tx_statuses,
        })
    }
    
    /// Calculate recovery time based on blockchain height convergence
    fn calculate_recovery_time(&self, metrics: &[NetworkMetrics]) -> Option<Duration> {
        if metrics.is_empty() {
            return None;
        }
        
        // We consider the network "recovered" when the blockchain height
        // variance is below a threshold (e.g., all nodes within 1 block)
        let threshold = 1;
        
        for (i, m) in metrics.iter().enumerate() {
            if m.blockchain_height_variance <= threshold {
                // Calculate time based on check interval
                let seconds = i as u64 * 15; // 15-second check interval
                return Some(Duration::from_secs(seconds));
            }
        }
        
        None
    }
    
    /// Analyze partition test results
    async fn analyze_partition_results(
        &self,
        initial_metrics: NetworkMetrics,
        partition_metrics: NetworkMetrics,
        recovery_metrics: NetworkMetrics,
        txs_a: Vec<String>,
        txs_b: Vec<String>,
    ) -> Result<()> {
        info!("=== Network Partition Test Analysis ===");
        
        // Analyze blockchain height variance
        info!(
            "Blockchain height variance: initial {}, during partition {}, after recovery {}",
            initial_metrics.blockchain_height_variance,
            partition_metrics.blockchain_height_variance,
            recovery_metrics.blockchain_height_variance
        );
        
        // Analyze network connectivity
        info!(
            "Network connectivity: initial {:.2}%, during partition {:.2}%, after recovery {:.2}%",
            initial_metrics.network_connectivity_percentage,
            partition_metrics.network_connectivity_percentage,
            recovery_metrics.network_connectivity_percentage
        );
        
        // Check final transaction status
        let tx_status = self.check_transaction_consensus(&txs_a, &txs_b).await?;
        
        info!(
            "Final transaction consensus: {:.2}% agreement",
            tx_status.consensus_percentage
        );
        
        // Check for potential double spends
        let double_spends = self.check_for_double_spends(&txs_a, &txs_b).await?;
        
        if double_spends.is_empty() {
            info!("No double spends detected during partition test");
        } else {
            error!("Detected {} potential double spends", double_spends.len());
            for ds in &double_spends {
                error!(
                    "Double spend: tx_a={}, tx_b={}, conflicting={}", 
                    ds.transaction_a,
                    ds.transaction_b,
                    ds.conflicting_outputs.join(", ")
                );
            }
        }
        
        // Final assessment
        if recovery_metrics.blockchain_height_variance <= 1 &&
           recovery_metrics.network_connectivity_percentage >= 95.0 &&
           tx_status.consensus_percentage >= 95.0 &&
           double_spends.is_empty() {
            info!("PASSED: Network successfully recovered from partition");
        } else {
            warn!("PARTIAL RECOVERY: Network may not have fully recovered from partition");
            
            if recovery_metrics.blockchain_height_variance > 1 {
                warn!("Blockchain height variance still significant");
            }
            
            if recovery_metrics.network_connectivity_percentage < 95.0 {
                warn!("Network connectivity not fully restored");
            }
            
            if tx_status.consensus_percentage < 95.0 {
                warn!("Transaction consensus not achieved");
            }
            
            if !double_spends.is_empty() {
                error!("Double spends detected, consensus mechanism may be compromised");
            }
        }
        
        info!("======================================");
        
        Ok(())
    }
    
    /// Check for double spends after partition
    async fn check_for_double_spends(
        &self,
        txs_a: &[String],
        txs_b: &[String],
    ) -> Result<Vec<DoubleSpendInfo>> {
        let mut double_spends = Vec::new();
        
        // Get detailed transaction info for all transactions
        let mut tx_details_a = Vec::new();
        for tx_id in txs_a {
            if let Ok(details) = self.get_transaction_details(tx_id).await {
                tx_details_a.push(details);
            }
        }
        
        let mut tx_details_b = Vec::new();
        for tx_id in txs_b {
            if let Ok(details) = self.get_transaction_details(tx_id).await {
                tx_details_b.push(details);
            }
        }
        
        // Compare each transaction from partition A with each from partition B
        for tx_a in &tx_details_a {
            for tx_b in &tx_details_b {
                let mut conflicting_outputs = Vec::new();
                
                // Check if any inputs are shared (potential double spend)
                for input_a in &tx_a.inputs {
                    for input_b in &tx_b.inputs {
                        if input_a == input_b {
                            conflicting_outputs.push(input_a.clone());
                        }
                    }
                }
                
                if !conflicting_outputs.is_empty() {
                    double_spends.push(DoubleSpendInfo {
                        transaction_a: tx_a.id.clone(),
                        transaction_b: tx_b.id.clone(),
                        conflicting_outputs,
                    });
                }
            }
        }
        
        Ok(double_spends)
    }
    
    /// Get detailed transaction information
    async fn get_transaction_details(&self, tx_id: &str) -> Result<TransactionDetails> {
        let endpoint = format!("{}/transactions/{}", self.config.control_node_url, tx_id);
        
        let response = self.client
            .get(&endpoint)
            .send()
            .await
            .context("Failed to get transaction details")?;
            
        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to get transaction details"));
        }
        
        let details: TransactionDetails = response
            .json()
            .await
            .context("Failed to parse transaction details response")?;
            
        Ok(details)
    }
}

/// Network metrics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetworkMetrics {
    /// Number of active nodes
    pub active_nodes: usize,
    
    /// Average blockchain height
    pub average_blockchain_height: u64,
    
    /// Variance in blockchain height across nodes
    pub blockchain_height_variance: u64,
    
    /// Average number of peers per node
    pub average_peers_per_node: f64,
    
    /// Network connectivity percentage (actual/possible connections)
    pub network_connectivity_percentage: f64,
    
    /// Average transactions in mempool
    pub average_mempool_size: f64,
}

/// Transaction status check
#[derive(Debug, Clone)]
pub struct TransactionStatus {
    /// Transaction ID
    pub id: String,
    
    /// Number of nodes that have confirmed this transaction
    pub confirmations: usize,
    
    /// Number of nodes that have rejected this transaction
    pub rejections: usize,
    
    /// Number of nodes where this transaction is pending
    pub pending: usize,
    
    /// Number of nodes where this transaction is unknown
    pub unknown: usize,
    
    /// Total number of nodes checked
    pub total_nodes: usize,
}

/// Transaction consensus status
#[derive(Debug, Clone)]
pub struct TransactionConsensusStatus {
    /// Whether full consensus has been achieved (all nodes agree)
    pub full_consensus: bool,
    
    /// Percentage of transactions that have consensus
    pub consensus_percentage: f64,
    
    /// Status of individual transactions
    pub transaction_statuses: Vec<TransactionStatus>,
}

/// Double spend information
#[derive(Debug, Clone)]
pub struct DoubleSpendInfo {
    /// First transaction involved in double spend
    pub transaction_a: String,
    
    /// Second transaction involved in double spend
    pub transaction_b: String,
    
    /// The outputs that both transactions tried to spend
    pub conflicting_outputs: Vec<String>,
}

/// Transaction details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionDetails {
    /// Transaction ID
    pub id: String,
    
    /// Transaction inputs (UTXOs being spent)
    pub inputs: Vec<String>,
    
    /// Transaction outputs
    pub outputs: Vec<TransactionOutput>,
    
    /// Transaction status
    pub status: String,
    
    /// Block hash (if confirmed)
    pub block_hash: Option<String>,
    
    /// Block height (if confirmed)
    pub block_height: Option<u64>,
}

/// Transaction output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionOutput {
    /// Output index
    pub index: u32,
    
    /// Recipient address
    pub address: String,
    
    /// Amount
    pub amount: f64,
}

/// Run a basic network partition test
pub async fn run_network_partition_test(
    control_node_url: &str,
    partition_duration_seconds: u64,
    recovery_duration_seconds: u64,
) -> Result<()> {
    info!("Starting basic network partition test");
    
    let config = PartitionConfig {
        control_node_url: control_node_url.to_string(),
        partition_duration_seconds,
        recovery_duration_seconds,
        partition_a_percentage: 50,
        complete_isolation: true,
        test_transactions: 50,
    };
    
    let test = NetworkPartitionTest::new(config);
    test.run().await?;
    
    Ok(())
}

/// Run a series of network partition tests with different configurations
pub async fn run_comprehensive_partition_tests(
    control_node_url: &str,
) -> Result<()> {
    info!("Starting comprehensive network partition tests");
    
    // Test 1: Short partition with equal split
    info!("Test 1: Short partition with equal split (50/50)");
    let config1 = PartitionConfig {
        control_node_url: control_node_url.to_string(),
        partition_duration_seconds: 30,
        recovery_duration_seconds: 60,
        partition_a_percentage: 50,
        complete_isolation: true,
        test_transactions: 20,
    };
    let test1 = NetworkPartitionTest::new(config1);
    test1.run().await?;
    
    // Brief cooldown
    time::sleep(Duration::from_secs(30)).await;
    
    // Test 2: Longer partition with unequal split
    info!("Test 2: Longer partition with unequal split (25/75)");
    let config2 = PartitionConfig {
        control_node_url: control_node_url.to_string(),
        partition_duration_seconds: 60,
        recovery_duration_seconds: 120,
        partition_a_percentage: 25,
        complete_isolation: true,
        test_transactions: 50,
    };
    let test2 = NetworkPartitionTest::new(config2);
    test2.run().await?;
    
    // Brief cooldown
    time::sleep(Duration::from_secs(30)).await;
    
    // Test 3: Partial partition (not complete isolation)
    info!("Test 3: Partial partition (not complete isolation)");
    let config3 = PartitionConfig {
        control_node_url: control_node_url.to_string(),
        partition_duration_seconds: 45,
        recovery_duration_seconds: 90,
        partition_a_percentage: 50,
        complete_isolation: false,
        test_transactions: 30,
    };
    let test3 = NetworkPartitionTest::new(config3);
    test3.run().await?;
    
    info!("Comprehensive network partition tests completed");
    
    Ok(())
}