//! Hash functionality for cryptographic operations
//!
//! This module provides hash functionality for various cryptographic operations,
//! including SHA-256, SHA-3, and BLAKE3 hashing.
//!
//! The [`Hash`] struct always stores exactly 32 bytes internally, making it
//! interoperable with `common::types::Hash` (which is `[u8; 32]`) via
//! `From`/`Into` conversions.

use anyhow::Result;
use num_traits;
use serde::{Deserialize, Serialize};
use sha2::{Digest as Sha2Digest, Sha256};
use sha3::Sha3_256;
use std::fmt;
use std::ops::Add;
use std::str::FromStr;

/// Trait for objects that can be hashed
pub trait Hashable {
    /// Convert the object to a byte array for hashing
    fn bytes(&self) -> Vec<u8>;

    /// Hash the object with the default algorithm
    fn calculate_hash(&self) -> Hash {
        Hash::hash_bytes(&self.bytes())
    }

    /// Hash method that returns a Hash
    /// This method is used for compatibility with code that expects a hash() method
    fn hash(&self) -> Hash {
        self.calculate_hash()
    }
}

/// Hash type enum for different supported hash algorithms
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Hash,
)]
pub enum HashType {
    /// SHA-256 hash algorithm
    Sha256,

    /// SHA3-256 hash algorithm (default for better security)
    #[default]
    Sha3,

    /// BLAKE3 hash algorithm
    Blake3,
}

/// Hash representation for Wrfcoin.
///
/// Always stores exactly 32 bytes. Interoperable with `[u8; 32]` via
/// `From<[u8; 32]>` and `Into<[u8; 32]>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Hash)]
pub struct Hash {
    /// Type of hash algorithm used
    hash_type: HashType,

    /// Actual hash bytes (always 32 bytes)
    bytes: [u8; 32],
}

impl Default for Hash {
    fn default() -> Self {
        Self {
            hash_type: HashType::Sha3,
            bytes: [0; 32],
        }
    }
}

// Implement Zero trait for Hash
impl num_traits::Zero for Hash {
    fn zero() -> Self {
        Self {
            hash_type: HashType::Sha3,
            bytes: [0; 32],
        }
    }

    fn is_zero(&self) -> bool {
        self.bytes.iter().all(|&b| b == 0)
    }
}

impl Hash {
    /// Create a new hash from bytes and type
    pub fn new(bytes: [u8; 32], hash_type: HashType) -> Self {
        Self { hash_type, bytes }
    }

    /// Get the hash type
    pub fn hash_type(&self) -> HashType {
        self.hash_type
    }

    /// Get a reference to the 32-byte hash
    #[inline]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.bytes
    }

    /// Get the hash as a hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.bytes)
    }

    /// Create a hash from a hex string
    pub fn from_hex(hex_str: &str, hash_type: HashType) -> Result<Self> {
        let decoded = hex::decode(hex_str)?;
        let bytes: [u8; 32] = decoded
            .try_into()
            .map_err(|v: Vec<u8>| anyhow::anyhow!("expected 32 bytes, got {}", v.len()))?;
        Ok(Self { hash_type, bytes })
    }

    /// Create a hash directly from a 32-byte array
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self {
            hash_type: HashType::Sha3,
            bytes,
        }
    }

    /// Hash bytes using SHA3-256 algorithm and return a new Hash
    #[inline]
    pub fn hash_bytes(data: &[u8]) -> Self {
        let mut hasher = Sha3_256::new();
        hasher.update(data);
        let result = hasher.finalize();

        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&result);
        Self::new(bytes, HashType::Sha3)
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl FromStr for Hash {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Default to SHA3 if not specified
        Hash::from_hex(s, HashType::Sha3)
    }
}

// Convert [u8; 32] (common::types::Hash) -> crypto::hash::Hash
impl From<[u8; 32]> for Hash {
    fn from(bytes: [u8; 32]) -> Self {
        Self {
            hash_type: HashType::Sha3,
            bytes,
        }
    }
}

// Convert crypto::hash::Hash -> [u8; 32] (common::types::Hash)
impl From<Hash> for [u8; 32] {
    fn from(hash: Hash) -> Self {
        hash.bytes
    }
}

impl AsRef<[u8; 32]> for Hash {
    fn as_ref(&self) -> &[u8; 32] {
        &self.bytes
    }
}

impl AsRef<[u8]> for Hash {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

impl Add for Hash {
    type Output = Self;

    fn add(self, other: Self) -> Self::Output {
        let mut combined = self.bytes;
        for (i, byte) in other.bytes.iter().enumerate() {
            combined[i] = combined[i].wrapping_add(*byte);
        }

        Hash {
            hash_type: self.hash_type,
            bytes: combined,
        }
    }
}

/// Hash data using SHA-256
#[inline]
pub fn sha256_hash(data: &[u8]) -> Hash {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();

    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&result);
    Hash::new(bytes, HashType::Sha256)
}

/// Hash data using SHA3-256
#[inline]
pub fn sha3_hash(data: &[u8]) -> Hash {
    let mut hasher = Sha3_256::new();
    hasher.update(data);
    let result = hasher.finalize();

    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&result);
    Hash::new(bytes, HashType::Sha3)
}

/// Hash data using BLAKE3
#[inline]
pub fn blake3_hash(data: &[u8]) -> Hash {
    let hash = blake3::hash(data);

    Hash::new(*hash.as_bytes(), HashType::Blake3)
}

/// Hash bytes using the default hash algorithm (SHA3)
#[inline]
pub fn hash_bytes(data: &[u8]) -> Hash {
    sha3_hash(data)
}

/// Hash a serializable object
pub fn hash_object<T: Serialize>(obj: &T) -> Result<Hash> {
    let bytes = serde_json::to_vec(obj)?;
    Ok(hash_bytes(&bytes))
}

/// Hash a string - convenience function for creating test hashes
pub fn hash_string(s: &str) -> Hash {
    hash_bytes(s.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_hash() {
        let data = b"test data";
        let hash = sha256_hash(data);

        assert_eq!(hash.hash_type(), HashType::Sha256);

        // Known SHA-256 hash of "test data"
        let expected = "916f0027a575074ce72a331777c3478d6513f786a591bd892da1a577bf2335f9";
        assert_eq!(hash.to_hex(), expected);
    }

    #[test]
    fn test_sha3_hash() {
        let data = b"test data";
        let hash = sha3_hash(data);

        assert_eq!(hash.hash_type(), HashType::Sha3);

        // Known SHA3-256 hash of "test data"
        let expected = "fc88e0ac33ff105e376f4ece95fb06925d5ab20080dbe3aede7dd47e45dfd931";
        assert_eq!(hash.to_hex(), expected);
    }

    #[test]
    fn test_blake3_hash() {
        let data = b"test data";
        let hash = blake3_hash(data);

        assert_eq!(hash.hash_type(), HashType::Blake3);
    }

    #[test]
    fn test_hash_bytes() {
        let data = b"test data";
        let hash = hash_bytes(data);

        // Should use the default hash algorithm (SHA3)
        assert_eq!(hash.hash_type(), HashType::Sha3);
    }

    #[test]
    fn test_hash_from_hex() {
        let hex_str = "7b5931dc36f078d84d61d912498e5a5e53bb27253d14a871c3ebe9766c7628ec";
        let hash = Hash::from_hex(hex_str, HashType::Sha3).unwrap();

        assert_eq!(hash.hash_type(), HashType::Sha3);
        assert_eq!(hash.to_hex(), hex_str);
    }

    #[test]
    fn test_hash_from_str() {
        let hex_str = "7b5931dc36f078d84d61d912498e5a5e53bb27253d14a871c3ebe9766c7628ec";
        let hash: Hash = hex_str.parse().unwrap();

        assert_eq!(hash.hash_type(), HashType::Sha3);
        assert_eq!(hash.to_hex(), hex_str);
    }

    #[test]
    fn test_hash_object() {
        #[derive(Serialize)]
        struct TestObj {
            field1: String,
            field2: u32,
        }

        let obj = TestObj {
            field1: "test".to_string(),
            field2: 42,
        };

        let hash = hash_object(&obj).unwrap();

        // Should use the default hash algorithm (SHA3)
        assert_eq!(hash.hash_type(), HashType::Sha3);
    }

    #[test]
    fn test_roundtrip_from_into_array() {
        let raw: [u8; 32] = [42; 32];
        let hash = Hash::from(raw);
        let back: [u8; 32] = hash.into();
        assert_eq!(raw, back);
    }

    #[test]
    fn test_as_ref_array() {
        let hash = Hash::from([7; 32]);
        let arr: &[u8; 32] = hash.as_ref();
        assert_eq!(arr, &[7; 32]);
    }

    #[test]
    fn test_from_hex_rejects_wrong_length() {
        let short_hex = "abcdef";
        let result = Hash::from_hex(short_hex, HashType::Sha3);
        assert!(result.is_err());
    }
}
