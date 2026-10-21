//! Discovered-peer log for Kademlia DHT visibility (#1407).
//!
//! Kademlia emits `RoutingUpdated` events whenever it learns of a new
//! peer or refreshes addresses for an existing one. Without somewhere
//! to put those events the operator only sees them as `debug!` lines —
//! the routing table itself is private to the swarm task.
//!
//! [`DiscoveredPeers`] is a small ring-buffer that the swarm event
//! loop pushes to and the HTTP layer reads from via [`P2pClient`].
//! It is intentionally cap-bounded so that a long-running node with a
//! large DHT does not retain unbounded history.

use std::{
    collections::VecDeque,
    sync::{Arc, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};

use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};

/// Default ring-buffer capacity. 200 covers the "show me recent
/// discovery" UX without retaining stale entries from week-old churn.
pub const DEFAULT_DISCOVERED_PEERS_CAPACITY: usize = 200;

/// A single Kademlia routing-update record exposed to operators.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredPeer {
    pub peer_id: String,
    pub addresses: Vec<String>,
    pub observed_at_secs: u64,
}

/// Cheap-clone handle to a shared, bounded log of discovered peers.
///
/// All operations take the inner lock for a brief critical section;
/// reads return owned snapshots so the lock is never held across
/// `.await` points or HTTP serialization.
#[derive(Clone)]
pub struct DiscoveredPeers {
    inner: Arc<RwLock<Inner>>,
}

struct Inner {
    log: VecDeque<DiscoveredPeer>,
    capacity: usize,
}

impl DiscoveredPeers {
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_DISCOVERED_PEERS_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            inner: Arc::new(RwLock::new(Inner {
                log: VecDeque::with_capacity(capacity),
                capacity,
            })),
        }
    }

    /// Record (or refresh) the discovery of `peer_id` with the given addresses.
    ///
    /// If the peer is already present its prior record is removed so the
    /// snapshot reflects the most recent observation; if the buffer is at
    /// capacity the oldest entry is evicted before the new one is pushed.
    pub fn record<I>(&self, peer_id: PeerId, addresses: I)
    where
        I: IntoIterator<Item = Multiaddr>,
    {
        let observed_at_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let peer_id_str = peer_id.to_string();
        let addresses: Vec<String> =
            addresses.into_iter().map(|addr| addr.to_string()).collect();

        let Ok(mut inner) = self.inner.write() else {
            return;
        };
        inner.log.retain(|p| p.peer_id != peer_id_str);
        while inner.log.len() >= inner.capacity {
            inner.log.pop_front();
        }
        inner.log.push_back(DiscoveredPeer {
            peer_id: peer_id_str,
            addresses,
            observed_at_secs,
        });
    }

    /// Return a most-recent-first snapshot of all discovered peers.
    pub fn snapshot(&self) -> Vec<DiscoveredPeer> {
        let Ok(inner) = self.inner.read() else {
            return Vec::new();
        };
        inner.log.iter().rev().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.inner.read().map(|i| i.log.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for DiscoveredPeers {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::identity::Keypair;

    fn make_peer() -> PeerId {
        PeerId::from(Keypair::generate_ed25519().public())
    }

    fn make_addr(suffix: u16) -> Multiaddr {
        format!("/ip4/127.0.0.1/tcp/{suffix}").parse().unwrap()
    }

    #[test]
    fn record_appends_new_peers() {
        let log = DiscoveredPeers::with_capacity(8);
        log.record(make_peer(), [make_addr(1)]);
        log.record(make_peer(), [make_addr(2)]);
        assert_eq!(log.len(), 2);
        let snap = log.snapshot();
        assert_eq!(snap.len(), 2);
        assert_eq!(snap[0].addresses, vec!["/ip4/127.0.0.1/tcp/2".to_string()]);
    }

    #[test]
    fn record_dedupes_by_peer_id() {
        let log = DiscoveredPeers::with_capacity(8);
        let peer = make_peer();
        log.record(peer, [make_addr(1)]);
        log.record(make_peer(), [make_addr(2)]);
        log.record(peer, [make_addr(3), make_addr(4)]);
        assert_eq!(log.len(), 2);
        let snap = log.snapshot();
        assert_eq!(snap[0].peer_id, peer.to_string());
        assert_eq!(snap[0].addresses.len(), 2);
    }

    #[test]
    fn record_evicts_oldest_at_capacity() {
        let log = DiscoveredPeers::with_capacity(2);
        let peer_a = make_peer();
        let peer_b = make_peer();
        let peer_c = make_peer();
        log.record(peer_a, [make_addr(1)]);
        log.record(peer_b, [make_addr(2)]);
        log.record(peer_c, [make_addr(3)]);
        let snap = log.snapshot();
        assert_eq!(snap.len(), 2);
        let ids: Vec<_> = snap.iter().map(|p| p.peer_id.as_str()).collect();
        assert!(ids.contains(&peer_b.to_string().as_str()));
        assert!(ids.contains(&peer_c.to_string().as_str()));
        assert!(!ids.contains(&peer_a.to_string().as_str()));
    }

    #[test]
    fn snapshot_is_most_recent_first() {
        let log = DiscoveredPeers::with_capacity(8);
        let peer_a = make_peer();
        let peer_b = make_peer();
        log.record(peer_a, [make_addr(1)]);
        log.record(peer_b, [make_addr(2)]);
        let snap = log.snapshot();
        assert_eq!(snap[0].peer_id, peer_b.to_string());
        assert_eq!(snap[1].peer_id, peer_a.to_string());
    }
}
