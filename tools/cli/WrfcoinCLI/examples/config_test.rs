//! Example to test the configuration manager functionality
//!
//! This example demonstrates loading and managing configuration.
//!
//! Usage:
//! ```
//! cargo run --example config_test
//! ```

use anyhow::Result;
use colored::*;
use env_logger::Env;

// Import from the main crate
use wrfcoin_cli::config::{ConfigManager, Network};

fn main() -> Result<()> {
    // Initialize logger
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    // Display header
    println!("{}", "Wrfcoin Configuration Test".bold().green());
    println!("{}", "==========================".green());

    // Create a temporary directory for testing
    let temp_dir = std::env::temp_dir().join("wrfcoin-test");
    if !temp_dir.exists() {
        std::fs::create_dir_all(&temp_dir)?;
    }

    let config_path = temp_dir.join("config.json");
    println!(
        "\nUsing temporary configuration file: {}",
        config_path.display()
    );

    // Create a configuration manager
    let mut config_manager = ConfigManager::with_path(config_path)?;

    // Get the default configuration
    let config = config_manager.get_config();

    // Print the default configuration
    println!("\n{}", "Default Configuration:".yellow());
    println!("  Version: {}", config.version);
    println!("  Network: {}", config.network.to_string());
    println!("  API URL: {}", config.api_url());
    println!("  WebSocket URL: {}", config.websocket_url());

    // Update the configuration
    println!("\n{}", "Updating Configuration:".yellow());
    println!("  Changing network to Mainnet");
    println!("  Changing API port to 3001");
    println!("  Setting default wallet address");

    config_manager.update_config(|config| {
        config.network = Network::Mainnet;
        config.api.port = 3001;
        config.wallet.default_address = Some("Wrf1AbCdEfGhIjKlMnOpQrStUvWxYz12345678".to_string());
    })?;

    // Get the updated configuration
    let updated_config = config_manager.get_config();

    // Print the updated configuration
    println!("\n{}", "Updated Configuration:".yellow());
    println!("  Version: {}", updated_config.version);
    println!("  Network: {}", updated_config.network.to_string());
    println!("  API URL: {}", updated_config.api_url());
    println!("  WebSocket URL: {}", updated_config.websocket_url());
    println!(
        "  Default Wallet: {}",
        updated_config.wallet.default_address.as_ref().unwrap()
    );

    // Create the required directories
    println!("\n{}", "Creating Required Directories:".yellow());
    config_manager.create_directories()?;

    // Print the directory paths
    println!(
        "  Base Directory: {}",
        updated_config.base_dir_path().display()
    );
    println!(
        "  Keys Directory: {}",
        updated_config.keys_dir_path().display()
    );
    println!(
        "  Data Directory: {}",
        updated_config.data_dir_path().display()
    );
    println!(
        "  Logs Directory: {}",
        updated_config.logs_dir_path().display()
    );

    // Reset the configuration to default
    println!("\n{}", "Resetting Configuration:".yellow());
    config_manager.reset_config()?;

    // Get the reset configuration
    let reset_config = config_manager.get_config();

    // Print the reset configuration
    println!("\n{}", "Reset Configuration:".yellow());
    println!("  Version: {}", reset_config.version);
    println!("  Network: {}", reset_config.network.to_string());
    println!("  API URL: {}", reset_config.api_url());
    println!("  WebSocket URL: {}", reset_config.websocket_url());

    println!(
        "\n{}",
        "Configuration test completed successfully!".green().bold()
    );
    Ok(())
}
