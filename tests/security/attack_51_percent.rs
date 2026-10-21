//! 51% Attack Simulation for Wrfcoin
//!
//! This module implements a simulation of a 51% attack against the Wrfcoin blockchain.
//! It attempts to create a fork and reorganize the blockchain, potentially leading
//! to double-spending attacks.

use anyhow::{Context, Result};
use log::{info, warn};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time;

/// Configuration for 51% attack simulation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attack51PercentConfig {
    /// Target API URL
    pub api_url: String,
    
    /// Miner address to use for the attack
    pub miner_address: String,
    
    /// Test wallet address for double-spending
    pub test_wallet_address: String,
    
    /// Amount to use in test transactions
    pub test_amount: f64,
    
    /// Number of blocks to mine in the private fork
    pub private_fork_length: usize,
    
    /// Whether to actually broadcast the attack blocks
    /// When false, only simulates the attack
    pub broadcast_attack: bool,
}

impl Default for Attack51PercentConfig {
    fn default() -> Self {
        Self {
            api_url: std::env::var("WRFCOIN_TEST_API_URL")
                .unwrap_or_else(|_| "http://localhost:8556".to_string()),
            miner_address: "attacker_miner".to_string(),
            test_wallet_address: "attacker_wallet".to_string(),
            test_amount: 1.0,
            private_fork_length: 3,
            broadcast_attack: false,
        }
    }
}

/// 51% attack simulation results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attack51PercentResults {
    /// Test configuration
    pub config: Attack51PercentConfig,
    
    /// Whether the attack was successful
    pub attack_successful: bool,
    
    /// Original chain height before attack
    pub original_chain_height: u64,
    
    /// Private fork height
    pub private_fork_height: u64,
    
    /// Final chain height after attack
    pub final_chain_height: u64,
    
    /// Number of blocks in the private fork
    pub private_fork_blocks: usize,
    
    /// Number of blocks that were successfully reorged
    pub reorged_blocks: usize,
    
    /// Whether double-spending was successful
    pub double_spending_successful: bool,
    
    /// Test start time
    pub start_time: chrono::DateTime<chrono::Utc>,
    
    /// Test end time
    pub end_time: chrono::DateTime<chrono::Utc>,
    
    /// Estimated cost of the attack
    pub estimated_attack_cost: f64,
    
    /// Recommendations to prevent 51% attacks
    pub recommendations: Vec<String>,
}

/// Block information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockInfo {
    /// Block hash
    pub hash: String,
    
    /// Previous block hash
    pub prev_hash: String,
    
    /// Block height
    pub height: u64,
    
    /// Block timestamp
    pub timestamp: u64,
    
    /// Miner address
    pub miner: String,
    
    /// Block difficulty
    pub difficulty: f64,
    
    /// Transactions in the block
    pub transactions: Vec<TransactionInfo>,
}

/// Transaction information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionInfo {
    /// Transaction ID
    pub id: String,
    
    /// Sender address
    pub sender: String,
    
    /// Recipient address
    pub recipient: String,
    
    /// Transaction amount
    pub amount: f64,
    
    /// Transaction fee
    pub fee: f64,
    
    /// Transaction nonce
    pub nonce: u64,
}

/// 51% attack simulator
pub struct Attack51PercentSimulator {
    config: Attack51PercentConfig,
    client: Client,
    results: Attack51PercentResults,
    private_chain: Vec<BlockInfo>,
    main_chain_tip: Option<BlockInfo>,
}

impl Attack51PercentSimulator {
    /// Create a new 51% attack simulator
    pub fn new(config: Attack51PercentConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");
            
        let results = Attack51PercentResults {
            config: config.clone(),
            attack_successful: false,
            original_chain_height: 0,
            private_fork_height: 0,
            final_chain_height: 0,
            private_fork_blocks: 0,
            reorged_blocks: 0,
            double_spending_successful: false,
            start_time: chrono::Utc::now(),
            end_time: chrono::Utc::now(),
            estimated_attack_cost: 0.0,
            recommendations: Vec::new(),
        };
        
        Self {
            config,
            client,
            results,
            private_chain: Vec::new(),
            main_chain_tip: None,
        }
    }
    
    /// Run the 51% attack simulation
    pub async fn run_simulation(&mut self) -> Result<Attack51PercentResults> {
        info!("Starting 51% attack simulation");
        
        self.results.start_time = chrono::Utc::now();
        
        // Get the current blockchain state
        self.get_current_blockchain_state().await?;
        
        // Create a transaction to double-spend later
        let (tx_id, recipient) = self.create_initial_transaction().await?;
        
        // Mine the transaction in the main chain
        self.mine_transaction_in_main_chain(&tx_id).await?;
        
        // Build a private fork
        self.build_private_fork(recipient).await?;
        
        // Attempt to broadcast the private fork
        if self.config.broadcast_attack {
            self.broadcast_private_fork().await?;
        }
        
        // Check if the attack was successful
        self.verify_attack_success().await?;
        
        // Calculate the estimated cost
        self.calculate_attack_cost();
        
        // Add recommendations
        self.add_recommendations();
        
        // Update test results
        self.results.end_time = chrono::Utc::now();
        
        // Generate report
        let report = self.generate_report();
        info!("\n{}", report);
        
        Ok(self.results.clone())
    }
    
    /// Get the current blockchain state
    async fn get_current_blockchain_state(&mut self) -> Result<()> {
        info!("Getting current blockchain state");
        
        // Get the latest block
        let url = format!("{}/blocks/latest", self.config.api_url);
        
        let response = self.client.get(&url).send().await
            .context("Failed to get latest block")?;
            
        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to get latest block"));
        }
        
        let latest_block: BlockInfo = response.json().await?;
        
        self.results.original_chain_height = latest_block.height;
        self.main_chain_tip = Some(latest_block.clone());
        
        info!("Current blockchain height: {}", latest_block.height);
        
        Ok(())
    }
    
    /// Create an initial transaction to double-spend later
    async fn create_initial_transaction(&mut self) -> Result<(String, String)> {
        info!("Creating initial transaction");
        
        // Create a recipient address
        let recipient = format!("victim_{}", chrono::Utc::now().timestamp());
        
        // Create and broadcast a transaction
        let tx = serde_json::json!({
            "sender": self.config.test_wallet_address,
            "recipient": recipient,
            "amount": self.config.test_amount,
            "fee": 0.001,
            "nonce": 1,
        });
        
        let url = format!("{}/transactions", self.config.api_url);
        
        let response = self.client.post(&url).json(&tx).send().await
            .context("Failed to broadcast transaction")?;
            
        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to broadcast transaction"));
        }
        
        let result: serde_json::Value = response.json().await?;
        
        if let Some(tx_id) = result.get("id").and_then(|id| id.as_str()) {
            info!("Created transaction {} to recipient {}", tx_id, recipient);
            Ok((tx_id.to_string(), recipient))
        } else {
            Err(anyhow::anyhow!("No transaction ID in response"))
        }
    }
    
    /// Mine the transaction in the main chain
    async fn mine_transaction_in_main_chain(&mut self, tx_id: &str) -> Result<()> {
        info!("Mining transaction in main chain");
        
        // Mine a block with the transaction
        let url = format!("{}/mine", self.config.api_url);
        
        let mine_data = serde_json::json!({
            "miner": "honest_miner",
            "transactions": [tx_id],
        });
        
        let response = self.client.post(&url).json(&mine_data).send().await
            .context("Failed to mine block")?;
            
        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to mine block"));
        }
        
        // Get the new latest block
        let latest_url = format!("{}/blocks/latest", self.config.api_url);
        
        let latest_response = self.client.get(&latest_url).send().await
            .context("Failed to get latest block")?;
            
        if !latest_response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to get latest block"));
        }
        
        let latest_block: BlockInfo = latest_response.json().await?;
        
        // Check if our transaction is in the block
        let tx_in_block = latest_block.transactions.iter()
            .any(|tx| tx.id == tx_id);
            
        if tx_in_block {
            info!("Transaction {} mined in block {}", tx_id, latest_block.hash);
            self.main_chain_tip = Some(latest_block);
        } else {
            warn!("Transaction not found in latest block");
            // Try to mine another block
            let _ = self.client.post(&url).json(&mine_data).send().await;
            
            // Wait a moment
            time::sleep(Duration::from_secs(1)).await;
            
            // Try again to get the latest block
            let retry_response = self.client.get(&latest_url).send().await?;
            
            if retry_response.status().is_success() {
                let retry_block: BlockInfo = retry_response.json().await?;
                self.main_chain_tip = Some(retry_block);
            }
        }
        
        Ok(())
    }
    
    /// Build a private fork
    async fn build_private_fork(&mut self, _recipient: String) -> Result<()> {
        info!("Building private fork");
        
        // Start the fork from the block before our transaction
        let fork_point = self.main_chain_tip.as_ref()
            .map(|block| block.height - 1)
            .unwrap_or(0);
            
        // Get the block at the fork point
        let url = format!("{}/blocks/{}", self.config.api_url, fork_point);
        
        let response = self.client.get(&url).send().await
            .context("Failed to get fork point block")?;
            
        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to get fork point block"));
        }
        
        let fork_block: BlockInfo = response.json().await?;
        
        info!("Starting private fork from block {} at height {}", fork_block.hash, fork_block.height);
        
        // Create a double-spend transaction
        let double_spend_recipient = format!("attacker_{}", chrono::Utc::now().timestamp());
        
        let double_spend_tx = TransactionInfo {
            id: format!("double_spend_{}", chrono::Utc::now().timestamp()),
            sender: self.config.test_wallet_address.clone(),
            recipient: double_spend_recipient.clone(),
            amount: self.config.test_amount,
            fee: 0.002, // Higher fee
            nonce: 1,   // Same nonce as the original transaction
        };
        
        // Build a series of blocks for our private fork
        let mut prev_hash = fork_block.hash.clone();
        let mut height = fork_block.height;
        
        for i in 0..self.config.private_fork_length {
            height += 1;
            
            let mut transactions = Vec::new();
            
            // Include our double-spend in the first block
            if i == 0 {
                transactions.push(double_spend_tx.clone());
            }
            
            // Create a block
            let block = BlockInfo {
                hash: format!("private_block_{}", i),
                prev_hash,
                height,
                timestamp: chrono::Utc::now().timestamp() as u64,
                miner: self.config.miner_address.clone(),
                difficulty: 1.0,
                transactions,
            };
            
            info!("Created private block at height {}", block.height);
            
            self.private_chain.push(block.clone());
            prev_hash = block.hash.clone();
        }
        
        self.results.private_fork_blocks = self.private_chain.len();
        self.results.private_fork_height = height;
        
        info!("Private fork built with {} blocks, reaching height {}", 
            self.private_chain.len(), height);
            
        Ok(())
    }
    
    /// Broadcast the private fork
    async fn broadcast_private_fork(&mut self) -> Result<()> {
        info!("Broadcasting private fork");
        
        // In a real attack, the attacker would have mined actual valid blocks
        // and would broadcast them to the network. Here we'll simulate this
        // by making API calls to submit our blocks.
        
        let url = format!("{}/blocks", self.config.api_url);
        
        // Submit blocks in order
        for block in &self.private_chain {
            let response = self.client.post(&url).json(block).send().await;
            
            match response {
                Ok(resp) => {
                    if resp.status().is_success() {
                        info!("Successfully broadcast block {}", block.hash);
                    } else {
                        let error = resp.text().await.unwrap_or_default();
                        warn!("Failed to broadcast block {}: {}", block.hash, error);
                    }
                }
                Err(e) => {
                    warn!("Error broadcasting block {}: {}", block.hash, e);
                }
            }
            
            // Small delay between blocks
            time::sleep(Duration::from_millis(500)).await;
        }
        
        Ok(())
    }
    
    /// Verify if the attack was successful
    async fn verify_attack_success(&mut self) -> Result<()> {
        info!("Verifying attack success");
        
        // Get the current chain tip
        let url = format!("{}/blocks/latest", self.config.api_url);
        
        let response = self.client.get(&url).send().await
            .context("Failed to get latest block")?;
            
        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to get latest block"));
        }
        
        let latest_block: BlockInfo = response.json().await?;
        
        self.results.final_chain_height = latest_block.height;
        
        // Check if our private fork was accepted
        let fork_accepted = self.private_chain.iter()
            .any(|block| block.hash == latest_block.hash);
            
        if fork_accepted {
            info!("Attack successful! Fork accepted.");
            self.results.attack_successful = true;
            
            // Calculate how many blocks were reorged
            let reorged_blocks = latest_block.height - self.results.original_chain_height + 1;
            self.results.reorged_blocks = reorged_blocks as usize;
            
            // Check if double-spending was successful
            let double_spend_tx = self.private_chain[0].transactions.iter()
                .find(|tx| tx.sender == self.config.test_wallet_address);
                
            if let Some(tx) = double_spend_tx {
                // Verify if this transaction is confirmed
                let tx_url = format!("{}/transactions/{}", self.config.api_url, tx.id);
                
                let tx_response = self.client.get(&tx_url).send().await;
                
                if let Ok(resp) = tx_response {
                    if resp.status().is_success() {
                        info!("Double-spend transaction confirmed!");
                        self.results.double_spending_successful = true;
                    }
                }
            }
        } else {
            info!("Attack failed. Fork not accepted.");
        }
        
        Ok(())
    }
    
    /// Calculate the estimated cost of the attack
    fn calculate_attack_cost(&mut self) {
        // In a real-world scenario, the cost would depend on:
        // 1. The hashrate needed (>50% of network)
        // 2. The electricity cost
        // 3. The hardware cost
        // 4. The opportunity cost of mining honestly
        
        // This is a simplified calculation
        let network_hashrate = 100.0; // Hypothetical hashrate in MH/s
        let attack_hashrate = network_hashrate * 0.51; // 51% of network
        let electricity_cost_per_kwh = 0.10; // $0.10 per kWh
        let power_consumption_per_mh = 0.002; // 2W per MH/s
        
        // Calculate power consumption
        let power_consumption = attack_hashrate * power_consumption_per_mh; // in kW
        
        // Calculate attack duration
        let duration_hours = (self.results.end_time - self.results.start_time).num_seconds() as f64 / 3600.0;
        
        // Calculate electricity cost
        let electricity_cost = power_consumption * duration_hours * electricity_cost_per_kwh;
        
        // Add hardware costs (very simplified)
        let hardware_cost = attack_hashrate * 10.0; // $10 per MH/s
        
        // Total cost
        let total_cost = electricity_cost + hardware_cost;
        
        self.results.estimated_attack_cost = total_cost;
        
        info!("Estimated attack cost: ${:.2}", total_cost);
    }
    
    /// Add recommendations to prevent 51% attacks
    fn add_recommendations(&mut self) {
        self.results.recommendations.push(
            "Increase the number of confirmations required for large transactions".to_string()
        );
        
        self.results.recommendations.push(
            "Implement checkpointing to prevent deep reorganizations".to_string()
        );
        
        self.results.recommendations.push(
            "Consider implementing a consensus mechanism less susceptible to 51% attacks, such as Proof of Stake".to_string()
        );
        
        self.results.recommendations.push(
            "Monitor large hashrate increases on the network".to_string()
        );
        
        self.results.recommendations.push(
            "Implement a delayed withdrawal mechanism for large amounts".to_string()
        );
        
        if self.results.attack_successful {
            self.results.recommendations.push(
                "URGENT: Increase the difficulty of chain reorganization by implementing penalties for large reorgs".to_string()
            );
        }
    }
    
    /// Generate a human-readable report
    pub fn generate_report(&self) -> String {
        let mut report = String::new();
        
        report.push_str("======= WRFCOIN 51% ATTACK SIMULATION REPORT =======\n\n");
        
        // Test information
        report.push_str(&format!("Test Start: {}\n", self.results.start_time));
        report.push_str(&format!("Test End: {}\n", self.results.end_time));
        report.push_str(&format!("Duration: {:.2} seconds\n", 
            (self.results.end_time - self.results.start_time).num_milliseconds() as f64 / 1000.0));
        report.push_str(&format!("Target: {}\n\n", self.config.api_url));
        
        // Attack results
        report.push_str(&format!("Attack successful: {}\n", self.results.attack_successful));
        report.push_str(&format!("Double-spending successful: {}\n", self.results.double_spending_successful));
        report.push_str(&format!("Original chain height: {}\n", self.results.original_chain_height));
        report.push_str(&format!("Private fork height: {}\n", self.results.private_fork_height));
        report.push_str(&format!("Final chain height: {}\n", self.results.final_chain_height));
        report.push_str(&format!("Private fork blocks: {}\n", self.results.private_fork_blocks));
        report.push_str(&format!("Reorged blocks: {}\n", self.results.reorged_blocks));
        report.push_str(&format!("Estimated attack cost: ${:.2}\n\n", self.results.estimated_attack_cost));
        
        // Private chain details
        report.push_str("PRIVATE CHAIN DETAILS:\n");
        for (i, block) in self.private_chain.iter().enumerate() {
            report.push_str(&format!("Block {}: Height {}, Hash {}\n", i+1, block.height, block.hash));
            report.push_str(&format!("  Transactions: {}\n", block.transactions.len()));
            
            for tx in &block.transactions {
                report.push_str(&format!("    tx_id: {}, sender: {}, recipient: {}, amount: {}\n", 
                    tx.id, tx.sender, tx.recipient, tx.amount));
            }
        }
        report.push_str("\n");
        
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

/// Run a 51% attack simulation
pub async fn run_51_percent_attack(
    api_url: &str,
    broadcast_attack: bool,
) -> Result<()> {
    info!("Starting 51% attack simulation");

    let config = Attack51PercentConfig {
        api_url: api_url.to_string(),
        miner_address: "attacker_miner".to_string(),
        test_wallet_address: "test_wallet".to_string(),
        test_amount: 1.0,
        private_fork_length: 3,
        broadcast_attack,
    };

    let mut simulator = Attack51PercentSimulator::new(config);
    let results = simulator.run_simulation().await?;

    info!("51% attack simulation completed");

    if results.attack_successful {
        warn!("Blockchain vulnerable to 51% attacks!");

        if results.double_spending_successful {
            warn!("Double-spending attack successful!");
        }
    } else {
        info!("Blockchain appears resistant to simulated 51% attack");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_51_percent_config_default() {
        let config = Attack51PercentConfig::default();
        assert_eq!(config.api_url, "http://localhost:8556");
        assert_eq!(config.miner_address, "attacker_miner");
        assert_eq!(config.private_fork_length, 3);
        assert!(!config.broadcast_attack);
    }

    #[test]
    fn test_51_percent_simulator_construction() {
        let config = Attack51PercentConfig::default();
        let _sim = Attack51PercentSimulator::new(config);
    }

    /// Live-node integration test. Gated behind WRFCOIN_SECURITY_TEST=1
    /// because it requires a reachable API (`http://localhost:8556`).
    #[tokio::test]
    #[ignore]
    async fn test_51_percent_live_simulation() {
        if std::env::var("WRFCOIN_SECURITY_TEST").ok().as_deref() != Some("1") {
            return;
        }
        run_51_percent_attack("http://localhost:8556", false)
            .await
            .expect("51% attack simulation should not error in simulation mode");
    }
}