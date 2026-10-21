use crate::node::{NodeInitOptions, NodeOperator, NodeRole, NodeType, NodeStatus};
use crate::node::defaults::DefaultNodeOperator;
use crate::core::error::WrfError;
use std::error::Error;

/// Test that creating and operating a basic node works correctly
#[test]
fn test_basic_node_operations() -> Result<(), Box<dyn Error>> {
    // Create a node operator
    let operator = DefaultNodeOperator::new();
    
    // Initialize a node with test options
    let options = NodeInitOptions {
        node_type: NodeType::Full,
        role: NodeRole::Peer,
        network: "testnet".to_string(),
        data_dir: ".wrfcoin-test".to_string(),
        ..Default::default()
    };
    
    // Initialize the node
    let result = operator.init_node(options)?;
    assert!(!result.node_id.is_empty(), "Node ID should not be empty");
    
    // Check node status - should be Initialized
    let status = operator.get_node_status(&result.node_id)?;
    assert_eq!(status, NodeStatus::Initialized, "Node should be in Initialized state");
    
    // Start the node
    operator.start_node(&result.node_id)?;
    
    // Check status again - should be Running
    let status = operator.get_node_status(&result.node_id)?;
    assert_eq!(status, NodeStatus::Running, "Node should be in Running state");
    
    // Stop the node
    operator.stop_node(&result.node_id)?;
    
    // Check status again - should be Stopped
    let status = operator.get_node_status(&result.node_id)?;
    assert_eq!(status, NodeStatus::Stopped, "Node should be in Stopped state");
    
    // Get node info
    let info = operator.get_node_info(&result.node_id)?;
    assert_eq!(info.node_type, NodeType::Full, "Node type should be Full");
    assert_eq!(info.role, NodeRole::Peer, "Node role should be Peer");
    assert_eq!(info.network, "testnet", "Network should be testnet");
    
    Ok(())
}

/// Test invalid node operations to ensure proper error handling
#[test]
fn test_invalid_node_operations() -> Result<(), Box<dyn Error>> {
    // Create a node operator
    let operator = DefaultNodeOperator::new();
    
    // Try to get status of a non-existent node
    let result = operator.get_node_status("non-existent-node");
    assert!(result.is_err(), "Should error on non-existent node");
    
    if let Err(err) = result {
        assert!(matches!(err, WrfError::NodeNotFound { .. }), 
                "Error should be NodeNotFound, got: {:?}", err);
    }
    
    // Create a valid node
    let options = NodeInitOptions {
        node_type: NodeType::Light,
        role: NodeRole::Client,
        network: "testnet".to_string(),
        data_dir: ".wrfcoin-test-invalid".to_string(),
        ..Default::default()
    };
    
    let node_result = operator.init_node(options)?;
    
    // Try to stop a node that isn't running
    let stop_result = operator.stop_node(&node_result.node_id);
    assert!(stop_result.is_err(), "Should error when stopping a non-running node");
    
    if let Err(err) = stop_result {
        assert!(matches!(err, WrfError::InvalidNodeState { .. }), 
                "Error should be InvalidNodeState, got: {:?}", err);
    }
    
    Ok(())
}

/// Test node with invalid configuration
#[test]
fn test_invalid_node_config() -> Result<(), Box<dyn Error>> {
    // Create a node operator
    let operator = DefaultNodeOperator::new();
    
    // Initialize a node with invalid options (empty data directory)
    let options = NodeInitOptions {
        node_type: NodeType::Full,
        role: NodeRole::Peer,
        network: "testnet".to_string(),
        data_dir: "".to_string(), // Invalid empty data directory
        ..Default::default()
    };
    
    // Initialize should fail
    let result = operator.init_node(options);
    assert!(result.is_err(), "Should error on invalid config");
    
    if let Err(err) = result {
        assert!(matches!(err, WrfError::InvalidConfiguration { .. }), 
                "Error should be InvalidConfiguration, got: {:?}", err);
    }
    
    Ok(())
}

/// Test node restart functionality
#[test]
fn test_node_restart() -> Result<(), Box<dyn Error>> {
    // Create a node operator
    let operator = DefaultNodeOperator::new();
    
    // Initialize a node
    let options = NodeInitOptions {
        node_type: NodeType::Full,
        role: NodeRole::Peer,
        network: "testnet".to_string(),
        data_dir: ".wrfcoin-test-restart".to_string(),
        ..Default::default()
    };
    
    let result = operator.init_node(options)?;
    
    // Start the node
    operator.start_node(&result.node_id)?;
    
    // Restart the node
    operator.restart_node(&result.node_id)?;
    
    // Check status - should still be Running after restart
    let status = operator.get_node_status(&result.node_id)?;
    assert_eq!(status, NodeStatus::Running, "Node should be in Running state after restart");
    
    // Stop the node
    operator.stop_node(&result.node_id)?;
    
    Ok(())
}