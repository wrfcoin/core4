use colored::*;
use comfy_table::{ContentArrangement, Table};
use serde::Serialize;
use serde_json::Value;
use std::io::IsTerminal;
use std::process;

use crate::config::OutputPreference;

/// WRF base units per coin.
pub const WRF_UNITS_PER_COIN: u64 = 100_000_000;

/// Output format mode for all CLI commands
#[derive(Clone, Debug, PartialEq)]
pub enum OutputMode {
    /// Human-readable colored tables (default)
    Human,
    /// Machine-readable JSON (for agents/scripts)
    Json,
    /// Quiet mode - minimal output, just essential data
    Quiet,
}

/// Structured CLI response that can render as human-readable or JSON
#[derive(Debug, Serialize)]
pub struct CliResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<CliError>,
}

#[derive(Debug, Serialize)]
pub struct CliError {
    pub code: String,
    pub message: String,
}

/// Exit codes following the agent-friendly design spec (#282)
#[allow(dead_code)]
pub mod exit_codes {
    pub const SUCCESS: i32 = 0;
    pub const USER_ERROR: i32 = 1;
    pub const NETWORK_ERROR: i32 = 2;
    pub const AUTH_ERROR: i32 = 3;
    pub const RATE_LIMITED: i32 = 4;
    pub const BLOCKCHAIN_ERROR: i32 = 5;
}

/// Rich output format for the CLI (#282).
///
/// Separate from `OutputMode` (which is preserved for backward compat).
/// `OutputFormat` adds Csv and Table variants and drives the emit layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Human,
    Json,
    Csv,
    Table,
    Quiet,
}

impl OutputFormat {
    /// True for formats intended to be consumed by machines/scripts.
    pub fn is_machine_readable(self) -> bool {
        matches!(self, Self::Json | Self::Csv | Self::Quiet)
    }
}

/// Resolve the effective `OutputFormat` from layered sources.
///
/// Priority: explicit `--output` flag > `--json` shorthand > `--quiet` shorthand
/// > profile preference > pipe detection > Human default.
pub fn resolve_output_format(
    output: Option<OutputFormat>,
    json_flag: bool,
    quiet_flag: bool,
    pref: &OutputPreference,
) -> OutputFormat {
    if let Some(fmt) = output {
        return fmt;
    }
    if json_flag {
        return OutputFormat::Json;
    }
    if quiet_flag {
        return OutputFormat::Quiet;
    }
    match pref {
        OutputPreference::Json => OutputFormat::Json,
        OutputPreference::Quiet => OutputFormat::Quiet,
        OutputPreference::Human => {
            if !is_terminal() {
                OutputFormat::Json
            } else {
                OutputFormat::Human
            }
        },
    }
}

impl CliResponse {
    pub fn success(data: Value) -> Self {
        Self {
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn error(code: &str, message: &str) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(CliError {
                code: code.to_string(),
                message: message.to_string(),
            }),
        }
    }
}

/// Detect whether stdout is a terminal (not piped)
pub fn is_terminal() -> bool {
    std::io::stdout().is_terminal()
}

/// Resolve the effective output mode based on flags and environment.
/// Priority: --json > --quiet > pipe detection > default Human
pub fn resolve_mode(json_flag: bool, quiet_flag: bool) -> OutputMode {
    if json_flag {
        OutputMode::Json
    } else if quiet_flag {
        OutputMode::Quiet
    } else if !is_terminal() {
        // When piped to another program, auto-switch to JSON for agent-friendliness
        OutputMode::Json
    } else {
        OutputMode::Human
    }
}

/// Print a success response
pub fn print_success(mode: &OutputMode, data: Value, human_lines: &[String]) {
    match mode {
        OutputMode::Json => {
            let response = CliResponse::success(data);
            println!("{}", serde_json::to_string_pretty(&response).unwrap());
        },
        OutputMode::Quiet => {
            // In quiet mode, print just the raw JSON data (no envelope)
            println!("{}", serde_json::to_string(&data).unwrap());
        },
        OutputMode::Human => {
            for line in human_lines {
                println!("{}", line);
            }
        },
    }
}

/// Print an error and exit with appropriate code
pub fn print_error(mode: &OutputMode, code: &str, message: &str, exit_code: i32) -> ! {
    match mode {
        OutputMode::Json | OutputMode::Quiet => {
            let response = CliResponse::error(code, message);
            eprintln!("{}", serde_json::to_string_pretty(&response).unwrap());
        },
        OutputMode::Human => {
            eprintln!("{} {}", "Error:".red().bold(), message);
        },
    }
    process::exit(exit_code);
}

/// Print a styled header for human output
pub fn print_header(mode: &OutputMode, title: &str) {
    if *mode == OutputMode::Human {
        println!("{}", title.bold().cyan());
        println!("{}", "=".repeat(60));
    }
}

/// Build and print a comfy-table for human output, or JSON for machine output
pub fn print_table(mode: &OutputMode, headers: &[&str], rows: &[Vec<String>], json_data: Value) {
    match mode {
        OutputMode::Json => {
            let response = CliResponse::success(json_data);
            println!("{}", serde_json::to_string_pretty(&response).unwrap());
        },
        OutputMode::Quiet => {
            println!("{}", serde_json::to_string(&json_data).unwrap());
        },
        OutputMode::Human => {
            let mut table = Table::new();
            table.set_content_arrangement(ContentArrangement::Dynamic);
            table.set_header(headers.iter().map(|h| h.to_string()).collect::<Vec<_>>());
            for row in rows {
                table.add_row(row.clone());
            }
            println!("{}", table);
        },
    }
}

/// Colorize a status string for human output
pub fn colorize_status(status: &str) -> ColoredString {
    match status.to_lowercase().as_str() {
        "healthy" | "active" | "online" | "connected" | "finalized" | "done" => status.green(),
        "degraded" | "busy" | "syncing" | "pending" | "in-progress" => status.yellow(),
        "unhealthy" | "inactive" | "offline" | "error" | "halted" | "disputed" => status.red(),
        _ => status.normal(),
    }
}

/// Format a WRF token amount for human display
pub fn format_wrf(amount: f64) -> String {
    if amount >= 1_000_000.0 {
        format!("{:.2}M WRF", amount / 1_000_000.0)
    } else if amount >= 1_000.0 {
        format!("{:.2}K WRF", amount / 1_000.0)
    } else {
        format!("{:.4} WRF", amount)
    }
}

/// Format a base-unit WRF amount for human display.
pub fn format_wrf_units(units: u64) -> String {
    let whole = units / WRF_UNITS_PER_COIN;
    let frac = units % WRF_UNITS_PER_COIN;
    format!("{whole}.{frac:08} WRF")
}

/// Print a key-value pair for human output (used in version/config display)
#[allow(dead_code)]
pub fn print_kv(mode: &OutputMode, key: &str, value: &str) {
    if *mode == OutputMode::Human {
        println!("  {:<18} {}", key.dimmed(), value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_machine_readable_classification() {
        assert!(!OutputFormat::Human.is_machine_readable());
        assert!(OutputFormat::Json.is_machine_readable());
        assert!(OutputFormat::Csv.is_machine_readable());
        assert!(!OutputFormat::Table.is_machine_readable());
        assert!(OutputFormat::Quiet.is_machine_readable());
    }

    #[test]
    fn explicit_output_flag_wins() {
        let result = resolve_output_format(
            Some(OutputFormat::Csv),
            true, // --json
            true, // --quiet
            &OutputPreference::Human,
        );
        assert_eq!(result, OutputFormat::Csv);
    }

    #[test]
    fn json_flag_overrides_quiet_and_pref() {
        let result = resolve_output_format(
            None,
            true, // --json
            true, // --quiet
            &OutputPreference::Human,
        );
        assert_eq!(result, OutputFormat::Json);
    }

    #[test]
    fn quiet_flag_overrides_pref() {
        let result = resolve_output_format(
            None,
            false,
            true, // --quiet
            &OutputPreference::Human,
        );
        assert_eq!(result, OutputFormat::Quiet);
    }

    #[test]
    fn profile_pref_json_applied() {
        // Note: when pref is Json, it returns Json regardless of pipe detection
        let result = resolve_output_format(None, false, false, &OutputPreference::Json);
        assert_eq!(result, OutputFormat::Json);
    }

    #[test]
    fn profile_pref_quiet_applied() {
        let result = resolve_output_format(None, false, false, &OutputPreference::Quiet);
        assert_eq!(result, OutputFormat::Quiet);
    }
}
