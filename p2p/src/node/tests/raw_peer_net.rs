//! Raw-peer test network: one node running the production event handler, and
//! any number of peers the test controls completely.
//!
//! [`BehaviourHarness`] drives `handle_behaviour_event` on a real libp2p swarm
//! over the in-memory transport, but every message it can send goes through
//! the node's own publish path, which only produces well-formed, correctly
//! signed messages. A [`RawPeer`] is a real libp2p peer with the production
//! behaviour stack and no handler at all: the test decides what bytes it
//! publishes on which topic, and reads back exactly what the node sent it.
//!
//! [`RawPeerNet`] joins raw peers to one node in a star. A raw peer is therefore
//! both the way to hand the node input no honest node would send, and the way
//! to observe what the node forwards: a message published by one raw peer
//! reaches another only if the node relayed it.
//!
//! Each raw peer goes through the same steps a real peer does before the node
//! treats it as a chain peer: it connects, is admitted as unproven (the
//! `event_loop` step this harness has to mirror, as the two-node harness
//! does), and publishes a genesis handshake the node verifies.
//!
//! Nothing here binds a socket.

use std::time::Duration;

use libp2p::futures::StreamExt as _;
use libp2p::futures::future::select_all;
use libp2p::gossipsub::{self, IdentTopic, TopicHash};
use libp2p::swarm::SwarmEvent;
use libp2p::{Multiaddr, PeerId, Swarm};
use tokio::sync::mpsc;

use super::BehaviourHarness;
use crate::P2pEvent;
use crate::behaviour::{WrfcoinBehaviour, WrfcoinBehaviourEvent};
use crate::message::WrfcoinMessage;

/// How long a harness wait may take before the test fails. Generous: the
/// failures these waits discriminate are "never happens", not "happens slowly".
pub(super) const WAIT: Duration = Duration::from_secs(30);

/// A libp2p peer under the test's full control.
pub(super) struct RawPeer {
    pub(super) swarm: Swarm<WrfcoinBehaviour>,
    /// Every gossipsub message this peer's swarm delivered, in arrival order.
    /// The peer never reports a validation verdict, so it relays nothing.
    pub(super) inbound: Vec<gossipsub::Message>,
}

impl RawPeer {
    fn new() -> Self {
        Self {
            swarm: crate::behaviour::test_swarm().expect("test swarm"),
            inbound: Vec::new(),
        }
    }

    pub(super) fn peer_id(&self) -> PeerId {
        *self.swarm.local_peer_id()
    }

    /// Publish arbitrary bytes on a topic. No encoding, size or content check
    /// is applied on this side.
    pub(super) fn publish_bytes(
        &mut self,
        topic: &IdentTopic,
        bytes: Vec<u8>,
    ) -> Result<gossipsub::MessageId, gossipsub::PublishError> {
        self.swarm
            .behaviour_mut()
            .gossipsub
            .publish(topic.clone(), bytes)
    }

    /// Publish a message in the node's wire encoding, whatever its content.
    pub(super) fn publish(
        &mut self,
        topic: &IdentTopic,
        message: &WrfcoinMessage,
    ) -> Result<gossipsub::MessageId, gossipsub::PublishError> {
        self.publish_bytes(topic, message.to_bytes().expect("message serializes"))
    }

    /// The payloads this peer received on `topic`, in arrival order.
    pub(super) fn received_on(&self, topic: &TopicHash) -> Vec<&[u8]> {
        self.inbound
            .iter()
            .filter(|message| &message.topic == topic)
            .map(|message| message.data.as_slice())
            .collect()
    }

    fn record(&mut self, event: SwarmEvent<WrfcoinBehaviourEvent>) {
        if let SwarmEvent::Behaviour(WrfcoinBehaviourEvent::Gossipsub(
            gossipsub::Event::Message { message, .. },
        )) = event
        {
            self.inbound.push(message);
        }
    }
}

/// One node on the production handler with raw peers connected to it.
pub(super) struct RawPeerNet {
    pub(super) node: BehaviourHarness,
    /// What the node's handler emitted to the launch layer.
    pub(super) node_events: mpsc::Receiver<P2pEvent>,
    pub(super) peers: Vec<RawPeer>,
    node_addr: Multiaddr,
}

impl RawPeerNet {
    /// Start the node listening in memory and dial it from `peer_count` raw
    /// peers. Returns once every raw peer is connected, has passed the node's
    /// genesis handshake check, and is in the node's mesh for `mesh_topic`
    /// in both directions, so a publish on that topic is routed.
    pub(super) async fn start(peer_count: usize, mesh_topic: &IdentTopic) -> Self {
        assert!(peer_count > 0, "a raw-peer net needs at least one raw peer");
        let (mut node, node_events) = BehaviourHarness::new();
        node.swarm
            .listen_on("/memory/0".parse().expect("valid memory multiaddr"))
            .expect("the in-memory transport must listen without binding a socket");
        let node_addr = loop {
            if let SwarmEvent::NewListenAddr { address, .. } = node.swarm.select_next_some().await {
                break address;
            }
        };

        let mut net = Self {
            node,
            node_events,
            peers: Vec::new(),
            node_addr,
        };
        for _ in 0..peer_count {
            net.add_peer().await;
        }
        net.join_mesh(mesh_topic).await;
        net
    }

    /// Connect one more raw peer and take it through the genesis handshake.
    /// Returns its index in [`Self::peers`].
    pub(super) async fn add_peer(&mut self) -> usize {
        let mut peer = RawPeer::new();
        let peer_id = peer.peer_id();
        peer.swarm
            .dial(self.node_addr.clone())
            .expect("a raw peer must be able to dial the node in memory");
        self.peers.push(peer);
        let index = self.peers.len() - 1;

        // `event_loop` admits a peer as unproven on ConnectionEstablished;
        // `step` mirrors that. Wait for it, so the handshake below is what
        // proves the peer rather than the peer never having been tracked.
        let admitted = self
            .run_until(|net| net.node.unproven_peers.contains(&peer_id))
            .await;
        assert!(admitted, "the node never saw the raw peer's connection");

        // The handshake travels as gossip on its own topic, so the peer can
        // only send it once the node is known to be subscribed. Retry the
        // publish until gossipsub takes it, then wait for the node's verdict.
        let handshake = WrfcoinMessage::GenesisHandshake {
            network_id: self.node.local_handshake.network_id.clone(),
            chain_id: self.node.local_handshake.chain_id.clone(),
            genesis_hash: self.node.local_handshake.genesis_hash.clone(),
            protocol_version: self.node.local_handshake.protocol_version,
        };
        let topic = self.node.tm.genesis_handshake().clone();
        let mut published = false;
        let proven = self
            .run_until(|net| {
                if !published {
                    published = net.peers[index].publish(&topic, &handshake).is_ok();
                }
                !net.node.unproven_peers.contains(&peer_id)
            })
            .await;
        assert!(
            proven,
            "the node never verified the raw peer's genesis handshake (published: {published})"
        );
        index
    }

    /// Wait until the node and every raw peer hold each other in the gossipsub
    /// mesh for `topic`. Before that a publish may go nowhere, or reach the
    /// node and have no one to be forwarded to.
    pub(super) async fn join_mesh(&mut self, topic: &IdentTopic) {
        let hash = topic.hash();
        let node_id = *self.node.swarm.local_peer_id();
        let meshed = self
            .run_until(|net| {
                let node_mesh: Vec<PeerId> = net
                    .node
                    .swarm
                    .behaviour()
                    .gossipsub
                    .mesh_peers(&hash)
                    .copied()
                    .collect();
                net.peers.iter().all(|peer| {
                    node_mesh.contains(&peer.peer_id())
                        && peer
                            .swarm
                            .behaviour()
                            .gossipsub
                            .mesh_peers(&hash)
                            .any(|id| *id == node_id)
                })
            })
            .await;
        assert!(meshed, "the gossipsub mesh for {hash} never formed");
    }

    /// Advance whichever swarm has work. The node's behaviour events go to the
    /// production handler; a raw peer only records what it was sent.
    pub(super) async fn step(&mut self) {
        enum Got {
            Node(SwarmEvent<WrfcoinBehaviourEvent>),
            Raw(usize, SwarmEvent<WrfcoinBehaviourEvent>),
            Idle,
        }

        let got = {
            let raw = select_all(self.peers.iter_mut().enumerate().map(|(index, peer)| {
                Box::pin(async move { (index, peer.swarm.select_next_some().await) })
            }));
            tokio::select! {
                event = self.node.swarm.select_next_some() => Got::Node(event),
                ((index, event), _, _) = raw => Got::Raw(index, event),
                _ = tokio::time::sleep(Duration::from_millis(50)) => Got::Idle,
            }
        };

        match got {
            Got::Node(SwarmEvent::Behaviour(event)) => self.node.dispatch(event).await,
            Got::Node(SwarmEvent::ConnectionEstablished {
                peer_id, endpoint, ..
            }) => {
                // The `event_loop` arm the behaviour harness does not run.
                let _ = self.node.unproven_peers.admit(
                    peer_id,
                    endpoint.get_remote_address(),
                    std::time::Instant::now(),
                );
            },
            Got::Node(_) | Got::Idle => {},
            Got::Raw(index, event) => self.peers[index].record(event),
        }
    }

    /// Step until `done` holds or [`WAIT`] passes. Returns whether it held.
    pub(super) async fn run_until(&mut self, mut done: impl FnMut(&mut Self) -> bool) -> bool {
        let deadline = tokio::time::Instant::now() + WAIT;
        loop {
            if done(self) {
                return true;
            }
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            self.step().await;
        }
    }

    /// Step for a fixed number of rounds, for "and nothing further arrives".
    pub(super) async fn settle(&mut self, rounds: usize) {
        for _ in 0..rounds {
            self.step().await;
        }
    }
}
