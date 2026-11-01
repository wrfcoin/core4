use anyhow::{Result, anyhow};

pub mod aiml;
pub mod batch;
pub mod bridge;
pub mod ceremony;
pub mod chain;
pub mod compute;
pub mod contribute;
pub mod data;
pub mod dev;
pub mod emit;
pub mod forecast;
pub mod governance;
pub mod infra;
pub mod iot;
pub mod monitor;
pub mod nft;
pub mod node;
pub mod node_bond;
pub mod node_deposit;
pub mod node_fraud;
pub mod node_receipt;
pub mod oracle;
pub mod security_key;
pub mod operator;
pub mod send_interactive;
pub mod session_key;
pub mod setup;
pub mod snapshot;
pub mod station;
pub mod send_allowlist;
pub mod station_owner;
pub mod station_owner_exchange;
pub mod storage;
pub mod swap;
pub mod wallet;
pub mod watch;
pub mod weather;

use wrfcoin_cli::output::WRF_UNITS_PER_COIN;

pub(crate) fn wrf_amount_to_units(amount: f64) -> Result<u64> {
    if !amount.is_finite() || amount < 0.0 {
        return Err(anyhow!("Invalid WRF amount"));
    }

    let units = (amount * WRF_UNITS_PER_COIN as f64).round();
    if !units.is_finite() || units < 0.0 || units > u64::MAX as f64 {
        return Err(anyhow!("WRF amount is out of range"));
    }

    Ok(units as u64)
}

pub use aiml::AiMlCommands;
pub use batch::BatchCommands;
pub use bridge::BridgeCommands;
pub use ceremony::CeremonyCommands;
pub use chain::ChainCommands;
pub use compute::ComputeCommands;
pub use contribute::ContributeCommands;
pub use data::DataCommands;
pub use dev::DevCommands;
pub use forecast::ForecastCommands;
pub use governance::GovernanceCommands;
pub use infra::InfraCommands;
pub use iot::IoTCommands;
pub use monitor::MonitorCommands;
pub use nft::NftCommands;
pub use node::NodeCommands;
pub use oracle::OracleCommands;
pub use operator::OperatorCommands;
pub use setup::SetupCommands;
pub use snapshot::SnapshotCommands;
pub use station::StationCommands;
pub use storage::StorageCommands;
pub use wallet::WalletCommands;
pub use weather::WeatherCommands;
