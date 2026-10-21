use hex::{FromHex, ToHex};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use std::fmt;
use std::hash::Hash as StdHash;
use std::hash::Hasher;
use std::str::FromStr;
use thiserror::Error;
pub use wrf_primitives::{
    Balance, BlockId, BlockNumber, ChainId, Gas, Hash, Nonce, ShardId, Timestamp, TransactionId,
    VerificationResult,
};

/// Error type for address operations
#[derive(Debug, Error)]
pub enum AddressError {
    #[error("Invalid address format: {0}")]
    InvalidFormat(String),

    #[error("Invalid address length: expected 40 hex chars, got {0}")]
    InvalidLength(usize),

    #[error("Invalid hex character in address")]
    InvalidHexCharacter,

    #[error("Address checksum mismatch")]
    ChecksumMismatch,
}

/// Blockchain address (20 bytes, similar to Ethereum)
///
/// `PartialOrd`/`Ord` (#2172) are a byte-lexicographic order over the raw
/// address bytes — added so `Address` can key a `BTreeMap` directly (e.g.
/// `State::account_recovery`, `State::session_keys`, and `State::paymasters`)
/// with the same canonical, cross-node-agreed
/// iteration order every other consensus-hashed `BTreeMap` in this codebase
/// relies on. Purely additive: no existing code matched on ordering
/// unavailability, and the derived order is deterministic and total.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Address(pub [u8; 20]);

impl Address {
    /// Create a new address from raw bytes
    pub fn new(bytes: [u8; 20]) -> Self {
        Self(bytes)
    }

    /// Create a new address from a hex string
    pub fn from_hex(hex_str: &str) -> Result<Self, AddressError> {
        // Remove 0x prefix if present
        let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);

        // Check length
        if hex_str.len() != 40 {
            return Err(AddressError::InvalidLength(hex_str.len()));
        }

        // Parse hex
        let bytes = <[u8; 20]>::from_hex(hex_str).map_err(|_| AddressError::InvalidHexCharacter)?;

        Ok(Self(bytes))
    }

    /// Convert address to hex string
    pub fn to_hex(&self) -> String {
        format!("0x{}", self.0.encode_hex::<String>())
    }

    /// Get raw bytes
    pub fn as_bytes(&self) -> &[u8; 20] {
        &self.0
    }

    /// Derive an address from an ed25519 public key by hashing and taking
    /// the first 20 bytes.
    pub fn from_public_key(pk: &crate::crypto::signature::PublicKey) -> Self {
        let hash = crate::crypto::hash::hash_bytes(pk.as_bytes());
        let mut bytes = [0u8; 20];
        bytes.copy_from_slice(&hash.as_bytes()[..20]);
        Self(bytes)
    }

    /// Create a checksum address (EIP-55 mixed-case encoding).
    ///
    /// Keccak-256 hashes the lowercase hex address, then capitalizes each
    /// hex digit whose corresponding hash nibble is >= 8.
    pub fn to_checksum(&self) -> String {
        let hex_lower = self.0.encode_hex::<String>(); // 40 lowercase hex chars

        let hash = Keccak256::digest(hex_lower.as_bytes());

        let mut checksummed = String::with_capacity(42);
        checksummed.push_str("0x");

        for (i, c) in hex_lower.chars().enumerate() {
            if c.is_ascii_alphabetic() {
                // Each byte of the hash covers two hex characters (two nibbles).
                let nibble = if i % 2 == 0 {
                    hash[i / 2] >> 4
                } else {
                    hash[i / 2] & 0x0F
                };
                if nibble >= 8 {
                    checksummed.push(c.to_ascii_uppercase());
                } else {
                    checksummed.push(c);
                }
            } else {
                checksummed.push(c);
            }
        }

        checksummed
    }
}

impl fmt::Debug for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl FromStr for Address {
    type Err = AddressError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_hex(s)
    }
}

impl StdHash for Address {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

/// NFT ID type
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NFTId(pub String);

impl NFTId {
    pub fn new(id: String) -> Self {
        Self(id)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.0.as_bytes().to_vec()
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, std::str::Utf8Error> {
        let id = std::str::from_utf8(bytes)?;
        Ok(Self(id.to_string()))
    }
}

impl fmt::Display for NFTId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// NFT metadata type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NFTMetadata {
    pub name: String,
    pub description: Option<String>,
    pub data_type: String,
    pub region: Option<String>,
    pub accuracy: Option<f64>,
    pub creation_time: Timestamp,
    pub creator: Address,
    pub metadata_uri: Option<String>,
    pub properties: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// EIP-55 test vectors from the specification.
    #[test]
    fn eip55_test_vectors() {
        // All-caps and all-lower addresses from the EIP-55 spec
        let vectors: &[(&str, &str)] = &[
            (
                "5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed",
                "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed",
            ),
            (
                "fB6916095ca1df60bB79Ce92cE3Ea74c37c5d359",
                "0xfB6916095ca1df60bB79Ce92cE3Ea74c37c5d359",
            ),
            (
                "dbF03B407c01E7cD3CBea99509d93f8DDDC8C6FB",
                "0xdbF03B407c01E7cD3CBea99509d93f8DDDC8C6FB",
            ),
            (
                "D1220A0cf47c7B9Be7A2E6BA89F429762e7b9aDb",
                "0xD1220A0cf47c7B9Be7A2E6BA89F429762e7b9aDb",
            ),
        ];

        for (hex_input, expected) in vectors {
            let addr = Address::from_hex(hex_input).unwrap();
            let checksummed = addr.to_checksum();
            assert_eq!(
                checksummed, *expected,
                "EIP-55 mismatch for input {hex_input}"
            );
        }
    }

    #[test]
    fn checksum_roundtrip() {
        let addr = Address::from_hex("5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed").unwrap();
        let checksummed = addr.to_checksum();
        let recovered = Address::from_hex(&checksummed).unwrap();
        assert_eq!(addr, recovered);
    }

    #[test]
    fn all_zero_address_checksum() {
        let addr = Address::new([0; 20]);
        let checksummed = addr.to_checksum();
        // All zeros — only digits, no case changes
        assert_eq!(checksummed, "0x0000000000000000000000000000000000000000");
    }
}
