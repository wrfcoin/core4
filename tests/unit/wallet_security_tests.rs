//! Security tests for wallet encryption functionality
//!
//! These tests validate that Task 1.1 security fixes work correctly:
//! - AES-256-GCM encryption replaces vulnerable XOR
//! - PBKDF2 key derivation with 100,000 iterations
//! - Proper salt and nonce generation
//! - Encryption/decryption round-trip validation

use crate::wallet::{Wallet, WalletError};
use std::collections::HashMap;

/// Test basic encryption/decryption round-trip
#[test]
fn test_encryption_decryption_roundtrip() {
    let mut wallet = Wallet {
        master_password: "test_password_123".to_string(),
        keys: HashMap::new(),
        balances: HashMap::new(),
    };

    let test_private_key = "a1b2c3d4e5f67890abcdef1234567890abcdef1234567890abcdef1234567890";
    
    // Encrypt the private key
    let encrypted = wallet.encrypt_private_key(test_private_key).unwrap();
    
    // Verify encrypted data is different from original
    assert_ne!(encrypted, test_private_key);
    assert!(encrypted.len() > test_private_key.len());
    
    // Decrypt and verify it matches original
    let decrypted = wallet.decrypt_private_key(&encrypted).unwrap();
    assert_eq!(decrypted, test_private_key);
}

/// Test that different passwords produce different encrypted outputs
#[test]
fn test_different_passwords_different_encryption() {
    let test_private_key = "a1b2c3d4e5f67890abcdef1234567890abcdef1234567890abcdef1234567890";
    
    let wallet1 = Wallet {
        master_password: "password1".to_string(),
        keys: HashMap::new(),
        balances: HashMap::new(),
    };
    
    let wallet2 = Wallet {
        master_password: "password2".to_string(),
        keys: HashMap::new(),
        balances: HashMap::new(),
    };
    
    let encrypted1 = wallet1.encrypt_private_key(test_private_key).unwrap();
    let encrypted2 = wallet2.encrypt_private_key(test_private_key).unwrap();
    
    // Different passwords should produce different encrypted outputs
    assert_ne!(encrypted1, encrypted2);
}

/// Test that each encryption produces unique output (different salts/nonces)
#[test]
fn test_unique_encryption_outputs() {
    let wallet = Wallet {
        master_password: "test_password".to_string(),
        keys: HashMap::new(),
        balances: HashMap::new(),
    };

    let test_private_key = "a1b2c3d4e5f67890abcdef1234567890abcdef1234567890abcdef1234567890";
    
    let encrypted1 = wallet.encrypt_private_key(test_private_key).unwrap();
    let encrypted2 = wallet.encrypt_private_key(test_private_key).unwrap();
    let encrypted3 = wallet.encrypt_private_key(test_private_key).unwrap();
    
    // Each encryption should be unique due to random salt and nonce
    assert_ne!(encrypted1, encrypted2);
    assert_ne!(encrypted2, encrypted3);
    assert_ne!(encrypted1, encrypted3);
    
    // But all should decrypt to the same original
    assert_eq!(wallet.decrypt_private_key(&encrypted1).unwrap(), test_private_key);
    assert_eq!(wallet.decrypt_private_key(&encrypted2).unwrap(), test_private_key);
    assert_eq!(wallet.decrypt_private_key(&encrypted3).unwrap(), test_private_key);
}

/// Test decryption with wrong password fails
#[test]
fn test_wrong_password_decryption_fails() {
    let wallet1 = Wallet {
        master_password: "correct_password".to_string(),
        keys: HashMap::new(),
        balances: HashMap::new(),
    };
    
    let wallet2 = Wallet {
        master_password: "wrong_password".to_string(),
        keys: HashMap::new(),
        balances: HashMap::new(),
    };

    let test_private_key = "a1b2c3d4e5f67890abcdef1234567890abcdef1234567890abcdef1234567890";
    
    let encrypted = wallet1.encrypt_private_key(test_private_key).unwrap();
    
    // Correct password should work
    assert!(wallet1.decrypt_private_key(&encrypted).is_ok());
    
    // Wrong password should fail
    assert!(wallet2.decrypt_private_key(&encrypted).is_err());
}

/// Test various password lengths work correctly
#[test]
fn test_various_password_lengths() {
    let test_private_key = "a1b2c3d4e5f67890abcdef1234567890abcdef1234567890abcdef1234567890";
    
    let passwords = vec![
        "a",                    // 1 character
        "ab",                   // 2 characters
        "password",             // 8 characters
        "very_long_password_with_special_chars_123!@#", // 42 characters
        "🔐🔑💰",               // Unicode characters
    ];
    
    for password in passwords {
        let wallet = Wallet {
            master_password: password.to_string(),
            keys: HashMap::new(),
            balances: HashMap::new(),
        };
        
        let encrypted = wallet.encrypt_private_key(test_private_key).unwrap();
        let decrypted = wallet.decrypt_private_key(&encrypted).unwrap();
        assert_eq!(decrypted, test_private_key);
    }
}

/// Test encrypted data structure validation
#[test]
fn test_encrypted_data_structure() {
    let wallet = Wallet {
        master_password: "test_password".to_string(),
        keys: HashMap::new(),
        balances: HashMap::new(),
    };

    let test_private_key = "a1b2c3d4e5f67890abcdef1234567890abcdef1234567890abcdef1234567890";
    let encrypted = wallet.encrypt_private_key(test_private_key).unwrap();
    
    // Decode hex to verify structure
    let encrypted_bytes = hex::decode(&encrypted).unwrap();
    
    // Should have at least: 32 bytes salt + 12 bytes nonce + 16 bytes auth tag + data
    assert!(encrypted_bytes.len() >= 60);
    
    // First 32 bytes should be salt (should be random)
    let salt = &encrypted_bytes[0..32];
    assert!(salt.iter().any(|&b| b != 0)); // Salt shouldn't be all zeros
    
    // Next 12 bytes should be nonce
    let nonce = &encrypted_bytes[32..44];
    assert!(nonce.iter().any(|&b| b != 0)); // Nonce shouldn't be all zeros
}

/// Test invalid encrypted data handling
#[test]
fn test_invalid_encrypted_data() {
    let wallet = Wallet {
        master_password: "test_password".to_string(),
        keys: HashMap::new(),
        balances: HashMap::new(),
    };
    
    // Test various invalid inputs
    let invalid_inputs = vec![
        "",                    // Empty string
        "invalid_hex",         // Invalid hex
        "a1b2c3",             // Too short
        "a1b2c3d4e5f67890abcdef1234567890abcdef1234567890", // Too short for proper structure
    ];
    
    for invalid_input in invalid_inputs {
        assert!(wallet.decrypt_private_key(invalid_input).is_err());
    }
}

/// Performance test to ensure PBKDF2 iterations are sufficient
#[test]
fn test_pbkdf2_performance() {
    use std::time::Instant;
    
    let wallet = Wallet {
        master_password: "test_password".to_string(),
        keys: HashMap::new(),
        balances: HashMap::new(),
    };

    let test_private_key = "a1b2c3d4e5f67890abcdef1234567890abcdef1234567890abcdef1234567890";
    
    let start = Instant::now();
    let _encrypted = wallet.encrypt_private_key(test_private_key).unwrap();
    let encryption_time = start.elapsed();
    
    // With 100,000 iterations, encryption should take reasonable time (< 1 second)
    // but not be instant (should take at least 10ms to deter brute force)
    assert!(encryption_time.as_millis() >= 10);
    assert!(encryption_time.as_secs() < 1);
    
    println!("Encryption time with 100,000 PBKDF2 iterations: {:?}", encryption_time);
}

/// Test nonce uniqueness across multiple encryptions
#[test]
fn test_nonce_uniqueness() {
    let wallet = Wallet {
        master_password: "test_password".to_string(),
        keys: HashMap::new(),
        balances: HashMap::new(),
    };

    let test_private_key = "a1b2c3d4e5f67890abcdef1234567890abcdef1234567890abcdef1234567890";
    
    let mut nonces = std::collections::HashSet::new();
    
    // Generate 100 encryptions and verify all nonces are unique
    for _ in 0..100 {
        let encrypted = wallet.encrypt_private_key(test_private_key).unwrap();
        let encrypted_bytes = hex::decode(&encrypted).unwrap();
        let nonce = &encrypted_bytes[32..44]; // Extract nonce
        
        // Nonce should be unique
        assert!(nonces.insert(nonce.to_vec()), "Duplicate nonce detected!");
    }
    
    assert_eq!(nonces.len(), 100);
}