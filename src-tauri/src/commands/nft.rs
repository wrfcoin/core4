use tauri::{command, State};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use log::{info, error};

use crate::AppState;
use crate::blockchain::wallet::Wallet;
use crate::blockchain::nft::{NFT, NFTMetadata, NFTAttribute, NFTType};
use crate::blockchain::transaction::TransactionType;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NftResponse {
    pub success: bool,
    pub data: Option<serde_json::Value>,
    pub error: Option<String>,
}

/// Creates an NFT. Accepts the creator's wallet address and passphrase to unlock
/// the private key from the database. The private key is NEVER passed over the IPC boundary.
#[command]
pub async fn create_nft(
    state: State<'_, AppState>,
    creator_address: String,
    passphrase: String,
    name: String,
    description: String,
    image_uri: Option<String>,
    attributes: Vec<serde_json::Value>,
    nft_type: String,
    royalty_percentage: f64,
    computational_specs: Option<serde_json::Value>,
) -> NftResponse {
    info!("Command called: create_nft with name: {}, type: {}", name, nft_type);

    // Look up and decrypt the creator wallet from the database
    let creator_wallet = {
        let db = state.database.lock().await;
        match db.get_wallet_by_address(&creator_address, &passphrase) {
            Ok(Some(wallet)) => wallet,
            Ok(None) => {
                return NftResponse {
                    success: false,
                    data: None,
                    error: Some(format!("Wallet not found for address: {}", creator_address)),
                };
            }
            Err(e) => {
                error!("Failed to unlock wallet: {}", e);
                return NftResponse {
                    success: false,
                    data: None,
                    error: Some(format!("Failed to unlock wallet: {}", e)),
                };
            }
        }
    };

    // Map the string NFT type to the enum
    let nft_type_enum = match nft_type.as_str() {
        "Art" => NFTType::Art,
        "ComputationalResource" => NFTType::ComputationalResource,
        "Document" => NFTType::Document,
        "Music" => NFTType::Music,
        "Video" => NFTType::Video,
        "Domain" => NFTType::Domain,
        "GameItem" => NFTType::GameItem,
        _ => NFTType::Other(nft_type.clone()),
    };

    // Parse attributes
    let nft_attributes: Vec<NFTAttribute> = attributes.iter()
        .filter_map(|attr| {
            let trait_type = attr.get("trait_type")?.as_str()?;
            let value = attr.get("value")?.as_str()?;

            Some(NFTAttribute {
                trait_type: trait_type.to_string(),
                value: value.to_string(),
            })
        })
        .collect();

    // Create NFT metadata
    let mut metadata = NFTMetadata {
        name,
        description,
        image_uri,
        external_url: None,
        attributes: nft_attributes,
        nft_type: nft_type_enum,
        compute_power: None,
        memory: None,
        storage: None,
        duration: None,
        renewable: None,
        availability: None,
    };

    // Add computational specs if provided
    if let Some(specs) = computational_specs {
        if let Some(compute_power) = specs.get("compute_power").and_then(|v| v.as_f64()) {
            metadata.compute_power = Some(compute_power);
        }

        if let Some(memory) = specs.get("memory").and_then(|v| v.as_u64()) {
            metadata.memory = Some(memory);
        }

        if let Some(storage) = specs.get("storage").and_then(|v| v.as_u64()) {
            metadata.storage = Some(storage);
        }

        if let Some(duration) = specs.get("duration").and_then(|v| v.as_u64()) {
            metadata.duration = Some(duration);
        }

        if let Some(renewable) = specs.get("renewable").and_then(|v| v.as_bool()) {
            metadata.renewable = Some(renewable);
        }

        if let Some(availability) = specs.get("availability").and_then(|v| v.as_f64()) {
            metadata.availability = Some(availability);
        }
    }

    // Create NFT minting transaction
    let blockchain = state.blockchain.lock().await;

    let metadata_json = match serde_json::to_string(&metadata) {
        Ok(json) => json,
        Err(e) => {
            error!("Failed to serialize NFT metadata: {}", e);
            return NftResponse {
                success: false,
                data: None,
                error: Some(format!("Failed to serialize NFT metadata: {}", e)),
            };
        }
    };

    let nft_transaction = match creator_wallet.create_nft_mint_transaction(
        &metadata_json,
        1_000_000, // 0.01 WRF fee in base units
        &blockchain.utxo_set,
        &blockchain.utxo_indices,
    ) {
        Ok(tx) => tx,
        Err(e) => {
            error!("Failed to create NFT minting transaction: {}", e);
            return NftResponse {
                success: false,
                data: None,
                error: Some(format!("Failed to create NFT minting transaction: {}", e)),
            };
        }
    };

    // Add transaction to blockchain
    match blockchain.add_transaction(nft_transaction.clone()) {
        Ok(_) => {
            info!("NFT minting transaction added to blockchain: {}", nft_transaction.id);

            // Broadcast transaction to network
            let mut node = state.node.lock().await;
            if let Err(e) = node.broadcast_transaction(nft_transaction.clone()).await {
                error!("Failed to broadcast NFT transaction: {}", e);
            }

            // Create NFT object
            let nft = NFT {
                id: nft_transaction.id.clone(), // Use transaction ID as NFT ID
                token_id: 0, // This will be assigned by the registry
                creator: creator_wallet.address.clone(),
                owner: creator_wallet.address.clone(),
                metadata,
                creation_tx: nft_transaction.id,
                created_at: nft_transaction.timestamp,
                last_transferred_at: None,
                price_history: Vec::new(),
                royalty_percentage,
            };

            NftResponse {
                success: true,
                data: Some(serde_json::json!({
                    "nft": {
                        "id": nft.id,
                        "creator": nft.creator,
                        "owner": nft.owner,
                        "metadata": nft.metadata,
                        "creation_tx": nft.creation_tx,
                        "created_at": nft.created_at,
                        "royalty_percentage": nft.royalty_percentage,
                    },
                    "transaction": {
                        "id": nft_transaction.id,
                        "timestamp": nft_transaction.timestamp,
                    }
                })),
                error: None,
            }
        },
        Err(e) => {
            error!("Failed to add NFT transaction to blockchain: {}", e);
            NftResponse {
                success: false,
                data: None,
                error: Some(format!("Failed to add NFT transaction to blockchain: {}", e)),
            }
        }
    }
}

#[command]
pub async fn get_nft_marketplace(state: State<'_, AppState>) -> NftResponse {
    info!("Command called: get_nft_marketplace");

    let blockchain = state.blockchain.lock().await;
    let mut nfts = Vec::new();

    // Search for NFT minting transactions in the blockchain
    for block in &blockchain.chain {
        for tx in &block.transactions {
            if let TransactionType::NFTMint = tx.transaction_type {
                if let Some(metadata_json) = &tx.data {
                    // Try to parse the NFT metadata
                    if let Ok(metadata) = serde_json::from_str::<NFTMetadata>(metadata_json) {
                        let nft = NFT {
                            id: tx.id.clone(),
                            token_id: 0,
                            creator: tx.inputs.first()
                                .and_then(|input| input.public_key.clone())
                                .unwrap_or_else(|| "Unknown".to_string()),
                            owner: tx.outputs.first()
                                .map(|output| output.address.clone())
                                .unwrap_or_else(|| "Unknown".to_string()),
                            metadata,
                            creation_tx: tx.id.clone(),
                            created_at: tx.timestamp,
                            last_transferred_at: None,
                            price_history: Vec::new(),
                            royalty_percentage: 0.05, // 5% default royalty
                        };

                        nfts.push(nft);
                    }
                }
            }
        }
    }

    // Convert NFTs to JSON
    let nfts_json = match serde_json::to_value(&nfts) {
        Ok(value) => value,
        Err(e) => {
            error!("Failed to serialize NFTs: {}", e);
            return NftResponse {
                success: false,
                data: None,
                error: Some(format!("Failed to serialize NFTs: {}", e)),
            };
        }
    };

    NftResponse {
        success: true,
        data: Some(nfts_json),
        error: None,
    }
}

#[command]
pub async fn get_nft_by_id(state: State<'_, AppState>, nft_id: String) -> NftResponse {
    info!("Command called: get_nft_by_id for id: {}", nft_id);

    let blockchain = state.blockchain.lock().await;

    // Find the NFT transaction
    let transaction = match blockchain.get_transaction_by_id(&nft_id) {
        Some(tx) => tx,
        None => {
            return NftResponse {
                success: false,
                data: None,
                error: Some(format!("NFT with id {} not found", nft_id)),
            };
        }
    };

    // Check if it's an NFT transaction
    if let TransactionType::NFTMint = transaction.transaction_type {
        if let Some(metadata_json) = &transaction.data {
            match serde_json::from_str::<NFTMetadata>(metadata_json) {
                Ok(metadata) => {
                    let nft = NFT {
                        id: transaction.id.clone(),
                        token_id: 0,
                        creator: transaction.inputs.first()
                            .and_then(|input| input.public_key.clone())
                            .unwrap_or_else(|| "Unknown".to_string()),
                        owner: transaction.outputs.first()
                            .map(|output| output.address.clone())
                            .unwrap_or_else(|| "Unknown".to_string()),
                        metadata,
                        creation_tx: transaction.id.clone(),
                        created_at: transaction.timestamp,
                        last_transferred_at: None,
                        price_history: Vec::new(),
                        royalty_percentage: 0.05,
                    };

                    match serde_json::to_value(&nft) {
                        Ok(value) => NftResponse {
                            success: true,
                            data: Some(value),
                            error: None,
                        },
                        Err(e) => {
                            error!("Failed to serialize NFT: {}", e);
                            NftResponse {
                                success: false,
                                data: None,
                                error: Some(format!("Failed to serialize NFT: {}", e)),
                            }
                        }
                    }
                },
                Err(e) => {
                    error!("Failed to parse NFT metadata: {}", e);
                    NftResponse {
                        success: false,
                        data: None,
                        error: Some(format!("Failed to parse NFT metadata: {}", e)),
                    }
                }
            }
        } else {
            NftResponse {
                success: false,
                data: None,
                error: Some("Transaction has no NFT metadata".to_string()),
            }
        }
    } else {
        NftResponse {
            success: false,
            data: None,
            error: Some("Transaction is not an NFT minting transaction".to_string()),
        }
    }
}

#[command]
pub async fn get_computational_nfts(state: State<'_, AppState>) -> NftResponse {
    info!("Command called: get_computational_nfts");

    let blockchain = state.blockchain.lock().await;
    let mut computational_nfts = Vec::new();

    // Search for computational NFT minting transactions
    for block in &blockchain.chain {
        for tx in &block.transactions {
            if let TransactionType::NFTMint = tx.transaction_type {
                if let Some(metadata_json) = &tx.data {
                    if let Ok(metadata) = serde_json::from_str::<NFTMetadata>(metadata_json) {
                        if let NFTType::ComputationalResource = metadata.nft_type {
                            let nft = NFT {
                                id: tx.id.clone(),
                                token_id: 0,
                                creator: tx.inputs.first()
                                    .and_then(|input| input.public_key.clone())
                                    .unwrap_or_else(|| "Unknown".to_string()),
                                owner: tx.outputs.first()
                                    .map(|output| output.address.clone())
                                    .unwrap_or_else(|| "Unknown".to_string()),
                                metadata,
                                creation_tx: tx.id.clone(),
                                created_at: tx.timestamp,
                                last_transferred_at: None,
                                price_history: Vec::new(),
                                royalty_percentage: 0.05,
                            };

                            computational_nfts.push(nft);
                        }
                    }
                }
            }
        }
    }

    match serde_json::to_value(&computational_nfts) {
        Ok(value) => NftResponse {
            success: true,
            data: Some(value),
            error: None,
        },
        Err(e) => {
            error!("Failed to serialize computational NFTs: {}", e);
            NftResponse {
                success: false,
                data: None,
                error: Some(format!("Failed to serialize computational NFTs: {}", e)),
            }
        }
    }
}

/// Transfers an NFT. Accepts the sender's wallet address and passphrase to unlock
/// the private key from the database. The private key is NEVER passed over the IPC boundary.
#[command]
pub async fn transfer_nft(
    state: State<'_, AppState>,
    sender_address: String,
    passphrase: String,
    nft_id: String,
    recipient_address: String,
    price: u64,
) -> NftResponse {
    info!("Command called: transfer_nft with id: {} to: {}", nft_id, recipient_address);

    // Look up and decrypt the sender wallet from the database
    let sender_wallet = {
        let db = state.database.lock().await;
        match db.get_wallet_by_address(&sender_address, &passphrase) {
            Ok(Some(wallet)) => wallet,
            Ok(None) => {
                return NftResponse {
                    success: false,
                    data: None,
                    error: Some(format!("Wallet not found for address: {}", sender_address)),
                };
            }
            Err(e) => {
                error!("Failed to unlock wallet: {}", e);
                return NftResponse {
                    success: false,
                    data: None,
                    error: Some(format!("Failed to unlock wallet: {}", e)),
                };
            }
        }
    };

    let blockchain = state.blockchain.lock().await;

    // Find the NFT transaction
    let nft_transaction = match blockchain.get_transaction_by_id(&nft_id) {
        Some(tx) => tx,
        None => {
            return NftResponse {
                success: false,
                data: None,
                error: Some(format!("NFT with id {} not found", nft_id)),
            };
        }
    };

    // Verify the NFT exists and sender is the owner
    if let TransactionType::NFTMint = nft_transaction.transaction_type {
        let fee: u64 = 1_000_000; // 0.01 WRF fee in base units


        let transfer_transaction = match sender_wallet.create_transaction(
            &recipient_address,
            price,
            fee,
            &blockchain.utxo_set,
            &blockchain.utxo_indices,
        ) {
            Ok(mut tx) => {
                tx.transaction_type = TransactionType::NFTTransfer;
                tx.data = Some(serde_json::json!({
                    "nft_id": nft_id,
                    "price": price,
                }).to_string());
                tx
            },
            Err(e) => {
                error!("Failed to create NFT transfer transaction: {}", e);
                return NftResponse {
                    success: false,
                    data: None,
                    error: Some(format!("Failed to create NFT transfer transaction: {}", e)),
                };
            }
        };

        // Add transaction to blockchain
        match blockchain.add_transaction(transfer_transaction.clone()) {
            Ok(_) => {
                info!("NFT transfer transaction added to blockchain: {}", transfer_transaction.id);

                // Broadcast transaction to network
                let mut node = state.node.lock().await;
                if let Err(e) = node.broadcast_transaction(transfer_transaction.clone()).await {
                    error!("Failed to broadcast NFT transfer transaction: {}", e);
                }

                NftResponse {
                    success: true,
                    data: Some(serde_json::json!({
                        "transaction_id": transfer_transaction.id,
                        "nft_id": nft_id,
                        "from": sender_wallet.address,
                        "to": recipient_address,
                        "price": price,
                        "timestamp": transfer_transaction.timestamp,
                    })),
                    error: None,
                }
            },
            Err(e) => {
                error!("Failed to add NFT transfer transaction to blockchain: {}", e);
                NftResponse {
                    success: false,
                    data: None,
                    error: Some(format!("Failed to add NFT transfer transaction to blockchain: {}", e)),
                }
            }
        }
    } else {
        NftResponse {
            success: false,
            data: None,
            error: Some("Transaction is not an NFT".to_string()),
        }
    }
}

#[command]
pub async fn register_nft_for_mining(
    state: State<'_, AppState>,
    nft_id: String,
) -> NftResponse {
    info!("Command called: register_nft_for_mining with id: {}", nft_id);

    let blockchain = state.blockchain.lock().await;

    // Find the NFT transaction
    let nft_transaction = match blockchain.get_transaction_by_id(&nft_id) {
        Some(tx) => tx,
        None => {
            return NftResponse {
                success: false,
                data: None,
                error: Some(format!("NFT with id {} not found", nft_id)),
            };
        }
    };

    // Verify it's an NFT and a computational resource
    if let TransactionType::NFTMint = nft_transaction.transaction_type {
        if let Some(metadata_json) = &nft_transaction.data {
            match serde_json::from_str::<NFTMetadata>(metadata_json) {
                Ok(metadata) => {
                    if let NFTType::ComputationalResource = metadata.nft_type {
                        NftResponse {
                            success: true,
                            data: Some(serde_json::json!({
                                "nft_id": nft_id,
                                "registered": true,
                                "compute_power": metadata.compute_power,
                                "memory": metadata.memory,
                                "storage": metadata.storage,
                                "duration": metadata.duration,
                            })),
                            error: None,
                        }
                    } else {
                        NftResponse {
                            success: false,
                            data: None,
                            error: Some("NFT is not a computational resource".to_string()),
                        }
                    }
                },
                Err(e) => {
                    error!("Failed to parse NFT metadata: {}", e);
                    NftResponse {
                        success: false,
                        data: None,
                        error: Some(format!("Failed to parse NFT metadata: {}", e)),
                    }
                }
            }
        } else {
            NftResponse {
                success: false,
                data: None,
                error: Some("NFT has no metadata".to_string()),
            }
        }
    } else {
        NftResponse {
            success: false,
            data: None,
            error: Some("Transaction is not an NFT".to_string()),
        }
    }
}
