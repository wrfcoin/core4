use tauri::{command, State};
use serde::{Serialize, Deserialize};
use log::{info, error};

use crate::AppState;
use crate::network::node::NodeStatus;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkResponse {
    pub success: bool,
    pub data: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[command]
pub async fn get_network_status(state: State<'_, AppState>) -> NetworkResponse {
    info!("Command called: get_network_status");
    
    let node = state.node.lock().await;
    let status = node.get_status().await;
    
    match serde_json::to_value(&status) {
        Ok(value) => NetworkResponse {
            success: true,
            data: Some(value),
            error: None,
        },
        Err(e) => {
            error!("Failed to serialize network status: {}", e);
            NetworkResponse {
                success: false,
                data: None,
                error: Some(format!("Failed to serialize network status: {}", e)),
            }
        }
    }
}

#[command]
pub async fn connect_to_peer(state: State<'_, AppState>, peer_address: String) -> NetworkResponse {
    info!("Command called: connect_to_peer with address: {}", peer_address);
    
    let node = state.node.lock().await;
    
    match node.connect_to_peer(&peer_address).await {
        Ok(_) => NetworkResponse {
            success: true,
            data: Some(serde_json::json!({
                "message": format!("Connecting to peer at {}", peer_address),
            })),
            error: None,
        },
        Err(e) => {
            error!("Failed to connect to peer: {}", e);
            NetworkResponse {
                success: false,
                data: None,
                error: Some(format!("Failed to connect to peer: {}", e)),
            }
        }
    }
}

#[command]
pub async fn disconnect_from_peer(state: State<'_, AppState>, peer_id: String) -> NetworkResponse {
    info!("Command called: disconnect_from_peer with id: {}", peer_id);
    
    // In a real implementation, you'd have a method to disconnect from a specific peer
    // For this example, we'll just return success
    
    NetworkResponse {
        success: true,
        data: Some(serde_json::json!({
            "message": format!("Disconnected from peer {}", peer_id),
        })),
        error: None,
    }
}

#[command]
pub async fn get_known_peers(state: State<'_, AppState>) -> NetworkResponse {
    info!("Command called: get_known_peers");
    
    let node = state.node.lock().await;
    let status = node.get_status().await;
    
    NetworkResponse {
        success: true,
        data: Some(serde_json::json!({
            "peers": status.known_addresses,
        })),
        error: None,
    }
}

#[command]
pub async fn start_node(state: State<'_, AppState>, listen_address: String) -> NetworkResponse {
    info!("Command called: start_node with listen address: {}", listen_address);
    
    let mut node = state.node.lock().await;
    
    match node.start(&listen_address).await {
        Ok(_) => NetworkResponse {
            success: true,
            data: Some(serde_json::json!({
                "message": format!("Node started on {}", listen_address),
            })),
            error: None,
        },
        Err(e) => {
            error!("Failed to start node: {}", e);
            NetworkResponse {
                success: false,
                data: None,
                error: Some(format!("Failed to start node: {}", e)),
            }
        }
    }
}

#[command]
pub async fn stop_node(state: State<'_, AppState>) -> NetworkResponse {
    info!("Command called: stop_node");
    
    let mut node = state.node.lock().await;
    node.stop().await;
    
    NetworkResponse {
        success: true,
        data: Some(serde_json::json!({
            "message": "Node stopped",
        })),
        error: None,
    }
}

#[command]
pub async fn broadcast_block(state: State<'_, AppState>, block_hash: String) -> NetworkResponse {
    info!("Command called: broadcast_block with hash: {}", block_hash);
    
    let blockchain = state.blockchain.lock().await;
    
    match blockchain.get_block_by_hash(&block_hash) {
        Some(block) => {
            let mut node = state.node.lock().await;
            match node.broadcast_block(block.clone()).await {
                Ok(_) => NetworkResponse {
                    success: true,
                    data: Some(serde_json::json!({
                        "message": format!("Block {} broadcasted to network", block_hash),
                    })),
                    error: None,
                },
                Err(e) => {
                    error!("Failed to broadcast block: {}", e);
                    NetworkResponse {
                        success: false,
                        data: None,
                        error: Some(format!("Failed to broadcast block: {}", e)),
                    }
                }
            }
        },
        None => NetworkResponse {
            success: false,
            data: None,
            error: Some(format!("Block with hash {} not found", block_hash)),
        }
    }
}

#[command]
pub async fn sync_blockchain(state: State<'_, AppState>) -> NetworkResponse {
    info!("Command called: sync_blockchain");
    
    let mut node = state.node.lock().await;
    
    match node.sync_blockchain().await {
        Ok(_) => NetworkResponse {
            success: true,
            data: Some(serde_json::json!({
                "message": "Blockchain synchronized with the network",
            })),
            error: None,
        },
        Err(e) => {
            error!("Failed to sync blockchain: {}", e);
            NetworkResponse {
                success: false,
                data: None,
                error: Some(format!("Failed to sync blockchain: {}", e)),
            }
        }
    }
}
