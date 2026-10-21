use std::collections::{BTreeMap, HashMap};
use std::sync::{LazyLock, Mutex};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use wrfcoin_shared_protocol::monitoring::{
    ColorCode, CoreMetricSnapshot, CORE_METRICS, Heatmap, HeatmapMetrics, HeatmapNode,
    HeatmapSection, Location, NetworkHealthSummary, NodeMetricPoint, NodeMetrics, NodeStatus,
    NodeType, P2pBandwidthSnapshot, P2pMeshSnapshot, PerformanceScore, Region,
};

const HISTORY_LIMIT: usize = 64;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkTelemetry {
    pub connected_peers: usize,
    pub mesh_peers_blocks: usize,
    pub mesh_peers_transactions: usize,
    pub mesh_peers_weather: usize,
    pub mesh_peers_forecast: usize,
    pub blocks_received: u64,
    pub blocks_published: u64,
    pub txs_received: u64,
    pub txs_published: u64,
    pub weather_msgs_received: u64,
    pub weather_msgs_published: u64,
    pub forecast_tasks_received: u64,
    pub forecast_tasks_published: u64,
    pub bytes_inbound: u64,
    pub bytes_outbound: u64,
    // Per-topic inbound bytes
    pub bytes_inbound_blocks: u64,
    pub bytes_inbound_txs: u64,
    pub bytes_inbound_weather: u64,
    pub bytes_inbound_forecast: u64,
    // Per-topic outbound bytes
    pub bytes_outbound_blocks: u64,
    pub bytes_outbound_txs: u64,
    pub bytes_outbound_weather: u64,
    pub bytes_outbound_forecast: u64,
    pub uptime_secs: u64,
    /// Whether the node is operating as a Wi-Fi access point (from WAN failover monitor, #1802).
    #[serde(default)]
    pub ap_mode_active: bool,
    /// Whether the node is on a metered (cellular/LTE) link (from WAN failover monitor, #1802).
    #[serde(default)]
    pub is_metered: bool,
}

#[derive(Debug, Clone)]
pub struct MonitoringInputs {
    pub node_id: String,
    pub node_name: String,
    pub chain_height: u64,
    pub tip_hash: String,
    pub account_count: usize,
    pub nft_count: usize,
    pub pending_transactions: usize,
    pub quality_event_count: usize,
    /// Per-reason slashing event counts keyed by [`SlashingReason::as_str()`].
    pub slashing_events_by_reason: HashMap<String, u64>,
    pub transactions_processed_total: u64,
    pub transactions_failed_total: u64,
    pub zk_proofs_observed_total: u64,
    pub state_diverged: bool,
    /// Height of the most recently finalized block. Callers compute this as
    /// `chain_height.saturating_sub(MAX_REORG_DEPTH)` (or whichever stronger
    /// finality guarantee the chain layer provides). Used to populate
    /// `wrfcoin_chain_finalized_height` (#1148).
    pub chain_finalized_height: u64,
    /// Height at which state pruning last ran (#1202).
    pub state_pruning_last_height: u64,
    pub network: Option<NetworkTelemetry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoringSnapshot {
    pub chain_height: u64,
    pub tip_hash: String,
    pub pending_transactions: usize,
    pub account_count: usize,
    pub nft_count: usize,
    pub quality_event_count: usize,
    /// Per-reason slashing event counts keyed by [`SlashingReason::as_str()`].
    pub slashing_events_by_reason: HashMap<String, u64>,
    pub network_telemetry: NetworkTelemetry,
    pub network_health: NetworkHealthSummary,
    pub nodes: Vec<NodeMetrics>,
    pub heatmap: Heatmap,
}

#[derive(Default)]
struct NodeMetricsRegistry {
    nodes: BTreeMap<String, NodeMetrics>,
}

impl NodeMetricsRegistry {
    fn observe_local_node(&mut self, inputs: &MonitoringInputs) {
        let now = Utc::now();
        let network = inputs.network.clone().unwrap_or_default();
        let point = NodeMetricPoint {
            timestamp: now,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            disk_usage: 0.0,
            network_in: network.bytes_inbound,
            network_out: network.bytes_outbound,
            block_height: inputs.chain_height,
            mempool_size: inputs.pending_transactions as u64,
            peer_count: network.connected_peers as u32,
            response_time_ms: if network.connected_peers > 0 { 150 } else { 300 },
            uptime_secs: network.uptime_secs,
        };

        let score = compute_score(inputs, &network);
        let status = status_from_score(score.overall, &network, inputs.chain_height);

        let entry = self
            .nodes
            .entry(inputs.node_id.clone())
            .or_insert_with(|| NodeMetrics {
                node_id: inputs.node_id.clone(),
                name: inputs.node_name.clone(),
                node_type: NodeType::BlockchainMaintainer,
                status,
                location: Location {
                    region: Region::Unknown,
                    latitude: None,
                    longitude: None,
                    country_code: None,
                },
                score: score.clone(),
                current: point.clone(),
                history: Vec::new(),
                reputation: score.overall,
                last_updated: now,
            });

        entry.name = inputs.node_name.clone();
        entry.status = status;
        entry.score = score.clone();
        entry.current = point.clone();
        entry.reputation = score.overall;
        entry.last_updated = now;
        entry.history.push(point);
        if entry.history.len() > HISTORY_LIMIT {
            let excess = entry.history.len() - HISTORY_LIMIT;
            entry.history.drain(0..excess);
        }
    }

    fn nodes(&self) -> Vec<NodeMetrics> {
        self.nodes.values().cloned().collect()
    }
}

static NODE_REGISTRY: LazyLock<Mutex<NodeMetricsRegistry>> =
    LazyLock::new(|| Mutex::new(NodeMetricsRegistry::default()));

pub fn build_snapshot(inputs: MonitoringInputs) -> MonitoringSnapshot {
    let mut registry = NODE_REGISTRY
        .lock()
        .expect("monitoring registry lock poisoned");
    build_snapshot_with_registry(inputs, &mut registry)
}

fn build_snapshot_with_registry(
    inputs: MonitoringInputs,
    registry: &mut NodeMetricsRegistry,
) -> MonitoringSnapshot {
    let network_telemetry = inputs.network.clone().unwrap_or_default();
    registry.observe_local_node(&inputs);
    let nodes = registry.nodes();

    let network_health = compute_network_health(&nodes);
    let heatmap = build_heatmap(&nodes, &network_health);

    CORE_METRICS.apply_snapshot(&CoreMetricSnapshot {
        block_height: inputs.chain_height,
        mempool_size: inputs.pending_transactions as u64,
        connected_peers: network_telemetry.connected_peers as u64,
        transactions_processed_total: inputs.transactions_processed_total,
        transactions_failed_total: inputs.transactions_failed_total,
        zk_proofs_observed_total: inputs.zk_proofs_observed_total,
        slashing_events_by_reason: inputs.slashing_events_by_reason.clone(),
        state_diverged: inputs.state_diverged,
        chain_finalized_height: inputs.chain_finalized_height,
        state_pruning_last_height: inputs.state_pruning_last_height,
        ..Default::default()
    });

    CORE_METRICS.apply_p2p_snapshot(&P2pBandwidthSnapshot {
        bytes_received: [
            network_telemetry.bytes_inbound_blocks,
            network_telemetry.bytes_inbound_txs,
            network_telemetry.bytes_inbound_weather,
            network_telemetry.bytes_inbound_forecast,
        ],
        bytes_sent: [
            network_telemetry.bytes_outbound_blocks,
            network_telemetry.bytes_outbound_txs,
            network_telemetry.bytes_outbound_weather,
            network_telemetry.bytes_outbound_forecast,
        ],
        messages_received: [
            network_telemetry.blocks_received,
            network_telemetry.txs_received,
            network_telemetry.weather_msgs_received,
            network_telemetry.forecast_tasks_received,
        ],
        messages_sent: [
            network_telemetry.blocks_published,
            network_telemetry.txs_published,
            network_telemetry.weather_msgs_published,
            network_telemetry.forecast_tasks_published,
        ],
    });

    CORE_METRICS.apply_p2p_mesh_snapshot(&P2pMeshSnapshot {
        mesh_peers: [
            network_telemetry.mesh_peers_blocks as u64,
            network_telemetry.mesh_peers_transactions as u64,
            network_telemetry.mesh_peers_weather as u64,
            network_telemetry.mesh_peers_forecast as u64,
        ],
    });

    MonitoringSnapshot {
        chain_height: inputs.chain_height,
        tip_hash: inputs.tip_hash,
        pending_transactions: inputs.pending_transactions,
        account_count: inputs.account_count,
        nft_count: inputs.nft_count,
        quality_event_count: inputs.quality_event_count,
        slashing_events_by_reason: inputs.slashing_events_by_reason,
        network_telemetry,
        network_health,
        nodes,
        heatmap,
    }
}

fn compute_score(inputs: &MonitoringInputs, network: &NetworkTelemetry) -> PerformanceScore {
    let reliability = if inputs.chain_height == 0 {
        55
    } else {
        (70 + (network.connected_peers.min(5) as u8 * 6)).min(100)
    };
    let responsiveness = if inputs.pending_transactions == 0 {
        95
    } else {
        (95u8).saturating_sub(inputs.pending_transactions.min(40) as u8)
    };
    let resource_utilization = if inputs.pending_transactions < 10 {
        90
    } else if inputs.pending_transactions < 25 {
        78
    } else {
        65
    };
    let contribution = ((inputs.transactions_processed_total / 4)
        .saturating_add(network.blocks_published.saturating_mul(10))
        .saturating_add(network.txs_published))
        .min(100) as u8;
    let overall = ((reliability as u16
        + responsiveness as u16
        + resource_utilization as u16
        + contribution as u16)
        / 4) as u8;

    PerformanceScore {
        overall,
        reliability,
        responsiveness,
        resource_utilization,
        contribution,
    }
}

fn status_from_score(score: u8, network: &NetworkTelemetry, chain_height: u64) -> NodeStatus {
    if chain_height == 0 && network.connected_peers == 0 {
        return NodeStatus::Offline;
    }
    match score {
        90..=u8::MAX => NodeStatus::Excellent,
        75..=89 => NodeStatus::Good,
        55..=74 => NodeStatus::Fair,
        1..=54 => NodeStatus::Poor,
        _ => NodeStatus::Unknown,
    }
}

fn compute_network_health(nodes: &[NodeMetrics]) -> NetworkHealthSummary {
    let mut status_counts = HashMap::new();
    for node in nodes {
        *status_counts
            .entry(node.status.as_label().to_string())
            .or_insert(0usize) += 1;
    }

    let active_nodes = nodes
        .iter()
        .filter(|node| !matches!(node.status, NodeStatus::Offline))
        .count();
    let total_nodes = nodes.len();
    let active_percentage = if total_nodes == 0 {
        0.0
    } else {
        (active_nodes as f64 / total_nodes as f64) * 100.0
    };
    let status = if active_percentage >= 95.0 {
        "excellent"
    } else if active_percentage >= 75.0 {
        "good"
    } else if active_percentage >= 50.0 {
        "fair"
    } else {
        "poor"
    };

    NetworkHealthSummary {
        total_nodes,
        active_nodes,
        active_percentage,
        status: status.to_string(),
        status_counts,
    }
}

fn build_heatmap(nodes: &[NodeMetrics], network_health: &NetworkHealthSummary) -> Heatmap {
    let mut points = Vec::with_capacity(nodes.len());
    for (idx, node) in nodes.iter().enumerate() {
        let grid_width = (nodes.len().max(1) as f64).sqrt().ceil() as usize;
        let row = idx / grid_width;
        let col = idx % grid_width;
        let x = 15.0 + (col as f64 * 35.0);
        let y = 25.0 + (row as f64 * 35.0);
        let total_network_bytes = node.current.network_in.saturating_add(node.current.network_out);
        points.push(HeatmapNode {
            node_id: node.node_id.clone(),
            name: node.name.clone(),
            node_type: node.node_type.as_label().to_string(),
            status: node.status.as_label().to_string(),
            color: color_for_status(node.status),
            x,
            y,
            size: ((node.score.overall / 15).max(3)).min(10),
            tooltip: format!(
                "{} | height {} | peers {} | mempool {}",
                node.name, node.current.block_height, node.current.peer_count, node.current.mempool_size
            ),
            score: node.score.overall,
            region: node.location.region.as_label().to_string(),
            metrics: HeatmapMetrics {
                cpu: node.current.cpu_usage,
                memory: node.current.memory_usage,
                disk: node.current.disk_usage,
                network_kbps: total_network_bytes as f64 / 1024.0,
                response_time_ms: node.current.response_time_ms,
                uptime_secs: node.current.uptime_secs,
                last_updated: node.last_updated,
            },
        });
    }

    Heatmap {
        title: "WRFCoin Node Monitoring Heatmap".to_string(),
        description: "Runtime-backed view of local node health and network telemetry".to_string(),
        timestamp: Utc::now(),
        generated_at: Utc::now(),
        sections: vec![HeatmapSection {
            title: "Observed Nodes".to_string(),
            description: "Nodes currently represented by the runtime monitoring registry".to_string(),
            nodes: points.clone(),
        }],
        network_health: network_health.clone(),
        points,
    }
}

fn color_for_status(status: NodeStatus) -> ColorCode {
    match status {
        NodeStatus::Excellent => ColorCode { r: 23, g: 140, b: 92 },
        NodeStatus::Good => ColorCode { r: 76, g: 175, b: 80 },
        NodeStatus::Fair => ColorCode { r: 255, g: 193, b: 7 },
        NodeStatus::Poor => ColorCode { r: 255, g: 87, b: 34 },
        NodeStatus::Offline => ColorCode { r: 120, g: 120, b: 120 },
        NodeStatus::Unknown => ColorCode { r: 96, g: 125, b: 139 },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use super::{
        build_snapshot_with_registry, MonitoringInputs, NetworkTelemetry, NodeMetricsRegistry,
    };

    fn monitoring_inputs(node_id: &str, node_name: &str) -> MonitoringInputs {
        MonitoringInputs {
            node_id: node_id.to_string(),
            node_name: node_name.to_string(),
            chain_height: 42,
            tip_hash: "abc123".to_string(),
            account_count: 4,
            nft_count: 2,
            pending_transactions: 3,
            quality_event_count: 5,
            slashing_events_by_reason: HashMap::new(),
            transactions_processed_total: 11,
            transactions_failed_total: 0,
            zk_proofs_observed_total: 2,
            state_diverged: false,
            chain_finalized_height: 0,
            state_pruning_last_height: 0,
            network: Some(NetworkTelemetry {
                connected_peers: 3,
                txs_published: 4,
                blocks_published: 1,
                uptime_secs: 90,
                ..NetworkTelemetry::default()
            }),
        }
    }

    #[test]
    fn build_snapshot_creates_runtime_backed_node_and_heatmap() {
        let mut slashing = HashMap::new();
        slashing.insert("invalid_reveal".to_string(), 1u64);
        let mut inputs = monitoring_inputs("local-node", "Local Validator");
        inputs.slashing_events_by_reason = slashing;
        let mut registry = NodeMetricsRegistry::default();
        let snapshot = build_snapshot_with_registry(inputs, &mut registry);

        assert_eq!(snapshot.chain_height, 42);
        assert_eq!(snapshot.nodes.len(), 1);
        assert_eq!(snapshot.nodes[0].node_id, "local-node");
        assert_eq!(snapshot.heatmap.points.len(), 1);
        assert_eq!(snapshot.network_health.total_nodes, 1);
        assert_eq!(snapshot.network_telemetry.connected_peers, 3);
    }

    #[test]
    fn capability_telemetry_fields_propagate_through_build_snapshot() {
        let mut inputs = monitoring_inputs("cap-node", "Capability Test");
        inputs.chain_height = 10;
        inputs.tip_hash = "cafebabe".to_string();
        inputs.account_count = 0;
        inputs.nft_count = 0;
        inputs.pending_transactions = 0;
        inputs.quality_event_count = 0;
        inputs.transactions_processed_total = 0;
        inputs.network = Some(NetworkTelemetry {
            ap_mode_active: true,
            is_metered: true,
            ..NetworkTelemetry::default()
        });
        let mut registry = NodeMetricsRegistry::default();
        let snapshot = build_snapshot_with_registry(inputs, &mut registry);
        assert!(
            snapshot.network_telemetry.ap_mode_active,
            "ap_mode_active should propagate through build_snapshot (#1802)"
        );
        assert!(
            snapshot.network_telemetry.is_metered,
            "is_metered should propagate through build_snapshot (#1802)"
        );
    }

    #[test]
    fn injected_snapshot_registries_do_not_share_nodes() {
        let mut first_registry = NodeMetricsRegistry::default();
        let first = build_snapshot_with_registry(
            monitoring_inputs("first-node", "First Validator"),
            &mut first_registry,
        );

        let mut second_registry = NodeMetricsRegistry::default();
        let second = build_snapshot_with_registry(
            monitoring_inputs("second-node", "Second Validator"),
            &mut second_registry,
        );

        assert_eq!(first.nodes.len(), 1);
        assert_eq!(first.nodes[0].node_id, "first-node");
        assert_eq!(second.nodes.len(), 1);
        assert_eq!(second.nodes[0].node_id, "second-node");
    }
}
