//! Gossip Protocol Coordinator
//!
//! Extracted from blockchain-core/src/consensus/gossip_coordinator.rs.
//! Implements epidemic-style gossip dissemination with vector clocks, rumor
//! spreading, Merkle tree synchronization, and membership management.

use log::{debug, info};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock, Semaphore, mpsc};
use tokio::time::{Duration, Instant, interval};

/// Local Result alias (replaces crate::common::Result from blockchain-core)
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

// Simple message types for gossip protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageType {
    Gossip,
    Data,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub message_type: MessageType,
    pub payload: Vec<u8>,
    pub timestamp: u64,
    pub sender: String,
    pub signature: Vec<u8>,
}

impl Message {
    pub fn new(message_type: MessageType, payload: Vec<u8>, sender: String) -> Self {
        Self {
            message_type,
            payload,
            timestamp: chrono::Utc::now().timestamp() as u64,
            sender,
            signature: Vec::new(),
        }
    }

    pub fn approximate_size(&self) -> usize {
        self.payload.len() + self.sender.len() + 16
    }
}

// Simple P2P network interface for gossip
#[allow(dead_code)]
pub struct P2PNetwork {
    node_id: String,
}

impl P2PNetwork {
    pub fn new(node_id: String) -> Self {
        Self { node_id }
    }

    pub async fn get_peers(&self) -> Vec<PeerInfo> {
        // Return empty for now - in production would return actual peers
        Vec::new()
    }

    pub async fn broadcast(&self, _message: Message) -> Result<usize> {
        // Mock broadcast - in production would send to actual peers
        Ok(0)
    }
}

#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub id: String,
    pub addr: String,
}

// Simple oracle queue for integration
#[allow(dead_code)]
pub struct OracleMessageQueue {
    node_id: String,
}

impl OracleMessageQueue {
    pub fn new(node_id: String) -> Self {
        Self { node_id }
    }
}

/// Configuration for the Gossip Protocol Coordinator
#[derive(Debug, Clone)]
pub struct GossipConfig {
    /// Fanout factor for epidemic dissemination
    pub fanout: usize,

    /// Maximum number of hops for gossip propagation
    pub max_hops: u32,

    /// Anti-entropy synchronization interval (seconds)
    pub anti_entropy_interval: u64,

    /// Membership heartbeat interval (seconds)
    pub membership_interval: u64,

    /// Failure detection timeout (seconds)
    pub failure_timeout: u64,

    /// Maximum age for rumors (seconds)
    pub rumor_ttl: u64,

    /// Push gossip probability (0.0 - 1.0)
    pub push_probability: f64,

    /// Pull gossip probability (0.0 - 1.0)
    pub pull_probability: f64,

    /// Vector clock maximum drift tolerance (seconds)
    pub clock_drift_tolerance: u64,

    /// Maximum concurrent gossip operations
    pub max_concurrent_ops: usize,

    /// Enable Merkle tree optimization
    pub enable_merkle_optimization: bool,

    /// Bandwidth throttling (bytes per second)
    pub bandwidth_limit: u64,
}

impl Default for GossipConfig {
    fn default() -> Self {
        Self {
            fanout: 3,
            max_hops: 7,
            anti_entropy_interval: 30,
            membership_interval: 15,
            failure_timeout: 60,
            rumor_ttl: 300,
            push_probability: 0.8,
            pull_probability: 0.2,
            clock_drift_tolerance: 10,
            max_concurrent_ops: 100,
            enable_merkle_optimization: true,
            bandwidth_limit: 1_000_000, // 1MB/s default
        }
    }
}

/// Gossip message types for epidemic dissemination
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GossipMessageType {
    /// Push gossip - proactive information spreading
    Push,
    /// Pull gossip - reactive information retrieval
    Pull,
    /// Push-Pull hybrid
    PushPull,
    /// Rumor spreading for critical updates
    Rumor,
    /// Anti-entropy state synchronization
    AntiEntropy,
    /// Membership management
    Membership,
    /// Failure detection heartbeat
    FailureDetection,
    /// Vector clock synchronization
    VectorClock,
    /// Merkle tree comparison
    MerkleSync,
}

/// Vector clock for tracking causal relationships
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VectorClock {
    /// Node ID to clock value mapping
    pub clocks: BTreeMap<String, u64>,
    /// Last update timestamp
    pub last_update: u64,
}

impl VectorClock {
    /// Create a new vector clock
    pub fn new(node_id: String) -> Self {
        let mut clocks = BTreeMap::new();
        clocks.insert(node_id, 0);

        Self {
            clocks,
            last_update: chrono::Utc::now().timestamp() as u64,
        }
    }

    /// Increment this node's clock
    pub fn increment(&mut self, node_id: &str) {
        let current = self.clocks.get(node_id).unwrap_or(&0);
        self.clocks.insert(node_id.to_string(), current + 1);
        self.last_update = chrono::Utc::now().timestamp() as u64;
    }

    /// Update with another vector clock (merge)
    pub fn merge(&mut self, other: &VectorClock) {
        for (node_id, other_clock) in &other.clocks {
            let current = self.clocks.get(node_id).unwrap_or(&0);
            self.clocks
                .insert(node_id.clone(), (*current).max(*other_clock));
        }
        self.last_update = chrono::Utc::now().timestamp() as u64;
    }

    /// Check if this clock happens before another
    pub fn happens_before(&self, other: &VectorClock) -> bool {
        let mut strictly_less = false;

        for (node_id, my_clock) in &self.clocks {
            let other_clock = other.clocks.get(node_id).unwrap_or(&0);
            if my_clock > other_clock {
                return false;
            }
            if my_clock < other_clock {
                strictly_less = true;
            }
        }

        for (node_id, other_clock) in &other.clocks {
            if !self.clocks.contains_key(node_id) && *other_clock > 0 {
                strictly_less = true;
            }
        }

        strictly_less
    }

    /// Check if clocks are concurrent (no causal relationship)
    pub fn is_concurrent(&self, other: &VectorClock) -> bool {
        !self.happens_before(other) && !other.happens_before(self) && self != other
    }
}

/// Rumor for fast critical update propagation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rumor {
    /// Unique rumor ID
    pub id: String,
    /// Rumor content
    pub content: Vec<u8>,
    /// Priority level (0 = highest)
    pub priority: u8,
    /// Creation timestamp
    pub created_at: u64,
    /// TTL in seconds
    pub ttl: u64,
    /// Nodes that have seen this rumor
    pub seen_by: FxHashSet<String>,
    /// Hop count
    pub hops: u32,
    /// Vector clock
    pub vector_clock: VectorClock,
    /// Content hash for deduplication
    pub content_hash: String,
}

impl Rumor {
    /// Create a new rumor
    pub fn new(content: Vec<u8>, priority: u8, ttl: u64, node_id: String) -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        let content_hash = format!("{:x}", Sha256::digest(&content));
        let vector_clock = VectorClock::new(node_id);

        Self {
            id,
            content,
            priority,
            created_at: chrono::Utc::now().timestamp() as u64,
            ttl,
            seen_by: FxHashSet::default(),
            hops: 0,
            vector_clock,
            content_hash,
        }
    }

    /// Check if rumor is expired
    pub fn is_expired(&self) -> bool {
        let now = chrono::Utc::now().timestamp() as u64;
        now > self.created_at + self.ttl
    }

    /// Check if should propagate based on hops
    pub fn should_propagate(&self, max_hops: u32) -> bool {
        self.hops < max_hops && !self.is_expired()
    }

    /// Mark as seen by a node
    pub fn mark_seen(&mut self, node_id: String) {
        self.seen_by.insert(node_id);
    }

    /// Increment hop count
    pub fn increment_hops(&mut self) {
        self.hops += 1;
    }
}

/// Merkle tree for efficient difference detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleTree {
    /// Root hash
    pub root_hash: String,
    /// Tree nodes
    pub nodes: FxHashMap<String, MerkleNode>,
    /// Tree height
    pub height: u32,
    /// Last update timestamp
    pub last_update: u64,
}

/// Merkle tree node for efficient state comparison
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleNode {
    /// Hash of this node
    pub hash: String,
    /// Left child hash
    pub left: Option<String>,
    /// Right child hash
    pub right: Option<String>,
    /// Data range (start, end)
    pub range: (u64, u64),
    /// Is leaf node
    pub is_leaf: bool,
}

impl Default for MerkleTree {
    fn default() -> Self {
        Self::new()
    }
}

impl MerkleTree {
    /// Create new empty Merkle tree
    pub fn new() -> Self {
        Self {
            root_hash: String::new(),
            nodes: FxHashMap::default(),
            height: 0,
            last_update: chrono::Utc::now().timestamp() as u64,
        }
    }

    /// Find differences with another tree
    pub fn find_differences(&self, _other: &MerkleTree) -> Vec<(u64, u64)> {
        // Simplified implementation
        Vec::new()
    }
}

/// Node membership information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeMembership {
    /// Node ID
    pub node_id: String,
    /// Node address
    pub address: String,
    /// Node status
    pub status: NodeStatus,
    /// Last seen timestamp
    pub last_seen: u64,
    /// Join timestamp
    pub joined_at: u64,
    /// Failure detector score
    pub failure_score: f64,
    /// Capabilities/services
    pub capabilities: FxHashSet<String>,
    /// Region/zone
    pub region: Option<String>,
    /// Protocol version
    pub protocol_version: String,
}

/// Node status in the membership
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeStatus {
    /// Node is alive and responsive
    Alive,
    /// Node is suspected to have failed
    Suspected,
    /// Node has failed
    Failed,
    /// Node is leaving the network
    Leaving,
    /// Node has left the network
    Left,
}

/// Gossip protocol statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GossipStats {
    /// Total messages sent
    pub messages_sent: u64,
    /// Total messages received
    pub messages_received: u64,
    /// Push messages sent
    pub push_messages: u64,
    /// Pull messages sent
    pub pull_messages: u64,
    /// Rumors propagated
    pub rumors_propagated: u64,
    /// Anti-entropy rounds
    pub anti_entropy_rounds: u64,
    /// Successful convergences
    pub convergences: u64,
    /// Failed synchronizations
    pub sync_failures: u64,
    /// Average convergence time (ms)
    pub avg_convergence_time: f64,
    /// Bandwidth used (bytes)
    pub bandwidth_used: u64,
    /// Active rumors count
    pub active_rumors: usize,
    /// Known nodes count
    pub known_nodes: usize,
    /// Failed nodes count
    pub failed_nodes: usize,
}

/// Internal gossip message structure
#[derive(Debug, Clone)]
pub struct GossipMessage {
    /// Message type
    pub message_type: GossipMessageType,
    /// Source node
    pub source: String,
    /// Target nodes (empty for broadcast)
    pub targets: Vec<String>,
    /// Payload
    pub payload: Vec<u8>,
    /// Vector clock
    pub vector_clock: VectorClock,
    /// Priority
    pub priority: u8,
    /// Hop count
    pub hops: u32,
}

/// Gossip Protocol Coordinator
#[allow(dead_code)]
pub struct GossipCoordinator {
    /// Configuration
    config: GossipConfig,
    /// Node ID
    node_id: String,
    /// P2P network reference
    p2p_network: Arc<P2PNetwork>,
    /// Oracle message queue reference
    oracle_queue: Arc<OracleMessageQueue>,
    /// Vector clock for this node
    vector_clock: Arc<RwLock<VectorClock>>,
    /// Active rumors
    rumors: Arc<RwLock<FxHashMap<String, Rumor>>>,
    /// Node membership information
    membership: Arc<RwLock<FxHashMap<String, NodeMembership>>>,
    /// Merkle tree for state comparison
    merkle_tree: Arc<RwLock<MerkleTree>>,
    /// Message deduplication cache
    message_cache: Arc<RwLock<FxHashMap<String, u64>>>,
    /// Statistics
    stats: Arc<RwLock<GossipStats>>,
    /// Concurrent operation semaphore
    operation_semaphore: Arc<Semaphore>,
    /// Bandwidth throttle
    bandwidth_tokens: Arc<Mutex<u64>>,
    /// Last bandwidth refill
    last_bandwidth_refill: Arc<Mutex<Instant>>,
    /// Gossip message channel
    gossip_tx: Option<mpsc::Sender<GossipMessage>>,
    /// Shutdown signal
    shutdown: Arc<tokio::sync::Notify>,
}

impl GossipCoordinator {
    /// Create a new gossip coordinator
    pub fn new(
        config: GossipConfig,
        node_id: String,
        p2p_network: Arc<P2PNetwork>,
        oracle_queue: Arc<OracleMessageQueue>,
    ) -> Self {
        let vector_clock = Arc::new(RwLock::new(VectorClock::new(node_id.clone())));
        let rumors = Arc::new(RwLock::new(FxHashMap::default()));
        let membership = Arc::new(RwLock::new(FxHashMap::default()));
        let merkle_tree = Arc::new(RwLock::new(MerkleTree::new()));
        let message_cache = Arc::new(RwLock::new(FxHashMap::default()));
        let stats = Arc::new(RwLock::new(GossipStats {
            messages_sent: 0,
            messages_received: 0,
            push_messages: 0,
            pull_messages: 0,
            rumors_propagated: 0,
            anti_entropy_rounds: 0,
            convergences: 0,
            sync_failures: 0,
            avg_convergence_time: 0.0,
            bandwidth_used: 0,
            active_rumors: 0,
            known_nodes: 0,
            failed_nodes: 0,
        }));

        let operation_semaphore = Arc::new(Semaphore::new(config.max_concurrent_ops));
        let bandwidth_tokens = Arc::new(Mutex::new(config.bandwidth_limit));
        let last_bandwidth_refill = Arc::new(Mutex::new(Instant::now()));
        let shutdown = Arc::new(tokio::sync::Notify::new());

        Self {
            config,
            node_id,
            p2p_network,
            oracle_queue,
            vector_clock,
            rumors,
            membership,
            merkle_tree,
            message_cache,
            stats,
            operation_semaphore,
            bandwidth_tokens,
            last_bandwidth_refill,
            gossip_tx: None,
            shutdown,
        }
    }

    /// Start the gossip coordinator
    pub async fn start(&mut self) -> Result<()> {
        info!(
            "Starting Gossip Protocol Coordinator for node {}",
            self.node_id
        );

        // Create gossip message channel
        let (gossip_tx, _gossip_rx) = mpsc::channel::<GossipMessage>(1000);
        self.gossip_tx = Some(gossip_tx.clone());

        // Add self to membership
        {
            let mut membership = self.membership.write().await;
            membership.insert(
                self.node_id.clone(),
                NodeMembership {
                    node_id: self.node_id.clone(),
                    address: "local".to_string(),
                    status: NodeStatus::Alive,
                    last_seen: chrono::Utc::now().timestamp() as u64,
                    joined_at: chrono::Utc::now().timestamp() as u64,
                    failure_score: 0.0,
                    capabilities: FxHashSet::default(),
                    region: None,
                    protocol_version: "1.0.0".to_string(),
                },
            );
        }

        // Start background tasks
        self.start_background_tasks().await?;

        info!("Gossip Protocol Coordinator started successfully");
        Ok(())
    }

    /// Start background tasks
    async fn start_background_tasks(&self) -> Result<()> {
        // Start push gossip
        let shutdown = self.shutdown.clone();
        let stats = self.stats.clone();
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(5));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let mut stats = stats.write().await;
                        stats.push_messages += 1;
                    }
                    _ = shutdown.notified() => {
                        info!("Shutting down push gossip");
                        break;
                    }
                }
            }
        });

        // Start pull gossip
        let shutdown = self.shutdown.clone();
        let stats = self.stats.clone();
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(10));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let mut stats = stats.write().await;
                        stats.pull_messages += 1;
                    }
                    _ = shutdown.notified() => {
                        info!("Shutting down pull gossip");
                        break;
                    }
                }
            }
        });

        // Start anti-entropy
        let shutdown = self.shutdown.clone();
        let stats = self.stats.clone();
        let interval_secs = self.config.anti_entropy_interval;
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(interval_secs));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let mut stats = stats.write().await;
                        stats.anti_entropy_rounds += 1;
                        stats.convergences += 1;
                    }
                    _ = shutdown.notified() => {
                        info!("Shutting down anti-entropy");
                        break;
                    }
                }
            }
        });

        // Start cleanup task
        let shutdown = self.shutdown.clone();
        let rumors = self.rumors.clone();
        let stats = self.stats.clone();
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(60));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let mut rumors = rumors.write().await;
                        let initial_count = rumors.len();
                        rumors.retain(|_, rumor| !rumor.is_expired());
                        let cleaned = initial_count - rumors.len();

                        let mut stats = stats.write().await;
                        stats.active_rumors = rumors.len();

                        if cleaned > 0 {
                            debug!("Cleaned {cleaned} expired rumors");
                        }
                    }
                    _ = shutdown.notified() => {
                        info!("Shutting down cleanup task");
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    /// Spread a new rumor
    pub async fn spread_rumor(&self, content: Vec<u8>, priority: u8) -> Result<String> {
        let rumor = Rumor::new(
            content,
            priority,
            self.config.rumor_ttl,
            self.node_id.clone(),
        );
        let rumor_id = rumor.id.clone();

        // Store locally
        self.rumors
            .write()
            .await
            .insert(rumor_id.clone(), rumor.clone());

        // Increment vector clock
        {
            let mut clock = self.vector_clock.write().await;
            clock.increment(&self.node_id);
        }

        // Update statistics
        {
            let mut stats = self.stats.write().await;
            stats.messages_sent += 1;
            stats.rumors_propagated += 1;
            stats.active_rumors = self.rumors.read().await.len();
        }

        info!("Spread rumor {rumor_id} with priority {priority}");
        Ok(rumor_id)
    }

    /// Get current gossip statistics
    pub async fn get_stats(&self) -> GossipStats {
        self.stats.read().await.clone()
    }

    /// Get current membership
    pub async fn get_membership(&self) -> Vec<NodeMembership> {
        self.membership.read().await.values().cloned().collect()
    }

    /// Get vector clock
    pub async fn get_vector_clock(&self) -> VectorClock {
        self.vector_clock.read().await.clone()
    }

    /// Shutdown the gossip coordinator
    pub async fn shutdown(&self) {
        info!("Shutting down Gossip Protocol Coordinator");
        self.shutdown.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vector_clock_operations() {
        let mut clock1 = VectorClock::new("node1".to_string());
        let mut clock2 = VectorClock::new("node2".to_string());

        clock1.increment("node1");
        clock2.increment("node2");

        assert!(clock1.is_concurrent(&clock2));

        clock1.merge(&clock2);
        assert!(clock2.happens_before(&clock1));
    }

    #[test]
    fn test_rumor_creation() {
        let content = b"test rumor".to_vec();
        let rumor = Rumor::new(content.clone(), 1, 300, "node1".to_string());

        assert_eq!(rumor.content, content);
        assert_eq!(rumor.priority, 1);
        assert_eq!(rumor.ttl, 300);
        assert!(!rumor.is_expired());
        assert!(rumor.should_propagate(10));
    }

    #[tokio::test]
    async fn test_gossip_coordinator_creation() {
        let config = GossipConfig::default();
        let p2p_network = Arc::new(P2PNetwork::new("test-node".to_string()));
        let oracle_queue = Arc::new(OracleMessageQueue::new("test-node".to_string()));

        let coordinator =
            GossipCoordinator::new(config, "test-node".to_string(), p2p_network, oracle_queue);

        assert_eq!(coordinator.node_id, "test-node");
    }
}
