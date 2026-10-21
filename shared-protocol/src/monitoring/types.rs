use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum NodeStatus {
    Excellent,
    Good,
    Fair,
    Poor,
    Offline,
    Unknown,
}

impl NodeStatus {
    pub fn as_label(self) -> &'static str {
        match self {
            Self::Excellent => "excellent",
            Self::Good => "good",
            Self::Fair => "fair",
            Self::Poor => "poor",
            Self::Offline => "offline",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum NodeType {
    BlockchainMaintainer,
    RawDataSubmitter,
    AnalysisMiner,
    AlgorithmUpgrader,
    NftValidator,
    General,
}

impl NodeType {
    pub fn as_label(self) -> &'static str {
        match self {
            Self::BlockchainMaintainer => "blockchain_maintainer",
            Self::RawDataSubmitter => "raw_data_submitter",
            Self::AnalysisMiner => "analysis_miner",
            Self::AlgorithmUpgrader => "algorithm_upgrader",
            Self::NftValidator => "nft_validator",
            Self::General => "general",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Region {
    NorthAmerica,
    SouthAmerica,
    Europe,
    Africa,
    Asia,
    Oceania,
    Unknown,
}

impl Region {
    pub fn as_label(self) -> &'static str {
        match self {
            Self::NorthAmerica => "North America",
            Self::SouthAmerica => "South America",
            Self::Europe => "Europe",
            Self::Africa => "Africa",
            Self::Asia => "Asia",
            Self::Oceania => "Oceania",
            Self::Unknown => "Unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NodeMetricPoint {
    pub timestamp: DateTime<Utc>,
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub disk_usage: f64,
    pub network_in: u64,
    pub network_out: u64,
    pub block_height: u64,
    pub mempool_size: u64,
    pub peer_count: u32,
    pub response_time_ms: u64,
    pub uptime_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PerformanceScore {
    pub overall: u8,
    pub reliability: u8,
    pub responsiveness: u8,
    pub resource_utilization: u8,
    pub contribution: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Location {
    pub region: Region,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub country_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NodeMetrics {
    pub node_id: String,
    pub name: String,
    pub node_type: NodeType,
    pub status: NodeStatus,
    pub location: Location,
    pub score: PerformanceScore,
    pub current: NodeMetricPoint,
    pub history: Vec<NodeMetricPoint>,
    pub reputation: u8,
    pub last_updated: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ColorCode {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl ColorCode {
    pub fn as_hex(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HeatmapMetrics {
    pub cpu: f64,
    pub memory: f64,
    pub disk: f64,
    pub network_kbps: f64,
    pub response_time_ms: u64,
    pub uptime_secs: u64,
    pub last_updated: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HeatmapNode {
    pub node_id: String,
    pub name: String,
    pub node_type: String,
    pub status: String,
    pub color: ColorCode,
    pub x: f64,
    pub y: f64,
    pub size: u8,
    pub tooltip: String,
    pub score: u8,
    pub region: String,
    pub metrics: HeatmapMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HeatmapSection {
    pub title: String,
    pub description: String,
    pub nodes: Vec<HeatmapNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetworkHealthSummary {
    pub total_nodes: usize,
    pub active_nodes: usize,
    pub active_percentage: f64,
    pub status: String,
    pub status_counts: HashMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Heatmap {
    pub title: String,
    pub description: String,
    pub timestamp: DateTime<Utc>,
    pub generated_at: DateTime<Utc>,
    pub sections: Vec<HeatmapSection>,
    pub network_health: NetworkHealthSummary,
    pub points: Vec<HeatmapNode>,
}
