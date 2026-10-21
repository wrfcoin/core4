//! Mesh networking types for local contributor-node interconnect.
//!
//! Models 802.11s WiFi mesh peers, link quality, and topology snapshots.
//! These types are consumed by the P2P layer (peer tagging), the API
//! (`GET /node/network/mesh`), and the capability scoring system (#735).
//!
//! See `docs/architecture/wifi-mesh-networking.md` for the full design.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Tunable parameters for mesh peer management.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshConfig {
    /// Maximum number of mesh peers to maintain.
    pub max_peers: usize,
    /// How often to poll mesh link quality from nl80211 / batman-adv.
    pub link_poll_interval: Duration,
    /// Minimum RSSI (dBm) to consider a mesh link usable.
    pub min_rssi_dbm: i16,
    /// Seconds without a heartbeat before a mesh peer is considered stale.
    pub peer_stale_timeout: Duration,
    /// Whether to accept sensor gossip relayed from mesh peers.
    pub accept_relayed_observations: bool,
}

impl Default for MeshConfig {
    fn default() -> Self {
        Self {
            max_peers: 8,
            link_poll_interval: Duration::from_secs(30),
            min_rssi_dbm: -85,
            peer_stale_timeout: Duration::from_secs(120),
            accept_relayed_observations: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Mesh link quality
// ---------------------------------------------------------------------------

/// Transport medium of a mesh link (backhaul classification).
///
/// Used by the capability scorer and operator metrics to distinguish
/// WiFi-native mesh links from metered cellular backhaul (#1802).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MeshTransportClass {
    /// 802.11s WiFi mesh — the native on-device mesh transport.
    #[default]
    WiFi80211s,
    /// Ethernet backhaul between two mesh-capable nodes.
    Ethernet,
    /// LTE / 4G cellular backhaul (metered, WAN failover path).
    LteBackhaul,
    /// 5G / NR cellular backhaul (metered, WAN failover path).
    FiveGBackhaul,
}

/// Radio and routing quality metrics for a single mesh link.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct MeshLinkQuality {
    /// Received signal strength at this node in dBm.
    pub rssi_dbm: i16,
    /// Transmit bitrate in Mbps (from nl80211 station info).
    pub tx_bitrate_mbps: Option<u32>,
    /// Expected throughput in kbps (from batman-adv OGM, if available).
    pub expected_throughput_kbps: Option<u32>,
    /// Routing hop count to this peer (1 = direct neighbor).
    pub hop_count: u8,
    /// Transport medium of the underlying link (#1802).
    #[serde(default)]
    pub transport_class: MeshTransportClass,
    /// True when this link is over a metered (cellular) backhaul (#1802).
    #[serde(default)]
    pub is_metered: bool,
}

impl MeshLinkQuality {
    /// A link is considered usable when RSSI is above the threshold and it's
    /// reachable within a reasonable hop count.
    pub fn is_usable(&self, min_rssi_dbm: i16) -> bool {
        self.rssi_dbm >= min_rssi_dbm && self.hop_count > 0 && self.hop_count <= 10
    }

    /// Compute a composite quality score (0–100) from the available metrics.
    ///
    /// Scoring formula (integer arithmetic):
    /// - RSSI contributes 0–50: `clamp((rssi + 100) * 50 / 60, 0, 50)`
    ///   (maps -100 dBm → 0, -40 dBm → 50)
    /// - Hop count contributes 0–30: `clamp((11 - hop) * 30 / 10, 0, 30)`
    ///   (maps 1 hop → 30, 10 hops → 3)
    /// - Throughput contributes 0–20: `clamp(kbps * 20 / 50_000, 0, 20)`
    ///   (maps 50 Mbps → 20, 0 → 0)
    pub fn quality_score(&self) -> u8 {
        let rssi_score = ((self.rssi_dbm as i32 + 100) * 50 / 60).clamp(0, 50) as u8;
        let hop_score = ((11i32 - self.hop_count as i32) * 30 / 10).clamp(0, 30) as u8;
        let throughput_score = self
            .expected_throughput_kbps
            .map(|kbps| (kbps as u64 * 20 / 50_000).min(20) as u8)
            .unwrap_or(0);

        rssi_score + hop_score + throughput_score
    }
}

// ---------------------------------------------------------------------------
// Mesh peer lifecycle
// ---------------------------------------------------------------------------

/// Lifecycle state of a mesh peer link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MeshPeerState {
    /// Peer discovered via mDNS or batman-adv, not yet authenticated.
    Discovered,
    /// Ed25519 identity handshake in progress.
    Authenticating,
    /// Fully connected and exchanging data.
    Connected,
    /// Previously connected, now unreachable.
    Disconnected,
}

impl fmt::Display for MeshPeerState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discovered => write!(f, "Discovered"),
            Self::Authenticating => write!(f, "Authenticating"),
            Self::Connected => write!(f, "Connected"),
            Self::Disconnected => write!(f, "Disconnected"),
        }
    }
}

/// A mesh peer as seen by this node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeshPeer {
    /// The peer's WRFCoin node identity (ed25519 public key, hex-encoded).
    pub node_id: String,
    /// IPv4 or IPv6 address on the mesh interface.
    pub mesh_address: String,
    /// Current lifecycle state.
    pub state: MeshPeerState,
    /// Link quality metrics (present when `Connected`).
    pub link_quality: Option<MeshLinkQuality>,
    /// Seconds since the last heartbeat or data exchange.
    pub last_seen_secs_ago: u64,
}

// ---------------------------------------------------------------------------
// Mesh topology snapshot
// ---------------------------------------------------------------------------

/// A point-in-time snapshot of the local mesh topology, suitable for API
/// responses and capability scoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshTopologySnapshot {
    /// This node's mesh interface address.
    pub local_mesh_address: Option<String>,
    /// Whether the mesh interface is up and configured.
    pub mesh_active: bool,
    /// Connected mesh peers.
    pub peers: Vec<MeshPeer>,
    /// Average link quality score across connected peers (0–100).
    pub avg_quality_score: u8,
}

impl MeshTopologySnapshot {
    /// Build a snapshot from a list of mesh peers.
    pub fn from_peers(local_addr: Option<String>, peers: Vec<MeshPeer>) -> Self {
        let connected: Vec<&MeshPeer> = peers
            .iter()
            .filter(|p| p.state == MeshPeerState::Connected)
            .collect();
        let avg_quality_score = if connected.is_empty() {
            0
        } else {
            let total: u32 = connected
                .iter()
                .filter_map(|p| p.link_quality.as_ref())
                .map(|lq| lq.quality_score() as u32)
                .sum();
            let count = connected
                .iter()
                .filter(|p| p.link_quality.is_some())
                .count() as u32;
            if count > 0 {
                (total / count) as u8
            } else {
                0
            }
        };

        Self {
            mesh_active: local_addr.is_some(),
            local_mesh_address: local_addr,
            peers,
            avg_quality_score,
        }
    }

    /// Number of connected mesh peers.
    pub fn connected_count(&self) -> usize {
        self.peers
            .iter()
            .filter(|p| p.state == MeshPeerState::Connected)
            .count()
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Validate a mesh peer entry.  Returns a list of errors (empty = valid).
pub fn validate_mesh_peer(peer: &MeshPeer) -> Vec<String> {
    let mut errors = Vec::new();

    if peer.node_id.is_empty() {
        errors.push("node_id must not be empty".to_string());
    } else if peer.node_id.len() != 64 || !peer.node_id.chars().all(|c| c.is_ascii_hexdigit()) {
        errors.push("node_id must be a 64-character hex-encoded ed25519 public key".to_string());
    }

    if peer.mesh_address.is_empty() {
        errors.push("mesh_address must not be empty".to_string());
    }

    if peer.state == MeshPeerState::Connected && peer.link_quality.is_none() {
        errors.push("connected peer must have link_quality metrics".to_string());
    }

    if let Some(lq) = &peer.link_quality {
        if lq.hop_count == 0 {
            errors.push("hop_count must be >= 1".to_string());
        }
    }

    errors
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_node_id() -> String {
        "a1b2c3d4e5f6071890abcdef1234567890abcdef1234567890abcdef12345678".to_string()
    }

    fn sample_link_quality() -> MeshLinkQuality {
        MeshLinkQuality {
            rssi_dbm: -55,
            tx_bitrate_mbps: Some(144),
            expected_throughput_kbps: Some(25_000),
            hop_count: 1,
            ..Default::default()
        }
    }

    fn connected_peer() -> MeshPeer {
        MeshPeer {
            node_id: sample_node_id(),
            mesh_address: "192.168.77.2".to_string(),
            state: MeshPeerState::Connected,
            link_quality: Some(sample_link_quality()),
            last_seen_secs_ago: 5,
        }
    }

    // ---- Link quality ----

    #[test]
    fn link_quality_usable_above_threshold() {
        let lq = sample_link_quality();
        assert!(lq.is_usable(-85));
        assert!(lq.is_usable(-55)); // exactly at RSSI
    }

    #[test]
    fn link_quality_not_usable_below_threshold() {
        let lq = MeshLinkQuality {
            rssi_dbm: -90,
            ..sample_link_quality()
        };
        assert!(!lq.is_usable(-85));
    }

    #[test]
    fn link_quality_not_usable_zero_hops() {
        let lq = MeshLinkQuality {
            hop_count: 0,
            ..sample_link_quality()
        };
        assert!(!lq.is_usable(-100));
    }

    #[test]
    fn quality_score_strong_direct_link() {
        let lq = MeshLinkQuality {
            rssi_dbm: -40,
            tx_bitrate_mbps: Some(300),
            expected_throughput_kbps: Some(50_000),
            hop_count: 1,
            ..Default::default()
        };
        // RSSI: (-40 + 100) * 50 / 60 = 50 (clamped)
        // Hop:  (11 - 1) * 30 / 10 = 30
        // Throughput: 50000 * 20 / 50000 = 20
        // Total: 100
        assert_eq!(lq.quality_score(), 100);
    }

    #[test]
    fn quality_score_weak_multihop_link() {
        let lq = MeshLinkQuality {
            rssi_dbm: -95,
            tx_bitrate_mbps: None,
            expected_throughput_kbps: Some(1_000),
            hop_count: 5,
            ..Default::default()
        };
        // RSSI: (-95 + 100) * 50 / 60 = 5 * 50 / 60 = 4
        // Hop:  (11 - 5) * 30 / 10 = 18
        // Throughput: 1000 * 20 / 50000 = 0
        // Total: 22
        assert_eq!(lq.quality_score(), 22);
    }

    #[test]
    fn quality_score_no_throughput_data() {
        let lq = MeshLinkQuality {
            rssi_dbm: -60,
            tx_bitrate_mbps: None,
            expected_throughput_kbps: None,
            hop_count: 1,
            ..Default::default()
        };
        // RSSI: (-60 + 100) * 50 / 60 = 33
        // Hop: 30
        // Throughput: 0
        assert_eq!(lq.quality_score(), 63);
    }

    // ---- Peer state ----

    #[test]
    fn mesh_peer_state_display() {
        assert_eq!(MeshPeerState::Discovered.to_string(), "Discovered");
        assert_eq!(MeshPeerState::Connected.to_string(), "Connected");
    }

    #[test]
    fn mesh_peer_state_serializes() {
        let json = serde_json::to_string(&MeshPeerState::Authenticating).unwrap();
        assert_eq!(json, "\"Authenticating\"");
        let recovered: MeshPeerState = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered, MeshPeerState::Authenticating);
    }

    // ---- Validation ----

    #[test]
    fn valid_peer_passes_validation() {
        let errors = validate_mesh_peer(&connected_peer());
        assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
    }

    #[test]
    fn empty_node_id_rejected() {
        let mut peer = connected_peer();
        peer.node_id = String::new();
        let errors = validate_mesh_peer(&peer);
        assert!(errors.iter().any(|e| e.contains("node_id must not be empty")));
    }

    #[test]
    fn short_node_id_rejected() {
        let mut peer = connected_peer();
        peer.node_id = "abcd1234".to_string();
        let errors = validate_mesh_peer(&peer);
        assert!(errors.iter().any(|e| e.contains("64-character hex")));
    }

    #[test]
    fn connected_peer_without_link_quality_rejected() {
        let mut peer = connected_peer();
        peer.link_quality = None;
        let errors = validate_mesh_peer(&peer);
        assert!(errors.iter().any(|e| e.contains("link_quality")));
    }

    #[test]
    fn discovered_peer_without_link_quality_ok() {
        let peer = MeshPeer {
            node_id: sample_node_id(),
            mesh_address: "192.168.77.3".to_string(),
            state: MeshPeerState::Discovered,
            link_quality: None,
            last_seen_secs_ago: 0,
        };
        let errors = validate_mesh_peer(&peer);
        assert!(errors.is_empty(), "discovered peer needs no link_quality: {errors:?}");
    }

    #[test]
    fn zero_hop_count_rejected() {
        let mut peer = connected_peer();
        peer.link_quality = Some(MeshLinkQuality {
            hop_count: 0,
            ..sample_link_quality()
        });
        let errors = validate_mesh_peer(&peer);
        assert!(errors.iter().any(|e| e.contains("hop_count")));
    }

    // ---- Topology snapshot ----

    #[test]
    fn topology_snapshot_counts_connected_peers() {
        let peers = vec![
            connected_peer(),
            MeshPeer {
                state: MeshPeerState::Disconnected,
                ..connected_peer()
            },
            {
                let mut p = connected_peer();
                p.mesh_address = "192.168.77.3".to_string();
                p
            },
        ];
        let snap = MeshTopologySnapshot::from_peers(Some("192.168.77.1".into()), peers);
        assert_eq!(snap.connected_count(), 2);
        assert!(snap.mesh_active);
        assert!(snap.avg_quality_score > 0);
    }

    #[test]
    fn topology_snapshot_inactive_without_local_address() {
        let snap = MeshTopologySnapshot::from_peers(None, vec![]);
        assert!(!snap.mesh_active);
        assert_eq!(snap.connected_count(), 0);
        assert_eq!(snap.avg_quality_score, 0);
    }

    // ---- Config ----

    #[test]
    fn default_config_is_sane() {
        let config = MeshConfig::default();
        assert_eq!(config.max_peers, 8);
        assert_eq!(config.min_rssi_dbm, -85);
        assert!(config.accept_relayed_observations);
    }

    // ---- Serialization ----

    #[test]
    fn mesh_peer_roundtrips_through_json() {
        let peer = connected_peer();
        let json = serde_json::to_string(&peer).expect("serialize");
        let recovered: MeshPeer = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(recovered, peer);
    }

    #[test]
    fn mesh_topology_roundtrips_through_json() {
        let snap = MeshTopologySnapshot::from_peers(
            Some("192.168.77.1".into()),
            vec![connected_peer()],
        );
        let json = serde_json::to_string(&snap).expect("serialize");
        let recovered: MeshTopologySnapshot = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(recovered.connected_count(), 1);
        assert!(recovered.mesh_active);
    }
}
