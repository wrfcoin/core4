//! Transaction Flooding Test for Wrfcoin
//!
//! This module implements a transaction flooding test to stress test the Wrfcoin blockchain.
//! It generates a high volume of transactions to measure system performance under load.

use crate::stress::stress_test_framework::{
    StressTestConfig, StressTestRunner, TransactionType,
};
use anyhow::Result;
use log::{error, info};

/// Run a transaction flooding test
///
/// This test generates a high volume of transactions in parallel to stress test
/// the blockchain's mempool, consensus, and transaction processing capabilities.
pub async fn run_transaction_flooding_test(
    api_url: &str,
    concurrent_clients: usize,
    transactions_per_client: usize,
    duration_seconds: u64,
) -> Result<()> {
    info!("Starting transaction flooding test");
    info!(
        "Settings: {} clients, {} transactions per client, {} seconds",
        concurrent_clients, transactions_per_client, duration_seconds
    );

    // Create test configuration
    let config = StressTestConfig {
        api_url: api_url.to_string(),
        concurrent_clients,
        transactions_per_client,
        batch_delay_ms: 50,
        batch_size: 20,
        transaction_types: vec![
            TransactionType::Transfer,
            TransactionType::Transfer,
            TransactionType::Transfer,
            TransactionType::Complex,
        ],
        random_seed: Some(12345), // For reproducibility
        duration_seconds,
        log_interval_seconds: 5,
        detailed_metrics: true,
    };

    // Create and run the test
    let runner = StressTestRunner::new(config);
    
    match runner.run().await {
        Ok(stats) => {
            info!("Transaction flooding test completed successfully");
            info!(
                "Results: {} transactions ({} successful, {} failed) at {:.2} TPS",
                stats.transactions_submitted,
                stats.transactions_successful,
                stats.transactions_failed,
                stats.tps
            );
            
            if stats.transactions_failed > 0 {
                let failure_rate = (stats.transactions_failed as f64) / 
                    (stats.transactions_submitted as f64) * 100.0;
                    
                if failure_rate > 10.0 {
                    error!("High failure rate: {:.2}%", failure_rate);
                    error!("System may be under excessive load");
                }
            }
        }
        Err(e) => {
            error!("Transaction flooding test failed: {}", e);
            return Err(e);
        }
    }

    Ok(())
}

/// Run a burst transaction test
///
/// This test generates short bursts of transactions with increasing intensity
/// to find the breaking point of the system.
pub async fn run_burst_transaction_test(
    api_url: &str,
    max_concurrent_clients: usize,
    transactions_per_burst: usize,
) -> Result<()> {
    info!("Starting burst transaction test");
    
    let mut last_success_rate = 100.0;
    let mut found_breaking_point = false;
    
    // Start with a low number of clients and increase until failure
    for clients in (5..=max_concurrent_clients).step_by(5) {
        info!("Testing with {} concurrent clients", clients);
        
        // Create test configuration
        let config = StressTestConfig {
            api_url: api_url.to_string(),
            concurrent_clients: clients,
            transactions_per_client: transactions_per_burst,
            batch_delay_ms: 0, // No delay for burst testing
            batch_size: transactions_per_burst, // Send all at once
            transaction_types: vec![TransactionType::Transfer],
            random_seed: Some(12345), // For reproducibility
            duration_seconds: 30, // Short duration for each test
            log_interval_seconds: 5,
            detailed_metrics: true,
        };

        // Create and run the test
        let runner = StressTestRunner::new(config);
        
        match runner.run().await {
            Ok(stats) => {
                let success_rate = (stats.transactions_successful as f64) / 
                    (stats.transactions_submitted as f64) * 100.0;
                    
                info!(
                    "Burst test with {} clients: {:.2}% success rate, {:.2} TPS",
                    clients, 
                    success_rate,
                    stats.tps
                );
                
                // Check if we've found the breaking point (significant drop in success rate)
                if success_rate < 90.0 && last_success_rate > 95.0 {
                    info!("Found potential breaking point at {} concurrent clients", clients);
                    found_breaking_point = true;
                }
                
                last_success_rate = success_rate;
                
                // If success rate is very low, stop testing
                if success_rate < 50.0 {
                    info!("System severely degraded at {} concurrent clients, stopping test", clients);
                    break;
                }
            }
            Err(e) => {
                error!("Burst test failed at {} concurrent clients: {}", clients, e);
                found_breaking_point = true;
                break;
            }
        }
        
        // Brief cooldown between tests
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    }
    
    if !found_breaking_point {
        info!(
            "No clear breaking point found up to {} concurrent clients",
            max_concurrent_clients
        );
    }

    Ok(())
}

/// Run a sustained load test
///
/// This test maintains a constant transaction rate for an extended period
/// to evaluate system stability under sustained load.
pub async fn run_sustained_load_test(
    api_url: &str,
    concurrent_clients: usize,
    duration_minutes: u64,
    transactions_per_second_target: u64,
) -> Result<()> {
    info!("Starting sustained load test");
    info!(
        "Settings: Target {} TPS for {} minutes",
        transactions_per_second_target, duration_minutes
    );
    
    // Calculate batch parameters to achieve target TPS
    let batch_size = 10;
    let transactions_per_client = 0; // Unlimited
    
    // Adjust batch delay to control TPS
    // For example, if we have 10 clients each sending batches of 10 transactions,
    // and we want 100 TPS, each client should send a batch every 1000ms
    let ms_between_batches = 
        (concurrent_clients as u64 * batch_size as u64 * 1000) / 
        transactions_per_second_target;
    
    // Ensure we don't have unreasonably small delays
    let batch_delay_ms = std::cmp::max(ms_between_batches, 50);
    
    info!(
        "Using {} concurrent clients with {} ms between batches",
        concurrent_clients, batch_delay_ms
    );

    // Create test configuration
    let config = StressTestConfig {
        api_url: api_url.to_string(),
        concurrent_clients,
        transactions_per_client,
        batch_delay_ms,
        batch_size,
        transaction_types: vec![
            TransactionType::Transfer, 
            TransactionType::Transfer,
            TransactionType::Complex,
            TransactionType::CreateNft,
        ],
        random_seed: Some(12345), // For reproducibility
        duration_seconds: duration_minutes * 60,
        log_interval_seconds: 30, // Longer intervals for long tests
        detailed_metrics: true,
    };

    // Create and run the test
    let runner = StressTestRunner::new(config);
    
    match runner.run().await {
        Ok(stats) => {
            info!("Sustained load test completed successfully");
            info!(
                "Results: {} transactions ({} successful, {} failed) at {:.2} TPS",
                stats.transactions_submitted,
                stats.transactions_successful,
                stats.transactions_failed,
                stats.tps
            );
            
            // Analyze stability
            if !stats.latencies_ms.is_empty() {
                let mut sorted_latencies = stats.latencies_ms.clone();
                sorted_latencies.sort_unstable();
                
                let p50 = sorted_latencies[sorted_latencies.len() * 50 / 100];
                let p99 = sorted_latencies[sorted_latencies.len() * 99 / 100];
                
                // Calculate stability metric - ratio of p99 to p50
                let stability_ratio = p99 as f64 / p50 as f64;
                
                if stability_ratio > 10.0 {
                    error!("System showed high latency variability (ratio {:.2})", stability_ratio);
                    error!("This indicates potential stability issues under sustained load");
                } else if stability_ratio > 5.0 {
                    info!("System showed moderate latency variability (ratio {:.2})", stability_ratio);
                } else {
                    info!("System showed good stability under sustained load");
                }
            }
            
            // Analyze mempool growth
            if !stats.mempool_sizes.is_empty() {
                let first_quarter = &stats.mempool_sizes[0..stats.mempool_sizes.len()/4];
                let last_quarter = &stats.mempool_sizes[3*stats.mempool_sizes.len()/4..];
                
                let first_avg = first_quarter.iter().sum::<usize>() as f64 / first_quarter.len() as f64;
                let last_avg = last_quarter.iter().sum::<usize>() as f64 / last_quarter.len() as f64;
                
                if last_avg > first_avg * 2.0 {
                    error!("Mempool size grew significantly during the test");
                    error!("This indicates potential transaction processing slowdown");
                } else {
                    info!("Mempool size remained stable during the test");
                }
            }
        }
        Err(e) => {
            error!("Sustained load test failed: {}", e);
            return Err(e);
        }
    }

    Ok(())
}