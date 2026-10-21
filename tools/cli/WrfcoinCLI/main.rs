use anyhow::Result;
use clap::{Parser, Subcommand};
use env_logger::Env;
use log::{info, error};
use std::path::PathBuf;
use wrfcoin::{WrfcoinNode, api, cli};

#[derive(Parser)]
#[clap(name = "wrfcoin", about = "Wrfcoin - A blockchain with computational-NFT support")]
struct Cli {
    /// Optional config file path
    #[clap(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start a Wrfcoin node
    Start {
        /// Run the node with API server
        #[clap(short, long)]
        api: bool,
        
        /// Mining mode (if enabled, the node will start mining blocks)
        #[clap(short, long)]
        mine: bool,
    },
    
    /// Wallet management commands
    Wallet {
        #[clap(subcommand)]
        action: cli::commands::WalletCommands,
    },
    
    /// Blockchain management and information commands
    Chain {
        #[clap(subcommand)]
        action: cli::commands::ChainCommands,
    },
    
    /// NFT management commands
    Nft {
        #[clap(subcommand)]
        action: cli::commands::NftCommands,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize the logger
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    // Parse command line arguments
    let cli = Cli::parse();
    
    // Determine config file path
    let config_path = cli.config.as_deref().map(|p| p.to_str().unwrap_or("config.json"));
    
    // Process commands
    match &cli.command {
        Commands::Start { api: run_api, mine } => {
            info!("Starting Wrfcoin node");
            
            // Initialize and start the node
            let node = WrfcoinNode::new(config_path).await?;
            node.start().await?;
            
            // Start the API server if requested
            if *run_api {
                info!("Starting API server");
                let api_server = api::handlers::start_api_server(
                    &node.config.api_host,
                    node.config.api_port,
                    node.blockchain.clone(),
                    node.mempool.clone(),
                    node.wallet.clone(),
                    node.nft_manager.clone(),
                )?;
                
                tokio::spawn(async move {
                    if let Err(e) = api_server.await {
                        error!("API server error: {}", e);
                    }
                });
            }
            
            // Start mining if requested
            if *mine {
                info!("Starting mining process");
                let blockchain = node.blockchain.clone();
                let mempool = node.mempool.clone();
                let config = node.config.clone();
                let wallet = node.wallet.clone();
                
                tokio::spawn(async move {
                    let miner_address = wallet.read().await.get_primary_address().unwrap_or_default();
                    let mut consensus = wrfcoin::consensus::proof_of_work::ProofOfWork::new(
                        config.mining_difficulty,
                        config.block_time_target_seconds,
                    );
                    
                    loop {
                        if let Err(e) = consensus.mine_next_block(
                            blockchain.clone(),
                            mempool.clone(),
                            &miner_address,
                        ).await {
                            error!("Mining error: {}", e);
                        }
                        
                        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    }
                });
            }
            
            // Keep the main thread alive
            tokio::signal::ctrl_c().await?;
            info!("Shutting down Wrfcoin node");
            node.stop().await?;
        },
        Commands::Wallet { action } => {
            // Initialize the node to get access to the wallet
            let node = WrfcoinNode::new(config_path).await?;
            cli::commands::handle_wallet_command(action, node.wallet.clone()).await?;
        },
        Commands::Chain { action } => {
            // Initialize the node to get access to the blockchain
            let node = WrfcoinNode::new(config_path).await?;
            cli::commands::handle_chain_command(action, node.blockchain.clone(), node.storage.clone()).await?;
        },
        Commands::Nft { action } => {
            // Initialize the node to get access to the NFT manager
            let node = WrfcoinNode::new(config_path).await?;
            cli::commands::handle_nft_command(
                action, 
                node.nft_manager.clone(), 
                node.blockchain.clone(), 
                node.wallet.clone()
            ).await?;
        }
    }
    
    Ok(())
}
