use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::time::sleep;
use log::{info, warn, error, debug};
use serde::{Serialize, Deserialize};
use libp2p::{
    kad::{self, store::MemoryStore},
    identify,
    ping,
    gossipsub::{self, IdentTopic},
    swarm::{NetworkBehaviour, SwarmEvent},
    PeerId, Multiaddr,
};

use crate::blockchain::chain::Blockchain;
use crate::blockchain::block::Block;
use crate::blockchain::transaction::Transaction;
use crate::network::peer::Peer;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStatus {
    pub peer_id: String,
    pub connected_peers: usize,
    pub is_syncing: bool,
    pub inbound_bandwidth: u64,
    pub outbound_bandwidth: u64,
    pub uptime: u64,
    pub total_data_transferred: u64,
    pub known_addresses: Vec<String>,
}

#[derive(NetworkBehaviour)]
struct NodeBehaviour {
    kademlia: kad::Behaviour<MemoryStore>,
    identify: identify::Behaviour,
    ping: ping::Behaviour,
    gossipsub: gossipsub::Behaviour,
}

pub struct Node {
    blockchain: Arc<Mutex<Blockchain>>,
    swarm: Arc<Mutex<libp2p::Swarm<NodeBehaviour>>>,
    peers: HashMap<PeerId, Peer>,
    /// Discovered peer listen addresses, populated from Identify events.
    peer_addresses: Arc<Mutex<HashMap<PeerId, Vec<Multiaddr>>>>,
    start_time: Instant,
    inbound_bandwidth: u64,
    outbound_bandwidth: u64,
    total_data_transferred: u64,
    blockchain_topic: IdentTopic,
    transaction_topic: IdentTopic,
    is_running: bool,
    is_syncing: bool,
}

impl Node {
    pub fn new(blockchain: Arc<Mutex<Blockchain>>) -> Result<Self, Box<dyn std::error::Error>> {
        let local_key = libp2p::identity::Keypair::generate_ed25519();
        let local_peer_id = PeerId::from(local_key.public());

        info!("Local peer id: {}", local_peer_id);

        let blockchain_topic = IdentTopic::new("wrfcoin-blockchain");
        let transaction_topic = IdentTopic::new("wrfcoin-transactions");

        let mut swarm = libp2p::SwarmBuilder::with_existing_identity(local_key)
            .with_tokio()
            .with_tcp(
                libp2p::tcp::Config::default(),
                libp2p::noise::Config::new,
                libp2p::yamux::Config::default,
            )?
            .with_behaviour(|key| {
                let peer_id = key.public().to_peer_id();
                let store = MemoryStore::new(peer_id);
                let kademlia = kad::Behaviour::new(peer_id, store);
                let identify = identify::Behaviour::new(
                    identify::Config::new(
                        "wrfcoin/1.0.0".to_string(),
                        key.public(),
                    )
                    .with_agent_version(format!("wrfcoin-core/{}", env!("CARGO_PKG_VERSION"))),
                );
                let ping = ping::Behaviour::default();
                let gossipsub = gossipsub::Behaviour::new(
                    gossipsub::MessageAuthenticity::Signed(key.clone()),
                    gossipsub::Config::default(),
                )
                .expect("valid gossipsub config");
                NodeBehaviour { kademlia, identify, ping, gossipsub }
            })?
            .build();

        swarm.behaviour_mut().gossipsub.subscribe(&blockchain_topic)?;
        swarm.behaviour_mut().gossipsub.subscribe(&transaction_topic)?;

        Ok(Node {
            blockchain,
            swarm: Arc::new(Mutex::new(swarm)),
            peers: HashMap::new(),
            peer_addresses: Arc::new(Mutex::new(HashMap::new())),
            start_time: Instant::now(),
            inbound_bandwidth: 0,
            outbound_bandwidth: 0,
            total_data_transferred: 0,
            blockchain_topic,
            transaction_topic,
            is_running: false,
            is_syncing: false,
        })
    }

    pub async fn start(&mut self, listen_address: &str) -> Result<(), Box<dyn std::error::Error>> {
        let addr: Multiaddr = listen_address.parse()?;

        let mut swarm = self.swarm.lock().await;
        swarm.listen_on(addr.clone())?;
        info!("Node listening on {}", addr);

        self.is_running = true;

        let swarm_clone = self.swarm.clone();
        let blockchain_clone = self.blockchain.clone();
        let blockchain_topic_hash = self.blockchain_topic.hash();
        let transaction_topic_hash = self.transaction_topic.hash();
        let peer_addresses_clone = self.peer_addresses.clone();

        tokio::spawn(async move {
            let mut swarm = swarm_clone.lock().await;

            loop {
                match swarm.select_next_some().await {
                    SwarmEvent::Behaviour(NodeBehaviourEvent::Kademlia(event)) => {
                        debug!("Kademlia event: {:?}", event);
                    }
                    SwarmEvent::Behaviour(NodeBehaviourEvent::Identify(
                        identify::Event::Received { peer_id, info },
                    )) => {
                        debug!("Identify event from {}", peer_id);
                        // Cache discovered listen addresses for get_status diagnostics.
                        peer_addresses_clone
                            .lock()
                            .await
                            .insert(peer_id, info.listen_addrs.clone());
                        for addr in info.listen_addrs {
                            swarm.behaviour_mut().kademlia.add_address(&peer_id, addr);
                        }
                    }
                    SwarmEvent::Behaviour(NodeBehaviourEvent::Ping(ping::Event {
                        peer,
                        result,
                        ..
                    })) => match result {
                        Ok(rtt) => debug!("Ping to {} is {}ms", peer, rtt.as_millis()),
                        Err(_) => warn!("Ping to {} failed", peer),
                    },
                    SwarmEvent::Behaviour(NodeBehaviourEvent::Gossipsub(
                        gossipsub::Event::Message {
                            propagation_source: peer_id,
                            message,
                            ..
                        },
                    )) => {
                        if message.topic == blockchain_topic_hash {
                            if let Ok(block_data) =
                                serde_json::from_slice::<Block>(&message.data)
                            {
                                info!("Received block from peer {}", peer_id);
                                let mut bc = blockchain_clone.lock().await;
                                if let Err(e) = bc.add_block(block_data) {
                                    warn!("Failed to add received block: {}", e);
                                }
                            }
                        } else if message.topic == transaction_topic_hash {
                            if let Ok(tx_data) =
                                serde_json::from_slice::<Transaction>(&message.data)
                            {
                                info!("Received transaction from peer {}", peer_id);
                                let mut bc = blockchain_clone.lock().await;
                                if let Err(e) = bc.add_transaction(tx_data) {
                                    warn!("Failed to add received transaction: {}", e);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        });

        drop(swarm);
        self.connect_to_bootstrap_nodes().await?;
        self.sync_blockchain().await?;

        Ok(())
    }

    pub async fn stop(&mut self) {
        self.is_running = false;
        info!("Node stopped");
    }

    pub async fn broadcast_block(&mut self, block: Block) -> Result<(), Box<dyn std::error::Error>> {
        let block_data = serde_json::to_vec(&block)?;
        let payload_size = block_data.len() as u64;
        let mut swarm = self.swarm.lock().await;
        swarm
            .behaviour_mut()
            .gossipsub
            .publish(self.blockchain_topic.clone(), block_data)
            .map_err(|e| format!("broadcast block: {e}"))?;
        drop(swarm);
        self.outbound_bandwidth += payload_size;
        self.total_data_transferred += payload_size;
        info!("Broadcasted block with hash: {}", block.hash);
        Ok(())
    }

    pub async fn broadcast_transaction(
        &mut self,
        transaction: Transaction,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let tx_data = serde_json::to_vec(&transaction)?;
        let payload_size = tx_data.len() as u64;
        let mut swarm = self.swarm.lock().await;
        swarm
            .behaviour_mut()
            .gossipsub
            .publish(self.transaction_topic.clone(), tx_data)
            .map_err(|e| format!("broadcast transaction: {e}"))?;
        drop(swarm);
        self.outbound_bandwidth += payload_size;
        self.total_data_transferred += payload_size;
        info!("Broadcasted transaction with id: {}", transaction.id);
        Ok(())
    }

    pub async fn connect_to_peer(
        &self,
        address: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let addr: Multiaddr = address.parse()?;
        let mut swarm = self.swarm.lock().await;
        swarm.dial(addr.clone())?;
        info!("Dialing peer at {}", addr);
        Ok(())
    }

    async fn connect_to_bootstrap_nodes(&self) -> Result<(), Box<dyn std::error::Error>> {
        let bootstrap_nodes = vec![
            "/dns4/bootstrap1.wrfcoin.org/tcp/8000/p2p/QmRjkXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX",
            "/dns4/bootstrap2.wrfcoin.org/tcp/8000/p2p/QmRjkXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX",
        ];

        for node in bootstrap_nodes {
            if let Ok(addr) = node.parse::<Multiaddr>() {
                let mut swarm = self.swarm.lock().await;
                match swarm.dial(addr.clone()) {
                    Ok(_) => info!("Dialing bootstrap node {}", addr),
                    Err(e) => warn!("Failed to dial bootstrap node {}: {}", addr, e),
                }
            }
        }

        Ok(())
    }

    async fn sync_blockchain(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.is_syncing = true;
        sleep(Duration::from_secs(2)).await;
        self.is_syncing = false;
        info!("Blockchain synced");
        Ok(())
    }

    pub async fn get_status(&self) -> NodeStatus {
        let uptime = self.start_time.elapsed().as_secs();
        let swarm = self.swarm.lock().await;
        let peer_id = swarm.local_peer_id().to_string();
        let connected_peers = self.peers.len();
        drop(swarm);
        let known_addresses: Vec<String> = self.peer_addresses
            .lock()
            .await
            .values()
            .flat_map(|addrs| addrs.iter().map(|a| a.to_string()))
            .collect();

        NodeStatus {
            peer_id,
            connected_peers,
            is_syncing: self.is_syncing,
            inbound_bandwidth: self.inbound_bandwidth,
            outbound_bandwidth: self.outbound_bandwidth,
            uptime,
            total_data_transferred: self.total_data_transferred,
            known_addresses,
        }
    }
}
