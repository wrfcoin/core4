//! Example to test the WebSocket client functionality
//!
//! This example connects to the Wrfcoin WebSocket server and
//! subscribes to blockchain events, displaying them in real-time.
//!
//! Usage:
//! ```
//! cargo run --example websocket_test
//! ```

use anyhow::{Result, anyhow};
use colored::*;
use env_logger::Env;
use log::error;
use std::time::Duration;
use tokio::signal;

// Import from the main crate
use wrfcoin_cli::websocket::{WebSocketClient, WebSocketConfig};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logger
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    // Display header
    println!("{}", "Wrfcoin WebSocket Client Test".bold().green());
    println!("{}", "================================".green());
    println!("Connecting to WebSocket server...");

    // Create WebSocket client with default configuration
    let config = WebSocketConfig {
        url: "ws://localhost:5001/ws".to_string(),
        heartbeat_interval: Duration::from_secs(30),
        reconnect_interval: Duration::from_secs(3),
        max_reconnect_attempts: 5,
    };

    let mut client = WebSocketClient::new(config).await?;

    // Subscribe to blockchain events
    client.subscribe("blockchain").await?;
    println!("Subscribed to blockchain events");

    // Create a channel to signal when we receive the first message
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let mut tx_option = Some(tx);

    // Print instructions
    println!("\n{}", "Waiting for blockchain events...".yellow());
    println!("Press Ctrl+C to exit\n");

    // Process incoming messages
    let message_task = tokio::spawn(async move {
        let mut message_count = 0;

        // Process messages until stopped
        while let Some(message) = client.receive().await {
            message_count += 1;

            match message.event.as_str() {
                "STATE_UPDATE" => {
                    if let Some(state) = message.data.get("state") {
                        println!(
                            "{} {}",
                            "BLOCKCHAIN STATE:".blue().bold(),
                            format!(
                                "Height: {}, Hashrate: {}, Difficulty: {}, Pending: {}",
                                state
                                    .get("blockHeight")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0),
                                state.get("hashrate").and_then(|v| v.as_u64()).unwrap_or(0),
                                state
                                    .get("difficulty")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0),
                                state
                                    .get("pendingTransactions")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0),
                            )
                            .cyan()
                        );
                    }
                },
                "TRANSACTION_CREATED" => {
                    if let Some(tx_data) = message.data.get("transaction") {
                        println!(
                            "{} {} {} {} {}",
                            "TX:".magenta().bold(),
                            format!(
                                "ID: {}",
                                tx_data
                                    .get("id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown")
                            )
                            .yellow(),
                            format!(
                                "Type: {}",
                                tx_data
                                    .get("type")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown")
                            )
                            .yellow(),
                            format!(
                                "Amount: {}",
                                tx_data
                                    .get("amount")
                                    .and_then(|v| v.as_f64())
                                    .unwrap_or(0.0)
                            )
                            .yellow(),
                            format!(
                                "Fee: {}",
                                tx_data.get("fee").and_then(|v| v.as_str()).unwrap_or("0")
                            )
                            .yellow(),
                        );
                    }
                },
                "BLOCK_CREATED" => {
                    if let Some(block_data) = message.data.get("block") {
                        println!(
                            "{} {} {}",
                            "BLOCK:".green().bold(),
                            format!(
                                "Height: {}",
                                block_data
                                    .get("height")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0)
                            )
                            .green(),
                            format!(
                                "Hash: {}",
                                block_data
                                    .get("hash")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown")
                            )
                            .green(),
                        );
                    }
                },
                _ => {
                    println!(
                        "{} {}: {}",
                        "EVENT:".yellow().bold(),
                        message.event,
                        serde_json::to_string_pretty(&message.data)
                            .unwrap_or_else(|_| "{}".to_string())
                    );
                },
            }

            // Signal that we've received the first message
            if message_count == 1 {
                if let Some(tx) = tx_option.take() {
                    let _ = tx.send(());
                }
            }
        }

        Ok::<(), anyhow::Error>(())
    });

    // Wait for the first message or timeout
    tokio::select! {
        _ = rx => {
            println!("{}", "✅ Successfully connected and received data!".green().bold());
        }
        _ = tokio::time::sleep(Duration::from_secs(10)) => {
            println!("{}", "⚠️  No data received in 10 seconds.".red().bold());
            println!("This could mean the server is not sending any events or there's a connection issue.");
            return Err(anyhow!("No data received within timeout period"));
        }
    }

    // Wait for Ctrl+C signal
    match signal::ctrl_c().await {
        Ok(()) => {
            println!("\n{}", "Received Ctrl+C, shutting down...".yellow());
        },
        Err(err) => {
            error!("Unable to listen for shutdown signal: {}", err);
        },
    }

    // Cancel the message task
    message_task.abort();

    println!("{}", "WebSocket client test completed.".green());
    Ok(())
}
