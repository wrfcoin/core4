#![allow(dead_code)]
#![allow(clippy::disallowed_types)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

/// Types of nodes in the Wrfcoin network
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum NodeType {
    /// Full nodes that maintain the entire blockchain
    Validator,
    /// Nodes that contribute raw data for analysis
    DataProvider,
    /// Nodes that perform computational analysis
    Analyzer,
    /// Nodes that develop and upgrade algorithms
    Developer,
    /// Lightweight nodes that only process transactions
    Light,
}

/// Status of a node
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeStatus {
    /// Node is active and connected
    Active,
    /// Node is offline or disconnected
    Offline,
    /// Node is syncing with the network
    Syncing,
    /// Node is banned due to malicious behavior
    Banned,
}

/// Geographic region of a node
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Region {
    NorthAmerica,
    SouthAmerica,
    Europe,
    Asia,
    Africa,
    Oceania,
}

/// Structure to track node metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeMetrics {
    /// Total uptime in seconds
    pub uptime: u64,
    /// Number of blocks mined/validated
    pub blocks_processed: u64,
    /// Number of transactions processed
    pub transactions_processed: u64,
    /// Number of raw data submissions
    pub data_contributions: u64,
    /// Number of analysis computations performed
    pub computations: u64,
    /// Number of algorithm upgrades proposed
    pub upgrades_proposed: u64,
    /// Number of algorithm upgrades accepted
    pub upgrades_accepted: u64,
    /// Reputation score (0-100)
    pub reputation: u8,
    /// Last time this node was seen online
    pub last_seen: SystemTime,
    /// Timestamp when node joined the network
    pub joined_at: SystemTime,
}

/// Network node representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    /// Unique identifier for the node
    pub id: String,
    /// Name of the node
    pub name: String,
    /// Node type
    pub node_type: NodeType,
    /// Current status
    pub status: NodeStatus,
    /// Version of the node software
    pub version: String,
    /// Geographic region
    pub region: Region,
    /// IP address (truncated for privacy)
    pub ip_prefix: String,
    /// Node's public key
    pub public_key: String,
    /// Performance metrics
    pub metrics: NodeMetrics,
    /// Additional attributes
    pub attributes: HashMap<String, String>,
}

/// Registry of all nodes in the network
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRegistry {
    /// Collection of all nodes indexed by ID
    pub nodes: HashMap<String, Node>,
    /// Summary metrics
    pub total_nodes: usize,
    pub active_nodes: usize,
    pub node_types: HashMap<NodeType, usize>,
    pub regional_distribution: HashMap<Region, usize>,
    pub average_uptime: u64,
    pub network_health: u8,         // 0-100
    pub decentralization_score: u8, // 0-100
}

impl NodeRegistry {
    /// Create a new empty node registry
    pub fn new() -> Self {
        NodeRegistry {
            nodes: HashMap::new(),
            total_nodes: 0,
            active_nodes: 0,
            node_types: HashMap::new(),
            regional_distribution: HashMap::new(),
            average_uptime: 0,
            network_health: 0,
            decentralization_score: 0,
        }
    }

    /// Add a node to the registry
    pub fn add_node(&mut self, node: Node) {
        let node_id = node.id.clone();
        self.nodes.insert(node_id, node);
        self.update_metrics();
    }

    /// Update a node's status
    #[allow(dead_code)]
    pub fn update_node_status(&mut self, node_id: &str, status: NodeStatus) {
        if let Some(node) = self.nodes.get_mut(node_id) {
            node.status = status;
            self.update_metrics();
        }
    }

    /// Update a node's metrics
    #[allow(dead_code)]
    pub fn update_node_metrics(&mut self, node_id: &str, metrics: NodeMetrics) {
        if let Some(node) = self.nodes.get_mut(node_id) {
            node.metrics = metrics;
            self.update_metrics();
        }
    }

    /// Remove a node from the registry
    #[allow(dead_code)]
    pub fn remove_node(&mut self, node_id: &str) {
        self.nodes.remove(node_id);
        self.update_metrics();
    }

    /// Get nodes by type
    pub fn get_nodes_by_type(&self, node_type: &NodeType) -> Vec<&Node> {
        self.nodes
            .values()
            .filter(|node| node.node_type == *node_type)
            .collect()
    }

    /// Get nodes by region
    pub fn get_nodes_by_region(&self, region: &Region) -> Vec<&Node> {
        self.nodes
            .values()
            .filter(|node| node.region == *region)
            .collect()
    }

    /// Get nodes by status
    pub fn get_nodes_by_status(&self, status: &NodeStatus) -> Vec<&Node> {
        self.nodes
            .values()
            .filter(|node| node.status == *status)
            .collect()
    }

    /// Get top nodes by reputation
    pub fn get_top_nodes_by_reputation(&self, limit: usize) -> Vec<&Node> {
        let mut nodes: Vec<&Node> = self.nodes.values().collect();
        nodes.sort_by(|a, b| b.metrics.reputation.cmp(&a.metrics.reputation));
        nodes.into_iter().take(limit).collect()
    }

    /// Get all active nodes
    pub fn get_active_nodes(&self) -> Vec<&Node> {
        self.get_nodes_by_status(&NodeStatus::Active)
    }

    /// Get all nodes in the registry
    pub fn get_all_nodes(&self) -> Vec<&Node> {
        self.nodes.values().collect()
    }

    /// Get a single node by ID
    pub fn get_node(&self, node_id: &str) -> Option<&Node> {
        self.nodes.get(node_id)
    }

    /// Get regional distribution of nodes
    pub fn get_regional_distribution(&self) -> &HashMap<Region, usize> {
        &self.regional_distribution
    }

    /// Get average node uptime
    pub fn get_average_uptime(&self) -> u64 {
        self.average_uptime
    }

    /// Get average node reputation
    pub fn get_average_reputation(&self) -> f64 {
        let total_reputation: u64 = self
            .nodes
            .values()
            .map(|node| node.metrics.reputation as u64)
            .sum();

        if self.total_nodes > 0 {
            total_reputation as f64 / self.total_nodes as f64
        } else {
            0.0
        }
    }

    /// Get total blocks processed by all nodes
    pub fn get_total_blocks_processed(&self) -> u64 {
        self.nodes
            .values()
            .map(|node| node.metrics.blocks_processed)
            .sum()
    }

    /// Get total transactions processed by all nodes
    pub fn get_total_transactions_processed(&self) -> u64 {
        self.nodes
            .values()
            .map(|node| node.metrics.transactions_processed)
            .sum()
    }

    /// Get total data contributions by all nodes
    pub fn get_total_data_contributions(&self) -> u64 {
        self.nodes
            .values()
            .map(|node| node.metrics.data_contributions)
            .sum()
    }

    /// Get total computations performed by all nodes
    pub fn get_total_computations(&self) -> u64 {
        self.nodes
            .values()
            .map(|node| node.metrics.computations)
            .sum()
    }

    /// Generate HTML table rows for top performing nodes
    pub fn generate_top_nodes_table_rows(&self, limit: usize) -> String {
        let mut top_nodes_rows = String::new();

        for node in self.get_top_nodes_by_reputation(limit) {
            let region_str = match node.region {
                Region::NorthAmerica => "North America",
                Region::SouthAmerica => "South America",
                Region::Europe => "Europe",
                Region::Asia => "Asia",
                Region::Africa => "Africa",
                Region::Oceania => "Oceania",
            };

            let node_type_str = match node.node_type {
                NodeType::Validator => "Validator",
                NodeType::DataProvider => "Data Provider",
                NodeType::Analyzer => "Analyzer",
                NodeType::Developer => "Developer",
                NodeType::Light => "Light",
            };

            top_nodes_rows.push_str(&format!(
                r#"<tr>
                    <td>{}</td>
                    <td>{}</td>
                    <td>{}</td>
                    <td>{}%</td>
                    <td>{}ms</td>
                    <td>{}</td>
                </tr>"#,
                node.id,
                node_type_str,
                region_str,
                node.metrics.uptime,
                250, // Placeholder response time in ms
                node.metrics.transactions_processed
            ));
        }

        top_nodes_rows
    }

    /// Calculate and update registry metrics
    fn update_metrics(&mut self) {
        self.total_nodes = self.nodes.len();
        self.active_nodes = self
            .nodes
            .values()
            .filter(|node| node.status == NodeStatus::Active)
            .count();

        // Update node type distribution
        self.node_types.clear();
        for node in self.nodes.values() {
            *self.node_types.entry(node.node_type.clone()).or_insert(0) += 1;
        }

        // Update regional distribution
        self.regional_distribution.clear();
        for node in self.nodes.values() {
            *self
                .regional_distribution
                .entry(node.region.clone())
                .or_insert(0) += 1;
        }

        // Calculate average uptime
        if self.total_nodes > 0 {
            let total_uptime: u64 = self.nodes.values().map(|node| node.metrics.uptime).sum();
            self.average_uptime = total_uptime / self.total_nodes as u64;
        } else {
            self.average_uptime = 0;
        }

        // Calculate network health (based on active nodes percentage and distribution)
        if self.total_nodes > 0 {
            let active_percentage = (self.active_nodes as f64 / self.total_nodes as f64) * 100.0;
            self.network_health = active_percentage as u8;
        } else {
            self.network_health = 0;
        }

        // Calculate decentralization score
        // Based on node type diversity, regional distribution, and validator count
        self.calculate_decentralization_score();
    }

    /// Calculate a decentralization score for the network
    fn calculate_decentralization_score(&mut self) {
        if self.total_nodes == 0 {
            self.decentralization_score = 0;
            return;
        }

        // Type diversity score (higher is better)
        let type_diversity_score = 20.0 * (self.node_types.len() as f64 / 5.0); // 5 types total

        // Regional diversity score (higher is better)
        let region_diversity_score = 30.0 * (self.regional_distribution.len() as f64 / 6.0); // 6 regions total

        // Validator concentration score (lower concentration is better)
        let validator_count = self.get_nodes_by_type(&NodeType::Validator).len();
        let validator_percentage = validator_count as f64 / self.total_nodes as f64;
        let validator_score = if validator_percentage > 0.0 && validator_percentage < 0.5 {
            // Optimal is between 10-30% validators
            let optimal_range = (0.1..=0.3).contains(&validator_percentage);
            if optimal_range { 30.0 } else { 15.0 }
        } else {
            5.0 // Too few or too many validators
        };

        // Geography concentration score
        let geography_score = if self
            .regional_distribution
            .values()
            .any(|&count| count > self.total_nodes / 2)
        {
            // One region has more than 50% of nodes - not good for decentralization
            10.0
        } else {
            20.0
        };

        // Combine scores
        let total_score =
            type_diversity_score + region_diversity_score + validator_score + geography_score;
        self.decentralization_score = total_score as u8;
    }

    /// Initialize with some sample nodes for testing
    pub fn initialize_sample_nodes(&mut self) {
        // Add a variety of node types across different regions
        let now = SystemTime::now();

        // Sample validator nodes
        for i in 1..=5 {
            let region = match i % 6 {
                0 => Region::NorthAmerica,
                1 => Region::Europe,
                2 => Region::Asia,
                3 => Region::SouthAmerica,
                4 => Region::Africa,
                _ => Region::Oceania,
            };

            let metrics = NodeMetrics {
                uptime: 86400 * (30 + i as u64), // 30+ days uptime
                blocks_processed: 1000 * i as u64,
                transactions_processed: 10000 * i as u64,
                data_contributions: 0,
                computations: 0,
                upgrades_proposed: 0,
                upgrades_accepted: 0,
                reputation: 70 + (i as u8 % 30), // 70-99 reputation
                last_seen: now,
                joined_at: now
                    .checked_sub(Duration::from_secs(86400 * 90))
                    .unwrap_or(now), // 90 days ago
            };

            let node = Node {
                id: format!("validator-{i}"),
                name: format!("Validator Node {i}"),
                node_type: NodeType::Validator,
                status: NodeStatus::Active,
                version: "1.0.0".to_string(),
                region,
                ip_prefix: format!("10.0.{i}.0"),
                public_key: format!("pk_validator_{i}"),
                metrics,
                attributes: HashMap::new(),
            };

            self.add_node(node);
        }

        // Sample data provider nodes
        for i in 1..=8 {
            let region = match i % 6 {
                0 => Region::NorthAmerica,
                1 => Region::Europe,
                2 => Region::Asia,
                3 => Region::SouthAmerica,
                4 => Region::Africa,
                _ => Region::Oceania,
            };

            let metrics = NodeMetrics {
                uptime: 86400 * (10 + i as u64), // 10+ days uptime
                blocks_processed: 0,
                transactions_processed: 100 * i as u64,
                data_contributions: 500 * i as u64,
                computations: 0,
                upgrades_proposed: 0,
                upgrades_accepted: 0,
                reputation: 60 + (i as u8 % 30), // 60-89 reputation
                last_seen: now,
                joined_at: now
                    .checked_sub(Duration::from_secs(86400 * 60))
                    .unwrap_or(now), // 60 days ago
            };

            let status = if i % 4 == 0 {
                NodeStatus::Offline
            } else {
                NodeStatus::Active
            };

            let node = Node {
                id: format!("data-provider-{i}"),
                name: format!("Data Provider {i}"),
                node_type: NodeType::DataProvider,
                status,
                version: "1.0.0".to_string(),
                region,
                ip_prefix: format!("10.1.{i}.0"),
                public_key: format!("pk_data_provider_{i}"),
                metrics,
                attributes: HashMap::new(),
            };

            self.add_node(node);
        }

        // Sample analyzer nodes
        for i in 1..=6 {
            let region = match i % 6 {
                0 => Region::NorthAmerica,
                1 => Region::Europe,
                2 => Region::Asia,
                3 => Region::SouthAmerica,
                4 => Region::Africa,
                _ => Region::Oceania,
            };

            let metrics = NodeMetrics {
                uptime: 86400 * (15 + i as u64), // 15+ days uptime
                blocks_processed: 200 * i as u64,
                transactions_processed: 500 * i as u64,
                data_contributions: 0,
                computations: 1000 * i as u64,
                upgrades_proposed: 0,
                upgrades_accepted: 0,
                reputation: 65 + (i as u8 % 25), // 65-89 reputation
                last_seen: now,
                joined_at: now
                    .checked_sub(Duration::from_secs(86400 * 45))
                    .unwrap_or(now), // 45 days ago
            };

            let status = if i % 5 == 0 {
                NodeStatus::Syncing
            } else {
                NodeStatus::Active
            };

            let node = Node {
                id: format!("analyzer-{i}"),
                name: format!("Analyzer Node {i}"),
                node_type: NodeType::Analyzer,
                status,
                version: "1.0.0".to_string(),
                region,
                ip_prefix: format!("10.2.{i}.0"),
                public_key: format!("pk_analyzer_{i}"),
                metrics,
                attributes: HashMap::new(),
            };

            self.add_node(node);
        }

        // Sample developer nodes
        for i in 1..=3 {
            let region = match i % 3 {
                0 => Region::NorthAmerica,
                1 => Region::Europe,
                _ => Region::Asia,
            };

            let metrics = NodeMetrics {
                uptime: 86400 * (20 + i as u64), // 20+ days uptime
                blocks_processed: 100 * i as u64,
                transactions_processed: 300 * i as u64,
                data_contributions: 0,
                computations: 200 * i as u64,
                upgrades_proposed: 5 * i as u64,
                upgrades_accepted: 2 * i as u64,
                reputation: 80 + (i as u8 % 20), // 80-99 reputation
                last_seen: now,
                joined_at: now
                    .checked_sub(Duration::from_secs(86400 * 120))
                    .unwrap_or(now), // 120 days ago
            };

            let node = Node {
                id: format!("developer-{i}"),
                name: format!("Developer Node {i}"),
                node_type: NodeType::Developer,
                status: NodeStatus::Active,
                version: "1.0.0".to_string(),
                region,
                ip_prefix: format!("10.3.{i}.0"),
                public_key: format!("pk_developer_{i}"),
                metrics,
                attributes: HashMap::new(),
            };

            self.add_node(node);
        }

        // Sample light nodes
        for i in 1..=12 {
            let region = match i % 6 {
                0 => Region::NorthAmerica,
                1 => Region::Europe,
                2 => Region::Asia,
                3 => Region::SouthAmerica,
                4 => Region::Africa,
                _ => Region::Oceania,
            };

            let metrics = NodeMetrics {
                uptime: 86400 * (5 + i as u64 % 10), // 5-14 days uptime
                blocks_processed: 0,
                transactions_processed: 50 * i as u64,
                data_contributions: 0,
                computations: 0,
                upgrades_proposed: 0,
                upgrades_accepted: 0,
                reputation: 50 + (i as u8 % 30), // 50-79 reputation
                last_seen: now,
                joined_at: now
                    .checked_sub(Duration::from_secs(86400 * 30))
                    .unwrap_or(now), // 30 days ago
            };

            let status = if i % 3 == 0 {
                NodeStatus::Offline
            } else {
                NodeStatus::Active
            };

            let node = Node {
                id: format!("light-{i}"),
                name: format!("Light Node {i}"),
                node_type: NodeType::Light,
                status,
                version: "1.0.0".to_string(),
                region,
                ip_prefix: format!("10.4.{i}.0"),
                public_key: format!("pk_light_{i}"),
                metrics,
                attributes: HashMap::new(),
            };

            self.add_node(node);
        }

        // Calculate metrics after adding all nodes
        self.update_metrics();
    }
}
