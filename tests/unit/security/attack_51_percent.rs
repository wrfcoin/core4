//! 51% Attack Simulation for Wrfcoin
//!
//! This module implements a comprehensive simulation of a 51% attack against the Wrfcoin blockchain.
//! It attempts to create a fork and reorganize the blockchain, potentially leading
//! to double-spending attacks. The simulation includes multiple attack vectors and scenarios,
//! with detailed analytics on blockchain resistance to attacks.
//!
//! Enhanced version implements:
//! 1. Multi-stage attack simulation with escalating control percentage
//! 2. Attack persistence analysis measuring recovery time
//! 3. Network topology impact assessment
//! 4. Multiple double-spend confirmations with variable finality windows
//! 5. Cost/reward analysis based on real-world attack economics

use anyhow::{anyhow, Context, Result};
use log::{debug, error, info, trace, warn};
use reqwest::{Client, ClientBuilder};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::task;
use tokio::time;

/// Control percentage representing the attacker's hashpower
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum ControlPercentage {
    /// Below 33% - generally considered safe
    Minor(f64),
    /// 33-50% - can cause significant disruptions
    Moderate(f64),
    /// 51-66% - can perform reorganizations
    Majority(f64),
    /// Above 66% - full control (can halt the chain)
    Supermajority(f64),
}

impl ControlPercentage {
    /// Get the numerical percentage
    pub fn value(&self) -> f64 {
        match self {
            Self::Minor(v) => *v,
            Self::Moderate(v) => *v,
            Self::Majority(v) => *v,
            Self::Supermajority(v) => *v,
        }
    }
    
    /// Get a description of the attack capability at this percentage
    pub fn description(&self) -> String {
        match self {
            Self::Minor(_) => "Limited disruption capability".to_string(),
            Self::Moderate(_) => "Significant disruption capability, potential selfish mining".to_string(),
            Self::Majority(_) => "Chain reorganization capability, double-spending possible".to_string(),
            Self::Supermajority(_) => "Full chain control, transaction censorship possible".to_string(),
        }
    }
}

/// Attack propagation strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttackPropagationStrategy {
    /// Immediately attempt to propagate the attack blocks
    Immediate,
    /// Wait until the private chain is longer before propagating
    Delayed,
    /// Propagate selectively to specific nodes
    Selective,
    /// Gradually release blocks to avoid detection
    Gradual,
}

/// Optimal attack parameters based on control percentage and network conditions
#[derive(Debug, Clone)]
pub struct OptimalAttackParameters {
    /// Optimal number of blocks to mine in private fork
    pub private_fork_length: usize,
    
    /// Optimal wait time before broadcasting the fork in seconds
    pub wait_time_seconds: u64,
}

/// Attack detection information
#[derive(Debug, Clone)]
pub struct AttackDetectionInfo {
    /// Whether the attack was detected
    pub detected: bool,
    
    /// Detection method used
    pub detection_method: String,
    
    /// Time to detection in seconds
    pub time_to_detection: f64,
}

/// Countermeasure information
#[derive(Debug, Clone)]
pub struct CountermeasureInfo {
    /// Whether countermeasures were implemented
    pub implemented: bool,
    
    /// Effectiveness of the countermeasures (0-100%)
    pub effectiveness: f64,
    
    /// Time to implement countermeasures in seconds
    pub time_to_implement: f64,
}

/// Attack feasibility analysis
#[derive(Debug, Clone)]
pub struct AttackFeasibility {
    /// Probability of successful reorganization (0.0-1.0)
    pub success_probability: f64,
    
    /// Probability of successful double-spend (0.0-1.0)
    pub double_spend_probability: f64,
    
    /// Estimated cost of the attack in USD
    pub estimated_cost: f64,
    
    /// Hashpower required as a percentage of network total
    pub hashpower_required: f64,
}

/// Configuration for 51% attack simulation
#[derive(Debug, Clone)]
pub struct Attack51PercentConfig {
    /// Target API URL
    pub api_url: String,
    
    /// Secondary node URLs for topology testing
    pub secondary_nodes: Vec<String>,
    
    /// Miner address to use for the attack
    pub miner_address: String,
    
    /// Test wallet address for double-spending
    pub test_wallet_address: String,
    
    /// Amount to use in test transactions
    pub test_amount: f64,
    
    /// Number of blocks to mine in the private fork
    pub private_fork_length: usize,
    
    /// Control percentage representing the attacker's hashpower
    pub control_percentage: ControlPercentage,
    
    /// Propagation strategy for the attack
    pub propagation_strategy: AttackPropagationStrategy,
    
    /// Number of confirmation blocks to wait for victim transaction
    pub victim_confirmation_blocks: usize,
    
    /// Whether to attempt multiple double-spends
    pub multiple_double_spends: bool,
    
    /// Number of attack attempts to perform
    pub attack_attempts: usize,
    
    /// Time between attack attempts in seconds
    pub time_between_attempts_seconds: u64,
    
    /// Whether to actually broadcast the attack blocks
    /// When false, only simulates the attack
    pub broadcast_attack: bool,
    
    /// Whether to implement counter-measures after detecting attacks
    pub implement_countermeasures: bool,
    
    /// Specific finality algorithm to test against
    pub finality_algorithm: Option<String>,
    
    /// Cost per block in the attack (for economic analysis)
    pub cost_per_block: f64,
}

impl Default for Attack51PercentConfig {
    fn default() -> Self {
        Self {
            api_url: std::env::var("WRFCOIN_TEST_API_URL")
                .unwrap_or_else(|_| "http://localhost:8556".to_string()),
            secondary_nodes: Vec::new(),
            miner_address: "attacker_miner".to_string(),
            test_wallet_address: "attacker_wallet".to_string(),
            test_amount: 1.0,
            private_fork_length: 6,
            control_percentage: ControlPercentage::Majority(51.0),
            propagation_strategy: AttackPropagationStrategy::Delayed,
            victim_confirmation_blocks: 6,
            multiple_double_spends: true,
            attack_attempts: 3,
            time_between_attempts_seconds: 60,
            broadcast_attack: false,
            implement_countermeasures: true,
            finality_algorithm: None,
            cost_per_block: 10.0,
        }
    }
}

/// Information about an individual attack attempt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttackAttemptInfo {
    /// Sequence number of this attempt
    pub attempt_number: usize,
    
    /// Control percentage used for this attempt
    pub control_percentage: ControlPercentage,
    
    /// Whether the attack was successful
    pub successful: bool,
    
    /// Number of blocks in the private fork
    pub private_fork_blocks: usize,
    
    /// Number of blocks that were successfully reorged
    pub reorged_blocks: usize,
    
    /// Whether double-spending was successful
    pub double_spending_successful: bool,
    
    /// Time when this attempt started
    pub start_time: chrono::DateTime<chrono::Utc>,
    
    /// Time when this attempt completed
    pub end_time: chrono::DateTime<chrono::Utc>,
    
    /// Original transaction ID (victim transaction)
    pub original_tx_id: String,
    
    /// Double-spend transaction ID
    pub double_spend_tx_id: String,
    
    /// Network state during the attack
    pub network_state: Option<NetworkState>,
    
    /// Detailed log of events during the attack
    pub events: Vec<AttackEvent>,
    
    /// Notes about this attempt
    pub notes: String,
}

/// Network state during an attack
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkState {
    /// Current hashrate of the network
    pub hashrate: f64,
    
    /// Current difficulty of the network
    pub difficulty: f64,
    
    /// Number of active nodes in the network
    pub active_nodes: usize,
    
    /// Number of pending transactions in the mempool
    pub pending_transactions: usize,
    
    /// Average block time in seconds
    pub avg_block_time: f64,
    
    /// Total hashpower controlled by the attacker
    pub attacker_hashpower: f64,
}

/// Event during an attack
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttackEvent {
    /// Time when this event occurred
    pub timestamp: chrono::DateTime<chrono::Utc>,
    
    /// Type of event
    pub event_type: String,
    
    /// Description of the event
    pub description: String,
    
    /// Associated block height (if applicable)
    pub block_height: Option<u64>,
    
    /// Associated transaction ID (if applicable)
    pub transaction_id: Option<String>,
    
    /// Success/failure status of this event
    pub success: bool,
}

/// Optimal attack parameters based on control percentage and network conditions
#[derive(Debug, Clone)]
pub struct OptimalAttackParameters {
    /// Optimal number of blocks to mine in private fork
    pub private_fork_length: usize,
    
    /// Optimal wait time before broadcasting the fork in seconds
    pub wait_time_seconds: u64,
}

/// Attack detection information
#[derive(Debug, Clone)]
pub struct AttackDetectionInfo {
    /// Whether the attack was detected
    pub detected: bool,
    
    /// Detection method used
    pub detection_method: String,
    
    /// Time to detection in seconds
    pub time_to_detection: f64,
}

/// Countermeasure information
#[derive(Debug, Clone)]
pub struct CountermeasureInfo {
    /// Whether countermeasures were implemented
    pub implemented: bool,
    
    /// Effectiveness of the countermeasures (0-100%)
    pub effectiveness: f64,
    
    /// Time to implement countermeasures in seconds
    pub time_to_implement: f64,
}

/// Attack feasibility analysis
#[derive(Debug, Clone)]
pub struct AttackFeasibility {
    /// Probability of successful reorganization (0.0-1.0)
    pub success_probability: f64,
    
    /// Probability of successful double-spend (0.0-1.0)
    pub double_spend_probability: f64,
    
    /// Estimated cost of the attack in USD
    pub estimated_cost: f64,
    
    /// Hashpower required as a percentage of network total
    pub hashpower_required: f64,
}

/// Economic analysis of the attack
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttackEconomics {
    /// Total cost of the attack in USD
    pub total_cost_usd: f64,
    
    /// Breakdown of costs by category
    pub cost_breakdown: std::collections::HashMap<String, f64>,
    
    /// Potential profit from double-spending
    pub potential_profit: f64,
    
    /// Return on investment (ROI) percentage
    pub roi_percentage: f64,
    
    /// Estimated time to break even
    pub break_even_time: String,
    
    /// Whether the attack is economically viable
    pub economically_viable: bool,
    
    /// Minimum amount needed for profitable double-spend
    pub min_profitable_amount: f64,
}

/// Security response metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityMetrics {
    /// Time to detect the attack in seconds
    pub detection_time_seconds: f64,
    
    /// Time to respond to the attack in seconds
    pub response_time_seconds: f64,
    
    /// Effectiveness of countermeasures (0-100%)
    pub countermeasure_effectiveness: f64,
    
    /// Recovery time after the attack in seconds
    pub recovery_time_seconds: f64,
    
    /// Whether the attack was preventable
    pub preventable: bool,
    
    /// Overall security score (0-100)
    pub security_score: f64,
}

/// 51% attack simulation results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attack51PercentResults {
    /// Test configuration
    pub config: Attack51PercentConfig,
    
    /// Whether any attack attempt was successful
    pub attack_successful: bool,
    
    /// Original chain height before attack
    pub original_chain_height: u64,
    
    /// Private fork height (from the last attempt)
    pub private_fork_height: u64,
    
    /// Final chain height after attack
    pub final_chain_height: u64,
    
    /// Number of blocks in the private fork (from the last attempt)
    pub private_fork_blocks: usize,
    
    /// Number of blocks that were successfully reorged (from the last attempt)
    pub reorged_blocks: usize,
    
    /// Whether double-spending was successful in any attempt
    pub double_spending_successful: bool,
    
    /// Test start time
    pub start_time: chrono::DateTime<chrono::Utc>,
    
    /// Test end time
    pub end_time: chrono::DateTime<chrono::Utc>,
    
    /// Information about each attack attempt
    pub attack_attempts: Vec<AttackAttemptInfo>,
    
    /// Estimated cost of the attack
    pub estimated_attack_cost: f64,
    
    /// Economic analysis of the attack
    pub economics: AttackEconomics,
    
    /// Security response metrics
    pub security_metrics: SecurityMetrics,
    
    /// Specific vulnerabilities identified
    pub vulnerabilities: Vec<String>,
    
    /// Recommendations to prevent 51% attacks
    pub recommendations: Vec<String>,
    
    /// Minimum confirmation blocks needed for safety (based on economics)
    pub recommended_confirmations: usize,
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

/// 51% attack simulator with enhanced metrics
pub struct Attack51PercentSimulator {
    config: Attack51PercentConfig,
    client: Client,
    results: Attack51PercentResults,
    private_chain: Vec<BlockInfo>,
    main_chain_tip: Option<BlockInfo>,
    attack_attempts: Vec<AttackAttemptInfo>,
    network_state: Option<NetworkState>,
    detection_start_time: Option<Instant>,
    recovery_start_time: Option<Instant>,
    attack_detected: bool,
    total_blocks_mined: usize,
    detection_time_seconds: f64,
    response_time_seconds: f64,
    vulnerabilities: Vec<String>,
}

impl Attack51PercentSimulator {
    /// Create a new enhanced 51% attack simulator with improved metrics
    pub fn new(config: Attack51PercentConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");
            
        // Default recommendations for 51% attack prevention
        let default_recommendations = vec![
            "Increase confirmation requirements for high-value transactions".to_string(),
            "Implement checkpointing to prevent deep reorganizations".to_string(),
            "Consider hybrid consensus mechanisms like Proof of Stake + Proof of Work".to_string(),
            "Implement penalties for misbehaving miners".to_string(),
            "Use transaction and block signatures to help identify malicious miners".to_string(),
        ];
        
        // Initialize with empty economics and security metrics
        let economics = AttackEconomics {
            total_cost_usd: 0.0,
            cost_breakdown: std::collections::HashMap::new(),
            potential_profit: 0.0,
            roi_percentage: 0.0,
            break_even_time: "Never".to_string(),
            economically_viable: false,
            min_profitable_amount: 0.0,
        };
        
        let security_metrics = SecurityMetrics {
            detection_time_seconds: 0.0,
            response_time_seconds: 0.0,
            countermeasure_effectiveness: 0.0,
            recovery_time_seconds: 0.0,
            preventable: true,
            security_score: 0.0,
        };
        
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
            attack_attempts: Vec::new(),
            estimated_attack_cost: 0.0,
            economics,
            security_metrics,
            vulnerabilities: Vec::new(),
            recommendations: default_recommendations,
            recommended_confirmations: 6, // Default value
        };
        
        Self {
            config,
            client,
            results,
            private_chain: Vec::new(),
            main_chain_tip: None,
            attack_attempts: Vec::new(),
            network_state: None,
            detection_start_time: None,
            recovery_start_time: None,
            attack_detected: false,
            total_blocks_mined: 0,
            detection_time_seconds: 0.0,
            response_time_seconds: 0.0,
            vulnerabilities: Vec::new(),
        }
    }
    
    /// Calculate optimal attack parameters based on network conditions and attacker hashpower
    pub fn calculate_optimal_attack_parameters(&self) -> Result<OptimalAttackParameters> {
        info!("Calculating optimal attack parameters for 51% attack");
        
        // Get network state (should have been populated by get_current_blockchain_state)
        let network_state = self.network_state.as_ref()
            .ok_or_else(|| anyhow!("Network state not initialized"))?;
        
        // Calculate attacker hashpower percentage
        let hashpower_percentage = self.config.control_percentage.value();
        
        // Calculate network hashpower
        let network_hashpower = network_state.hashrate;
        
        // Calculate attacker hashpower
        let attacker_hashpower = network_hashpower * (hashpower_percentage / 100.0);
        
        // Calculate hash rate ratio (attacker/network)
        let hash_rate_ratio = attacker_hashpower / (network_hashpower - attacker_hashpower);
        
        // Calculate theoretically optimal private fork length based on network conditions
        // This is based on the selfish mining strategy formula
        let mut optimal_fork_length = if hashpower_percentage > 50.0 {
            // For majority attacks, fewer blocks are needed for guaranteed success
            // The formula is approximately: ln(0.01) / ln(1 - 2*p) where p is the hashpower percentage as a decimal
            let p = hashpower_percentage / 100.0;
            let numerator = (0.01f64).ln();
            let denominator = (1.0 - 2.0 * p).ln().abs();
            (numerator / denominator).ceil() as usize
        } else {
            // For minority attacks, longer forks are needed and success is not guaranteed
            // We use a more conservative approach
            self.config.victim_confirmation_blocks + 2
        };
        
        // Cap the fork length to be reasonable - never more than 2x the confirmation blocks
        let max_reasonable_fork = self.config.victim_confirmation_blocks * 2;
        if optimal_fork_length > max_reasonable_fork {
            optimal_fork_length = max_reasonable_fork;
        }
        
        // Calculate optimal wait time (in seconds) before broadcasting fork
        // This is based on expected block time and propagation strategy
        let avg_block_time = network_state.avg_block_time;
        let wait_time_seconds = match self.config.propagation_strategy {
            AttackPropagationStrategy::Immediate => 0,
            AttackPropagationStrategy::Delayed => {
                // Wait for confirmation blocks + 1
                (self.config.victim_confirmation_blocks as f64 * avg_block_time) as u64
            },
            AttackPropagationStrategy::Selective => {
                // Wait for half the confirmation time
                (self.config.victim_confirmation_blocks as f64 * avg_block_time * 0.5) as u64
            },
            AttackPropagationStrategy::Gradual => {
                // Wait for just enough time to mine the private fork
                (optimal_fork_length as f64 * avg_block_time * (1.0 / hash_rate_ratio)) as u64
            },
        };
        
        Ok(OptimalAttackParameters {
            private_fork_length: optimal_fork_length,
            wait_time_seconds,
        })
    }
    
    /// Analyze the feasibility of an attack based on the calculated parameters
    pub fn analyze_attack_feasibility(&self, params: &OptimalAttackParameters) -> AttackFeasibility {
        info!("Analyzing attack feasibility with parameters: private_fork_length={}, wait_time={}s", 
            params.private_fork_length, params.wait_time_seconds);
        
        // Get network state (should have been populated by get_current_blockchain_state)
        let network_state = match self.network_state.as_ref() {
            Some(state) => state,
            None => {
                warn!("Network state not initialized, using pessimistic estimates");
                return AttackFeasibility {
                    success_probability: 0.01,
                    double_spend_probability: 0.01,
                    estimated_cost: 10000.0,
                    hashpower_required: 51.0,
                };
            }
        };
        
        // Get attacker control percentage
        let hashpower_percentage = self.config.control_percentage.value();
        
        // Calculate success probability based on hashpower percentage and private fork length
        let success_probability = if hashpower_percentage <= 50.0 {
            // For minority attacks, success probability decreases exponentially with fork length
            // Estimate based on probability of the attacker being able to outrun the network
            let p = hashpower_percentage / 100.0;
            let q = 1.0 - p;
            // Probability of catching up when behind by private_fork_length blocks
            let catch_up_prob = if p >= q {
                1.0
            } else {
                (p / q).powi(params.private_fork_length as i32)
            };
            catch_up_prob.min(0.95) // Cap at 95% to account for network uncertainties
        } else {
            // For majority attacks, success is almost guaranteed if enough blocks are mined
            // But still account for network delays and uncertainty
            let base_probability = 0.95;
            let adjustment = (hashpower_percentage - 50.0) / 50.0 * 0.04; // Additional 0-4% based on excess hashpower
            (base_probability + adjustment).min(0.99)
        };
        
        // Calculate double spend probability (usually lower than success probability due to other factors)
        let double_spend_probability = success_probability * 0.9; // Generally 10% lower than fork success
        
        // Calculate cost of attack in USD
        // Cost is based on hardware cost, electricity, and opportunity cost of not mining the main chain
        let network_hashrate = network_state.hashrate; // in appropriate units (TH/s, GH/s etc.)
        let attacker_hashrate = network_hashrate * (hashpower_percentage / 100.0);
        
        // Estimate cost per block based on config and current difficulty
        let electricity_cost_per_day = attacker_hashrate * 0.05; // Simplified estimate
        let rental_cost_per_day = attacker_hashrate * 0.2; // Simplified hashpower rental estimate
        let opportunity_cost_per_day = attacker_hashrate * 0.1; // Lost mining rewards
        
        let daily_cost = electricity_cost_per_day + rental_cost_per_day + opportunity_cost_per_day;
        let avg_block_time_in_days = network_state.avg_block_time / (24.0 * 60.0 * 60.0);
        let blocks_per_day = 1.0 / avg_block_time_in_days;
        
        let cost_per_block = daily_cost / blocks_per_day;
        let total_blocks_needed = params.private_fork_length;
        let estimated_cost = cost_per_block * (total_blocks_needed as f64) + self.config.cost_per_block;
        
        // Add in overhead costs for attack preparation
        let overhead_factor = 1.2; // 20% overhead
        let final_cost = estimated_cost * overhead_factor;
        
        AttackFeasibility {
            success_probability,
            double_spend_probability,
            estimated_cost: final_cost,
            hashpower_required: hashpower_percentage,
        }
    }
    
    /// Adjust control percentage for successive attack attempts
    pub fn adjust_control_percentage(&self, attempt: usize) -> Result<ControlPercentage> {
        // First attempt uses the configured percentage
        if attempt == 1 {
            return Ok(self.config.control_percentage.clone());
        }
        
        // For subsequent attempts, increase the percentage to simulate an escalating attack
        let base_percentage = self.config.control_percentage.value();
        
        // Increase by 5% for each additional attempt
        let adjusted_percentage = base_percentage + ((attempt - 1) as f64 * 5.0);
        
        // Cap at 95% to keep it realistic
        let capped_percentage = adjusted_percentage.min(95.0);
        
        // Return the appropriate control percentage enum variant
        if capped_percentage < 33.0 {
            Ok(ControlPercentage::Minor(capped_percentage))
        } else if capped_percentage < 50.0 {
            Ok(ControlPercentage::Moderate(capped_percentage))
        } else if capped_percentage < 66.0 {
            Ok(ControlPercentage::Majority(capped_percentage))
        } else {
            Ok(ControlPercentage::Supermajority(capped_percentage))
        }
    }
    
    /// Run the enhanced 51% attack simulation with multi-stage attacks and comprehensive metrics
    pub async fn run_simulation(&mut self) -> Result<Attack51PercentResults> {
        info!("Starting enhanced 51% attack simulation with comprehensive metrics");
        
        // Initialize timers and state tracking
        self.results.start_time = chrono::Utc::now();
        self.detection_start_time = Some(Instant::now());
        
        // Get the current blockchain state
        self.get_current_blockchain_state().await?;
        
        // Get network topology information 
        self.analyze_network_topology().await?;
        
        // Calculate attack parameters based on control percentage
        let params = self.calculate_optimal_attack_parameters()?;
        info!("Calculated optimal attack parameters: private fork length={}, wait time={}s", 
              params.private_fork_length, params.wait_time_seconds);
              
        // Create attack feasibility analysis
        let feasibility = self.analyze_attack_feasibility(&params);
        info!("Attack feasibility analysis: success probability={:.2}, double-spend probability={:.2}, estimated cost=${:.2}", 
              feasibility.success_probability, feasibility.double_spend_probability, feasibility.estimated_cost);
              
        // Early termination if attack is deemed infeasible
        if feasibility.success_probability < 0.05 {
            info!("Attack deemed infeasible, probability of success too low");
            self.results.end_time = chrono::Utc::now();
            self.results.estimated_attack_cost = feasibility.estimated_cost;
            return Ok(self.results.clone());
        }
        
        let mut any_attack_successful = false;
        let mut any_double_spend_successful = false;
        
        // For each attack attempt
        for attempt in 1..=self.config.attack_attempts {
            info!("Starting attack attempt {} of {}", attempt, self.config.attack_attempts);
            
            // Adjust control percentage for this attempt if doing escalating attack
            let control_percentage = self.adjust_control_percentage(attempt)?;
            info!("Using control percentage for this attempt: {:.1}%", control_percentage.value());
            
            // Create victim transaction 
            let (tx_id, recipient) = self.create_initial_transaction().await?;
            info!("Victim transaction created with ID: {}", tx_id);
            
            let mut attack_events = Vec::new();
            attack_events.push(AttackEvent {
                timestamp: chrono::Utc::now(),
                event_type: "TRANSACTION_CREATED".to_string(),
                description: format!("Created victim transaction {}", tx_id),
                block_height: Some(self.results.original_chain_height),
                transaction_id: Some(tx_id.clone()),
                success: true,
            });
            
            // Wait for confirmation based on victim confirmation blocks
            info!("Waiting for transaction confirmation ({} blocks)", self.config.victim_confirmation_blocks);
            let confirmation_result = self.wait_for_transaction_confirmation(&tx_id, self.config.victim_confirmation_blocks).await;
            
            if let Err(e) = confirmation_result {
                warn!("Transaction confirmation failed: {}", e);
                
                attack_events.push(AttackEvent {
                    timestamp: chrono::Utc::now(),
                    event_type: "CONFIRMATION_FAILED".to_string(),
                    description: format!("Transaction confirmation failed: {}", e),
                    block_height: None,
                    transaction_id: Some(tx_id.clone()),
                    success: false,
                });
                
                // Add this failed attempt
                self.attack_attempts.push(AttackAttemptInfo {
                    attempt_number: attempt,
                    control_percentage: control_percentage.clone(),
                    successful: false,
                    private_fork_blocks: 0,
                    reorged_blocks: 0,
                    double_spending_successful: false,
                    start_time: chrono::Utc::now() - chrono::Duration::seconds(30),
                    end_time: chrono::Utc::now(),
                    original_tx_id: tx_id.clone(),
                    double_spend_tx_id: "".to_string(),
                    network_state: self.network_state.clone(),
                    events: attack_events,
                    notes: "Transaction confirmation failed".to_string(),
                });
                
                continue;
            }
            
            attack_events.push(AttackEvent {
                timestamp: chrono::Utc::now(),
                event_type: "TRANSACTION_CONFIRMED".to_string(),
                description: format!("Victim transaction confirmed with {} blocks", self.config.victim_confirmation_blocks),
                block_height: Some(self.results.original_chain_height + self.config.victim_confirmation_blocks as u64),
                transaction_id: Some(tx_id.clone()),
                success: true,
            });
            
            // Mine the transaction in the main chain
            self.mine_transaction_in_main_chain(&tx_id).await?;
            
            // Apply propagation strategy to determine fork length
            let fork_length = self.determine_fork_length(&params);
            
            // Build a private fork with specific strategy
            self.build_private_fork_with_strategy(recipient.clone(), fork_length, &control_percentage).await?;
            self.total_blocks_mined += self.private_chain.len();
            
            for (i, block) in self.private_chain.iter().enumerate() {
                attack_events.push(AttackEvent {
                    timestamp: chrono::Utc::now(),
                    event_type: "PRIVATE_BLOCK_MINED".to_string(),
                    description: format!("Mined private block at height {}", block.height),
                    block_height: Some(block.height),
                    transaction_id: None,
                    success: true,
                });
            }
            
            // Create double-spend transaction
            let double_spend_tx_id = self.create_double_spend_transaction(recipient.clone()).await?;
            info!("Double-spend transaction created: {}", double_spend_tx_id);
            
            attack_events.push(AttackEvent {
                timestamp: chrono::Utc::now(),
                event_type: "DOUBLE_SPEND_CREATED".to_string(),
                description: format!("Created double-spend transaction {}", double_spend_tx_id),
                block_height: None,
                transaction_id: Some(double_spend_tx_id.clone()),
                success: true,
            });
            
            // Add double-spend to private fork
            let double_spend_added = self.add_double_spend_to_private_fork(
                &double_spend_tx_id, 
                self.private_chain.last().map(|b| b.hash.clone())
            ).await?;
            
            if !double_spend_added {
                warn!("Failed to add double-spend transaction to private fork");
                attack_events.push(AttackEvent {
                    timestamp: chrono::Utc::now(),
                    event_type: "ADD_DOUBLE_SPEND_FAILED".to_string(),
                    description: "Failed to add double-spend transaction to private fork".to_string(),
                    block_height: None,
                    transaction_id: Some(double_spend_tx_id.clone()),
                    success: false,
                });
            } else {
                attack_events.push(AttackEvent {
                    timestamp: chrono::Utc::now(),
                    event_type: "ADD_DOUBLE_SPEND_SUCCESS".to_string(),
                    description: "Successfully added double-spend to private fork".to_string(),
                    block_height: None,
                    transaction_id: Some(double_spend_tx_id.clone()),
                    success: true,
                });
            }
            
            // Attempt to reorganize the chain
            let mut attempt_successful = false;
            let mut attempt_double_spend_successful = false;
            let mut attempt_reorged_blocks = 0;
            
            if self.config.broadcast_attack {
                info!("Broadcasting private fork to attempt chain reorganization");
                
                // Broadcast the private fork with the specified strategy
                attempt_reorged_blocks = self.broadcast_private_fork_with_strategy(
                    self.config.propagation_strategy,
                    &mut attack_events
                ).await?;
                
                attempt_successful = attempt_reorged_blocks > 0;
                
                // Check if attack was detected
                if !self.attack_detected {
                    let attack_detection = self.check_attack_detection().await?;
                    if attack_detection.detected {
                        self.attack_detected = true;
                        self.detection_time_seconds = self.detection_start_time.unwrap().elapsed().as_secs_f64();
                        info!("Attack detected after {:.2}s", self.detection_time_seconds);
                        
                        attack_events.push(AttackEvent {
                            timestamp: chrono::Utc::now(),
                            event_type: "ATTACK_DETECTED".to_string(),
                            description: format!("Attack detected after {:.2}s using {}", 
                                self.detection_time_seconds, attack_detection.detection_method),
                            block_height: None,
                            transaction_id: None,
                            success: true,
                        });
                        
                        // Check countermeasure implementation if configured
                        if self.config.implement_countermeasures {
                            info!("Checking countermeasure implementation");
                            let countermeasure_start = Instant::now();
                            
                            let countermeasures = self.check_countermeasures().await?;
                            if countermeasures.implemented {
                                self.response_time_seconds = countermeasure_start.elapsed().as_secs_f64();
                                info!("Countermeasures implemented after {:.2}s", self.response_time_seconds);
                                
                                attack_events.push(AttackEvent {
                                    timestamp: chrono::Utc::now(),
                                    event_type: "COUNTERMEASURES_IMPLEMENTED".to_string(),
                                    description: format!("Countermeasures implemented after {:.2}s with {:.1}% effectiveness", 
                                        self.response_time_seconds, countermeasures.effectiveness),
                                    block_height: None,
                                    transaction_id: None,
                                    success: true,
                                });
                            }
                        }
                    }
                }
                
                // Check if double-spend was successful
                if attempt_successful {
                    info!("Checking if double-spend was successful");
                    attempt_double_spend_successful = self.check_double_spend_success(&double_spend_tx_id).await?;
                    
                    if attempt_double_spend_successful {
                        info!("Double-spending was successful");
                        
                        attack_events.push(AttackEvent {
                            timestamp: chrono::Utc::now(),
                            event_type: "DOUBLE_SPEND_SUCCESS".to_string(),
                            description: "Double-spending transaction was successfully confirmed".to_string(),
                            block_height: None,
                            transaction_id: Some(double_spend_tx_id.clone()),
                            success: true,
                        });
                        
                        if !any_double_spend_successful {
                            any_double_spend_successful = true;
                            
                            // Add this as a critical vulnerability
                            self.vulnerabilities.push(format!(
                                "Critical: Double-spend attack succeeded with {:.1}% hashpower after {} confirmations",
                                control_percentage.value(),
                                self.config.victim_confirmation_blocks
                            ));
                        }
                    } else {
                        info!("Double-spending failed despite successful reorg");
                        
                        attack_events.push(AttackEvent {
                            timestamp: chrono::Utc::now(),
                            event_type: "DOUBLE_SPEND_FAILED".to_string(),
                            description: "Double-spending failed despite successful chain reorganization".to_string(),
                            block_height: None,
                            transaction_id: Some(double_spend_tx_id.clone()),
                            success: false,
                        });
                    }
                }
                
                // Set recovery start time after the first successful attack
                if attempt_successful && self.recovery_start_time.is_none() {
                    self.recovery_start_time = Some(Instant::now());
                }
            } else {
                info!("Attack simulation only, not broadcasting private fork");
                
                // Analyze feasibility without actual broadcast
                let feasibility = self.analyze_attack_feasibility(
                    control_percentage.value(),
                    fork_length,
                    self.network_state.as_ref().map(|n| n.hashrate).unwrap_or(1000.0),
                    self.network_state.as_ref().map(|n| n.difficulty).unwrap_or(1.0)
                );
                
                attack_events.push(AttackEvent {
                    timestamp: chrono::Utc::now(),
                    event_type: "ATTACK_SIMULATION".to_string(),
                    description: format!(
                        "Simulated attack feasibility: {:.1}% chance of success", 
                        feasibility.success_probability * 100.0
                    ),
                    block_height: None,
                    transaction_id: None,
                    success: feasibility.success_probability > 0.5,
                });
                
                attempt_successful = feasibility.success_probability > 0.5;
                attempt_double_spend_successful = attempt_successful && feasibility.double_spend_probability > 0.5;
                attempt_reorged_blocks = if attempt_successful { fork_length } else { 0 };
                
                if attempt_successful {
                    if !any_attack_successful {
                        any_attack_successful = true;
                    }
                    
                    if attempt_double_spend_successful && !any_double_spend_successful {
                        any_double_spend_successful = true;
                        
                        // Add this as a simulated vulnerability
                        self.vulnerabilities.push(format!(
                            "Simulated: Double-spend attack has {:.1}% chance of success with {:.1}% hashpower after {} confirmations",
                            feasibility.double_spend_probability * 100.0,
                            control_percentage.value(),
                            self.config.victim_confirmation_blocks
                        ));
                    }
                }
            }
            
            // Update metrics for this attempt
            if attempt_successful {
                if !any_attack_successful {
                    any_attack_successful = true;
                }
                
                self.results.private_fork_blocks = self.private_chain.len();
                self.results.reorged_blocks = attempt_reorged_blocks;
            }
            
            // Add this attempt to the list
            self.attack_attempts.push(AttackAttemptInfo {
                attempt_number: attempt,
                control_percentage: control_percentage.clone(),
                successful: attempt_successful,
                private_fork_blocks: self.private_chain.len(),
                reorged_blocks: attempt_reorged_blocks,
                double_spending_successful: attempt_double_spend_successful,
                start_time: chrono::Utc::now() - chrono::Duration::seconds(30), // Approximate
                end_time: chrono::Utc::now(),
                original_tx_id: tx_id.clone(),
                double_spend_tx_id: double_spend_tx_id.clone(),
                network_state: self.network_state.clone(),
                events: attack_events,
                notes: if attempt_successful {
                    if attempt_double_spend_successful {
                        "Attack successful with double-spending".to_string()
                    } else {
                        "Chain reorganization successful but no double-spending".to_string()
                    }
                } else {
                    "Attack failed".to_string()
                },
            });
            
            // Wait between attempts if this isn't the last one
            if attempt < self.config.attack_attempts {
                info!("Waiting {}s before next attempt", self.config.time_between_attempts_seconds);
                tokio::time::sleep(Duration::from_secs(self.config.time_between_attempts_seconds)).await;
            }
            
            // Clear private chain for next attempt
            self.private_chain.clear();
        }
        
        // Get final blockchain state
        self.get_final_blockchain_state().await?;
        
        // Calculate enhanced metrics
        self.calculate_enhanced_metrics(any_attack_successful, any_double_spend_successful);
        
        // Update results with comprehensive data
        self.results.attack_successful = any_attack_successful;
        self.results.double_spending_successful = any_double_spend_successful;
        self.results.attack_attempts = self.attack_attempts.clone();
        self.results.vulnerabilities = self.vulnerabilities.clone();
        self.results.end_time = chrono::Utc::now();
        
        // Calculate cost
        self.calculate_attack_cost();
        
        // Add recommendations
        self.add_enhanced_recommendations();
        
        // Generate enhanced report
        let report = self.generate_enhanced_report();
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
    async fn build_private_fork(&mut self, recipient: String) -> Result<()> {
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