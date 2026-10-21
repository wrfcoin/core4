#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod blockchain;
mod mining;
mod network;
mod storage;
mod commands;

use std::sync::Arc;
use std::path::PathBuf;
use tokio::sync::Mutex;
use log::{info, error};
use dirs::data_dir;

use blockchain::chain::Blockchain;
use network::node::Node;
use storage::database::Database;

// Main state structure for the application
pub struct AppState {
    blockchain: Arc<Mutex<Blockchain>>,
    node: Arc<Mutex<Node>>,
    database: Arc<Mutex<Database>>,
}

fn initialize_app_state() -> Result<AppState, Box<dyn std::error::Error>> {
    // Create data directory if it doesn't exist
    let data_path = data_dir()
        .unwrap_or_else(|| PathBuf::from("./data"))
        .join("wrfcoin-core");
    
    std::fs::create_dir_all(&data_path)?;
    
    // Initialize database
    let database = Arc::new(Mutex::new(Database::new(&data_path.join("wrfcoin.db"))?));
    
    // Initialize blockchain
    let blockchain = Arc::new(Mutex::new(Blockchain::new(database.clone())?));
    
    // Initialize network node
    let node = Arc::new(Mutex::new(Node::new(blockchain.clone())?));
    
    Ok(AppState {
        blockchain,
        node,
        database,
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::init();
    info!("Starting Wrfcoin Core...");
    
    // Initialize application state
    let app_state = match initialize_app_state() {
        Ok(state) => state,
        Err(e) => {
            error!("Failed to initialize application: {}", e);
            return Err(e);
        }
    };
    
    // Configure and run Tauri application
    tauri::Builder::default()
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            commands::blockchain::get_blockchain_info,
            commands::blockchain::get_latest_blocks,
            commands::blockchain::get_block_by_hash,
            commands::blockchain::get_transaction_by_id,
            commands::mining::start_cpu_mining,
            commands::mining::stop_cpu_mining,
            commands::mining::start_gpu_mining,
            commands::mining::stop_gpu_mining,
            commands::mining::get_mining_stats,
            commands::mining::one_click_mine,
            commands::wallet::create_wallet,
            commands::wallet::import_wallet,
            commands::wallet::save_wallet_to_file,
            commands::wallet::load_wallet_from_file,
            commands::wallet::get_wallet_balance,
            commands::wallet::send_transaction,
            commands::wallet::unlock_wallet,
            commands::wallet::get_wallet_transactions,
            commands::nft::create_nft,
            commands::nft::transfer_nft,
            commands::nft::get_nft_marketplace,
            commands::network::get_network_status,
            commands::network::connect_to_peer,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
    
    Ok(())
}
