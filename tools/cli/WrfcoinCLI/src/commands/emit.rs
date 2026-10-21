use anyhow::Result;
use colored::Colorize;
use comfy_table::{ContentArrangement, Table};
use serde_json::{Value, json};
use wrfcoin_cli::CliErrorCode;
use wrfcoin_cli::output::OutputFormat;
use wrfcoin_cli::why_report;

use crate::cli::commands::ExecutionContext;

pub fn emit_success(ctx: &ExecutionContext, command: &str, data: &Value) -> Result<()> {
    let why_meta = if ctx.why {
        let mut report = why_report::take_report();
        report.exit_code = 0;
        report.exit_reason = "success".to_string();
        Some(report)
    } else {
        None
    };

    match ctx.output_format {
        OutputFormat::Quiet => {
            println!("{}", serde_json::to_string(data)?);
        },
        OutputFormat::Json => {
            let envelope = json!({
                "ok": true,
                "command": command,
                "exit_code": 0,
                "data": data,
                "meta": {
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                    "cli_version": wrfcoin_cli::VERSION,
                },
                "why": why_meta.as_ref().map(|r| r.to_json()),
            });
            println!("{}", serde_json::to_string(&envelope)?);
        },
        OutputFormat::Csv => {
            emit_csv(data)?;
        },
        OutputFormat::Table => {
            emit_table(command, data)?;
        },
        OutputFormat::Human => {
            // Section header
            println!("{}", format!("wrfcoin {}", command).bold().cyan());
            println!("{}", "─".repeat(50).dimmed());

            // Summary line (first, prominent)
            if let Some(summary) = data.get("summary").and_then(Value::as_str) {
                println!("{}", summary.bold());
            }
            // Why-report lines (routing/retry diagnostics)
            if let Some(report) = &why_meta {
                for line in report.to_human_lines() {
                    println!("{}", line);
                }
            }
            // Render data fields in a human-friendly way (no raw JSON dump)
            render_human_fields(data)?;
        },
    }
    Ok(())
}

pub fn emit_error(ctx: &ExecutionContext, command: &str, err: &anyhow::Error) -> Result<()> {
    let error_code = CliErrorCode::from_error(err);
    let exit_code = error_code.exit_code();
    let why_meta = if ctx.why {
        let mut report = why_report::take_report();
        report.exit_code = exit_code;
        report.exit_reason = error_code.code_str().to_string();
        Some(report)
    } else {
        None
    };

    match ctx.output_format {
        OutputFormat::Quiet => {
            let mut payload = json!({
                "error": err.to_string(),
                "exit_code": exit_code,
            });
            if let Some(report) = &why_meta {
                payload["why"] = report.to_json();
            }
            println!("{}", serde_json::to_string(&payload)?);
        },
        OutputFormat::Json | OutputFormat::Csv | OutputFormat::Table => {
            let envelope = json!({
                "ok": false,
                "command": command,
                "exit_code": exit_code,
                "error": {
                    "code": error_code.code_str(),
                    "message": err.to_string(),
                    "hint": error_code.hint(),
                },
                "meta": {
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                    "cli_version": wrfcoin_cli::VERSION,
                },
                "why": why_meta.as_ref().map(|r| r.to_json()),
            });
            println!("{}", serde_json::to_string(&envelope)?);
        },
        OutputFormat::Human => {
            eprintln!("{}: {} (exit_code={})", command, err, exit_code);
            if let Some(report) = &why_meta {
                for line in report.to_human_lines() {
                    eprintln!("{}", line);
                }
            }
        },
    }
    Ok(())
}

/// Thin wrapper preserving the public API used by `main()`.
pub fn classify_exit_code(err: &anyhow::Error) -> i32 {
    CliErrorCode::from_error(err).exit_code()
}

/// Render data fields in human mode — no raw JSON dump.
///
/// Strategy:
/// - Skip the `summary` key (already printed by the caller).
/// - If the remaining data contains at least one array, render it as a table.
/// - Otherwise render scalar / string fields as a two-column key-value table;
///   nested objects are skipped (they are typically internal metadata).
fn render_human_fields(data: &Value) -> Result<()> {
    let Some(obj) = data.as_object() else {
        // Non-object top-level value — just print it.
        if !data.is_null() {
            println!("{}", serde_json::to_string_pretty(data)?);
        }
        return Ok(());
    };

    // Build a filtered view excluding the `summary` key.
    let filtered: serde_json::Map<String, Value> = obj
        .iter()
        .filter(|(k, _)| k.as_str() != "summary")
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    // Prefer array rendering when available.
    if let Some(arr) = find_first_array(&Value::Object(filtered.clone())) {
        if !arr.is_empty() {
            if let Some(first) = arr[0].as_object() {
                let headers: Vec<String> = first.keys().cloned().collect();
                let mut table = Table::new();
                table.set_content_arrangement(ContentArrangement::Dynamic);
                table.set_header(&headers);
                for item in arr {
                    let row: Vec<String> =
                        headers.iter().map(|h| value_to_cell(item.get(h))).collect();
                    table.add_row(row);
                }
                println!("{}", table);
                return Ok(());
            }
        }
    }

    // Key-value table for scalar / string fields.
    let scalar_pairs: Vec<(&str, &Value)> = filtered
        .iter()
        .filter(|(_, v)| matches!(v, Value::String(_) | Value::Number(_) | Value::Bool(_)))
        .map(|(k, v)| (k.as_str(), v))
        .collect();

    if !scalar_pairs.is_empty() {
        let mut table = Table::new();
        table.set_content_arrangement(ContentArrangement::Dynamic);
        table.set_header(["Field", "Value"]);
        for (k, v) in &scalar_pairs {
            table.add_row([*k, &value_to_cell(Some(v))]);
        }
        println!("{}", table);
    }

    Ok(())
}

/// Emit data as CSV rows to stdout.
///
/// If the data contains an array value, each element becomes a row with headers
/// derived from the first object's keys. Scalar/single objects become key-value rows.
fn emit_csv(data: &Value) -> Result<()> {
    // Find the first array value inside data (e.g. data.stations, data.history, ...)
    let array = find_first_array(data);

    match array {
        Some(items) if !items.is_empty() => {
            let mut wtr = csv::Writer::from_writer(std::io::stdout());
            // Headers from the first object's keys
            if let Some(obj) = items[0].as_object() {
                let headers: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
                wtr.write_record(&headers)?;
                for item in items {
                    let row: Vec<String> = headers
                        .iter()
                        .map(|h| value_to_cell(item.get(*h)))
                        .collect();
                    wtr.write_record(&row)?;
                }
            } else {
                // Array of scalars
                wtr.write_record(["value"])?;
                for item in items {
                    wtr.write_record([value_to_cell(Some(item))])?;
                }
            }
            wtr.flush()?;
        },
        _ => {
            // Single object → key,value rows
            let mut wtr = csv::Writer::from_writer(std::io::stdout());
            wtr.write_record(["key", "value"])?;
            if let Some(obj) = data.as_object() {
                for (k, v) in obj {
                    wtr.write_record([k.as_str(), &value_to_cell(Some(v))])?;
                }
            }
            wtr.flush()?;
        },
    }
    Ok(())
}

/// Emit data as a formatted table to stdout.
fn emit_table(command: &str, data: &Value) -> Result<()> {
    let array = find_first_array(data);

    match array {
        Some(items) if !items.is_empty() => {
            if let Some(obj) = items[0].as_object() {
                let headers: Vec<String> = obj.keys().cloned().collect();
                let mut table = Table::new();
                table.set_content_arrangement(ContentArrangement::Dynamic);
                table.set_header(&headers);
                for item in items {
                    let row: Vec<String> =
                        headers.iter().map(|h| value_to_cell(item.get(h))).collect();
                    table.add_row(row);
                }
                println!("{}", table);
            } else {
                let mut table = Table::new();
                table.set_content_arrangement(ContentArrangement::Dynamic);
                table.set_header(["value"]);
                for item in items {
                    table.add_row([value_to_cell(Some(item))]);
                }
                println!("{}", table);
            }
        },
        _ => {
            // Single object → two-column key-value table
            let mut table = Table::new();
            table.set_content_arrangement(ContentArrangement::Dynamic);
            table.set_header(["Key", "Value"]);
            if let Some(obj) = data.as_object() {
                for (k, v) in obj {
                    table.add_row([k.clone(), value_to_cell(Some(v))]);
                }
            } else {
                table.add_row([command.to_string(), value_to_cell(Some(data))]);
            }
            println!("{}", table);
        },
    }
    Ok(())
}

/// Find the first JSON array value inside an object (shallow search).
fn find_first_array(data: &Value) -> Option<&Vec<Value>> {
    if let Some(arr) = data.as_array() {
        return Some(arr);
    }
    if let Some(obj) = data.as_object() {
        for v in obj.values() {
            if let Some(arr) = v.as_array() {
                return Some(arr);
            }
        }
    }
    None
}

/// Convert a JSON value to a display string for CSV/table cells.
fn value_to_cell(v: Option<&Value>) -> String {
    match v {
        None => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) => String::new(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(other) => serde_json::to_string(other).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ---- render_human_fields ------------------------------------------------

    #[test]
    fn render_human_skips_summary_key() {
        // Should not panic and should not include "summary" as a table row.
        // We cannot capture stdout here without extra scaffolding, so we
        // simply verify the function does not error on a summary-only payload.
        let data = json!({ "summary": "hello" });
        render_human_fields(&data).expect("render should succeed");
    }

    #[test]
    fn render_human_handles_array_data() {
        let data = json!({
            "summary": "items listed",
            "items": [
                {"name": "alpha", "value": 1},
                {"name": "beta", "value": 2}
            ]
        });
        render_human_fields(&data).expect("array data should render without error");
    }

    #[test]
    fn render_human_handles_scalar_fields() {
        let data = json!({
            "summary": "node status",
            "height": 12345,
            "status": "healthy",
            "syncing": false
        });
        render_human_fields(&data).expect("scalar fields should render without error");
    }

    #[test]
    fn render_human_handles_null_data() {
        render_human_fields(&Value::Null).expect("null value should be a no-op");
    }

    // ---- find_first_array ---------------------------------------------------

    #[test]
    fn find_first_array_returns_top_level_array() {
        let v = json!([1, 2, 3]);
        assert!(find_first_array(&v).is_some());
    }

    #[test]
    fn find_first_array_returns_nested_array() {
        let v = json!({"items": [1, 2, 3], "count": 3});
        assert!(find_first_array(&v).is_some());
    }

    #[test]
    fn find_first_array_returns_none_for_scalar() {
        let v = json!({"key": "val"});
        assert!(find_first_array(&v).is_none());
    }

    // ---- value_to_cell ------------------------------------------------------

    #[test]
    fn value_to_cell_string() {
        assert_eq!(value_to_cell(Some(&json!("hello"))), "hello");
    }

    #[test]
    fn value_to_cell_number() {
        assert_eq!(value_to_cell(Some(&json!(42))), "42");
    }

    #[test]
    fn value_to_cell_bool() {
        assert_eq!(value_to_cell(Some(&json!(true))), "true");
    }

    #[test]
    fn value_to_cell_null() {
        assert_eq!(value_to_cell(Some(&Value::Null)), "");
    }

    #[test]
    fn value_to_cell_none() {
        assert_eq!(value_to_cell(None), "");
    }
}
