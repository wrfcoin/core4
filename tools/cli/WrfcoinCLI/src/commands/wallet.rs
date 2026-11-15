use clap::Subcommand;

/// Commands for wallet management
#[derive(Subcommand)]
pub enum WalletCommands {
    /// Create a new wallet address
    NewAddress,

    /// List all wallet addresses
    ListAddresses,

    /// Get wallet balance
    Balance {
        /// Address to check balance for (if omitted, use primary address)
        #[clap(long)]
        address: Option<String>,
    },

    /// Set primary wallet address
    SetPrimary {
        /// Address to set as primary
        address: String,
    },

    /// Send tokens
    Send {
        /// Recipient address
        #[clap(long)]
        to: String,

        /// Amount to send
        #[clap(long)]
        amount: f64,

        /// Fee amount (optional, will use recommended fee if omitted)
        #[clap(long)]
        fee: Option<f64>,

        /// Transaction memo
        #[clap(long)]
        memo: Option<String>,

        /// Interactive mode with preview, risk warnings, and confirmation
        #[clap(long)]
        interactive: bool,

        /// Dry-run mode: preview the transaction without executing
        #[clap(long)]
        dry_run: bool,

        /// Account nonce (required until the node exposes it on the address route)
        #[clap(long)]
        nonce: Option<u64>,
    },

    /// Explicitly migrate legacy wallet addresses to the chain format
    MigrateAddresses {
        /// Print the legacy-to-chain mapping without changing files
        #[clap(long)]
        dry_run: bool,

        /// Confirm the backed-up wallet index rewrite
        #[clap(long)]
        yes: bool,
    },

    /// Recover wallet from BIP-39 mnemonic phrase
    Recover {
        /// BIP-39 mnemonic phrase (24 words, quoted)
        #[clap(long)]
        mnemonic: String,
    },

    /// View transaction history
    History {
        /// Address to get history for (if omitted, use primary address)
        #[clap(long)]
        address: Option<String>,

        /// Maximum number of transactions to show
        #[clap(long, default_value = "10")]
        limit: u32,
    },

    /// Unavailable: no token swap service on the testnet
    #[clap(hide = true)]
    Swap {
        /// Token to swap from
        #[clap(long, default_value = "WRF")]
        from: String,

        /// Token to swap to
        #[clap(long)]
        to: String,

        /// Amount to swap
        #[clap(long)]
        amount: f64,

        /// Maximum slippage tolerance (percent)
        #[clap(long, default_value_t = 0.5)]
        slippage: f64,

        /// Dry runs are unavailable; retained for existing callers
        #[clap(long)]
        dry_run: bool,
    },

    /// Unavailable: no token price service on the testnet
    #[clap(hide = true)]
    Price {
        /// Trading pair (e.g. "WRF/USDC")
        #[clap(long)]
        pair: String,
    },

    /// Unavailable: no liquidity service on the testnet
    #[clap(hide = true)]
    Liquidity {
        /// Trading pair (e.g. "WRF/USDC")
        #[clap(long)]
        pair: String,
    },

    /// Require a FIDO2 security key (YubiKey) to send from this wallet
    #[clap(subcommand)]
    SecurityKey(super::security_key::SecurityKeyCommands),

    /// Stream wallet balance and activity continuously
    Watch {
        /// Refresh interval in seconds
        #[clap(long, default_value = "5")]
        interval: u64,

        /// Fetch once and exit (for automation)
        #[clap(long)]
        once: bool,

        /// Address to watch (if omitted, use primary address)
        #[clap(long)]
        address: Option<String>,
    },
}
