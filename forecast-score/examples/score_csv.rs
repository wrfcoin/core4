//! Score a predictions-plus-truth CSV with this crate's continuous metrics.
//!
//! The point of this example is that the number quoted for a model comes from
//! the chain's own scoring library, not from a Python reimplementation of it.
//! The trainer (`tools/forecast-proto/gpu/train_forecast.py`) writes the
//! held-out predictions it evaluated; this reads them back and prints what
//! `ForecastAccuracyMetrics` says about them.
//!
//! Input CSV columns (header required, order free, extra columns ignored):
//!
//! ```text
//! station,target_time_utc,variable,predicted,observed
//! ```
//!
//! Rows are grouped by `variable`; different physical units are never pooled.
//!
//! Usage:
//!
//! ```sh
//! cargo run -p wrfcoin-forecast-score --example score_csv -- \
//!     --predictions holdout-model.csv [--reference holdout-persistence-trend.csv] [--json]
//! ```
//!
//! Per variable it prints `n`, `mae`, `rmse`, `bias` and the crate's
//! `skill_score` (`1 - rmse / stddev(observed)`, clamped to `[0, 1]`, the
//! `ReferenceStrategy::SampleStdDev` definition). With `--reference`, a
//! second CSV in the same layout (for example the shipped
//! `persistence-trend/v1` forecasts for the same targets) supplies a
//! reference RMSE per variable and the crate's
//! `ReferenceStrategy::FixedClimatology { reference_rmse }` scorer reports the
//! skill against that reference as `skill_vs_reference`.
//!
//! Note: the receipt the on-chain prototype signs scores each forecast with
//! `1 - MAE / MAE_window_climatology` per variable (tools/forecast-proto
//! `skill::score`); that rule lives in the producer, not in this crate, so it
//! is not reproduced here.

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;

use wrfcoin_forecast_score::anti_gaming::ReferenceStrategy;
use wrfcoin_forecast_score::{ForecastAccuracyMetrics, Pair, rmse};

struct Args {
    predictions: String,
    reference: Option<String>,
    json: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut predictions = None;
    let mut reference = None;
    let mut json = false;
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--predictions" => predictions = it.next(),
            "--reference" => reference = it.next(),
            "--json" => json = true,
            "-h" | "--help" => {
                return Err(
                    "usage: score_csv --predictions <csv> [--reference <csv>] [--json]".to_string(),
                );
            },
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(Args {
        predictions: predictions.ok_or("--predictions <csv> is required")?,
        reference,
        json,
    })
}

/// Read `variable -> pairs` from a CSV with the documented header.
fn read_pairs(path: &str) -> Result<BTreeMap<String, Vec<Pair>>, Box<dyn Error>> {
    let text = fs::read_to_string(path)?;
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header = lines.next().ok_or("empty CSV")?;
    let columns: Vec<&str> = header.split(',').map(str::trim).collect();
    let index = |name: &str| -> Result<usize, Box<dyn Error>> {
        columns
            .iter()
            .position(|c| *c == name)
            .ok_or_else(|| format!("missing column {name} in {path}").into())
    };
    let (variable_col, predicted_col, observed_col) =
        (index("variable")?, index("predicted")?, index("observed")?);
    let mut out: BTreeMap<String, Vec<Pair>> = BTreeMap::new();
    for (line_no, line) in lines.enumerate() {
        let fields: Vec<&str> = line.split(',').map(str::trim).collect();
        let field = |i: usize| -> Result<&str, Box<dyn Error>> {
            fields
                .get(i)
                .copied()
                .ok_or_else(|| format!("{path}:{}: short row", line_no + 2).into())
        };
        let predicted: f64 = field(predicted_col)?.parse()?;
        let observed: f64 = field(observed_col)?.parse()?;
        if !predicted.is_finite() || !observed.is_finite() {
            return Err(format!("{path}:{}: non-finite value", line_no + 2).into());
        }
        out.entry(field(variable_col)?.to_string())
            .or_default()
            .push(Pair::new(predicted, observed));
    }
    Ok(out)
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = match parse_args() {
        Ok(a) => a,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(2);
        },
    };
    let predictions = read_pairs(&args.predictions)?;
    let reference = match &args.reference {
        Some(path) => Some(read_pairs(path)?),
        None => None,
    };

    let mut rows = Vec::new();
    let mut skill_sum = 0.0;
    let mut skill_ref_sum = 0.0;
    let mut n_vars = 0usize;
    for (variable, pairs) in &predictions {
        let metrics = ForecastAccuracyMetrics::from_pairs(pairs);
        metrics.validate()?;
        let mut row = serde_json::json!({
            "variable": variable,
            "n": pairs.len(),
            "mae": metrics.mae,
            "rmse": metrics.rmse,
            "bias": metrics.bias,
            "skill_score": metrics.skill_score,
        });
        if let Some(reference) = &reference {
            let ref_pairs = reference
                .get(variable)
                .ok_or_else(|| format!("reference CSV has no rows for {variable}"))?;
            if ref_pairs.len() != pairs.len() {
                return Err(format!(
                    "{variable}: reference has {} rows, predictions have {}",
                    ref_pairs.len(),
                    pairs.len()
                )
                .into());
            }
            let reference_rmse = rmse(ref_pairs);
            let scored = ForecastAccuracyMetrics::from_pairs_with_reference(
                pairs,
                ReferenceStrategy::FixedClimatology { reference_rmse },
            )?;
            row["reference_rmse"] = serde_json::json!(reference_rmse);
            row["skill_vs_reference"] = serde_json::json!(scored.skill_score);
            skill_ref_sum += scored.skill_score;
        }
        skill_sum += metrics.skill_score;
        n_vars += 1;
        rows.push(row);
    }
    if n_vars == 0 {
        return Err("no rows".into());
    }
    let mut summary = serde_json::json!({
        "predictions": args.predictions,
        "variables": rows,
        "mean_skill_score": skill_sum / n_vars as f64,
    });
    if reference.is_some() {
        summary["reference"] = serde_json::json!(args.reference);
        summary["mean_skill_vs_reference"] = serde_json::json!(skill_ref_sum / n_vars as f64);
    }

    if args.json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
        return Ok(());
    }
    println!("forecast-score {} — {}", env!("CARGO_PKG_VERSION"), args.predictions);
    println!(
        "{:<16} {:>6} {:>9} {:>9} {:>9} {:>11}{}",
        "variable",
        "n",
        "mae",
        "rmse",
        "bias",
        "skill_score",
        if reference.is_some() { "   ref_rmse  skill_vs_ref" } else { "" }
    );
    for row in summary["variables"].as_array().unwrap() {
        print!(
            "{:<16} {:>6} {:>9.4} {:>9.4} {:>9.4} {:>11.4}",
            row["variable"].as_str().unwrap(),
            row["n"],
            row["mae"].as_f64().unwrap(),
            row["rmse"].as_f64().unwrap(),
            row["bias"].as_f64().unwrap(),
            row["skill_score"].as_f64().unwrap(),
        );
        if let Some(r) = row.get("reference_rmse") {
            print!(
                " {:>10.4} {:>13.4}",
                r.as_f64().unwrap(),
                row["skill_vs_reference"].as_f64().unwrap()
            );
        }
        println!();
    }
    print!(
        "mean skill_score over {} variables: {:.4}",
        n_vars,
        summary["mean_skill_score"].as_f64().unwrap()
    );
    if let Some(v) = summary.get("mean_skill_vs_reference") {
        print!("; mean skill_vs_reference: {:.4}", v.as_f64().unwrap());
    }
    println!();
    Ok(())
}
