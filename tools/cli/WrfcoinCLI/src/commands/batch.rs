use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;
use std::path::PathBuf;

use crate::cli::commands::ExecutionContext;

/// Commands for batch / pipelined execution.
#[derive(Subcommand)]
pub enum BatchCommands {
    /// Unavailable legacy node-targeted weather batch execution
    #[clap(hide = true)]
    Run {
        /// Read commands from a file instead of stdin
        #[clap(long)]
        file: Option<PathBuf>,

        /// Stop on first failure
        #[clap(long)]
        fail_fast: bool,
    },
}

pub async fn handle_batch_command(
    _action: &BatchCommands,
    _ctx: &ExecutionContext,
) -> Result<Value> {
    // This dispatcher supported only weather submit/history, whose direct CLI
    // equivalents are unavailable. Refuse before files, stdin or client creation;
    // station registration's separately signed --batch path is independent.
    Err(wrfcoin_cli::errors::CliError::new(
        wrfcoin_cli::CliErrorCode::NotFound,
        "batch run: not wired in this CLI; legacy weather operations need a separate authenticated integration (see wrfcoin/core4#3247)",
    ).into())
}
