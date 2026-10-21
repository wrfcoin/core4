use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use sha2::{Sha256, Digest};
use uuid::Uuid;
use secp256k1::{Secp256k1, Message};
use bitcoin_hashes::sha256;
use std::collections::HashMap;

/// 1 WRF = 10^8 base units (similar to Bitcoin satoshis)
pub const WRF_UNITS_PER_COIN: u64 = 100_000_000;

/// Convert base units to a human-readable display string.
/// For example, `150_000_000` becomes `"1.50000000"`.
pub fn units_to_display(units: u64) -> String {
    let whole = units / WRF_UNITS_PER_COIN;
    let frac = units % WRF_UNITS_PER_COIN;
    format!("{}.{:08}", whole, frac)
}

/// Parse a decimal display string into base units.
/// Accepts inputs like `"1.5"`, `"0.01"`, or `"100"`.
/// Returns an error if the string is not a valid non-negative decimal
/// or has more than 8 fractional digits.
pub fn display_to_units(display: &str) -> Result<u64, String> {
    let trimmed = display.trim();
    if trimmed.is_empty() {
        return Err("Empty input".to_string());
    }

    let parts: Vec<&str> = trimmed.split('.').collect();
    if parts.len() > 2 {
        return Err("Invalid decimal format: multiple decimal points".to_string());
    }

    let whole: u64 = parts[0]
        .parse()
        .map_err(|_| format!("Invalid whole part: '{}'", parts[0]))?;

    let frac_units: u64 = if parts.len() == 2 {
        let frac_str = parts[1];
        if frac_str.len() > 8 {
            return Err(format!(
                "Too many fractional digits (max 8): '{}'",
                frac_str
            ));
        }
        // Pad or truncate to 8 digits
        let padded = format!("{:0<8}", frac_str);
        padded
            .parse()
            .map_err(|_| format!("Invalid fractional part: '{}'", frac_str))?
    } else {
        0
    };

    whole
        .checked_mul(WRF_UNITS_PER_COIN)
        .and_then(|w| w.checked_add(frac_units))
        .ok_or_else(|| "Overflow: amount too large".to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionInput {
    pub transaction_id: String,
    pub output_index: usize,
    pub signature: Option<String>,
    pub public_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionOutput {
    pub address: String,
    pub amount: u64, // in base units (1 WRF = 10^8 units)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransactionType {
    Regular,
    Coinbase,
    NFTMint,
    NFTTransfer,
    SmartContract,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub inputs: Vec<TransactionInput>,
    pub outputs: Vec<TransactionOutput>,
    pub transaction_type: TransactionType,
    pub data: Option<String>, // Used for additional data like NFT metadata
    pub fee: u64, // in base units (1 WRF = 10^8 units)
}

impl Transaction {
    pub fn new(
        inputs: Vec<TransactionInput>,
        outputs: Vec<TransactionOutput>,
        transaction_type: TransactionType,
        data: Option<String>,
        fee: u64,
    ) -> Self {
        let mut transaction = Transaction {
            id: String::new(),
            timestamp: Utc::now(),
            inputs,
            outputs,
            transaction_type,
            data,
            fee,
        };
        
        transaction.id = transaction.calculate_hash();
        transaction
    }
    
    pub fn coinbase(coinbase_data: String, miner_address: String, reward: u64) -> Self {
        let output = TransactionOutput {
            address: miner_address,
            amount: reward,
        };
        
        let mut transaction = Transaction {
            id: String::new(),
            timestamp: Utc::now(),
            inputs: Vec::new(),
            outputs: vec![output],
            transaction_type: TransactionType::Coinbase,
            data: Some(coinbase_data),
            fee: 0,
        };
        
        transaction.id = transaction.calculate_hash();
        transaction
    }
    
    pub fn calculate_hash(&self) -> String {
        let mut hasher = Sha256::new();
        
        // Serialize inputs
        let inputs_str: String = self.inputs.iter()
            .map(|input| format!("{}:{}", input.transaction_id, input.output_index))
            .collect::<Vec<String>>()
            .join("|");
        
        // Serialize outputs
        let outputs_str: String = self.outputs.iter()
            .map(|output| format!("{}:{}", output.address, output.amount))
            .collect::<Vec<String>>()
            .join("|");
        
        // Combine all transaction data
        let data = format!(
            "{}|{}|{}|{:?}|{}|{}",
            self.timestamp.timestamp(),
            inputs_str,
            outputs_str,
            self.transaction_type,
            self.data.clone().unwrap_or_default(),
            self.fee
        );
        
        hasher.update(data.as_bytes());
        hex::encode(hasher.finalize())
    }
    
    pub fn sign(&mut self, private_key: &str, utxo_set: &HashMap<String, Vec<TransactionOutput>>) -> bool {
        // Skip signing for coinbase transactions
        if matches!(self.transaction_type, TransactionType::Coinbase) {
            return true;
        }
        
        // Calculate the transaction hash to sign
        let tx_hash = self.calculate_hash();
        
        // Create secp256k1 context
        let secp = Secp256k1::new();
        
        // Convert private key from hex to bytes
        let private_key_bytes = match hex::decode(private_key) {
            Ok(bytes) => bytes,
            Err(_) => return false,
        };
        
        // Create private key from bytes
        let private_key = match secp256k1::SecretKey::from_slice(&private_key_bytes) {
            Ok(key) => key,
            Err(_) => return false,
        };
        
        // Generate public key from private key
        let public_key = secp256k1::PublicKey::from_secret_key(&secp, &private_key);
        let public_key_hex = hex::encode(public_key.serialize());
        
        // Sign each input
        for input in &mut self.inputs {
            // Verify input refers to an existing output
            if let Some(outputs) = utxo_set.get(&input.transaction_id) {
                if input.output_index >= outputs.len() {
                    return false;
                }
                
                // Create message to sign
                let message = Message::from_hashed_data::<sha256::Hash>(tx_hash.as_bytes());
                
                // Sign the message
                let signature = secp.sign_ecdsa(&message, &private_key);
                let signature_hex = hex::encode(signature.serialize_compact());
                
                // Set signature and public key
                input.signature = Some(signature_hex);
                input.public_key = Some(public_key_hex.clone());
            } else {
                return false;
            }
        }
        
        true
    }
    
    pub fn is_valid(&self) -> bool {
        // Coinbase transactions are always valid
        if matches!(self.transaction_type, TransactionType::Coinbase) {
            return true;
        }
        
        // Check if transaction ID is correct
        if self.id != self.calculate_hash() {
            return false;
        }
        
        // Check that inputs have signatures and public keys
        for input in &self.inputs {
            if input.signature.is_none() || input.public_key.is_none() {
                return false;
            }
        }
        
        // Verify each input's signature
        let secp = Secp256k1::new();
        let tx_hash = self.calculate_hash();
        
        for input in &self.inputs {
            let signature_hex = input.signature.as_ref().unwrap();
            let public_key_hex = input.public_key.as_ref().unwrap();
            
            // Decode signature and public key
            let signature_bytes = match hex::decode(signature_hex) {
                Ok(bytes) => bytes,
                Err(_) => return false,
            };
            
            let public_key_bytes = match hex::decode(public_key_hex) {
                Ok(bytes) => bytes,
                Err(_) => return false,
            };
            
            // Create secp256k1 signature and public key
            let signature = match secp256k1::ecdsa::Signature::from_compact(&signature_bytes) {
                Ok(sig) => sig,
                Err(_) => return false,
            };
            
            let public_key = match secp256k1::PublicKey::from_slice(&public_key_bytes) {
                Ok(key) => key,
                Err(_) => return false,
            };
            
            // Create message from transaction hash
            let message = Message::from_hashed_data::<sha256::Hash>(tx_hash.as_bytes());
            
            // Verify signature
            if secp.verify_ecdsa(&message, &signature, &public_key).is_err() {
                return false;
            }
        }
        
        true
    }
    
    pub fn get_input_amount(&self, utxo_set: &HashMap<String, Vec<TransactionOutput>>) -> u64 {
        let mut total: u64 = 0;

        for input in &self.inputs {
            if let Some(outputs) = utxo_set.get(&input.transaction_id) {
                if input.output_index < outputs.len() {
                    total = total.saturating_add(outputs[input.output_index].amount);
                }
            }
        }

        total
    }

    pub fn get_output_amount(&self) -> u64 {
        self.outputs.iter().map(|output| output.amount).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── units_to_display ────────────────────────────────────────────

    #[test]
    fn units_to_display_zero() {
        assert_eq!(units_to_display(0), "0.00000000");
    }

    #[test]
    fn units_to_display_one_coin() {
        assert_eq!(units_to_display(WRF_UNITS_PER_COIN), "1.00000000");
    }

    #[test]
    fn units_to_display_fractional() {
        // 1.5 WRF
        assert_eq!(units_to_display(150_000_000), "1.50000000");
    }

    #[test]
    fn units_to_display_small_fraction() {
        // 0.01 WRF
        assert_eq!(units_to_display(1_000_000), "0.01000000");
    }

    #[test]
    fn units_to_display_one_unit() {
        assert_eq!(units_to_display(1), "0.00000001");
    }

    #[test]
    fn units_to_display_large() {
        // 50 WRF (mining reward)
        assert_eq!(units_to_display(5_000_000_000), "50.00000000");
    }

    // ── display_to_units ────────────────────────────────────────────

    #[test]
    fn display_to_units_whole_number() {
        assert_eq!(display_to_units("100").unwrap(), 100 * WRF_UNITS_PER_COIN);
    }

    #[test]
    fn display_to_units_with_fraction() {
        assert_eq!(display_to_units("1.5").unwrap(), 150_000_000);
    }

    #[test]
    fn display_to_units_small_fraction() {
        assert_eq!(display_to_units("0.01").unwrap(), 1_000_000);
    }

    #[test]
    fn display_to_units_full_precision() {
        assert_eq!(display_to_units("1.23456789").unwrap(), 123_456_789);
    }

    #[test]
    fn display_to_units_zero() {
        assert_eq!(display_to_units("0").unwrap(), 0);
        assert_eq!(display_to_units("0.0").unwrap(), 0);
    }

    #[test]
    fn display_to_units_rejects_too_many_decimals() {
        assert!(display_to_units("1.123456789").is_err());
    }

    #[test]
    fn display_to_units_rejects_empty() {
        assert!(display_to_units("").is_err());
    }

    #[test]
    fn display_to_units_rejects_multiple_dots() {
        assert!(display_to_units("1.2.3").is_err());
    }

    #[test]
    fn display_to_units_rejects_negative() {
        assert!(display_to_units("-1").is_err());
    }

    // ── roundtrip ───────────────────────────────────────────────────

    #[test]
    fn roundtrip_conversion() {
        let amounts = [0u64, 1, 1_000_000, 150_000_000, 5_000_000_000, WRF_UNITS_PER_COIN];
        for &amount in &amounts {
            let display = units_to_display(amount);
            let back = display_to_units(&display).unwrap();
            assert_eq!(back, amount, "roundtrip failed for {}", amount);
        }
    }

    // ── Transaction creation with u64 ───────────────────────────────

    #[test]
    fn coinbase_transaction_u64() {
        let reward: u64 = 50 * WRF_UNITS_PER_COIN; // 50 WRF
        let tx = Transaction::coinbase("block 1".into(), "WRFaddr".into(), reward);

        assert_eq!(tx.fee, 0);
        assert_eq!(tx.outputs.len(), 1);
        assert_eq!(tx.outputs[0].amount, reward);
        assert!(tx.is_valid());
    }

    #[test]
    fn transaction_output_amount_exact() {
        let tx = Transaction::new(
            vec![],
            vec![
                TransactionOutput { address: "a".into(), amount: 100_000_001 },
                TransactionOutput { address: "b".into(), amount: 99_999_999 },
            ],
            TransactionType::Regular,
            None,
            1_000_000,
        );
        // Exact addition: no floating-point rounding
        assert_eq!(tx.get_output_amount(), 200_000_000);
    }

    // ── Balance calculation (no rounding) ───────────────────────────

    #[test]
    fn balance_accumulation_no_rounding() {
        // Simulate accumulating many small amounts that would cause
        // floating-point drift with f64 (e.g., 0.1 repeated).
        // 0.1 WRF = 10_000_000 base units
        let increment: u64 = 10_000_000;
        let count: u64 = 1_000;
        let mut balance: u64 = 0;
        for _ in 0..count {
            balance += increment;
        }
        // With u64, result is exact
        assert_eq!(balance, increment * count);
        assert_eq!(balance, 10_000_000_000); // 100 WRF exactly
    }

    // ── Edge cases ──────────────────────────────────────────────────

    #[test]
    fn max_u64_display() {
        // u64::MAX should display without panic
        let display = units_to_display(u64::MAX);
        assert!(!display.is_empty());
    }

    #[test]
    fn display_to_units_overflow() {
        // A value too large for u64
        assert!(display_to_units("184467440737.09551616").is_err());
    }

    #[test]
    fn zero_amount_transaction() {
        let tx = Transaction::new(
            vec![],
            vec![TransactionOutput { address: "a".into(), amount: 0 }],
            TransactionType::Regular,
            None,
            0,
        );
        assert_eq!(tx.get_output_amount(), 0);
        assert_eq!(tx.fee, 0);
    }

    // ── Malformed signature / public-key edge cases (#1204) ────────────

    /// Helper: build a regular transaction with one input that has the
    /// given signature and public key strings.
    fn tx_with_sig(sig: Option<String>, pubkey: Option<String>) -> Transaction {
        let mut tx = Transaction::new(
            vec![TransactionInput {
                transaction_id: "prev-tx-abc".into(),
                output_index: 0,
                signature: sig,
                public_key: pubkey,
            }],
            vec![TransactionOutput {
                address: "wrf-recipient".into(),
                amount: 1_000,
            }],
            TransactionType::Regular,
            None,
            100,
        );
        // Recalculate id so the hash check passes — we're testing
        // the signature/pubkey validation, not the id check.
        tx.id = tx.calculate_hash();
        tx
    }

    #[test]
    fn is_valid_rejects_missing_signature() {
        let tx = tx_with_sig(None, Some("02deadbeef".into()));
        assert!(!tx.is_valid(), "missing signature must fail");
    }

    #[test]
    fn is_valid_rejects_missing_public_key() {
        let tx = tx_with_sig(Some("aa".repeat(32)), None);
        assert!(!tx.is_valid(), "missing public key must fail");
    }

    #[test]
    fn is_valid_rejects_both_missing() {
        let tx = tx_with_sig(None, None);
        assert!(!tx.is_valid(), "both missing must fail");
    }

    #[test]
    fn is_valid_rejects_non_hex_signature() {
        let tx = tx_with_sig(
            Some("not-valid-hex!!".into()),
            Some("02".to_string() + &"ab".repeat(32)),
        );
        assert!(!tx.is_valid(), "non-hex signature must fail");
    }

    #[test]
    fn is_valid_rejects_non_hex_public_key() {
        let tx = tx_with_sig(
            Some("aa".repeat(32)),
            Some("ZZZZ-not-hex".into()),
        );
        assert!(!tx.is_valid(), "non-hex public key must fail");
    }

    #[test]
    fn is_valid_rejects_short_signature_bytes() {
        // Compact ECDSA signatures must be exactly 64 bytes.
        let tx = tx_with_sig(
            Some("aa".repeat(16)), // 16 bytes, too short
            Some("02".to_string() + &"ab".repeat(32)),
        );
        assert!(!tx.is_valid(), "short signature must fail");
    }

    #[test]
    fn is_valid_rejects_long_signature_bytes() {
        let tx = tx_with_sig(
            Some("aa".repeat(65)), // 65 bytes, too long
            Some("02".to_string() + &"ab".repeat(32)),
        );
        assert!(!tx.is_valid(), "oversized signature must fail");
    }

    #[test]
    fn is_valid_rejects_short_public_key_bytes() {
        // Compressed pubkey = 33 bytes. 16 bytes is wrong.
        let tx = tx_with_sig(
            Some("aa".repeat(32)),
            Some("aa".repeat(16)),
        );
        assert!(!tx.is_valid(), "short public key must fail");
    }

    #[test]
    fn is_valid_rejects_empty_hex_strings() {
        let tx = tx_with_sig(Some(String::new()), Some(String::new()));
        assert!(!tx.is_valid(), "empty hex strings must fail");
    }

    #[test]
    fn is_valid_rejects_valid_looking_but_wrong_signature() {
        // 64 bytes of zeros — valid length but not a valid ECDSA signature
        // for any message/key pair.
        let tx = tx_with_sig(
            Some("00".repeat(64)),
            Some("02".to_string() + &"ab".repeat(32)),
        );
        assert!(!tx.is_valid(), "zeroed-out signature must fail verification");
    }

    #[test]
    fn is_valid_rejects_all_zero_public_key() {
        // 33 bytes of zeros is not a valid secp256k1 point.
        let tx = tx_with_sig(
            Some("aa".repeat(32)),
            Some("00".repeat(33)),
        );
        assert!(!tx.is_valid(), "all-zero public key must fail");
    }

    #[test]
    fn is_valid_rejects_duplicate_inputs_with_bad_sigs() {
        // Two inputs spending the same output, both with garbage sigs.
        let bad_input = TransactionInput {
            transaction_id: "same-tx-id".into(),
            output_index: 0,
            signature: Some("ff".repeat(32)),
            public_key: Some("ff".repeat(33)),
        };
        let mut tx = Transaction::new(
            vec![bad_input.clone(), bad_input],
            vec![TransactionOutput {
                address: "wrf-dup".into(),
                amount: 500,
            }],
            TransactionType::Regular,
            None,
            50,
        );
        tx.id = tx.calculate_hash();
        assert!(!tx.is_valid(), "duplicate inputs with bad sigs must fail");
    }

    #[test]
    fn is_valid_rejects_tampered_id() {
        // Build a valid-looking tx, then corrupt the id.
        let mut tx = tx_with_sig(
            Some("aa".repeat(32)),
            Some("02".to_string() + &"ab".repeat(32)),
        );
        tx.id = "tampered-hash-value".into();
        assert!(!tx.is_valid(), "tampered transaction id must fail");
    }

    #[test]
    fn is_valid_passes_coinbase_regardless_of_inputs() {
        // Coinbase transactions skip all validation.
        let tx = Transaction {
            id: String::new(),
            timestamp: Utc::now(),
            inputs: vec![TransactionInput {
                transaction_id: "doesnt-matter".into(),
                output_index: 999,
                signature: None,
                public_key: None,
            }],
            outputs: vec![],
            transaction_type: TransactionType::Coinbase,
            data: None,
            fee: 0,
        };
        assert!(tx.is_valid(), "coinbase must always pass is_valid");
    }

    #[test]
    fn is_valid_empty_inputs_with_valid_hash() {
        // A regular tx with no inputs — hash will match but there's
        // nothing to verify. Current code returns true (no loop iterations).
        let tx = Transaction::new(
            vec![],
            vec![TransactionOutput {
                address: "wrf-empty".into(),
                amount: 1,
            }],
            TransactionType::Regular,
            None,
            0,
        );
        // Document current behavior: empty inputs = valid (no sig to check).
        // This is a known gap — the chain.rs caller should reject zero-input
        // regular transactions at a higher layer.
        assert!(tx.is_valid());
    }
}
