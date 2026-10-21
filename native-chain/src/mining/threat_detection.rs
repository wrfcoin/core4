// TODO(#960): Replace with wrfcoin_p2p::peer::PeerInfo when p2p dep is added to native-chain.
// Blocked on adding wrfcoin-p2p as a workspace dependency of native-chain.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PeerInfo {
    pub id: String,
    pub address: String,
}

use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::transaction::Transaction;
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::crypto::hash::Hash;
type Timestamp = u64;
use anyhow::Result;
use log::{info, debug, warn, error};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Serialize, Deserialize};

/// Threat severity level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThreatSeverity {
    /// Informational - not threatening but worth monitoring
    Info,

    /// Low severity - minor anomaly
    Low,

    /// Medium severity - potential threat that needs attention
    Medium,

    /// High severity - active threat requiring immediate response
    High,

    /// Critical severity - imminent danger to network
    Critical,
}

/// Threat detection type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ThreatType {
    /// Inconsistent blocks (e.g., invalid proofs)
    InconsistentBlock,

    /// Double spending attempt
    DoubleSpend,

    /// Denial of service
    DenialOfService,

    /// Transaction flooding
    TransactionFlooding,

    /// Mining power concentration
    MiningPowerConcentration,

    /// Unusual network patterns
    UnusualNetworkPatterns,

    /// Validator misbehavior
    ValidatorMisbehavior,

    /// Chain reorganization
    ChainReorganization,

    /// Replay attack
    ReplayAttack,

    /// Eclipse attack (network partitioning)
    EclipseAttack,
}

/// Threat detection event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreatEvent {
    /// Event ID
    pub id: String,

    /// Threat type
    pub threat_type: ThreatType,

    /// Threat severity
    pub severity: ThreatSeverity,

    /// Description of the threat
    pub description: String,

    /// Related addresses (if any)
    pub related_addresses: Vec<Address>,

    /// Related blocks (if any)
    pub related_block_hashes: Vec<Hash>,

    /// Related transactions (if any)
    pub related_tx_hashes: Vec<Hash>,

    /// Related peers (if any)
    pub related_peer_ids: Vec<String>,

    /// Detection timestamp
    pub timestamp: Timestamp,

    /// First observed timestamp
    pub first_observed: Timestamp,

    /// Event status (active/resolved)
    pub resolved: bool,

    /// Resolution timestamp (if resolved)
    pub resolution_timestamp: Option<Timestamp>,

    /// Resolution notes
    pub resolution_notes: Option<String>,

    /// Related events
    pub related_events: Vec<String>,

    /// Confidence score (0-100)
    pub confidence: u8,
}

/// Behavioral profile for a network participant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehavioralProfile {
    /// Address of the participant
    pub address: Address,

    /// Transaction patterns
    pub transaction_patterns: TransactionPatterns,

    /// Mining patterns
    pub mining_patterns: MiningPatterns,

    /// Network patterns
    pub network_patterns: NetworkPatterns,

    /// Reputation score (0-100)
    pub reputation_score: u8,

    /// First seen timestamp
    pub first_seen: Timestamp,

    /// Last active timestamp
    pub last_active: Timestamp,

    /// Past threat events
    pub past_threats: Vec<String>,
}

/// Transaction patterns for behavioral analysis
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransactionPatterns {
    /// Average transaction volume
    pub avg_transaction_volume: f64,

    /// Transaction frequency (transactions per day)
    pub transaction_frequency: f64,

    /// Common transaction targets
    pub common_targets: HashMap<Address, u64>,

    /// Transaction time distribution (hour -> count)
    pub time_distribution: HashMap<u8, u64>,

    /// Transaction value distribution
    pub value_distribution: HashMap<ValueRange, u64>,
}

/// Mining patterns for behavioral analysis
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MiningPatterns {
    /// Blocks mined
    pub blocks_mined: u64,

    /// Average hash power
    pub avg_hash_power: f64,

    /// Mining consistency
    pub mining_consistency: f64,

    /// Mining pool participation
    pub pool_participation: bool,

    /// Mining time distribution (hour -> count)
    pub time_distribution: HashMap<u8, u64>,
}

/// Network patterns for behavioral analysis
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetworkPatterns {
    /// Connection frequency
    pub connection_frequency: f64,

    /// Connection duration
    pub avg_connection_duration: f64,

    /// Peer distribution
    pub peer_distribution: HashMap<String, u64>,

    /// Geographic distribution
    pub geographic_distribution: HashMap<String, u64>,

    /// Message propagation patterns
    pub propagation_patterns: HashMap<String, f64>,
}

/// Value range for transaction value distribution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ValueRange {
    /// 0-10 tokens
    VerySmall,

    /// 10-100 tokens
    Small,

    /// 100-1000 tokens
    Medium,

    /// 1000-10000 tokens
    Large,

    /// 10000+ tokens
    VeryLarge,
}

/// Extract the miner address from block metadata, if present.
///
/// Convention: blocks carry an optional `"miner"` key in their metadata whose
/// value is a 40-character hex-encoded address.  Returns `None` when the key is
/// absent or the value cannot be decoded.
fn extract_miner_address(block: &Block) -> Option<Address> {
    let hex_str = block.metadata.get("miner")?;
    let bytes = hex::decode(hex_str).ok()?;
    if bytes.len() != 20 {
        return None;
    }
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&bytes);
    Some(Address(addr))
}

/// Advanced Threat Detection system
pub struct ThreatDetectionSystem {
    /// Behavioral profiles for network participants
    profiles: Arc<RwLock<HashMap<Address, BehavioralProfile>>>,

    /// Recent threat events
    threat_events: Arc<RwLock<VecDeque<ThreatEvent>>>,

    /// Transaction memory (for double-spend detection)
    transaction_memory: Arc<RwLock<HashMap<Hash, Transaction>>>,

    /// Block memory (for chain analysis)
    block_memory: Arc<RwLock<HashMap<Hash, Block>>>,

    /// Peer behavior tracking
    peer_behavior: Arc<RwLock<HashMap<String, PeerBehavior>>>,

    /// Configuration
    config: ThreatDetectionConfig,
}

/// Peer behavior tracking
#[derive(Debug, Clone, Default)]
pub struct PeerBehavior {
    /// Peer ID
    pub peer_id: String,

    /// Connection count
    pub connection_count: u64,

    /// Message count
    pub message_count: u64,

    /// Invalid message count
    pub invalid_message_count: u64,

    /// Last seen timestamp
    pub last_seen: Option<Timestamp>,

    /// Average response time
    pub avg_response_time: f64,

    /// Invalid block announcements
    pub invalid_block_announcements: u64,

    /// Invalid transaction announcements
    pub invalid_transaction_announcements: u64,
}

/// Threat detection configuration
#[derive(Debug, Clone)]
pub struct ThreatDetectionConfig {
    /// Maximum events to keep in memory
    pub max_events: usize,

    /// Maximum transactions to keep in memory
    pub max_transactions: usize,

    /// Maximum blocks to keep in memory
    pub max_blocks: usize,

    /// Minimum confidence threshold for reporting
    pub min_confidence: u8,

    /// Enable transaction flood detection
    pub enable_tx_flood_detection: bool,

    /// Enable double spend detection
    pub enable_double_spend_detection: bool,

    /// Enable mining power concentration detection
    pub enable_mining_concentration_detection: bool,

    /// Enable unusual network pattern detection
    pub enable_network_pattern_detection: bool,

    /// Enable behavioral analysis
    pub enable_behavioral_analysis: bool,
}

impl Default for ThreatDetectionConfig {
    fn default() -> Self {
        Self {
            max_events: 1000,
            max_transactions: 10000,
            max_blocks: 1000,
            min_confidence: 60,
            enable_tx_flood_detection: true,
            enable_double_spend_detection: true,
            enable_mining_concentration_detection: true,
            enable_network_pattern_detection: true,
            enable_behavioral_analysis: true,
        }
    }
}

impl ThreatDetectionSystem {
    /// Create a new threat detection system
    pub fn new(config: Option<ThreatDetectionConfig>) -> Self {
        Self {
            profiles: Arc::new(RwLock::new(HashMap::new())),
            threat_events: Arc::new(RwLock::new(VecDeque::new())),
            transaction_memory: Arc::new(RwLock::new(HashMap::new())),
            block_memory: Arc::new(RwLock::new(HashMap::new())),
            peer_behavior: Arc::new(RwLock::new(HashMap::new())),
            config: config.unwrap_or_default(),
        }
    }

    /// Process a new block for threat detection
    pub async fn process_block(&self, block: &Block) -> Result<()> {
        // block.hash is Option<Hash>; use calculate_hash() as key if not set
        let block_hash = block.hash.unwrap_or_else(|| block.calculate_hash());

        // Add block to memory
        let mut blocks = self.block_memory.write().await;
        blocks.insert(block_hash, block.clone());

        // Trim block memory if needed
        if blocks.len() > self.config.max_blocks {
            // Remove oldest blocks
            let block_hashes: Vec<Hash> = blocks.keys().cloned().collect();
            let excess = blocks.len() - self.config.max_blocks;

            for i in 0..excess {
                blocks.remove(&block_hashes[i]);
            }
        }

        drop(blocks);

        // Process block for threats
        self.detect_inconsistent_block(block).await?;

        if self.config.enable_mining_concentration_detection {
            self.detect_mining_concentration(block).await?;
        }

        self.update_mining_patterns(block).await?;

        Ok(())
    }

    /// Process a new transaction for threat detection
    pub async fn process_transaction(&self, transaction: &Transaction) -> Result<()> {
        // Transaction.id is Option<Hash>; compute one if absent
        let tx_hash = transaction.id.unwrap_or_default();

        // Add transaction to memory
        let mut transactions = self.transaction_memory.write().await;
        transactions.insert(tx_hash, transaction.clone());

        // Trim transaction memory if needed
        if transactions.len() > self.config.max_transactions {
            // Remove oldest transactions
            let tx_hashes: Vec<Hash> = transactions.keys().cloned().collect();
            let excess = transactions.len() - self.config.max_transactions;

            for i in 0..excess {
                transactions.remove(&tx_hashes[i]);
            }
        }

        drop(transactions);

        // Process transaction for threats
        if self.config.enable_double_spend_detection {
            self.detect_double_spend(transaction).await?;
        }

        if self.config.enable_tx_flood_detection {
            self.detect_transaction_flooding(transaction).await?;
        }

        self.update_transaction_patterns(transaction).await?;

        Ok(())
    }

    /// Process peer behavior for threat detection
    pub async fn process_peer_behavior(&self, peer_info: &PeerInfo) -> Result<()> {
        // Update peer behavior
        let mut peer_behaviors = self.peer_behavior.write().await;
        let behavior = peer_behaviors.entry(peer_info.id.clone()).or_insert_with(|| {
            let mut pb = PeerBehavior::default();
            pb.peer_id = peer_info.id.clone();
            pb
        });

        behavior.connection_count += 1;
        behavior.last_seen = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        );

        drop(peer_behaviors);

        // Process for threats
        if self.config.enable_network_pattern_detection {
            self.detect_unusual_network_patterns(peer_info).await?;
        }

        self.update_network_patterns(peer_info).await?;

        Ok(())
    }

    /// Detect inconsistent blocks
    async fn detect_inconsistent_block(&self, block: &Block) -> Result<()> {
        // In a real implementation, this would check for:
        // - Invalid merkle roots
        // - Invalid proof of work
        // - Invalid state transitions
        // - Invalid timestamps
        // - Invalid signatures

        // For demonstration purposes, we'll use a simple heuristic
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let block_ts = block.timestamp.timestamp() as u64;
        let block_hash = block.hash.unwrap_or_else(|| block.calculate_hash());

        // Check if block timestamp is future (more than 5 minutes)
        if block_ts > timestamp + 300 {
            // Ref #960: Extract miner from metadata; fall back to zero address when absent.
            let miner = extract_miner_address(block).unwrap_or(Address([0u8; 20]));
            self.report_threat(
                ThreatType::InconsistentBlock,
                ThreatSeverity::Medium,
                format!("Block with future timestamp: {} (current: {})", block_ts, timestamp),
                Some(vec![miner]),
                Some(vec![block_hash]),
                None,
                None,
                85,
            ).await?;
        }

        Ok(())
    }

    /// Detect double spend attempts
    async fn detect_double_spend(&self, transaction: &Transaction) -> Result<()> {
        // In a real implementation, this would check for:
        // - Same inputs used in multiple transactions
        // - Same outputs received multiple times

        let tx_hash = transaction.id.unwrap_or_default();
        let transactions = self.transaction_memory.read().await;

        let tx_recipient = transaction.recipient_address.unwrap_or(Address([0u8; 20]));

        for (hash, existing_tx) in transactions.iter() {
            let existing_recipient = existing_tx.recipient_address.unwrap_or(Address([0u8; 20]));
            if hash != &tx_hash &&
               existing_tx.sender_address == transaction.sender_address &&
               existing_tx.nonce == transaction.nonce &&
               existing_recipient != tx_recipient {

                self.report_threat(
                    ThreatType::DoubleSpend,
                    ThreatSeverity::High,
                    format!("Potential double spend detected: same sender and nonce but different recipient"),
                    Some(vec![transaction.sender_address, tx_recipient, existing_recipient]),
                    None,
                    Some(vec![tx_hash, *hash]),
                    None,
                    90,
                ).await?;

                break;
            }
        }

        Ok(())
    }

    /// Detect transaction flooding
    async fn detect_transaction_flooding(&self, transaction: &Transaction) -> Result<()> {
        // In a real implementation, this would check for:
        // - Unusually high transaction rate from a single address
        // - Transactions with very low fees
        // - Transactions with very small amounts

        let transactions = self.transaction_memory.read().await;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let tx_hash = transaction.id.unwrap_or_default();

        // Count transactions from this sender in the last hour
        let one_hour_ago = timestamp - 3600;
        let tx_count = transactions.values()
            .filter(|tx| {
                tx.sender_address == transaction.sender_address
                    && (tx.timestamp.timestamp() as u64) > one_hour_ago
            })
            .count();

        // If more than 100 transactions in the last hour, report
        if tx_count > 100 {
            self.report_threat(
                ThreatType::TransactionFlooding,
                ThreatSeverity::Medium,
                format!("Transaction flooding detected: {} transactions in the last hour from sender", tx_count),
                Some(vec![transaction.sender_address]),
                None,
                Some(vec![tx_hash]),
                None,
                75,
            ).await?;
        }

        Ok(())
    }

    /// Detect mining power concentration.
    ///
    /// Counts recent blocks by miner (extracted from `block.metadata["miner"]`)
    /// and raises a threat when any single miner exceeds 33% of the last
    /// `max_blocks` window.
    async fn detect_mining_concentration(&self, block: &Block) -> Result<()> {
        let _miner = match extract_miner_address(block) {
            Some(addr) => addr,
            // No miner metadata on this block — nothing to check.
            None => return Ok(()),
        };

        let blocks = self.block_memory.read().await;

        // Count blocks per miner across the in-memory window.
        let mut miner_counts: HashMap<Address, u64> = HashMap::new();
        for stored_block in blocks.values() {
            if let Some(addr) = extract_miner_address(stored_block) {
                *miner_counts.entry(addr).or_insert(0) += 1;
            }
        }

        let total_blocks = miner_counts.values().sum::<u64>();
        if total_blocks < 3 {
            // Too few blocks to draw conclusions.
            return Ok(());
        }

        let block_hash = block.hash.unwrap_or_else(|| block.calculate_hash());

        for (addr, count) in &miner_counts {
            // Flag if any single miner has mined >33% of recent blocks.
            let share_pct = (*count as f64 / total_blocks as f64) * 100.0;
            if share_pct > 33.0 {
                let severity = if share_pct > 50.0 {
                    ThreatSeverity::High
                } else {
                    ThreatSeverity::Medium
                };

                self.report_threat(
                    ThreatType::MiningPowerConcentration,
                    severity,
                    format!(
                        "Mining concentration: miner {} produced {:.1}% of last {} blocks",
                        hex::encode(addr.as_bytes()),
                        share_pct,
                        total_blocks,
                    ),
                    Some(vec![*addr]),
                    Some(vec![block_hash]),
                    None,
                    None,
                    (share_pct.min(99.0)) as u8,
                ).await?;
            }
        }

        Ok(())
    }

    /// Detect unusual network patterns
    async fn detect_unusual_network_patterns(&self, peer_info: &PeerInfo) -> Result<()> {
        // In a real implementation, this would check for:
        // - Unusual connection patterns
        // - Unusual message propagation
        // - Unusual peer discovery behavior
        // - Geographic anomalies

        // For demonstration purposes, we'll check connection frequency
        let peer_behaviors = self.peer_behavior.read().await;
        if let Some(behavior) = peer_behaviors.get(&peer_info.id) {
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            if let Some(last_seen) = behavior.last_seen {
                // If connecting too frequently (more than 10 times in the last hour)
                if behavior.connection_count > 10 && timestamp - last_seen < 3600 {
                    self.report_threat(
                        ThreatType::UnusualNetworkPatterns,
                        ThreatSeverity::Low,
                        format!("Unusual connection pattern: peer connecting too frequently ({} times)",
                                behavior.connection_count),
                        None,
                        None,
                        None,
                        Some(vec![peer_info.id.clone()]),
                        65,
                    ).await?;
                }
            }
        }

        Ok(())
    }

    /// Update transaction patterns for behavioral analysis
    async fn update_transaction_patterns(&self, transaction: &Transaction) -> Result<()> {
        if !self.config.enable_behavioral_analysis {
            return Ok(());
        }

        let mut profiles = self.profiles.write().await;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Update sender profile
        let sender_profile = profiles.entry(transaction.sender_address).or_insert_with(|| {
            self.create_new_profile(transaction.sender_address, timestamp)
        });

        sender_profile.last_active = timestamp;

        // Update transaction patterns
        let value_range = self.get_value_range(transaction.amount);
        *sender_profile.transaction_patterns.value_distribution.entry(value_range).or_insert(0) += 1;

        // Update time distribution
        let tx_ts = transaction.timestamp.timestamp() as u64;
        let hour = self.get_hour_from_timestamp(tx_ts);
        *sender_profile.transaction_patterns.time_distribution.entry(hour).or_insert(0) += 1;

        // Update common targets
        let recipient = transaction.recipient_address.unwrap_or(Address([0u8; 20]));
        *sender_profile.transaction_patterns.common_targets.entry(recipient).or_insert(0) += 1;

        // Update transaction frequency
        let tx_count = sender_profile.transaction_patterns.time_distribution.values().sum::<u64>();
        let days_active = ((timestamp - sender_profile.first_seen) as f64 / 86400.0).max(1.0);
        sender_profile.transaction_patterns.transaction_frequency = tx_count as f64 / days_active;

        // Update avg transaction volume
        let total_volume = sender_profile.transaction_patterns.value_distribution.iter()
            .map(|(range, count)| self.get_range_midpoint(*range) * count)
            .sum::<u64>();

        if tx_count > 0 {
            sender_profile.transaction_patterns.avg_transaction_volume = total_volume as f64 / tx_count as f64;
        }

        Ok(())
    }

    /// Update mining patterns for behavioral analysis.
    ///
    /// Extracts the miner address from `block.metadata["miner"]` and updates
    /// blocks-mined count and time-of-day distribution in their profile.
    async fn update_mining_patterns(&self, block: &Block) -> Result<()> {
        if !self.config.enable_behavioral_analysis {
            return Ok(());
        }

        // Ref #960: Extract miner from metadata; skip when absent.
        let miner = match extract_miner_address(block) {
            Some(addr) => addr,
            None => return Ok(()),
        };

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut profiles = self.profiles.write().await;
        let profile = profiles.entry(miner).or_insert_with(|| {
            self.create_new_profile(miner, timestamp)
        });

        profile.last_active = timestamp;
        profile.mining_patterns.blocks_mined += 1;

        // Update time distribution (hour of day).
        let block_ts = block.timestamp.timestamp() as u64;
        let hour = self.get_hour_from_timestamp(block_ts);
        *profile.mining_patterns.time_distribution.entry(hour).or_insert(0) += 1;

        Ok(())
    }

    /// Update network patterns for behavioral analysis
    async fn update_network_patterns(&self, _peer_info: &PeerInfo) -> Result<()> {
        if !self.config.enable_behavioral_analysis {
            return Ok(());
        }

        // This would update network patterns for addresses associated with peers
        // For demonstration, we'll skip the actual implementation as it would
        // require mapping peer IDs to blockchain addresses

        Ok(())
    }

    /// Create a new behavioral profile
    fn create_new_profile(&self, address: Address, timestamp: Timestamp) -> BehavioralProfile {
        BehavioralProfile {
            address,
            transaction_patterns: TransactionPatterns::default(),
            mining_patterns: MiningPatterns::default(),
            network_patterns: NetworkPatterns::default(),
            reputation_score: 75, // Start with neutral-positive score
            first_seen: timestamp,
            last_active: timestamp,
            past_threats: Vec::new(),
        }
    }

    /// Get the hour from a timestamp
    fn get_hour_from_timestamp(&self, timestamp: Timestamp) -> u8 {
        // Convert timestamp to hour of day (0-23)
        ((timestamp % 86400) / 3600) as u8
    }

    /// Get the value range for a transaction amount
    fn get_value_range(&self, amount: u64) -> ValueRange {
        match amount {
            0..=10 => ValueRange::VerySmall,
            11..=100 => ValueRange::Small,
            101..=1000 => ValueRange::Medium,
            1001..=10000 => ValueRange::Large,
            _ => ValueRange::VeryLarge,
        }
    }

    /// Get the midpoint value for a value range
    fn get_range_midpoint(&self, range: ValueRange) -> u64 {
        match range {
            ValueRange::VerySmall => 5,
            ValueRange::Small => 50,
            ValueRange::Medium => 500,
            ValueRange::Large => 5000,
            ValueRange::VeryLarge => 50000,
        }
    }

    /// Report a threat event
    async fn report_threat(
        &self,
        threat_type: ThreatType,
        severity: ThreatSeverity,
        description: String,
        related_addresses: Option<Vec<Address>>,
        related_block_hashes: Option<Vec<Hash>>,
        related_tx_hashes: Option<Vec<Hash>>,
        related_peer_ids: Option<Vec<String>>,
        confidence: u8,
    ) -> Result<String> {
        // Only report if confidence exceeds threshold
        if confidence < self.config.min_confidence {
            return Ok(String::from("threat_below_threshold"));
        }

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Generate a unique ID for the event
        let id = format!("threat-{}-{}", timestamp, rand::random::<u32>());

        let event = ThreatEvent {
            id: id.clone(),
            threat_type,
            severity,
            description: description.clone(),
            related_addresses: related_addresses.unwrap_or_default(),
            related_block_hashes: related_block_hashes.unwrap_or_default(),
            related_tx_hashes: related_tx_hashes.unwrap_or_default(),
            related_peer_ids: related_peer_ids.unwrap_or_default(),
            timestamp,
            first_observed: timestamp,
            resolved: false,
            resolution_timestamp: None,
            resolution_notes: None,
            related_events: Vec::new(),
            confidence,
        };

        // Update threat events
        let mut events = self.threat_events.write().await;
        events.push_back(event.clone());

        // Trim events if needed
        while events.len() > self.config.max_events {
            events.pop_front();
        }

        // Update profiles for related addresses
        if !self.config.enable_behavioral_analysis {
            return Ok(id);
        }

        let mut profiles = self.profiles.write().await;
        for address in &event.related_addresses {
            if let Some(profile) = profiles.get_mut(address) {
                profile.past_threats.push(id.clone());

                // Update reputation score based on threat severity
                let penalty = match event.severity {
                    ThreatSeverity::Info => 0,
                    ThreatSeverity::Low => 1,
                    ThreatSeverity::Medium => 3,
                    ThreatSeverity::High => 5,
                    ThreatSeverity::Critical => 10,
                };

                // Apply confidence-weighted penalty
                let weighted_penalty = (penalty as f64 * (confidence as f64 / 100.0)).round() as u8;
                profile.reputation_score = profile.reputation_score.saturating_sub(weighted_penalty);
            }
        }

        // Log the threat
        match severity {
            ThreatSeverity::Info => info!("Threat detected [{}]: {}", id, description),
            ThreatSeverity::Low => debug!("Threat detected [{}]: {}", id, description),
            ThreatSeverity::Medium => warn!("Threat detected [{}]: {}", id, description),
            ThreatSeverity::High => error!("Threat detected [{}]: {}", id, description),
            ThreatSeverity::Critical => error!("CRITICAL THREAT [{}]: {}", id, description),
        }

        Ok(id)
    }

    /// Resolve a threat event
    pub async fn resolve_threat(
        &self,
        threat_id: &str,
        resolution_notes: Option<String>,
    ) -> Result<()> {
        let mut events = self.threat_events.write().await;

        for event in events.iter_mut() {
            if event.id == threat_id {
                event.resolved = true;
                event.resolution_timestamp = Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                );
                event.resolution_notes = resolution_notes;

                info!("Threat [{}] resolved: {}", threat_id, event.description);
                break;
            }
        }

        Ok(())
    }

    /// Get recent threat events
    pub async fn get_recent_threats(&self, limit: Option<usize>) -> Vec<ThreatEvent> {
        let events = self.threat_events.read().await;
        let limit = limit.unwrap_or(10).min(events.len());

        events.iter().rev().take(limit).cloned().collect()
    }

    /// Get active (unresolved) threat events
    pub async fn get_active_threats(&self) -> Vec<ThreatEvent> {
        let events = self.threat_events.read().await;

        events.iter()
            .filter(|e| !e.resolved)
            .cloned()
            .collect()
    }

    /// Get behavioral profile for an address
    pub async fn get_behavioral_profile(&self, address: &Address) -> Option<BehavioralProfile> {
        let profiles = self.profiles.read().await;
        profiles.get(address).cloned()
    }

    /// Get threat statistics
    pub async fn get_threat_stats(&self) -> HashMap<ThreatType, u64> {
        let events = self.threat_events.read().await;
        let mut stats = HashMap::new();

        for event in events.iter() {
            *stats.entry(event.threat_type).or_insert(0) += 1;
        }

        stats
    }

    /// Get peer behavior statistics
    pub async fn get_peer_stats(&self) -> HashMap<String, PeerBehavior> {
        let behaviors = self.peer_behavior.read().await;
        behaviors.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Happy path: a freshly created system has no threat events, no active
    /// threats, and empty statistics.
    #[tokio::test]
    async fn new_system_has_no_threats() {
        let system = ThreatDetectionSystem::new(None);

        let recent = system.get_recent_threats(None).await;
        assert!(recent.is_empty(), "new system must have no recent threat events");

        let active = system.get_active_threats().await;
        assert!(active.is_empty(), "new system must have no active threats");

        let stats = system.get_threat_stats().await;
        assert!(stats.is_empty(), "new system must have empty threat statistics");
    }

    /// Error path: resolve_threat on an ID that does not exist must return Ok
    /// without panicking.  The events queue must remain empty.
    #[tokio::test]
    async fn resolve_nonexistent_threat_is_graceful() {
        let system = ThreatDetectionSystem::new(None);

        let result = system.resolve_threat("threat-id-that-does-not-exist", None).await;
        assert!(
            result.is_ok(),
            "resolve_threat on an unknown ID must not return Err"
        );

        let active = system.get_active_threats().await;
        assert!(
            active.is_empty(),
            "no active threats should exist after a no-op resolve"
        );
    }

    /// Edge case: a peer with a single connection is well below the flooding
    /// threshold — process_peer_behavior must succeed and leave the threat
    /// queue empty.
    #[tokio::test]
    async fn single_peer_connection_does_not_trigger_threat() {
        let system = ThreatDetectionSystem::new(None);
        let peer = PeerInfo {
            id: "peer-test-001".to_string(),
            address: "127.0.0.1:9001".to_string(),
        };

        system
            .process_peer_behavior(&peer)
            .await
            .expect("process_peer_behavior must succeed for a normal peer");

        let threats = system.get_recent_threats(None).await;
        assert!(
            threats.is_empty(),
            "a single peer connection must not raise a threat"
        );

        // Confirm the peer was tracked in the behavior map.
        let peer_stats = system.get_peer_stats().await;
        assert!(
            peer_stats.contains_key("peer-test-001"),
            "peer must appear in peer_stats after processing"
        );
        assert_eq!(
            peer_stats["peer-test-001"].connection_count,
            1,
            "connection_count must be 1 after one call"
        );
    }
}
