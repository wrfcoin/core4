//! Composite libp2p [`NetworkBehaviour`] for a WRFCoin node.
//!
//! The behaviour aggregates gossipsub (pub/sub), kademlia (DHT peer
//! discovery), identify (protocol negotiation / address advertisement),
//! and NAT traversal protocols (autonat, relay client, dcutr, upnp)
//! into a single swarm-compatible type.

use libp2p::{
    autonat, connection_limits, dcutr, gossipsub, identify, identity::Keypair, kad, relay,
    swarm::NetworkBehaviour, upnp, PeerId, StreamProtocol,
};

use crate::{
    config::P2pConfig,
    error::{P2pError, P2pResult},
    message::{message_id_fn, TopicManager},
    scoring::build_peer_scoring_params,
    sync::{
        BLOCK_SYNC_PROTOCOL, BLOCK_SYNC_REQUEST_TIMEOUT, BlockRequest, BlockResponse,
        BlockSyncCodec,
    },
};

// ---------------------------------------------------------------------------
// Composite behaviour
// ---------------------------------------------------------------------------

/// The aggregate [`NetworkBehaviour`] used by a WRFCoin swarm.
#[derive(NetworkBehaviour)]
#[behaviour(to_swarm = "WrfcoinBehaviourEvent")]
pub struct WrfcoinBehaviour {
    /// Address-derived concurrent-connection quotas (per source IP, per IPv4
    /// `/24`, per IPv6 `/64`), enforced at the *pending* inbound stage before
    /// the Noise handshake (#2113). Placed first so a flood concentrated on one
    /// address range is shed before any other behaviour is consulted.
    pub connection_quota: crate::quota::QuotaBehaviour,
    /// Hard inbound/total connection caps enforced at the swarm layer. Placed
    /// second so it vetoes over-limit connections at the *pending* stage —
    /// before the Noise handshake and any other behaviour does work — making a
    /// connection-flood DoS cheap to shed (#2037).
    pub connection_limits: connection_limits::Behaviour,
    /// Pub/sub overlay for blocks, transactions, and weather data.
    pub gossipsub: gossipsub::Behaviour,
    /// Distributed hash table for peer discovery.
    pub kademlia: kad::Behaviour<kad::store::MemoryStore>,
    /// Protocol that exchanges peer metadata (addresses, agent version).
    pub identify: identify::Behaviour,
    /// Block-range request / response protocol used during initial sync.
    pub block_sync: libp2p::request_response::Behaviour<BlockSyncCodec>,
    /// Relay client for NAT'd nodes to reach the network via relay nodes.
    pub relay_client: relay::client::Behaviour,
    /// Direct Connection Upgrade through Relay — upgrades relayed
    /// connections to direct connections via hole-punching.
    pub dcutr: dcutr::Behaviour,
    /// AutoNAT — detects whether the local node is behind NAT.
    pub autonat: autonat::Behaviour,
    /// UPnP — automatic port mapping on UPnP-capable routers.
    pub upnp: libp2p::swarm::behaviour::toggle::Toggle<upnp::tokio::Behaviour>,
}

// ---------------------------------------------------------------------------
// Aggregated event
// ---------------------------------------------------------------------------

/// Events emitted by the composite behaviour.
///
/// The [`NetworkBehaviour`] derive macro generates the `From` impls that
/// convert each sub-behaviour's event into this enum automatically.
#[derive(Debug)]
pub enum WrfcoinBehaviourEvent {
    /// A gossipsub event (message received, subscription change, etc.).
    Gossipsub(gossipsub::Event),
    /// A kademlia event (routing table update, query result, etc.).
    Kademlia(kad::Event),
    /// An identify event (peer info received, sent, etc.).
    Identify(identify::Event),
    /// A block-sync request / response event.
    BlockSync(libp2p::request_response::Event<BlockRequest, BlockResponse>),
    /// A relay client event (reservation accepted/rejected, etc.).
    RelayClient(relay::client::Event),
    /// A DCUtR event (direct connection upgrade result).
    Dcutr(dcutr::Event),
    /// An AutoNAT event (NAT status changed).
    Autonat(autonat::Event),
    /// A UPnP event (port mapping result).
    Upnp(upnp::Event),
}

impl From<gossipsub::Event> for WrfcoinBehaviourEvent {
    fn from(event: gossipsub::Event) -> Self {
        WrfcoinBehaviourEvent::Gossipsub(event)
    }
}

// `connection_limits::Behaviour` emits no events — its `ToSwarm` is the
// uninhabited `Infallible` type. The `NetworkBehaviour` derive still requires a
// `From` conversion into the aggregate event, so provide the trivial one; it can
// never actually be called.
impl From<std::convert::Infallible> for WrfcoinBehaviourEvent {
    fn from(event: std::convert::Infallible) -> Self {
        match event {}
    }
}

impl From<kad::Event> for WrfcoinBehaviourEvent {
    fn from(event: kad::Event) -> Self {
        WrfcoinBehaviourEvent::Kademlia(event)
    }
}

impl From<identify::Event> for WrfcoinBehaviourEvent {
    fn from(event: identify::Event) -> Self {
        WrfcoinBehaviourEvent::Identify(event)
    }
}

impl From<libp2p::request_response::Event<BlockRequest, BlockResponse>>
    for WrfcoinBehaviourEvent
{
    fn from(event: libp2p::request_response::Event<BlockRequest, BlockResponse>) -> Self {
        WrfcoinBehaviourEvent::BlockSync(event)
    }
}

impl From<relay::client::Event> for WrfcoinBehaviourEvent {
    fn from(event: relay::client::Event) -> Self {
        WrfcoinBehaviourEvent::RelayClient(event)
    }
}

impl From<dcutr::Event> for WrfcoinBehaviourEvent {
    fn from(event: dcutr::Event) -> Self {
        WrfcoinBehaviourEvent::Dcutr(event)
    }
}

impl From<autonat::Event> for WrfcoinBehaviourEvent {
    fn from(event: autonat::Event) -> Self {
        WrfcoinBehaviourEvent::Autonat(event)
    }
}

impl From<upnp::Event> for WrfcoinBehaviourEvent {
    fn from(event: upnp::Event) -> Self {
        WrfcoinBehaviourEvent::Upnp(event)
    }
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

/// Build a fully-configured [`WrfcoinBehaviour`] from a keypair, config,
/// and the relay client behaviour provided by the [`SwarmBuilder`].
///
/// The `relay_behaviour` parameter is created internally by
/// [`SwarmBuilder::with_relay_client`] and passed to the
/// `.with_behaviour()` closure.
pub fn build_behaviour(
    keypair: &Keypair,
    config: &P2pConfig,
    relay_behaviour: relay::client::Behaviour,
    metrics: std::sync::Arc<crate::metrics::MetricsCollector>,
) -> P2pResult<WrfcoinBehaviour> {
    // -- Address-derived connection quotas (#2113) ---------------------------
    let connection_quota =
        crate::quota::QuotaBehaviour::new(config.connection_quota, metrics);

    // -- Connection limits (#2037) -------------------------------------------
    // Hard caps on inbound / total / per-peer connections. Rejecting at the
    // pending-connection stage keeps a connection flood cheap to shed.
    let connection_limits = connection_limits::Behaviour::new(
        connection_limits::ConnectionLimits::default()
            .with_max_pending_incoming(config.max_pending_incoming)
            .with_max_established_incoming(config.max_established_incoming)
            .with_max_established_per_peer(config.max_established_per_peer)
            .with_max_established(config.max_established_total),
    );

    // -- Gossipsub -----------------------------------------------------------
    let gossipsub_config = gossipsub::ConfigBuilder::default()
        .mesh_n(config.mesh_n)
        .mesh_n_low(config.mesh_n_low)
        .mesh_n_high(config.mesh_n_high)
        .mesh_outbound_min(config.mesh_outbound_min)
        .heartbeat_interval(config.heartbeat_interval)
        .max_transmit_size(config.max_transmit_size)
        .flood_publish(config.flood_publish)
        .message_id_fn(message_id_fn)
        .validation_mode(gossipsub::ValidationMode::Strict)
        // Admission must run before gossipsub automatically forwards a batch.
        .validate_messages()
        .build()
        .map_err(|e| P2pError::Gossipsub(e.to_string()))?;

    let mut gossipsub_behaviour = gossipsub::Behaviour::new(
        gossipsub::MessageAuthenticity::Signed(keypair.clone()),
        gossipsub_config,
    )
    .map_err(|e| P2pError::Gossipsub(e.to_string()))?;

    // Apply peer scoring.
    let (score_params, score_thresholds) = build_peer_scoring_params();
    gossipsub_behaviour
        .with_peer_score(score_params, score_thresholds)
        .map_err(P2pError::Gossipsub)?;

    // Subscribe to all WRFCoin topics.
    let tm = TopicManager::new();
    for topic in tm.all_topics() {
        gossipsub_behaviour
            .subscribe(topic)
            .map_err(|e| P2pError::Gossipsub(format!("subscribe failed: {e}")))?;
    }

    // -- Kademlia ------------------------------------------------------------
    let local_peer_id = PeerId::from(keypair.public());
    let store = kad::store::MemoryStore::new(local_peer_id);

    let kad_protocol = StreamProtocol::try_from_owned(config.kademlia_protocol.clone())
        .map_err(|e| P2pError::Config(format!("invalid kademlia protocol: {e}")))?;

    let mut kad_config = kad::Config::new(kad_protocol);
    kad_config.set_query_timeout(std::time::Duration::from_secs(60));

    let kademlia = kad::Behaviour::with_config(local_peer_id, store, kad_config);

    // -- Identify ------------------------------------------------------------
    // When an operator-verified external address is configured, advertising the
    // expanded wildcard listen addresses alongside it leaks docker-bridge and
    // loopback multiaddrs to every peer; those unreachable addresses then
    // dominate remote dial candidates (#2819). hide_listen_addrs also stops
    // libp2p-identify's internal NewExternalAddrOfPeer injection at the sender,
    // which no receive-side filter can intercept. Nodes WITHOUT an external
    // address keep the default so LAN-only development discovery still works.
    let hide_listen_addrs =
        config.external_address.is_some() || config.quic_external_address.is_some();
    let identify = identify::Behaviour::new(
        identify::Config::new("/wrfcoin/id/1.0.0".to_string(), keypair.public())
            .with_hide_listen_addrs(hide_listen_addrs),
    );
    let protocols = std::iter::once((
        StreamProtocol::new(BLOCK_SYNC_PROTOCOL),
        libp2p::request_response::ProtocolSupport::Full,
    ));
    let block_sync_config = libp2p::request_response::Config::default()
        .with_request_timeout(BLOCK_SYNC_REQUEST_TIMEOUT);
    let block_sync = libp2p::request_response::Behaviour::with_codec(
        BlockSyncCodec,
        protocols,
        block_sync_config,
    );

    // -- NAT traversal -------------------------------------------------------
    let dcutr = dcutr::Behaviour::new(local_peer_id);
    let mut autonat_config = autonat::Config::default();
    autonat_config.only_global_ips = config.only_global_ips;
    let autonat = autonat::Behaviour::new(local_peer_id, autonat_config);
    // UPnP construction starts gateway discovery. A loopback-only rehearsal
    // must not send that discovery to a LAN router.
    let upnp = (!loopback_listeners_only(config))
        .then(upnp::tokio::Behaviour::default)
        .into();

    Ok(WrfcoinBehaviour {
        connection_quota,
        connection_limits,
        gossipsub: gossipsub_behaviour,
        kademlia,
        identify,
        block_sync,
        relay_client: relay_behaviour,
        dcutr,
        autonat,
        upnp,
    })
}

// ---------------------------------------------------------------------------
// Test harness (#2118)
// ---------------------------------------------------------------------------

/// Build a real [`Swarm<WrfcoinBehaviour>`] for tests, without binding a socket.
///
/// # Why this exists
///
/// `node::handle_behaviour_event` takes `&mut Swarm<WrfcoinBehaviour>`, and until now no
/// test in this crate could produce one — so every branch of that function was unreachable
/// by tests *by construction*, not by neglect. Four separate findings in one session traced
/// back to it: a rule wired into startup that no test proved was called, an observe-only
/// step whose removal left the whole suite green, and two more of the same shape. A RED
/// control that cannot fail is worse than no control, because it reads as evidence.
///
/// The missing piece was only ever `relay::client::Behaviour`, which production obtains
/// from [`libp2p::SwarmBuilder::with_relay_client`]. [`relay::client::new`] returns one
/// directly, so the composite behaviour needs nothing the builder alone could give.
///
/// # Listening is optional, and binds nothing
///
/// The swarm is constructed and not told to listen. Tests that drive
/// `handle_behaviour_event` feed it events directly, so no socket is required. A test that
/// wants two nodes to actually talk can call `listen_on("/memory/0")` and dial: the
/// transport is `MemoryTransport`, so even that binds no socket and stays usable in
/// sandboxes that deny binds (#1821).
///
/// # The relay transport must be kept
///
/// `relay::client::new` returns a transport *and* a behaviour joined by a channel, and
/// `relay::client::Behaviour` panics with `unreachable!` the moment it is polled after its
/// transport half has been dropped. Dropping the transport therefore produced a swarm that
/// was fine to feed events into but panicked on the first `select_next_some()` — silently
/// ruling out every test where two nodes exchange a real message. It is composed into the
/// transport stack here instead, which is what `SwarmBuilder::with_relay_client` does in
/// production.
#[cfg(any(test, feature = "test-harness"))]
pub fn test_swarm_with_config(
    config: &P2pConfig,
) -> P2pResult<libp2p::Swarm<WrfcoinBehaviour>> {
    use libp2p::SwarmBuilder;

    let keypair = Keypair::generate_ed25519();
    let local_peer_id = libp2p::PeerId::from(keypair.public());
    let (relay_transport, relay_behaviour) = relay::client::new(local_peer_id);

    // #2113 added a `metrics` parameter (the quota behaviour counts its own
    // rejections). The harness supplies a throwaway collector: these tests
    // assert on behaviour, not on counters.
    let behaviour = build_behaviour(
        &keypair,
        config,
        relay_behaviour,
        std::sync::Arc::new(crate::metrics::MetricsCollector::new()),
    )?;

    let swarm = SwarmBuilder::with_existing_identity(keypair)
        .with_tokio()
        .with_other_transport(|k| {
            use libp2p::Transport as _;
            libp2p::core::transport::OrTransport::new(
                relay_transport,
                libp2p::core::transport::MemoryTransport::default(),
            )
            .upgrade(libp2p::core::upgrade::Version::V1)
            .authenticate(
                libp2p::noise::Config::new(k)
                    .expect("noise config for an in-memory test transport"), // panic-ok: test-harness-only in-memory transport configuration
            )
            .multiplex(libp2p::yamux::Config::default())
        })
        .map_err(|e| crate::error::P2pError::Transport(format!("test transport: {e}")))?
        .with_behaviour(|_| behaviour)
        .map_err(|e| crate::error::P2pError::Swarm(format!("{e:?}")))?
        .build();

    Ok(swarm)
}

/// [`test_swarm_with_config`] with [`P2pConfig::default`].
#[cfg(any(test, feature = "test-harness"))]
pub fn test_swarm() -> P2pResult<libp2p::Swarm<WrfcoinBehaviour>> {
    test_swarm_with_config(&P2pConfig::default())
}

fn loopback_listeners_only(config: &P2pConfig) -> bool {
    let addresses: Vec<_> = config.listen_addresses.iter()
        .chain(config.quic_listen_addresses.iter()).collect();
    !addresses.is_empty() && addresses.iter().all(|address| {
        matches!(address.iter().next(),
            Some(libp2p::multiaddr::Protocol::Ip4(ip)) if ip.is_loopback())
        || matches!(address.iter().next(),
            Some(libp2p::multiaddr::Protocol::Ip6(ip)) if ip.is_loopback())
    })
}

#[cfg(test)]
mod loopback_discovery_tests {
    use super::*;

    #[test]
    fn only_exclusively_loopback_listeners_suppress_gateway_discovery() {
        let mut config = P2pConfig::default();
        config.listen_addresses = vec!["/ip4/127.0.0.1/tcp/19010".parse().unwrap()];
        config.quic_listen_addresses = vec!["/ip6/::1/udp/19010/quic-v1".parse().unwrap()];
        assert!(loopback_listeners_only(&config));
        config.listen_addresses.push("/ip4/0.0.0.0/tcp/19011".parse().unwrap());
        assert!(!loopback_listeners_only(&config));
        config.listen_addresses.clear();
        config.quic_listen_addresses.clear();
        assert!(!loopback_listeners_only(&config));
    }
}
