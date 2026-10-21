use crate::node::discovery::{DiscoveryOptions, PeerDiscovery, DiscoveryMethod, Peer};
use crate::core::error::WrfError;
use std::error::Error;
use std::time::Duration;

/// Test peer discovery with default options
#[test]
fn test_peer_discovery_default() -> Result<(), Box<dyn Error>> {
    // Create discovery with default options
    let discovery_options = DiscoveryOptions::default();
    let discovery = PeerDiscovery::new(discovery_options);
    
    // Discover peers
    let peers = discovery.discover_peers()?;
    
    // In test environment, we should find at least some default peers
    assert!(!peers.is_empty(), "Should discover at least one peer");
    
    // Check returned peer information
    for peer in peers {
        assert!(!peer.id.is_empty(), "Peer ID should not be empty");
        assert!(!peer.addr.is_empty(), "Peer address should not be empty");
        assert!(!peer.version.is_empty(), "Peer version should not be empty");
        assert!(peer.score >= 0.0, "Peer score should be non-negative");
    }
    
    Ok(())
}

/// Test peer discovery with DNS discovery method
#[test]
fn test_peer_discovery_dns() -> Result<(), Box<dyn Error>> {
    // Create discovery with DNS options
    let discovery_options = DiscoveryOptions {
        discovery_method: DiscoveryMethod::Dns,
        max_peers: 10,
        discovery_interval: Duration::from_secs(60),
        bootstrap_nodes: vec![
            "bootstrap1.wrfcoin.testnet:26656".to_string(),
            "bootstrap2.wrfcoin.testnet:26656".to_string(),
        ],
    };
    
    let discovery = PeerDiscovery::new(discovery_options);
    
    // Discover peers
    let peers = discovery.discover_peers()?;
    
    // Check that we got the expected number of peers
    assert!(peers.len() <= 10, "Should discover at most max_peers");
    
    Ok(())
}

/// Test peer discovery with static peers
#[test]
fn test_peer_discovery_static() -> Result<(), Box<dyn Error>> {
    // Create discovery with static peers
    let discovery_options = DiscoveryOptions {
        discovery_method: DiscoveryMethod::Static,
        max_peers: 5,
        discovery_interval: Duration::from_secs(60),
        bootstrap_nodes: vec![
            "static1.wrfcoin.testnet:26656".to_string(),
            "static2.wrfcoin.testnet:26656".to_string(),
            "static3.wrfcoin.testnet:26656".to_string(),
        ],
    };
    
    let discovery = PeerDiscovery::new(discovery_options);
    
    // Discover peers
    let peers = discovery.discover_peers()?;
    
    // With static discovery, we should get exactly the bootstrap nodes
    assert_eq!(peers.len(), 3, "Should discover exactly the static peers");
    
    // Verify that the peers match the bootstrap nodes
    let peer_addrs: Vec<String> = peers.iter().map(|p| p.addr.clone()).collect();
    assert!(peer_addrs.contains(&"static1.wrfcoin.testnet:26656".to_string()));
    assert!(peer_addrs.contains(&"static2.wrfcoin.testnet:26656".to_string()));
    assert!(peer_addrs.contains(&"static3.wrfcoin.testnet:26656".to_string()));
    
    Ok(())
}

/// Test peer discovery with invalid configuration
#[test]
fn test_peer_discovery_invalid_config() -> Result<(), Box<dyn Error>> {
    // Create discovery with invalid options (no bootstrap nodes for static method)
    let discovery_options = DiscoveryOptions {
        discovery_method: DiscoveryMethod::Static,
        max_peers: 5,
        discovery_interval: Duration::from_secs(60),
        bootstrap_nodes: vec![], // No bootstrap nodes - invalid for static method
    };
    
    let discovery = PeerDiscovery::new(discovery_options);
    
    // Discover peers should fail
    let result = discovery.discover_peers();
    assert!(result.is_err(), "Should error on invalid config");
    
    if let Err(err) = result {
        assert!(matches!(err, WrfError::InvalidConfiguration { .. }), 
                "Error should be InvalidConfiguration, got: {:?}", err);
    }
    
    Ok(())
}

/// Test filtering and sorting peers
#[test]
fn test_peer_filtering_and_sorting() -> Result<(), Box<dyn Error>> {
    // Create a discovery instance
    let discovery = PeerDiscovery::new(DiscoveryOptions::default());
    
    // Create a set of test peers
    let peers = vec![
        Peer {
            id: "peer1".to_string(),
            addr: "10.0.0.1:26656".to_string(),
            node_type: "full".to_string(),
            version: "1.0.0".to_string(),
            score: 0.8,
            last_seen: 100,
        },
        Peer {
            id: "peer2".to_string(),
            addr: "10.0.0.2:26656".to_string(),
            node_type: "validator".to_string(),
            version: "1.0.1".to_string(),
            score: 0.9,
            last_seen: 200,
        },
        Peer {
            id: "peer3".to_string(),
            addr: "10.0.0.3:26656".to_string(),
            node_type: "light".to_string(),
            version: "0.9.0".to_string(),
            score: 0.5,
            last_seen: 300,
        },
        Peer {
            id: "peer4".to_string(),
            addr: "10.0.0.4:26656".to_string(),
            node_type: "full".to_string(),
            version: "1.0.0".to_string(),
            score: 0.7,
            last_seen: 50,
        },
    ];
    
    // Filter to only get full nodes
    let full_nodes = discovery.filter_peers(&peers, |p| p.node_type == "full");
    assert_eq!(full_nodes.len(), 2, "Should have 2 full nodes");
    assert!(full_nodes.iter().all(|p| p.node_type == "full"));
    
    // Sort by score (highest first)
    let sorted_by_score = discovery.sort_peers_by_score(&peers);
    assert_eq!(sorted_by_score[0].id, "peer2", "Highest score should be first");
    assert_eq!(sorted_by_score[3].id, "peer3", "Lowest score should be last");
    
    // Sort by recency (most recent first)
    let sorted_by_recency = discovery.sort_peers_by_recency(&peers);
    assert_eq!(sorted_by_recency[0].id, "peer3", "Most recent should be first");
    assert_eq!(sorted_by_recency[3].id, "peer4", "Least recent should be last");
    
    Ok(())
}