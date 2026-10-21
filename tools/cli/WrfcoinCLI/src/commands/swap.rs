use anyhow::Result;
use serde_json::Value;
use wrfcoin_cli::errors::not_wired;

use crate::cli::commands::ExecutionContext;

/// Retain the command spelling for existing callers, without a phantom quote,
/// wallet access, signing, or network request. Dry runs are unavailable too.
pub async fn handle_swap(
    _from: &str,
    _to: &str,
    _amount: f64,
    _slippage: f64,
    _dry_run: bool,
    _ctx: &ExecutionContext,
) -> Result<Value> {
    Err(not_wired("wallet swap", "see wrfcoin/core4#3247"))
}

pub async fn handle_price(_pair: &str) -> Result<Value> {
    Err(not_wired("wallet price", "see wrfcoin/core4#3247"))
}

pub async fn handle_liquidity(_pair: &str) -> Result<Value> {
    Err(not_wired("wallet liquidity", "see wrfcoin/core4#3247"))
}
