//! Example to test the crypto functionality
//!
//! This example demonstrates key generation, transaction signing,
//! and signature verification.
//!
//! Usage:
//! ```
//! cargo run --example crypto_test
//! ```

use anyhow::Result;
use colored::*;
use env_logger::Env;

// Import from the main crate
use wrfcoin_cli::crypto::{
    KeyManager, TransactionSigningData, WrfKeyPair, verify_transaction_signature,
};

fn main() -> Result<()> {
    // Initialize logger
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    // Display header
    println!("{}", "Wrfcoin Crypto Test".bold().green());
    println!("{}", "===================".green());

    // Test key pair generation
    println!("\n{}", "Testing key pair generation:".yellow());
    let key_pair = WrfKeyPair::generate()?;
    println!("Generated key pair:");
    println!("  Public key: {}", key_pair.public_key);
    println!("  Address: {}", key_pair.address);

    // Test transaction signing
    println!("\n{}", "Testing transaction signing:".yellow());
    let tx_data = TransactionSigningData {
        tx_id: "tx12345".to_string(),
        from: key_pair.address.clone(),
        to: "WrfDestinationAddress".to_string(),
        amount: 10_000_000_000, // 100 WRF in base units
        fee: 100_000_000,       // 1 WRF in base units
        timestamp: "2025-05-12T12:00:00Z".to_string(),
        tx_type: "Regular".to_string(),
        nonce: 12345,
    };

    let signature = key_pair.sign_transaction(&tx_data)?;
    println!("Transaction signed successfully.");
    println!("  Signature: {}", signature);

    // Test signature verification
    println!("\n{}", "Testing signature verification:".yellow());
    let verified = verify_transaction_signature(&tx_data, &key_pair.public_key, &signature)?;
    println!(
        "Signature verification: {}",
        if verified {
            "✓ Valid".green()
        } else {
            "✗ Invalid".red()
        }
    );

    // Test key manager
    println!("\n{}", "Testing key manager:".yellow());

    // Create a temporary directory for testing
    let temp_dir = std::env::temp_dir().join("wrfcoin-test");
    if !temp_dir.exists() {
        std::fs::create_dir_all(&temp_dir)?;
    }
    println!("Using temporary directory: {}", temp_dir.display());

    let key_manager = KeyManager::new(&temp_dir)?;

    // Generate a key pair with the manager
    let key_pair1 = key_manager.generate_key_pair(Some("primary"))?;
    println!("Generated key pair with manager:");
    println!("  Address: {}", key_pair1.address);

    // Generate another key pair
    let key_pair2 = key_manager.generate_key_pair(Some("secondary"))?;
    println!("Generated second key pair:");
    println!("  Address: {}", key_pair2.address);

    // List key pairs
    let key_pairs = key_manager.list_key_pairs()?;
    println!("Key pairs in manager:");
    for (addr, path) in key_pairs {
        println!("  Address: {}, Path: {}", addr, path.display());
    }

    // Load a key pair
    let loaded_key_pair = key_manager.load_key_pair(&key_pair1.address)?;
    println!("Loaded key pair:");
    println!("  Address: {}", loaded_key_pair.address);

    // Sign with loaded key pair
    let _signature = loaded_key_pair.sign_transaction(&TransactionSigningData {
        tx_id: "tx67890".to_string(),
        from: loaded_key_pair.address.clone(),
        to: "WrfAnotherAddress".to_string(),
        amount: 20_000_000_000, // 200 WRF in base units
        fee: 200_000_000,       // 2 WRF in base units
        timestamp: "2025-05-12T13:00:00Z".to_string(),
        tx_type: "Regular".to_string(),
        nonce: 67890,
    })?;
    println!("Signed transaction with loaded key pair successfully.");

    // Delete a key pair
    key_manager.delete_key_pair(&key_pair2.address)?;
    println!("Deleted key pair: {}", key_pair2.address);

    // List remaining key pairs
    let key_pairs = key_manager.list_key_pairs()?;
    println!("Remaining key pairs:");
    for (addr, path) in key_pairs {
        println!("  Address: {}, Path: {}", addr, path.display());
    }

    println!(
        "\n{}",
        "All crypto tests completed successfully!".green().bold()
    );
    Ok(())
}
