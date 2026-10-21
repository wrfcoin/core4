use tauri::{command, State};
use serde::{Serialize, Deserialize};
use std::sync::Arc;
use crate::AppState;
use log::{info, error};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockchainResponse {
    pub success: bool,
    pub data: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[command]
pub async fn get_blockchain_info(state: State<'_, AppState>) -> BlockchainResponse {
    info!("Command called: get_blockchain_info");
    
    let blockchain = state.blockchain.lock().await;
    let blockchain_info = blockchain.get_blockchain_info();
    
    match serde_json::to_value(&blockchain_info) {
        Ok(value) => BlockchainResponse {
            success: true,
            data: Some(value),
            error: None,
        },
        Err(e) => {
            error!("Failed to serialize blockchain info: {}", e);
            BlockchainResponse {
                success: false,
                data: None,
                error: Some(format!("Failed to serialize blockchain info: {}", e)),
            }
        }
    }
}

#[command]
pub async fn get_latest_blocks(state: State<'_, AppState>, limit: u64) -> BlockchainResponse {
    info!("Command called: get_latest_blocks with limit {}", limit);
    
    let blockchain = state.blockchain.lock().await;
    let mut blocks = Vec::new();
    
    // Get the latest block height
    let height = match blockchain.get_latest_block() {
        Ok(block) => block.index,
        Err(e) => {
            error!("Failed to get latest block: {}", e);
            return BlockchainResponse {
                success: false,
                data: None,
                error: Some(format!("Failed to get latest block: {}", e)),
            };
        }
    };
    
    // Get the latest 'limit' blocks
    let start_height = if height >= limit { height - limit + 1 } else { 0 };
    
    for i in start_height..=height {
        if let Some(block) = blockchain.get_block_by_index(i) {
            blocks.push(block);
        }
    }
    
    match serde_json::to_value(&blocks) {
        Ok(value) => BlockchainResponse {
            success: true,
            data: Some(value),
            error: None,
        },
        Err(e) => {
            error!("Failed to serialize blocks: {}", e);
            BlockchainResponse {
                success: false,
                data: None,
                error: Some(format!("Failed to serialize blocks: {}", e)),
            }
        }
    }
}

#[command]
pub async fn get_block_by_hash(state: State<'_, AppState>, hash: String) -> BlockchainResponse {
    info!("Command called: get_block_by_hash with hash {}", hash);
    
    let blockchain = state.blockchain.lock().await;
    
    match blockchain.get_block_by_hash(&hash) {
        Some(block) => match serde_json::to_value(&block) {
            Ok(value) => BlockchainResponse {
                success: true,
                data: Some(value),
                error: None,
            },
            Err(e) => {
                error!("Failed to serialize block: {}", e);
                BlockchainResponse {
                    success: false,
                    data: None,
                    error: Some(format!("Failed to serialize block: {}", e)),
                }
            }
        },
        None => BlockchainResponse {
            success: false,
            data: None,
            error: Some(format!("Block with hash {} not found", hash)),
        }
    }
}

#[command]
pub async fn get_block_by_height(state: State<'_, AppState>, height: u64) -> BlockchainResponse {
    info!("Command called: get_block_by_height with height {}", height);
    
    let blockchain = state.blockchain.lock().await;
    
    match blockchain.get_block_by_index(height) {
        Some(block) => match serde_json::to_value(&block) {
            Ok(value) => BlockchainResponse {
                success: true,
                data: Some(value),
                error: None,
            },
            Err(e) => {
                error!("Failed to serialize block: {}", e);
                BlockchainResponse {
                    success: false,
                    data: None,
                    error: Some(format!("Failed to serialize block: {}", e)),
                }
            }
        },
        None => BlockchainResponse {
            success: false,
            data: None,
            error: Some(format!("Block with height {} not found", height)),
        }
    }
}

#[command]
pub async fn get_transaction_by_id(state: State<'_, AppState>, tx_id: String) -> BlockchainResponse {
    info!("Command called: get_transaction_by_id with tx_id {}", tx_id);
    
    let blockchain = state.blockchain.lock().await;
    
    match blockchain.get_transaction_by_id(&tx_id) {
        Some(tx) => match serde_json::to_value(&tx) {
            Ok(value) => BlockchainResponse {
                success: true,
                data: Some(value),
                error: None,
            },
            Err(e) => {
                error!("Failed to serialize transaction: {}", e);
                BlockchainResponse {
                    success: false,
                    data: None,
                    error: Some(format!("Failed to serialize transaction: {}", e)),
                }
            }
        },
        None => BlockchainResponse {
            success: false,
            data: None,
            error: Some(format!("Transaction with id {} not found", tx_id)),
        }
    }
}

#[command]
pub async fn get_blockchain_stats(state: State<'_, AppState>) -> BlockchainResponse {
    info!("Command called: get_blockchain_stats");
    
    let blockchain = state.blockchain.lock().await;
    let info = blockchain.get_blockchain_info();
    
    // Get additional stats
    let block_count = info.height + 1;
    let transaction_count = info.total_transactions;
    
    let stats = serde_json::json!({
        "blockCount": block_count,
        "transactionCount": transaction_count,
        "difficulty": info.difficulty,
        "averageBlockTime": info.average_block_time,
        "hashrate": info.network_hashrate,
        "miningReward": info.mining_reward,
    });
    
    BlockchainResponse {
        success: true,
        data: Some(stats),
        error: None,
    }
}

#[command]
pub async fn get_latest_transactions(state: State<'_, AppState>, limit: u64) -> BlockchainResponse {
    info!("Command called: get_latest_transactions with limit {}", limit);
    
    let blockchain = state.blockchain.lock().await;
    let mut transactions = Vec::new();
    let mut count = 0;
    
    // Iterate through blocks in reverse order to get latest transactions
    let blockchain_length = blockchain.chain.len();
    
    for i in (0..blockchain_length).rev() {
        let block = &blockchain.chain[i];
        
        for tx in &block.transactions {
            transactions.push(tx.clone());
            count += 1;
            
            if count >= limit {
                break;
            }
        }
        
        if count >= limit {
            break;
        }
    }
    
    match serde_json::to_value(&transactions) {
        Ok(value) => BlockchainResponse {
            success: true,
            data: Some(value),
            error: None,
        },
        Err(e) => {
            error!("Failed to serialize transactions: {}", e);
            BlockchainResponse {
                success: false,
                data: None,
                error: Some(format!("Failed to serialize transactions: {}", e)),
            }
        }
    }
}
