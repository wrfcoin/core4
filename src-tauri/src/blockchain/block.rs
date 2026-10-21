use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use sha2::{Sha256, Digest};
use crate::blockchain::transaction::Transaction;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub index: u64,
    pub timestamp: DateTime<Utc>,
    pub transactions: Vec<Transaction>,
    pub previous_hash: String,
    pub hash: String,
    pub nonce: u64,
    pub difficulty: u32,
    pub merkle_root: String,
}

impl Block {
    pub fn new(
        index: u64,
        timestamp: DateTime<Utc>,
        transactions: Vec<Transaction>,
        previous_hash: String,
        difficulty: u32,
    ) -> Self {
        let mut block = Block {
            index,
            timestamp,
            transactions,
            previous_hash,
            hash: String::new(),
            nonce: 0,
            difficulty,
            merkle_root: String::new(),
        };
        
        block.merkle_root = block.calculate_merkle_root();
        block
    }
    
    pub fn genesis() -> Self {
        let timestamp = Utc::now();
        let mut block = Block {
            index: 0,
            timestamp,
            transactions: vec![Transaction::coinbase(
                "Genesis Block".to_string(),
                "Wrfcoin Foundation".to_string(),
                100.0,
            )],
            previous_hash: "0".repeat(64),
            hash: String::new(),
            nonce: 0,
            difficulty: 4, // Starting difficulty
            merkle_root: String::new(),
        };
        
        block.merkle_root = block.calculate_merkle_root();
        block.hash = block.calculate_hash();
        block
    }
    
    pub fn calculate_hash(&self) -> String {
        let mut hasher = Sha256::new();
        let data = format!(
            "{}{}{}{}{}{}",
            self.index,
            self.timestamp.timestamp(),
            self.merkle_root,
            self.previous_hash,
            self.nonce,
            self.difficulty
        );
        
        hasher.update(data.as_bytes());
        hex::encode(hasher.finalize())
    }
    
    pub fn mine(&mut self) {
        loop {
            self.hash = self.calculate_hash();
            if hash_meets_difficulty(&self.hash, self.difficulty) {
                break;
            }
            self.nonce += 1;
        }
    }
    
    pub fn calculate_merkle_root(&self) -> String {
        if self.transactions.is_empty() {
            return "0".repeat(64);
        }
        
        let mut hashes: Vec<String> = self.transactions
            .iter()
            .map(|tx| tx.id.clone())
            .collect();
        
        while hashes.len() > 1 {
            let mut new_hashes = Vec::new();
            
            for chunk in hashes.chunks(2) {
                let left = &chunk[0];
                let right = if chunk.len() > 1 { &chunk[1] } else { left };
                
                let mut hasher = Sha256::new();
                hasher.update(format!("{}{}", left, right).as_bytes());
                let combined_hash = hex::encode(hasher.finalize());
                
                new_hashes.push(combined_hash);
            }
            
            hashes = new_hashes;
        }
        
        hashes[0].clone()
    }
    
    pub fn is_valid(&self) -> bool {
        // Check if hash is correctly calculated
        let calculated_hash = self.calculate_hash();
        if calculated_hash != self.hash {
            return false;
        }
        
        // Check if merkle root is correct
        let calculated_merkle_root = self.calculate_merkle_root();
        if calculated_merkle_root != self.merkle_root {
            return false;
        }
        
        // Check the hash meets difficulty requirement (full 256-bit comparison).
        if !hash_meets_difficulty(&self.hash, self.difficulty) {
            return false;
        }
        
        // Validate all transactions
        for transaction in &self.transactions {
            if !transaction.is_valid() {
                return false;
            }
        }
        
        true
    }
    
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

/// Check whether a hex-encoded hash (64 chars / 256 bits) has at least
/// `difficulty` leading zero bits.
///
/// Each hex digit encodes 4 bits.  We count full zero digits first, then
/// check the partial digit boundary when `difficulty` is not a multiple of 4.
fn hash_meets_difficulty(hash: &str, difficulty: u32) -> bool {
    let full_zero_chars = (difficulty / 4) as usize;
    let remaining_bits = difficulty % 4;

    // Require `full_zero_chars` leading '0' hex digits.
    if hash.len() < full_zero_chars {
        return false;
    }
    for ch in hash[..full_zero_chars].chars() {
        if ch != '0' {
            return false;
        }
    }

    // If difficulty is not a multiple of 4, the next hex digit must be
    // small enough that its top `remaining_bits` are zero.
    if remaining_bits > 0 {
        if hash.len() <= full_zero_chars {
            return false;
        }
        let next_char = match u8::from_str_radix(&hash[full_zero_chars..full_zero_chars + 1], 16) {
            Ok(v) => v,
            Err(_) => return false,
        };
        // For remaining_bits=1 → must be < 8  (0b1000)
        // For remaining_bits=2 → must be < 4  (0b0100)
        // For remaining_bits=3 → must be < 2  (0b0010)
        let max_allowed = 1u8 << (4 - remaining_bits);
        if next_char >= max_allowed {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── hash_meets_difficulty (#1205) ──────────────────────────────────

    #[test]
    fn difficulty_0_accepts_anything() {
        assert!(hash_meets_difficulty("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff", 0));
    }

    #[test]
    fn difficulty_4_requires_one_leading_zero_char() {
        assert!(hash_meets_difficulty("0abcdef0000000000000000000000000000000000000000000000000000000ff", 4));
        assert!(!hash_meets_difficulty("1abcdef0000000000000000000000000000000000000000000000000000000ff", 4));
    }

    #[test]
    fn difficulty_8_requires_two_leading_zero_chars() {
        assert!(hash_meets_difficulty("00abcdef000000000000000000000000000000000000000000000000000000ff", 8));
        assert!(!hash_meets_difficulty("010bcdef000000000000000000000000000000000000000000000000000000ff", 8));
    }

    #[test]
    fn difficulty_5_partial_boundary() {
        // difficulty=5: first char must be '0', second char must be < 8 (0b1000)
        assert!(hash_meets_difficulty("07ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff", 5));
        assert!(!hash_meets_difficulty("08ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff", 5));
        assert!(!hash_meets_difficulty("0fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff", 5));
    }

    #[test]
    fn difficulty_6_partial_boundary() {
        // difficulty=6: first char '0', second char < 4 (0b0100)
        assert!(hash_meets_difficulty("03ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff", 6));
        assert!(!hash_meets_difficulty("04ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff", 6));
    }

    #[test]
    fn difficulty_7_partial_boundary() {
        // difficulty=7: first char '0', second char < 2 (0b0010)
        assert!(hash_meets_difficulty("01ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff", 7));
        assert!(!hash_meets_difficulty("02ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff", 7));
    }

    #[test]
    fn full_hash_is_considered_not_just_first_128_bits() {
        // This is the regression test for the truncation bug.
        // A hash with 128 zero bits followed by non-zero high bits in the
        // second half.  The old code (u128 from first 32 hex chars) would
        // accept difficulty=128 because it only looked at the first half.
        let hash_front_zero_back_nonzero =
            "00000000000000000000000000000000ffffffffffffffffffffffffffffffff";
        // difficulty=128 should pass: first 128 bits (32 hex chars) are all zero
        assert!(hash_meets_difficulty(hash_front_zero_back_nonzero, 128));
        // difficulty=129 should fail: bit 129 (first bit of the second half) is 1
        assert!(!hash_meets_difficulty(hash_front_zero_back_nonzero, 129));
    }

    #[test]
    fn difficulty_beyond_128_bits_checks_second_half() {
        // 33 leading zero hex chars (132 zero bits), then 'f'
        let hash = "000000000000000000000000000000000f0000000000000000000000000000ff";
        assert!(hash_meets_difficulty(hash, 132));
        assert!(!hash_meets_difficulty(hash, 133)); // next char 'f' >= 8
    }

    #[test]
    fn max_difficulty_256_requires_all_zeros() {
        let all_zero = "0000000000000000000000000000000000000000000000000000000000000000";
        assert!(hash_meets_difficulty(all_zero, 256));

        let one_bit_set = "0000000000000000000000000000000000000000000000000000000000000001";
        assert!(!hash_meets_difficulty(one_bit_set, 256));
    }
}
