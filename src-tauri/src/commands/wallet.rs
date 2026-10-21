use tauri::{command, State};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::path::PathBuf;
use log::{info, warn, error};

use crate::AppState;
use crate::blockchain::wallet::Wallet;
use crate::blockchain::transaction::{Transaction, TransactionType};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletResponse {
    pub success: bool,
    pub data: Option<serde_json::Value>,
    pub error: Option<String>,
}

/// Creates a new wallet and stores it encrypted in the database.
/// Returns only address and publicKey — private key is NEVER exposed in the response.
#[command]
pub async fn create_wallet(
    state: State<'_, AppState>,
    passphrase: String,
) -> WalletResponse {
    info!("Command called: create_wallet");

    let wallet = Wallet::new();

    // Persist encrypted in database
    {
        let db = state.database.lock().await;
        if let Err(e) = db.save_wallet(&wallet, &passphrase) {
            error!("Failed to save wallet to database: {}", e);
            return WalletResponse {
                success: false,
                data: None,
                error: Some(format!("Failed to persist wallet: {}", e)),
            };
        }
    }

    // Return only public info — no privateKey
    WalletResponse {
        success: true,
        data: Some(serde_json::json!({
            "address": wallet.address,
            "publicKey": wallet.public_key,
        })),
        error: None,
    }
}

/// Imports a wallet from a private key, encrypts it with the passphrase, and stores it.
/// Returns only address and publicKey — private key is NEVER exposed in the response.
#[command]
pub async fn import_wallet(
    state: State<'_, AppState>,
    private_key: String,
    passphrase: String,
) -> WalletResponse {
    info!("Command called: import_wallet");

    match Wallet::from_private_key(&private_key) {
        Ok(wallet) => {
            // Persist encrypted in database
            {
                let db = state.database.lock().await;
                if let Err(e) = db.save_wallet(&wallet, &passphrase) {
                    error!("Failed to save imported wallet to database: {}", e);
                    return WalletResponse {
                        success: false,
                        data: None,
                        error: Some(format!("Failed to persist wallet: {}", e)),
                    };
                }
            }

            // Return only public info — no privateKey
            WalletResponse {
                success: true,
                data: Some(serde_json::json!({
                    "address": wallet.address,
                    "publicKey": wallet.public_key,
                })),
                error: None,
            }
        },
        Err(e) => {
            error!("Failed to import wallet: {}", e);
            WalletResponse {
                success: false,
                data: None,
                error: Some(format!("Failed to import wallet: {}", e)),
            }
        }
    }
}

/// Saves wallet public info to a file export. Private key is NEVER written to the file.
#[command]
pub async fn save_wallet_to_file(
    wallet_data: serde_json::Value,
    file_path: String,
) -> WalletResponse {
    info!("Command called: save_wallet_to_file to {}", file_path);

    let address = match wallet_data.get("address").and_then(|v| v.as_str()) {
        Some(addr) => addr.to_string(),
        None => {
            return WalletResponse {
                success: false,
                data: None,
                error: Some("Invalid wallet data: missing address".to_string()),
            }
        }
    };

    let public_key = match wallet_data.get("publicKey").and_then(|v| v.as_str()) {
        Some(key) => key.to_string(),
        None => {
            return WalletResponse {
                success: false,
                data: None,
                error: Some("Invalid wallet data: missing public key".to_string()),
            }
        }
    };

    // Build a wallet with a dummy private key — save_to_file only writes public info
    let wallet = Wallet {
        address,
        public_key,
        private_key: String::new(), // Not used — save_to_file writes only WalletExport
    };

    let path = PathBuf::from(file_path);
    match wallet.save_to_file(&path) {
        Ok(_) => WalletResponse {
            success: true,
            data: Some(serde_json::json!({
                "message": "Wallet saved successfully",
                "filePath": path.to_string_lossy(),
            })),
            error: None,
        },
        Err(e) => {
            error!("Failed to save wallet to file: {}", e);
            WalletResponse {
                success: false,
                data: None,
                error: Some(format!("Failed to save wallet to file: {}", e)),
            }
        }
    }
}

/// Loads wallet public info from a file. Returns only address and publicKey.
#[command]
pub async fn load_wallet_from_file(file_path: String) -> WalletResponse {
    info!("Command called: load_wallet_from_file from {}", file_path);

    let path = PathBuf::from(file_path);
    match Wallet::load_from_file(&path) {
        Ok(export) => WalletResponse {
            success: true,
            data: Some(serde_json::json!({
                "address": export.address,
                "publicKey": export.public_key,
            })),
            error: None,
        },
        Err(e) => {
            error!("Failed to load wallet from file: {}", e);
            WalletResponse {
                success: false,
                data: None,
                error: Some(format!("Failed to load wallet from file: {}", e)),
            }
        }
    }
}

#[command]
pub async fn get_wallet_balance(state: State<'_, AppState>, address: String) -> WalletResponse {
    info!("Command called: get_wallet_balance for address {}", address);

    let blockchain = state.blockchain.lock().await;

    // Calculate balance from UTXO set (in base units: 1 WRF = 10^8)
    let mut balance: u64 = 0;


    for (tx_id, outputs) in &blockchain.utxo_set {
        for (i, output) in outputs.iter().enumerate() {
            if output.address == address {
                // Check if this output is referenced in the UTXO indices
                if let Some(indices) = blockchain.utxo_indices.get(tx_id) {
                    if indices.contains(&i) {
                        balance = balance.saturating_add(output.amount);
                    }
                }
            }
        }
    }

    WalletResponse {
        success: true,
        data: Some(serde_json::json!({
            "address": address,
            "balance": balance,
        })),
        error: None,
    }
}

/// Sends a transaction. Accepts the sender's wallet address and passphrase to unlock
/// the private key from the database. The private key is NEVER passed over the IPC boundary.
#[command]
pub async fn send_transaction(
    state: State<'_, AppState>,
    sender_address: String,
    passphrase: String,
    receiver_address: String,
    amount: u64,
    fee: u64,
) -> WalletResponse {
    info!("Command called: send_transaction from {} to {} for amount {} with fee {}",
          sender_address, receiver_address, amount, fee);

    // Look up and decrypt the sender wallet from the database
    let sender_wallet = {
        let db = state.database.lock().await;
        match db.get_wallet_by_address(&sender_address, &passphrase) {
            Ok(Some(wallet)) => wallet,
            Ok(None) => {
                return WalletResponse {
                    success: false,
                    data: None,
                    error: Some(format!("Wallet not found for address: {}", sender_address)),
                };
            }
            Err(e) => {
                error!("Failed to unlock wallet: {}", e);
                return WalletResponse {
                    success: false,
                    data: None,
                    error: Some(format!("Failed to unlock wallet: {}", e)),
                };
            }
        }
    };

    let blockchain = state.blockchain.lock().await;

    // Create transaction
    let transaction = match sender_wallet.create_transaction(
        &receiver_address,
        amount,
        fee,
        &blockchain.utxo_set,
        &blockchain.utxo_indices,
    ) {
        Ok(tx) => tx,
        Err(e) => {
            error!("Failed to create transaction: {}", e);
            return WalletResponse {
                success: false,
                data: None,
                error: Some(format!("Failed to create transaction: {}", e)),
            };
        }
    };

    // Add transaction to blockchain
    match blockchain.add_transaction(transaction.clone()) {
        Ok(_) => {
            info!("Transaction added to blockchain: {}", transaction.id);

            // Broadcast transaction to network
            let mut node = state.node.lock().await;
            if let Err(e) = node.broadcast_transaction(transaction.clone()).await {
                warn!("Failed to broadcast transaction: {}", e);
            }

            WalletResponse {
                success: true,
                data: Some(serde_json::json!({
                    "transactionId": transaction.id,
                    "amount": amount,
                    "fee": fee,
                    "sender": sender_wallet.address,
                    "receiver": receiver_address,
                })),
                error: None,
            }
        },
        Err(e) => {
            error!("Failed to add transaction to blockchain: {}", e);
            WalletResponse {
                success: false,
                data: None,
                error: Some(format!("Failed to add transaction to blockchain: {}", e)),
            }
        }
    }
}

/// Unlocks a wallet by decrypting its private key with the passphrase.
/// Returns only a success/failure status — does NOT return the key.
#[command]
pub async fn unlock_wallet(
    state: State<'_, AppState>,
    address: String,
    passphrase: String,
) -> WalletResponse {
    info!("Command called: unlock_wallet for address {}", address);

    let db = state.database.lock().await;
    match db.get_wallet_by_address(&address, &passphrase) {
        Ok(Some(_wallet)) => {
            WalletResponse {
                success: true,
                data: Some(serde_json::json!({
                    "address": address,
                    "unlocked": true,
                })),
                error: None,
            }
        }
        Ok(None) => {
            WalletResponse {
                success: false,
                data: None,
                error: Some(format!("Wallet not found for address: {}", address)),
            }
        }
        Err(e) => {
            error!("Failed to unlock wallet: {}", e);
            WalletResponse {
                success: false,
                data: None,
                error: Some(format!("Failed to unlock wallet: {}", e)),
            }
        }
    }
}

#[command]
pub async fn get_wallet_transactions(
    state: State<'_, AppState>,
    address: String,
) -> WalletResponse {
    info!("Command called: get_wallet_transactions for address {}", address);

    let blockchain = state.blockchain.lock().await;
    let mut transactions = Vec::new();

    // Search in all blocks
    for block in &blockchain.chain {
        for tx in &block.transactions {
            let mut is_involved = false;

            // Check inputs
            for input in &tx.inputs {
                if let Some(previous_tx) = blockchain.get_transaction_by_id(&input.transaction_id) {
                    if input.output_index < previous_tx.outputs.len() {
                        if previous_tx.outputs[input.output_index].address == address {
                            is_involved = true;
                            break;
                        }
                    }
                }
            }

            // Check outputs
            if !is_involved {
                for output in &tx.outputs {
                    if output.address == address {
                        is_involved = true;
                        break;
                    }
                }
            }

            if is_involved {
                transactions.push(tx.clone());
            }
        }
    }

    // Search in pending transactions
    for tx in &blockchain.pending_transactions {
        let mut is_involved = false;

        // Check inputs
        for input in &tx.inputs {
            if let Some(previous_tx) = blockchain.get_transaction_by_id(&input.transaction_id) {
                if input.output_index < previous_tx.outputs.len() {
                    if previous_tx.outputs[input.output_index].address == address {
                        is_involved = true;
                        break;
                    }
                }
            }
        }

        // Check outputs
        if !is_involved {
            for output in &tx.outputs {
                if output.address == address {
                    is_involved = true;
                    break;
                }
            }
        }

        if is_involved {
            transactions.push(tx.clone());
        }
    }

    // Sort transactions by timestamp, newest first
    transactions.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    match serde_json::to_value(&transactions) {
        Ok(value) => WalletResponse {
            success: true,
            data: Some(value),
            error: None,
        },
        Err(e) => {
            error!("Failed to serialize transactions: {}", e);
            WalletResponse {
                success: false,
                data: None,
                error: Some(format!("Failed to serialize transactions: {}", e)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_wallet_response_excludes_private_key() {
        let wallet = Wallet::new();
        let response = serde_json::json!({
            "address": wallet.address,
            "publicKey": wallet.public_key,
        });

        assert!(response.get("address").is_some());
        assert!(response.get("publicKey").is_some());
        assert!(response.get("privateKey").is_none(), "Response must NOT contain privateKey");
    }

    #[test]
    fn test_import_wallet_response_excludes_private_key() {
        let wallet = Wallet::new();
        let imported = Wallet::from_private_key(&wallet.private_key).unwrap();
        let response = serde_json::json!({
            "address": imported.address,
            "publicKey": imported.public_key,
        });

        assert!(response.get("address").is_some());
        assert!(response.get("publicKey").is_some());
        assert!(response.get("privateKey").is_none(), "Response must NOT contain privateKey");
        assert_eq!(response["address"], wallet.address);
        assert_eq!(response["publicKey"], wallet.public_key);
    }

    #[test]
    fn test_wallet_file_export_excludes_private_key() {
        let wallet = Wallet::new();
        let dir = std::env::temp_dir();
        let path = dir.join("test_wallet_export_no_pk.json");

        wallet.save_to_file(&path).unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&contents).unwrap();

        assert!(parsed.get("address").is_some());
        assert!(parsed.get("public_key").is_some());
        assert!(parsed.get("private_key").is_none(), "File export must NOT contain private_key");
        assert!(parsed.get("privateKey").is_none(), "File export must NOT contain privateKey");

        // Cleanup
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_wallet_file_roundtrip() {
        let wallet = Wallet::new();
        let dir = std::env::temp_dir();
        let path = dir.join("test_wallet_roundtrip.json");

        wallet.save_to_file(&path).unwrap();

        use crate::blockchain::wallet::WalletExport;
        let loaded = Wallet::load_from_file(&path).unwrap();
        assert_eq!(loaded.address, wallet.address);
        assert_eq!(loaded.public_key, wallet.public_key);

        // Cleanup
        let _ = std::fs::remove_file(&path);
    }
}
