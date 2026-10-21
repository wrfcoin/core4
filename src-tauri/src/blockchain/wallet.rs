use secp256k1::{Secp256k1, SecretKey, PublicKey};
use serde::{Serialize, Deserialize};
use sha2::{Sha256, Digest};
use bs58;
use rand::Rng;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::blockchain::transaction::{Transaction, TransactionInput, TransactionOutput, TransactionType};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wallet {
    pub private_key: String,
    pub public_key: String,
    pub address: String,
}

/// Safe wallet export format that never includes the private key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletExport {
    pub address: String,
    pub public_key: String,
}

impl Wallet {
    pub fn new() -> Self {
        // Create secp256k1 context
        let secp = Secp256k1::new();
        let mut rng = rand::thread_rng();
        
        // Generate random private key
        let private_key = SecretKey::new(&mut rng);
        let private_key_hex = hex::encode(private_key.secret_bytes());
        
        // Derive public key from private key
        let public_key = PublicKey::from_secret_key(&secp, &private_key);
        let public_key_hex = hex::encode(public_key.serialize());
        
        // Generate address from public key
        let address = Self::generate_address(&public_key_hex);
        
        Wallet {
            private_key: private_key_hex,
            public_key: public_key_hex,
            address,
        }
    }
    
    pub fn from_private_key(private_key_hex: &str) -> Result<Self, &'static str> {
        // Create secp256k1 context
        let secp = Secp256k1::new();
        
        // Decode private key from hex
        let private_key_bytes = match hex::decode(private_key_hex) {
            Ok(bytes) => bytes,
            Err(_) => return Err("Invalid private key format"),
        };
        
        // Create private key object
        let private_key = match SecretKey::from_slice(&private_key_bytes) {
            Ok(key) => key,
            Err(_) => return Err("Invalid private key"),
        };
        
        // Derive public key from private key
        let public_key = PublicKey::from_secret_key(&secp, &private_key);
        let public_key_hex = hex::encode(public_key.serialize());
        
        // Generate address from public key
        let address = Self::generate_address(&public_key_hex);
        
        Ok(Wallet {
            private_key: private_key_hex.to_string(),
            public_key: public_key_hex,
            address,
        })
    }
    
    fn generate_address(public_key_hex: &str) -> String {
        // Hash public key with SHA-256
        let mut hasher = Sha256::new();
        hasher.update(public_key_hex.as_bytes());
        let hash = hasher.finalize();
        
        // Take first 20 bytes (RIPEMD-160 would be ideal but using first 20 bytes of SHA-256 for simplicity)
        let hash_bytes = &hash[0..20];
        
        // Add version byte (0x00 for main address)
        let mut address_bytes = vec![0x00];
        address_bytes.extend_from_slice(hash_bytes);
        
        // Encode with Base58Check
        let mut hasher = Sha256::new();
        hasher.update(&address_bytes);
        let checksum_1 = hasher.finalize();
        
        let mut hasher = Sha256::new();
        hasher.update(&checksum_1);
        let checksum_2 = hasher.finalize();
        
        // Take first 4 bytes of the second checksum as the checksum
        address_bytes.extend_from_slice(&checksum_2[0..4]);
        
        // Base58 encode the result
        let address = bs58::encode(address_bytes).into_string();
        
        // Add "WRF" prefix to distinguish Wrfcoin addresses
        format!("WRF{}", address)
    }
    
    /// Exports only the public wallet info (address + public key) to a file.
    /// Private keys are NEVER written to the file export.
    pub fn save_to_file(&self, file_path: &Path) -> Result<(), &'static str> {
        let export = WalletExport {
            address: self.address.clone(),
            public_key: self.public_key.clone(),
        };

        let wallet_json = match serde_json::to_string(&export) {
            Ok(json) => json,
            Err(_) => return Err("Failed to serialize wallet"),
        };

        if let Err(_) = fs::write(file_path, wallet_json) {
            return Err("Failed to write wallet to file");
        }

        Ok(())
    }

    /// Loads public wallet info from a file. Returns a WalletExport (no private key).
    pub fn load_export_from_file(file_path: &Path) -> Result<WalletExport, &'static str> {
        let wallet_json = match fs::read_to_string(file_path) {
            Ok(json) => json,
            Err(_) => return Err("Failed to read wallet file"),
        };

        match serde_json::from_str(&wallet_json) {
            Ok(export) => Ok(export),
            Err(_) => Err("Failed to deserialize wallet file"),
        }
    }

    /// Legacy: load from file that may contain a full wallet (for backward compatibility during migration).
    pub fn load_from_file(file_path: &Path) -> Result<WalletExport, &'static str> {
        Self::load_export_from_file(file_path)
    }
    
    pub fn create_transaction(
        &self,
        receiver_address: &str,
        amount: u64,
        fee: u64,
        utxo_set: &HashMap<String, Vec<TransactionOutput>>,
        utxo_indices: &HashMap<String, Vec<usize>>,
    ) -> Result<Transaction, &'static str> {
        let required = amount.checked_add(fee).ok_or("Amount + fee overflow")?;

        // Find available UTXOs
        let mut available_utxos = Vec::new();
        let mut available_amount: u64 = 0;

        for (tx_id, indices) in utxo_indices {
            if let Some(outputs) = utxo_set.get(tx_id) {
                for &idx in indices {
                    if idx < outputs.len() && outputs[idx].address == self.address {
                        available_utxos.push((tx_id.clone(), idx, outputs[idx].amount));
                        available_amount = available_amount.saturating_add(outputs[idx].amount);

                        if available_amount >= required {
                            break;
                        }
                    }
                }
            }

            if available_amount >= required {
                break;
            }
        }

        if available_amount < required {
            return Err("Insufficient funds");
        }

        // Create transaction inputs
        let inputs: Vec<TransactionInput> = available_utxos
            .iter()
            .map(|(tx_id, idx, _)| TransactionInput {
                transaction_id: tx_id.clone(),
                output_index: *idx,
                signature: None,
                public_key: None,
            })
            .collect();

        // Create transaction outputs
        let mut outputs = vec![
            TransactionOutput {
                address: receiver_address.to_string(),
                amount,
            }
        ];

        // Add change output if necessary
        let change = available_amount - amount - fee;
        if change > 0 {
            outputs.push(TransactionOutput {
                address: self.address.clone(),
                amount: change,
            });
        }
        
        // Create and sign transaction
        let mut transaction = Transaction::new(
            inputs,
            outputs,
            TransactionType::Regular,
            None,
            fee,
        );
        
        if !transaction.sign(&self.private_key, utxo_set) {
            return Err("Failed to sign transaction");
        }
        
        Ok(transaction)
    }
    
    pub fn create_nft_mint_transaction(
        &self,
        nft_data: &str,
        fee: u64,
        utxo_set: &HashMap<String, Vec<TransactionOutput>>,
        utxo_indices: &HashMap<String, Vec<usize>>,
    ) -> Result<Transaction, &'static str> {
        // NFT minting has a fixed cost of 10 WRF (10 * 10^8 base units)
        let nft_cost: u64 = 10 * crate::blockchain::transaction::WRF_UNITS_PER_COIN;
        
        // Create a regular transaction to self with NFT data
        let mut transaction = self.create_transaction(
            &self.address,
            nft_cost,
            fee,
            utxo_set,
            utxo_indices,
        )?;
        
        // Update transaction type and data
        transaction.transaction_type = TransactionType::NFTMint;
        transaction.data = Some(nft_data.to_string());
        
        // Update transaction ID
        transaction.id = transaction.calculate_hash();
        
        // Re-sign the transaction
        if !transaction.sign(&self.private_key, utxo_set) {
            return Err("Failed to sign NFT transaction");
        }
        
        Ok(transaction)
    }
}
