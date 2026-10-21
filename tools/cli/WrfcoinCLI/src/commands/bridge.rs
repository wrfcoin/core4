use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::commands::ExecutionContext;

/// Commands for cross-chain bridge operations
#[derive(Subcommand)]
pub enum BridgeCommands {
    /// List all configured cross-chain bridges and their status
    #[clap(hide = true)]
    ListBridges,

    /// Transfer assets to another chain
    #[clap(hide = true)]
    Transfer {
        /// Chain to transfer to (e.g., "ethereum", "cosmos", "polkadot", "near")
        #[clap(long)]
        to_chain: String,

        /// Destination address on the target chain
        #[clap(long)]
        to_address: String,

        /// Amount to transfer
        #[clap(long)]
        amount: f64,

        /// Source address (defaults to wallet primary address)
        #[clap(long)]
        from: Option<String>,

        /// Asset type to transfer (default: WRF)
        #[clap(long, default_value = "WRF")]
        asset: String,

        /// Advanced: use specific bridge provider (e.g., "layerzero", "axelar", "wormhole")
        #[clap(long)]
        provider: Option<String>,
    },

    /// Query information about a cross-chain transaction
    #[clap(hide = true)]
    TransactionInfo {
        /// Transaction hash
        #[clap(long)]
        hash: String,
    },

    /// Configure a new bridge connection
    #[clap(hide = true)]
    ConfigureBridge {
        /// Chain to connect to
        #[clap(long)]
        chain: String,

        /// Provider to use for the bridge
        #[clap(long)]
        provider: String,

        /// RPC endpoint for the chain
        #[clap(long)]
        rpc_endpoint: String,

        /// Contract address on the target chain
        #[clap(long)]
        contract_address: Option<String>,
    },

    /// Test a cross-chain connection
    #[clap(hide = true)]
    TestConnection {
        /// Chain to test connection with
        #[clap(long)]
        chain: String,

        /// Provider to use (e.g., "layerzero", "axelar", "wormhole")
        #[clap(long)]
        provider: Option<String>,
    },

    /// Setup Polkadot/Parity integration
    #[clap(hide = true)]
    SetupPolkadot {
        /// Polkadot node URL
        #[clap(long)]
        node_url: String,

        /// Polkadot account to use
        #[clap(long)]
        account: String,

        /// Advanced configuration in JSON format
        #[clap(long)]
        config: Option<String>,
    },

    /// Setup Near Protocol integration
    #[clap(hide = true)]
    SetupNear {
        /// Near node URL
        #[clap(long)]
        node_url: String,

        /// Near account ID
        #[clap(long)]
        account_id: String,

        /// Near network ID (default: testnet)
        #[clap(long, default_value = "testnet")]
        network_id: String,
    },

    /// Stream bridge status continuously
    #[clap(hide = true)]
    Watch {
        /// Refresh interval in seconds
        #[clap(long, default_value = "10")]
        interval: u64,

        /// Fetch once and exit (for automation)
        #[clap(long)]
        once: bool,
    },
}

pub async fn handle_bridge_command(
    _command: &BridgeCommands,
    _ctx: &ExecutionContext,
) -> Result<Value> {
    Err(wrfcoin_cli::errors::CliError::new(
        wrfcoin_cli::CliErrorCode::NotFound,
        "bridge: not wired on the testnet; bridges are unavailable by owner ruling (see wrfcoin/core4#3247)",
    )
    .into())
}
