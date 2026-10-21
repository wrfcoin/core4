use std::time::{Duration, Instant};
use libp2p::PeerId;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PeerDirection {
    Inbound,
    Outbound,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerStats {
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub blocks_received: u64,
    pub blocks_sent: u64,
    pub transactions_received: u64,
    pub transactions_sent: u64,
    pub ping_time: Option<Duration>,
    pub last_seen: Instant,
    pub connected_since: Instant,
}

impl Default for PeerStats {
    fn default() -> Self {
        let now = Instant::now();
        PeerStats {
            bytes_sent: 0,
            bytes_received: 0,
            blocks_received: 0,
            blocks_sent: 0,
            transactions_received: 0,
            transactions_sent: 0,
            ping_time: None,
            last_seen: now,
            connected_since: now,
        }
    }
}

pub struct Peer {
    pub id: PeerId,
    pub addresses: Vec<String>,
    pub direction: PeerDirection,
    pub stats: PeerStats,
    pub user_agent: Option<String>,
    pub protocol_version: Option<String>,
}

impl Peer {
    pub fn new(id: PeerId, address: String, direction: PeerDirection) -> Self {
        Peer {
            id,
            addresses: vec![address],
            direction,
            stats: PeerStats::default(),
            user_agent: None,
            protocol_version: None,
        }
    }
    
    pub fn add_address(&mut self, address: String) {
        if !self.addresses.contains(&address) {
            self.addresses.push(address);
        }
    }
    
    pub fn update_last_seen(&mut self) {
        self.stats.last_seen = Instant::now();
    }
    
    pub fn update_ping_time(&mut self, ping_time: Duration) {
        self.stats.ping_time = Some(ping_time);
    }
    
    pub fn record_received_bytes(&mut self, bytes: u64) {
        self.stats.bytes_received += bytes;
        self.update_last_seen();
    }
    
    pub fn record_sent_bytes(&mut self, bytes: u64) {
        self.stats.bytes_sent += bytes;
        self.update_last_seen();
    }
    
    pub fn record_received_block(&mut self) {
        self.stats.blocks_received += 1;
        self.update_last_seen();
    }
    
    pub fn record_sent_block(&mut self) {
        self.stats.blocks_sent += 1;
        self.update_last_seen();
    }
    
    pub fn record_received_transaction(&mut self) {
        self.stats.transactions_received += 1;
        self.update_last_seen();
    }
    
    pub fn record_sent_transaction(&mut self) {
        self.stats.transactions_sent += 1;
        self.update_last_seen();
    }
    
    pub fn is_stale(&self, timeout: Duration) -> bool {
        self.stats.last_seen.elapsed() > timeout
    }
    
    pub fn uptime(&self) -> Duration {
        self.stats.connected_since.elapsed()
    }
    
    pub fn to_json(&self) -> String {
        let peer_data = json!({
            "id": self.id.to_string(),
            "addresses": self.addresses,
            "direction": match self.direction {
                PeerDirection::Inbound => "inbound",
                PeerDirection::Outbound => "outbound",
            },
            "stats": {
                "bytes_sent": self.stats.bytes_sent,
                "bytes_received": self.stats.bytes_received,
                "blocks_received": self.stats.blocks_received,
                "blocks_sent": self.stats.blocks_sent,
                "transactions_received": self.stats.transactions_received,
                "transactions_sent": self.stats.transactions_sent,
                "ping_time": self.stats.ping_time.map(|pt| pt.as_millis()),
                "uptime": self.uptime().as_secs(),
            },
            "user_agent": self.user_agent,
            "protocol_version": self.protocol_version,
        });
        
        serde_json::to_string(&peer_data).unwrap_or_else(|_| "{}".to_string())
    }
}

use serde_json::json;
