//! Double Spending Attack Test for Wrfcoin
//!
//! This module implements tests to evaluate the blockchain's resistance to double-spending attacks.
//! It simulates attempts to spend the same funds multiple times to test the security of the
//! transaction validation and consensus mechanisms.

use anyhow::{Context, Result};
use log::{debug, info, warn};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time;

/// Configuration for double-spending tests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoubleSpendTestConfig {
    /// Target API URL
    pub api_url: String,
    
    /// Secondary node URL for network partition simulation
    pub secondary_node_url: Option<String>,
    
    /// Test wallet address with funds
    pub test_wallet_address: String,
    
    /// Private key for test wallet (if available)
    pub test_wallet_private_key: Option<String>,
    
    /// Password for test wallet (if using API-based signing)
    pub test_wallet_password: Option<String>,
    
    /// Amount to use in test transactions
    pub test_amount: f64,
    
    /// Whether to actually broadcast double-spend attempts
    /// When false, only simulates the attack
    pub broadcast_attempts: bool,
}

impl Default for DoubleSpendTestConfig {
    fn default() -> Self {
        Self {
            api_url: std::env::var("WRFCOIN_TEST_API_URL")
                .unwrap_or_else(|_| "http://localhost:8556".to_string()),
            secondary_node_url: None,
            test_wallet_address: "test_wallet".to_string(),
            test_wallet_private_key: None,
            test_wallet_password: None,
            test_amount: 0.1,
            broadcast_attempts: false,
        }
    }
}

/// Double-spending test results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoubleSpendTestResults {
    /// Test configuration
    pub config: DoubleSpendTestConfig,
    
    /// Whether the attack was successful
    pub attack_successful: bool,
    
    /// Number of successful double-spends
    pub successful_double_spends: usize,
    
    /// Number of attempted double-spends
    pub attempted_double_spends: usize,
    
    /// Details of successful attacks
    pub successful_attacks: Vec<DoubleSpendAttackDetails>,
    
    /// Test start time
    pub start_time: chrono::DateTime<chrono::Utc>,
    
    /// Test end time
    pub end_time: chrono::DateTime<chrono::Utc>,
    
    /// Identified vulnerabilities
    pub vulnerabilities: Vec<String>,
    
    /// Recommendations to prevent double-spending
    pub recommendations: Vec<String>,
}

/// Details of a double-spend attack
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoubleSpendAttackDetails {
    /// Transaction ID of the first transaction
    pub tx1_id: String,
    
    /// Transaction ID of the second transaction (double-spend)
    pub tx2_id: String,
    
    /// Recipient of the first transaction
    pub recipient1: String,
    
    /// Recipient of the second transaction
    pub recipient2: String,
    
    /// Amount spent
    pub amount: f64,
    
    /// Attack method used
    pub attack_method: DoubleSpendMethod,
    
    /// Whether the attack was successful
    pub successful: bool,
    
    /// Notes about the attack
    pub notes: String,
}

/// Methods for double-spending attacks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DoubleSpendMethod {
    /// Race attack (send two transactions in quick succession)
    RaceAttack,
    
    /// Finney attack (mine a transaction privately, then broadcast another)
    FinneyAttack,
    
    /// Network partition attack (exploit a network partition)
    NetworkPartitionAttack,
    
    /// 51% attack simulation (if mining control is available)
    Attack51Percent,
    
    /// Same transaction ID (exploit transaction malleability)
    TransactionMalleability,
    
    /// Replace-by-fee (use higher fee to replace existing transaction)
    ReplaceByFee,
}

/// Double-spending test runner
pub struct DoubleSpendTest {
    config: DoubleSpendTestConfig,
    client: Client,
    secondary_client: Option<Client>,
    results: DoubleSpendTestResults,
}

impl DoubleSpendTest {
    /// Create a new double-spending test runner
    pub fn new(config: DoubleSpendTestConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");
            
        let secondary_client = if config.secondary_node_url.is_some() {
            Some(
                Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("Failed to create secondary HTTP client")
            )
        } else {
            None
        };
        
        let results = DoubleSpendTestResults {
            config: config.clone(),
            attack_successful: false,
            successful_double_spends: 0,
            attempted_double_spends: 0,
            successful_attacks: Vec::new(),
            start_time: chrono::Utc::now(),
            end_time: chrono::Utc::now(),
            vulnerabilities: Vec::new(),
            recommendations: Vec::new(),
        };
        
        Self {
            config,
            client,
            secondary_client,
            results,
        }
    }
    
    /// Run the double-spending test
    pub async fn run_test(&mut self) -> Result<DoubleSpendTestResults> {
        info!("Starting double-spending test");
        
        self.results.start_time = chrono::Utc::now();
        
        // Verify test wallet exists and has funds
        self.verify_wallet_funds().await?;
        
        // Test race attack
        self.test_race_attack().await?;
        
        // Test Finney attack (if mining access is available)
        if self.has_mining_access().await? {
            self.test_finney_attack().await?;
        }
        
        // Test network partition attack (if secondary node is available)
        if self.config.secondary_node_url.is_some() {
            self.test_network_partition_attack().await?;
        }
        
        // Test transaction malleability
        self.test_transaction_malleability().await?;
        
        // Test replace-by-fee
        self.test_replace_by_fee().await?;
        
        // Update test results
        self.results.end_time = chrono::Utc::now();
        self.results.attack_successful = self.results.successful_double_spends > 0;
        
        // Add recommendations based on findings
        self.add_recommendations();
        
        // Generate report
        let report = self.generate_report();
        info!("\n{}", report);
        
        Ok(self.results.clone())
    }
    
    /// Verify the test wallet exists and has sufficient funds
    async fn verify_wallet_funds(&self) -> Result<()> {
        info!("Verifying test wallet funds");
        
        let url = format!("{}/balance/{}", self.config.api_url, self.config.test_wallet_address);
        
        let response = self.client.get(&url).send().await
            .context("Failed to check wallet balance")?;
            
        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to get wallet balance"));
        }
        
        let balance_data: serde_json::Value = response.json().await?;
        
        if let Some(balance) = balance_data.get("balance").and_then(|b| b.as_f64()) {
            if balance < self.config.test_amount * 3.0 {
                return Err(anyhow::anyhow!(
                    "Insufficient funds in test wallet. Need at least {} but only have {}",
                    self.config.test_amount * 3.0,
                    balance
                ));
            }
            
            info!("Test wallet has sufficient funds: {}", balance);
        } else {
            return Err(anyhow::anyhow!("Could not determine wallet balance"));
        }
        
        Ok(())
    }
    
    /// Check if the node provides mining access
    async fn has_mining_access(&self) -> Result<bool> {
        debug!("Checking for mining access");
        
        let url = format!("{}/mine", self.config.api_url);
        
        let data = serde_json::json!({
            "miner": self.config.test_wallet_address,
            "dryRun": true,  // Don't actually mine, just check if we can
        });
        
        let response = self.client.post(&url).json(&data).send().await;
        
        match response {
            Ok(resp) => {
                // If status is 2xx or if we get a specific error about mining parameters
                // but not about permission, we probably have mining access
                if resp.status().is_success() {
                    return Ok(true);
                }
                
                let text = resp.text().await.unwrap_or_default();
                if text.contains("parameters") || text.contains("dry run") || 
                   !text.contains("permission") && !text.contains("unauthorized") {
                    return Ok(true);
                }
                
                Ok(false)
            }
            Err(_) => Ok(false),
        }
    }
    
    /// Test race attack (sending two conflicting transactions in quick succession)
    async fn test_race_attack(&mut self) -> Result<()> {
        info!("Testing race attack");
        
        if !self.config.broadcast_attempts {
            info!("Skipping actual broadcast (simulation only)");
            return Ok(());
        }
        
        // Create two recipients
        let recipient1 = format!("race_test_recipient1_{}", chrono::Utc::now().timestamp());
        let recipient2 = format!("race_test_recipient2_{}", chrono::Utc::now().timestamp());
        
        // Create two transactions using the same funds
        let tx1 = self.create_transaction(&recipient1, self.config.test_amount)?;
        let tx2 = self.create_transaction(&recipient2, self.config.test_amount)?;
        
        // Broadcast both transactions nearly simultaneously
        let (tx1_result, tx2_result) = tokio::join!(
            self.broadcast_transaction(&tx1),
            self.broadcast_transaction(&tx2)
        );
        
        self.results.attempted_double_spends += 1;
        
        let mut attack_details = DoubleSpendAttackDetails {
            tx1_id: "unknown".to_string(),
            tx2_id: "unknown".to_string(),
            recipient1,
            recipient2,
            amount: self.config.test_amount,
            attack_method: DoubleSpendMethod::RaceAttack,
            successful: false,
            notes: String::new(),
        };
        
        // Check results
        if let Ok(tx1_id) = tx1_result {
            attack_details.tx1_id = tx1_id;
            
            if let Ok(tx2_id) = tx2_result {
                attack_details.tx2_id = tx2_id;
                
                // Wait a bit for transactions to be processed
                time::sleep(Duration::from_secs(2)).await;
                
                // Check if both transactions were accepted
                let (tx1_status, tx2_status) = tokio::join!(
                    self.check_transaction_status(&attack_details.tx1_id),
                    self.check_transaction_status(&attack_details.tx2_id)
                );
                
                if let (Ok(tx1_confirmed), Ok(tx2_confirmed)) = (tx1_status, tx2_status) {
                    if tx1_confirmed && tx2_confirmed {
                        // Both transactions confirmed - successful double-spend!
                        attack_details.successful = true;
                        attack_details.notes = "Both transactions were confirmed! Double-spend successful.".to_string();
                        self.results.successful_double_spends += 1;
                        
                        self.results.vulnerabilities.push(
                            "Vulnerable to race attack double-spending".to_string()
                        );
                    } else if tx1_confirmed {
                        attack_details.notes = "First transaction confirmed, second rejected. Protection working.".to_string();
                    } else if tx2_confirmed {
                        attack_details.notes = "Second transaction confirmed, first rejected. Protection working.".to_string();
                    } else {
                        attack_details.notes = "Both transactions rejected. Unusual result.".to_string();
                    }
                } else {
                    attack_details.notes = "Could not determine transaction status.".to_string();
                }
            } else {
                attack_details.notes = "Second transaction rejected immediately. Protection working.".to_string();
            }
        } else {
            attack_details.notes = "First transaction failed to broadcast.".to_string();
        }
        
        self.results.successful_attacks.push(attack_details);
        
        Ok(())
    }
    
    /// Test Finney attack (mine a transaction privately, then broadcast a conflicting one)
    async fn test_finney_attack(&mut self) -> Result<()> {
        info!("Testing Finney attack");
        
        if !self.config.broadcast_attempts {
            info!("Skipping actual broadcast (simulation only)");
            return Ok(());
        }
        
        // Create two recipients
        let recipient1 = format!("finney_test_recipient1_{}", chrono::Utc::now().timestamp());
        let recipient2 = format!("finney_test_recipient2_{}", chrono::Utc::now().timestamp());
        
        // Create two transactions using the same funds
        let tx1 = self.create_transaction(&recipient1, self.config.test_amount)?;
        let tx2 = self.create_transaction(&recipient2, self.config.test_amount)?;
        
        // Try to mine tx1 directly without broadcasting
        let mine_url = format!("{}/mine", self.config.api_url);
        
        let mine_data = serde_json::json!({
            "miner": self.config.test_wallet_address,
            "transactions": [tx1],
        });
        
        let mine_response = self.client.post(&mine_url).json(&mine_data).send().await;
        
        self.results.attempted_double_spends += 1;
        
        let mut attack_details = DoubleSpendAttackDetails {
            tx1_id: "unknown".to_string(),
            tx2_id: "unknown".to_string(),
            recipient1: recipient1.clone(),
            recipient2: recipient2.clone(),
            amount: self.config.test_amount,
            attack_method: DoubleSpendMethod::FinneyAttack,
            successful: false,
            notes: String::new(),
        };
        
        match mine_response {
            Ok(resp) => {
                if resp.status().is_success() {
                    let result: serde_json::Value = resp.json().await?;
                    
                    if let Some(block) = result.get("block") {
                        if let Some(txs) = block.get("transactions").and_then(|t| t.as_array()) {
                            if let Some(tx) = txs.iter().find(|t| {
                                t.get("recipient").and_then(|r| r.as_str()) == Some(&recipient1)
                            }) {
                                if let Some(tx_id) = tx.get("id").and_then(|id| id.as_str()) {
                                    attack_details.tx1_id = tx_id.to_string();
                                    
                                    // First transaction mined successfully, now broadcast the second one
                                    if let Ok(tx2_id) = self.broadcast_transaction(&tx2).await {
                                        attack_details.tx2_id = tx2_id;
                                        
                                        // Wait a bit for the transaction to be processed
                                        time::sleep(Duration::from_secs(2)).await;
                                        
                                        // Check if the second transaction was accepted
                                        if let Ok(tx2_confirmed) = self.check_transaction_status(&attack_details.tx2_id).await {
                                            if tx2_confirmed {
                                                // Both transactions confirmed - successful double-spend!
                                                attack_details.successful = true;
                                                attack_details.notes = "Finney attack successful! Both transactions confirmed.".to_string();
                                                self.results.successful_double_spends += 1;
                                                
                                                self.results.vulnerabilities.push(
                                                    "Vulnerable to Finney attack double-spending".to_string()
                                                );
                                            } else {
                                                attack_details.notes = "Second transaction rejected. Protection working.".to_string();
                                            }
                                        } else {
                                            attack_details.notes = "Could not determine second transaction status.".to_string();
                                        }
                                    } else {
                                        attack_details.notes = "Second transaction rejected immediately. Protection working.".to_string();
                                    }
                                }
                            }
                        }
                    }
                } else {
                    attack_details.notes = "Failed to mine first transaction privately.".to_string();
                }
            }
            Err(e) => {
                attack_details.notes = format!("Error during mining attempt: {}", e);
            }
        }
        
        self.results.successful_attacks.push(attack_details);
        
        Ok(())
    }
    
    /// Test network partition attack (exploit a network partition)
    async fn test_network_partition_attack(&mut self) -> Result<()> {
        info!("Testing network partition attack");
        
        if !self.config.broadcast_attempts {
            info!("Skipping actual broadcast (simulation only)");
            return Ok(());
        }
        
        if self.secondary_client.is_none() || self.config.secondary_node_url.is_none() {
            return Err(anyhow::anyhow!("Secondary node URL required for network partition test"));
        }
        
        // Create two recipients
        let recipient1 = format!("partition_test_recipient1_{}", chrono::Utc::now().timestamp());
        let recipient2 = format!("partition_test_recipient2_{}", chrono::Utc::now().timestamp());
        
        // Create two transactions using the same funds
        let tx1 = self.create_transaction(&recipient1, self.config.test_amount)?;
        let tx2 = self.create_transaction(&recipient2, self.config.test_amount)?;
        
        let secondary_client = self.secondary_client.as_ref().unwrap();
        let secondary_url = self.config.secondary_node_url.as_ref().unwrap();
        
        // Broadcast tx1 to primary node and tx2 to secondary node
        let tx1_url = format!("{}/transactions", self.config.api_url);
        let tx2_url = format!("{}/transactions", secondary_url);
        
        let tx1_resp = self.client.post(&tx1_url).json(&tx1).send().await;
        let tx2_resp = secondary_client.post(&tx2_url).json(&tx2).send().await;
        
        self.results.attempted_double_spends += 1;
        
        let mut attack_details = DoubleSpendAttackDetails {
            tx1_id: "unknown".to_string(),
            tx2_id: "unknown".to_string(),
            recipient1,
            recipient2,
            amount: self.config.test_amount,
            attack_method: DoubleSpendMethod::NetworkPartitionAttack,
            successful: false,
            notes: String::new(),
        };
        
        if let (Ok(tx1_response), Ok(tx2_response)) = (tx1_resp, tx2_resp) {
            if tx1_response.status().is_success() && tx2_response.status().is_success() {
                let tx1_result: serde_json::Value = tx1_response.json().await?;
                let tx2_result: serde_json::Value = tx2_response.json().await?;
                
                if let (Some(tx1_id), Some(tx2_id)) = (
                    tx1_result.get("id").and_then(|id| id.as_str()),
                    tx2_result.get("id").and_then(|id| id.as_str()),
                ) {
                    attack_details.tx1_id = tx1_id.to_string();
                    attack_details.tx2_id = tx2_id.to_string();
                    
                    // Mine some blocks on both nodes to confirm the transactions
                    let mine_url1 = format!("{}/mine", self.config.api_url);
                    let mine_url2 = format!("{}/mine", secondary_url);
                    
                    let mine_data = serde_json::json!({
                        "miner": self.config.test_wallet_address,
                    });
                    
                    // Mine on both nodes
                    for _ in 0..3 {
                        let _ = self.client.post(&mine_url1).json(&mine_data).send().await;
                        let _ = secondary_client.post(&mine_url2).json(&mine_data).send().await;
                        time::sleep(Duration::from_secs(1)).await;
                    }
                    
                    // Now try to reconnect the networks by "fixing" the partition
                    // (in a real scenario, this would happen naturally when the network heals)
                    let sync_url1 = format!("{}/sync", self.config.api_url);
                    let sync_url2 = format!("{}/sync", secondary_url);
                    
                    let sync_data1 = serde_json::json!({
                        "peer_url": secondary_url,
                    });
                    
                    let sync_data2 = serde_json::json!({
                        "peer_url": self.config.api_url,
                    });
                    
                    let _ = self.client.post(&sync_url1).json(&sync_data1).send().await;
                    let _ = secondary_client.post(&sync_url2).json(&sync_data2).send().await;
                    
                    // Wait for sync to complete
                    time::sleep(Duration::from_secs(5)).await;
                    
                    // Now check both transactions on both nodes
                    let (tx1_status_primary, tx2_status_primary) = tokio::join!(
                        self.check_transaction_status(&attack_details.tx1_id),
                        self.check_transaction_status(&attack_details.tx2_id)
                    );
                    
                    let tx1_url_secondary = format!("{}/transactions/{}", secondary_url, attack_details.tx1_id);
                    let tx2_url_secondary = format!("{}/transactions/{}", secondary_url, attack_details.tx2_id);
                    
                    let tx1_resp_secondary = secondary_client.get(&tx1_url_secondary).send().await;
                    let tx2_resp_secondary = secondary_client.get(&tx2_url_secondary).send().await;
                    
                    let tx1_status_secondary = tx1_resp_secondary.map(|r| r.status().is_success()).unwrap_or(false);
                    let tx2_status_secondary = tx2_resp_secondary.map(|r| r.status().is_success()).unwrap_or(false);
                    
                    // Analyze results
                    if let (Ok(tx1_confirmed_primary), Ok(tx2_confirmed_primary)) = (tx1_status_primary, tx2_status_primary) {
                        if (tx1_confirmed_primary && tx2_confirmed_primary) || 
                           (tx1_status_secondary && tx2_status_secondary) {
                            // Both transactions confirmed on at least one node - successful double-spend!
                            attack_details.successful = true;
                            attack_details.notes = "Network partition attack successful! Both transactions confirmed after reconnection.".to_string();
                            self.results.successful_double_spends += 1;
                            
                            self.results.vulnerabilities.push(
                                "Vulnerable to network partition attack double-spending".to_string()
                            );
                        } else if tx1_confirmed_primary && tx1_status_secondary {
                            attack_details.notes = "First transaction won in both partitions. Protection working.".to_string();
                        } else if tx2_confirmed_primary && tx2_status_secondary {
                            attack_details.notes = "Second transaction won in both partitions. Protection working.".to_string();
                        } else if tx1_confirmed_primary && tx2_status_secondary {
                            attack_details.notes = "Different transactions confirmed in different partitions. Fork resolution handled correctly.".to_string();
                        } else if tx2_confirmed_primary && tx1_status_secondary {
                            attack_details.notes = "Different transactions confirmed in different partitions. Fork resolution handled correctly.".to_string();
                        } else {
                            attack_details.notes = "Neither transaction was confirmed across partitions. Unusual result.".to_string();
                        }
                    } else {
                        attack_details.notes = "Could not determine transaction status after partition healing.".to_string();
                    }
                }
            } else {
                attack_details.notes = "Failed to broadcast transactions to partitioned nodes.".to_string();
            }
        } else {
            attack_details.notes = "Error during transaction broadcast to partitioned nodes.".to_string();
        }
        
        self.results.successful_attacks.push(attack_details);
        
        Ok(())
    }
    
    /// Test transaction malleability attack
    async fn test_transaction_malleability(&mut self) -> Result<()> {
        info!("Testing transaction malleability attack");
        
        if !self.config.broadcast_attempts {
            info!("Skipping actual broadcast (simulation only)");
            return Ok(());
        }
        
        // Create recipient
        let recipient = format!("malleability_test_recipient_{}", chrono::Utc::now().timestamp());
        
        // Create original transaction
        let tx = self.create_transaction(&recipient, self.config.test_amount)?;
        
        // Broadcast original transaction
        let tx_id_result = self.broadcast_transaction(&tx).await;
        
        self.results.attempted_double_spends += 1;
        
        let mut attack_details = DoubleSpendAttackDetails {
            tx1_id: "unknown".to_string(),
            tx2_id: "unknown".to_string(),
            recipient1: recipient.clone(),
            recipient2: recipient.clone(),
            amount: self.config.test_amount,
            attack_method: DoubleSpendMethod::TransactionMalleability,
            successful: false,
            notes: String::new(),
        };
        
        if let Ok(tx_id) = tx_id_result {
            attack_details.tx1_id = tx_id;
            
            // Create a slightly modified version of the same transaction
            // In a real attack, this would modify the signature without invalidating it
            // Here we'll just simulate by changing a non-essential field
            let mut malleable_tx = tx.clone();
            
            // Try to modify some field that won't affect validity
            if let Some(obj) = malleable_tx.as_object_mut() {
                // Add a comment field
                obj.insert("comment".to_string(), serde_json::Value::String("Modified transaction".to_string()));
                
                // Or modify the timestamp if it exists
                if let Some(timestamp) = obj.get_mut("timestamp") {
                    if let Some(ts) = timestamp.as_i64() {
                        *timestamp = serde_json::Value::Number(serde_json::Number::from(ts + 1));
                    }
                }
                
                // Or create a timestamp if it doesn't exist
                if !obj.contains_key("timestamp") {
                    obj.insert("timestamp".to_string(), serde_json::Value::Number(serde_json::Number::from(chrono::Utc::now().timestamp())));
                }
            }
            
            // Broadcast the malleable transaction
            if let Ok(malleable_tx_id) = self.broadcast_transaction(&malleable_tx).await {
                attack_details.tx2_id = malleable_tx_id;
                
                // Wait a bit for transactions to be processed
                time::sleep(Duration::from_secs(2)).await;
                
                // Check if both transactions were accepted
                let (tx1_status, tx2_status) = tokio::join!(
                    self.check_transaction_status(&attack_details.tx1_id),
                    self.check_transaction_status(&attack_details.tx2_id)
                );
                
                if let (Ok(tx1_confirmed), Ok(tx2_confirmed)) = (tx1_status, tx2_status) {
                    if tx1_confirmed && tx2_confirmed {
                        // Both transactions confirmed - successful malleability attack!
                        attack_details.successful = true;
                        attack_details.notes = "Transaction malleability attack successful! Both versions of the transaction were confirmed.".to_string();
                        self.results.successful_double_spends += 1;
                        
                        self.results.vulnerabilities.push(
                            "Vulnerable to transaction malleability attacks".to_string()
                        );
                    } else if tx1_confirmed {
                        attack_details.notes = "Original transaction confirmed, modified transaction rejected. Protection working.".to_string();
                    } else if tx2_confirmed {
                        attack_details.notes = "Modified transaction confirmed, original rejected. Protection working.".to_string();
                    } else {
                        attack_details.notes = "Both transactions rejected. Unusual result.".to_string();
                    }
                } else {
                    attack_details.notes = "Could not determine transaction status.".to_string();
                }
            } else {
                attack_details.notes = "Modified transaction rejected immediately. Protection working.".to_string();
            }
        } else {
            attack_details.notes = "Original transaction failed to broadcast.".to_string();
        }
        
        self.results.successful_attacks.push(attack_details);
        
        Ok(())
    }
    
    /// Test replace-by-fee attack
    async fn test_replace_by_fee(&mut self) -> Result<()> {
        info!("Testing replace-by-fee attack");
        
        if !self.config.broadcast_attempts {
            info!("Skipping actual broadcast (simulation only)");
            return Ok(());
        }
        
        // Create two recipients
        let recipient1 = format!("rbf_test_recipient1_{}", chrono::Utc::now().timestamp());
        let recipient2 = format!("rbf_test_recipient2_{}", chrono::Utc::now().timestamp());
        
        // Create transaction with low fee
        let mut tx1 = self.create_transaction(&recipient1, self.config.test_amount)?;
        
        // Set a low fee
        if let Some(obj) = tx1.as_object_mut() {
            obj.insert("fee".to_string(), serde_json::Value::Number(serde_json::Number::from_f64(0.0001).unwrap()));
        }
        
        // Broadcast first transaction
        let tx1_id_result = self.broadcast_transaction(&tx1).await;
        
        self.results.attempted_double_spends += 1;
        
        let mut attack_details = DoubleSpendAttackDetails {
            tx1_id: "unknown".to_string(),
            tx2_id: "unknown".to_string(),
            recipient1,
            recipient2: recipient2.clone(),
            amount: self.config.test_amount,
            attack_method: DoubleSpendMethod::ReplaceByFee,
            successful: false,
            notes: String::new(),
        };

        if let Ok(tx1_id) = tx1_id_result {
            attack_details.tx1_id = tx1_id;

            // Create second transaction with higher fee
            let mut tx2 = self.create_transaction(&recipient2, self.config.test_amount)?;
            
            // Set a higher fee
            if let Some(obj) = tx2.as_object_mut() {
                obj.insert("fee".to_string(), serde_json::Value::Number(serde_json::Number::from_f64(0.01).unwrap()));
                
                // Explicitly mark as replace-by-fee
                obj.insert("rbf".to_string(), serde_json::Value::Bool(true));
            }
            
            // Broadcast second transaction
            if let Ok(tx2_id) = self.broadcast_transaction(&tx2).await {
                attack_details.tx2_id = tx2_id;
                
                // Wait a bit for transactions to be processed
                time::sleep(Duration::from_secs(2)).await;
                
                // Check transaction status
                let (tx1_status, tx2_status) = tokio::join!(
                    self.check_transaction_status(&attack_details.tx1_id),
                    self.check_transaction_status(&attack_details.tx2_id)
                );
                
                if let (Ok(tx1_confirmed), Ok(tx2_confirmed)) = (tx1_status, tx2_status) {
                    if !tx1_confirmed && tx2_confirmed {
                        // First transaction replaced by second - RBF working
                        attack_details.notes = "Replace-by-fee successful. Higher fee transaction replaced lower fee transaction.".to_string();
                        
                        // Note: This isn't necessarily a vulnerability - RBF is a feature in some blockchains
                        // But we should note it, as it means 0-conf transactions aren't safe
                        self.results.vulnerabilities.push(
                            "Zero-confirmation transactions vulnerable due to replace-by-fee support".to_string()
                        );
                    } else if tx1_confirmed && !tx2_confirmed {
                        attack_details.notes = "Replace-by-fee rejected. Original transaction maintained.".to_string();
                    } else if tx1_confirmed && tx2_confirmed {
                        // Both confirmed - this is unusual and potentially a double-spend
                        attack_details.successful = true;
                        attack_details.notes = "Both transactions confirmed! Double-spend successful via replace-by-fee.".to_string();
                        self.results.successful_double_spends += 1;
                        
                        self.results.vulnerabilities.push(
                            "Vulnerable to replace-by-fee double-spending".to_string()
                        );
                    } else {
                        attack_details.notes = "Neither transaction confirmed. Unusual result.".to_string();
                    }
                } else {
                    attack_details.notes = "Could not determine transaction status.".to_string();
                }
            } else {
                attack_details.notes = "Second transaction rejected immediately. No replace-by-fee support.".to_string();
            }
        } else {
            attack_details.notes = "First transaction failed to broadcast.".to_string();
        }
        
        self.results.successful_attacks.push(attack_details);
        
        Ok(())
    }
    
    /// Create a transaction
    fn create_transaction(&self, recipient: &str, amount: f64) -> Result<serde_json::Value> {
        let tx = serde_json::json!({
            "sender": self.config.test_wallet_address,
            "recipient": recipient,
            "amount": amount,
            "fee": 0.001,
            "timestamp": chrono::Utc::now().timestamp(),
            "nonce": rand::random::<u64>(),
        });
        
        Ok(tx)
    }
    
    /// Broadcast a transaction
    async fn broadcast_transaction(&self, tx: &serde_json::Value) -> Result<String> {
        let url = format!("{}/transactions", self.config.api_url);
        
        let response = self.client.post(&url).json(tx).send().await
            .context("Failed to broadcast transaction")?;
            
        if !response.status().is_success() {
            let error = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Transaction rejected: {}", error));
        }
        
        let result: serde_json::Value = response.json().await?;
        
        if let Some(tx_id) = result.get("id").and_then(|id| id.as_str()) {
            Ok(tx_id.to_string())
        } else {
            Err(anyhow::anyhow!("No transaction ID in response"))
        }
    }
    
    /// Check transaction status
    async fn check_transaction_status(&self, tx_id: &str) -> Result<bool> {
        let url = format!("{}/transactions/{}", self.config.api_url, tx_id);
        
        let response = self.client.get(&url).send().await?;
        
        if !response.status().is_success() {
            return Ok(false);
        }
        
        let tx: serde_json::Value = response.json().await?;
        
        // Check if confirmed in a block
        if let Some(status) = tx.get("status").and_then(|s| s.as_str()) {
            return Ok(status == "confirmed" || status == "finalized");
        }
        
        // Alternative check for blockHash or blockHeight
        if tx.get("blockHash").is_some() || tx.get("blockHeight").is_some() {
            return Ok(true);
        }
        
        Ok(false)
    }
    
    /// Add recommendations based on test findings
    fn add_recommendations(&mut self) {
        // Basic recommendations for all blockchains
        self.results.recommendations.push(
            "Wait for multiple confirmations before considering high-value transactions final".to_string()
        );
        
        self.results.recommendations.push(
            "Implement a more robust consensus mechanism with finality guarantees".to_string()
        );
        
        // Add specific recommendations based on vulnerabilities
        if self.results.vulnerabilities.iter().any(|v| v.contains("race attack")) {
            self.results.recommendations.push(
                "Improve mempool transaction validation to detect double-spending attempts".to_string()
            );
            
            self.results.recommendations.push(
                "Implement a first-seen policy for conflicting transactions".to_string()
            );
        }
        
        if self.results.vulnerabilities.iter().any(|v| v.contains("Finney attack")) {
            self.results.recommendations.push(
                "Require more confirmations for high-value transactions".to_string()
            );
            
            self.results.recommendations.push(
                "Implement a more distributed mining model to prevent miner-controlled double-spends".to_string()
            );
        }
        
        if self.results.vulnerabilities.iter().any(|v| v.contains("network partition")) {
            self.results.recommendations.push(
                "Implement stronger network partition resistance with better fork resolution".to_string()
            );
            
            self.results.recommendations.push(
                "Consider using BFT-style consensus mechanisms for better partition tolerance".to_string()
            );
        }
        
        if self.results.vulnerabilities.iter().any(|v| v.contains("malleability")) {
            self.results.recommendations.push(
                "Use transaction formats that prevent malleability (e.g., Segregated Witness)".to_string()
            );
            
            self.results.recommendations.push(
                "Include all signed fields in the transaction hash calculation".to_string()
            );
        }
        
        if self.results.vulnerabilities.iter().any(|v| v.contains("replace-by-fee")) {
            self.results.recommendations.push(
                "Disable replace-by-fee for security-critical applications".to_string()
            );
            
            self.results.recommendations.push(
                "Wait for at least one confirmation before trusting a payment".to_string()
            );
        }
    }
    
    /// Generate a human-readable report
    pub fn generate_report(&self) -> String {
        let mut report = String::new();
        
        report.push_str("======= WRFCOIN DOUBLE-SPEND SECURITY REPORT =======\n\n");
        
        // Test information
        report.push_str(&format!("Test Start: {}\n", self.results.start_time));
        report.push_str(&format!("Test End: {}\n", self.results.end_time));
        report.push_str(&format!("Duration: {:.2} seconds\n", 
            (self.results.end_time - self.results.start_time).num_milliseconds() as f64 / 1000.0));
        report.push_str(&format!("Target: {}\n", self.config.api_url));
        
        if let Some(secondary_url) = &self.config.secondary_node_url {
            report.push_str(&format!("Secondary Target: {}\n", secondary_url));
        }
        
        report.push_str(&format!("Attempted attacks: {}\n", self.results.attempted_double_spends));
        report.push_str(&format!("Successful attacks: {}\n\n", self.results.successful_double_spends));
        
        // Vulnerabilities summary
        if !self.results.vulnerabilities.is_empty() {
            report.push_str("VULNERABILITIES FOUND:\n");
            for (i, vuln) in self.results.vulnerabilities.iter().enumerate() {
                report.push_str(&format!("{}. {}\n", i+1, vuln));
            }
            report.push_str("\n");
        } else {
            report.push_str("No vulnerabilities found in double-spend tests.\n\n");
        }
        
        // Attack details
        report.push_str("ATTACK DETAILS:\n\n");
        
        for (i, attack) in self.results.successful_attacks.iter().enumerate() {
            report.push_str(&format!("Attack {}: {:?}\n", i+1, attack.attack_method));
            report.push_str(&format!("  Recipient 1: {}\n", attack.recipient1));
            report.push_str(&format!("  Recipient 2: {}\n", attack.recipient2));
            report.push_str(&format!("  Transaction 1: {}\n", attack.tx1_id));
            report.push_str(&format!("  Transaction 2: {}\n", attack.tx2_id));
            report.push_str(&format!("  Amount: {}\n", attack.amount));
            report.push_str(&format!("  Successful: {}\n", attack.successful));
            report.push_str(&format!("  Notes: {}\n\n", attack.notes));
        }
        
        // Recommendations
        if !self.results.recommendations.is_empty() {
            report.push_str("RECOMMENDATIONS:\n");
            for (i, rec) in self.results.recommendations.iter().enumerate() {
                report.push_str(&format!("{}. {}\n", i+1, rec));
            }
            report.push_str("\n");
        }
        
        report.push_str("============= END OF REPORT =============\n");
        
        report
    }
}

/// Run a double-spending attack test
pub async fn run_double_spend_test(
    api_url: &str,
    broadcast_attempts: bool,
) -> Result<()> {
    info!("Starting double-spending test");

    let config = DoubleSpendTestConfig {
        api_url: api_url.to_string(),
        secondary_node_url: None, // Can be set if available
        test_wallet_address: "test_wallet".to_string(),
        test_wallet_private_key: None,
        test_wallet_password: None,
        test_amount: 0.1,
        broadcast_attempts,
    };

    let mut tester = DoubleSpendTest::new(config);
    let results = tester.run_test().await?;

    info!("Double-spending test completed");

    if results.attack_successful {
        warn!("Double-spending vulnerabilities found!");
    } else {
        info!("No double-spending vulnerabilities found");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_double_spend_config_default() {
        let config = DoubleSpendTestConfig::default();
        assert_eq!(config.test_wallet_address, "test_wallet");
        assert!(!config.broadcast_attempts);
    }

    #[test]
    fn test_double_spend_tester_construction() {
        let config = DoubleSpendTestConfig::default();
        let _tester = DoubleSpendTest::new(config);
    }

    #[test]
    fn test_double_spend_method_variants() {
        assert_ne!(DoubleSpendMethod::RaceAttack, DoubleSpendMethod::FinneyAttack);
        assert_ne!(
            DoubleSpendMethod::TransactionMalleability,
            DoubleSpendMethod::ReplaceByFee
        );
    }

    /// Live-node integration test. Gated behind WRFCOIN_SECURITY_TEST=1
    /// because it requires a reachable API (`http://localhost:8556`).
    #[tokio::test]
    #[ignore]
    async fn test_double_spend_live_simulation() {
        if std::env::var("WRFCOIN_SECURITY_TEST").ok().as_deref() != Some("1") {
            return;
        }
        run_double_spend_test("http://localhost:8556", false)
            .await
            .expect("double-spend simulation should not error in simulation mode");
    }
}