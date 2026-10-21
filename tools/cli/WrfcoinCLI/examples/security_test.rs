//! Example to test the security module functionality
//!
//! This example demonstrates using the SecurityService for transaction verification,
//! DDoS protection, and other security features.
//!
//! Usage:
//! ```
//! cargo run --example security_test
//! ```

use anyhow::Result;
use chrono::Utc;
use colored::*;
use env_logger::Env;
use std::thread;
use std::time::Duration;

// Import from the main crate
use wrfcoin_cli::crypto::TransactionSigningData;
use wrfcoin_cli::security::{
    AntiDDoSOptions, CheckpointBlock, SecurityService, ValidationOptions, VerificationResult,
};

fn main() -> Result<()> {
    // Initialize logger
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    // Display header
    println!("{}", "Wrfcoin Security Module Test".bold().green());
    println!("{}", "============================".green());

    // Create security service with custom options
    let validation_options = ValidationOptions {
        required_confirmations: 3,
        max_transaction_age: 7200, // 2 hours
        strict_nonce_validation: true,
        enhanced_validation_for_high_value: true,
        high_value_threshold: 50_000_000_000, // 500 WRF in base units
    };

    let anti_ddos_options = AntiDDoSOptions {
        max_requests_per_minute: 60,
        max_concurrent_connections: 5,
        blacklist_time: 300, // 5 minutes
        pow_difficulty: 1,
        enable_throttling: true,
    };

    // Create security service
    let mut security_service = SecurityService::new(
        validation_options.clone(),
        anti_ddos_options.clone(),
        Vec::new(), // No checkpoints initially
    );

    // Test 1: Add a checkpoint
    println!("\n{}", "Test 1: Adding Checkpoint Blocks".yellow());
    let checkpoint1 = CheckpointBlock {
        height: 1000,
        hash: "0000000000000000a8d6e432db7acd65b6ade4f2779e6b5523c5f836e3c5d159".to_string(),
        timestamp: Utc::now().to_rfc3339(),
    };
    security_service.add_checkpoint(checkpoint1);

    let checkpoint2 = CheckpointBlock {
        height: 2000,
        hash: "0000000000000000c3adeedf5be8ebc5195118b223499df61637f9b31acebacd".to_string(),
        timestamp: Utc::now().to_rfc3339(),
    };
    security_service.add_checkpoint(checkpoint2);

    if let Some(latest) = security_service.get_latest_checkpoint() {
        println!(
            "Latest checkpoint at height {} with hash {}",
            latest.height, latest.hash
        );
    }

    println!(
        "Checkpoint verification at height 1000 with correct hash: {}",
        security_service.verify_checkpoint(
            1000,
            "0000000000000000a8d6e432db7acd65b6ade4f2779e6b5523c5f836e3c5d159"
        )?
    );

    println!(
        "Checkpoint verification at height 1000 with incorrect hash: {}",
        security_service.verify_checkpoint(1000, "invalid_hash")?
    );

    // Test 2: DDoS Protection
    println!("\n{}", "Test 2: DDoS Protection".yellow());
    let test_ip = "192.168.1.100";

    for i in 1..=70 {
        let allowed = security_service.check_connection_allowed(test_ip)?;
        if i <= 60 {
            assert!(allowed, "Connection should be allowed for request {}", i);
            println!("Request {}: Connection allowed", i);
        } else {
            assert!(!allowed, "Connection should be blocked after request 60");
            println!("Request {}: Connection blocked (rate limit exceeded)", i);
        }
    }

    println!("Waiting for rate limit to reset...");
    thread::sleep(Duration::from_secs(1));

    // Test blacklisting
    println!("\nBlacklisting test IP address for 5 seconds");
    security_service.blacklist_ip(test_ip, 5);

    assert!(
        !security_service.check_connection_allowed(test_ip)?,
        "Connection should be blocked for blacklisted IP"
    );
    println!("Connection attempt while blacklisted: Blocked");

    println!("Waiting for blacklist to expire...");
    thread::sleep(Duration::from_secs(6));

    assert!(
        security_service.check_connection_allowed(test_ip)?,
        "Connection should be allowed after blacklist expires"
    );
    println!("Connection after blacklist expired: Allowed");

    // Test 3: Transaction verification with nonces
    println!("\n{}", "Test 3: Transaction Verification".yellow());

    // Create a mock transaction
    let tx_data = TransactionSigningData {
        tx_id: "tx_12345".to_string(),
        from: "WrfUser123456789abcdef".to_string(),
        to: "WrfUser987654321fedcba".to_string(),
        amount: 10_000_000_000, // 100 WRF in base units
        fee: 100_000,           // 0.001 WRF in base units
        nonce: 12345,
        timestamp: Utc::now().to_rfc3339(),
        tx_type: "transfer".to_string(),
    };

    // Verify the transaction
    let result = security_service.verify_transaction(&tx_data, "mock_signature")?;
    match result {
        VerificationResult::Valid => {
            println!("Transaction valid");
        },
        VerificationResult::Invalid(reason) => {
            println!("Transaction invalid: {}", reason);
        },
        VerificationResult::NeedsAdditionalVerification(reason) => {
            println!("Transaction needs additional verification: {}", reason);
        },
    }

    // Try to reuse the same nonce
    let result = security_service.verify_transaction(&tx_data, "mock_signature")?;
    match result {
        VerificationResult::Valid => {
            println!("Second transaction with same nonce valid (unexpected)");
        },
        VerificationResult::Invalid(reason) => {
            println!("Second transaction with same nonce invalid: {}", reason);
        },
        VerificationResult::NeedsAdditionalVerification(reason) => {
            println!(
                "Second transaction with same nonce needs additional verification: {}",
                reason
            );
        },
    }

    // Create a high-value transaction
    let high_value_tx = TransactionSigningData {
        tx_id: "tx_12346".to_string(),
        from: "WrfUser123456789abcdef".to_string(),
        to: "WrfUser987654321fedcba".to_string(),
        amount: 100_000_000_000, // 1000 WRF in base units, above high_value_threshold
        fee: 500_000,            // 0.005 WRF in base units
        nonce: 12346, // Different nonce
        timestamp: Utc::now().to_rfc3339(),
        tx_type: "transfer".to_string(),
    };

    // Verify the high-value transaction
    let result = security_service.verify_transaction(&high_value_tx, "mock_signature")?;
    match result {
        VerificationResult::Valid => {
            println!("High-value transaction valid (unexpected)");
        },
        VerificationResult::Invalid(reason) => {
            println!("High-value transaction invalid: {}", reason);
        },
        VerificationResult::NeedsAdditionalVerification(reason) => {
            println!(
                "High-value transaction needs additional verification: {}",
                reason
            );
        },
    }

    // Test 4: PoW Challenge
    println!("\n{}", "Test 4: Proof-of-Work Challenges".yellow());

    let (challenge, difficulty) = security_service.generate_pow_challenge();
    println!(
        "Generated PoW challenge: {} with difficulty {}",
        challenge, difficulty
    );

    // Mock solutions
    let invalid_solution = "invalid";
    let valid_solution = "0123456789"; // Starts with difficulty=1 zeros

    println!(
        "Verifying invalid solution: {}",
        security_service.verify_pow_solution(&challenge, invalid_solution, difficulty)
    );

    println!(
        "Verifying valid solution: {}",
        security_service.verify_pow_solution(&challenge, valid_solution, difficulty)
    );

    println!(
        "\n{}",
        "Security testing completed successfully!".green().bold()
    );
    Ok(())
}
