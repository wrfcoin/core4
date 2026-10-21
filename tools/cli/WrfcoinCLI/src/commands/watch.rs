use anyhow::{Result, anyhow};
use chrono::Utc;
use log::warn;
use serde_json::{Value, json};
use std::future::Future;
use std::time::Duration;
use tokio::time::sleep;

use super::emit::emit_success;
use crate::cli::commands::ExecutionContext;

/// Configuration for the watch polling loop.
pub struct WatchConfig {
    /// Seconds between refreshes.
    pub interval: u64,
    /// If true, fetch once and exit.
    pub once: bool,
}

/// Maximum consecutive errors before the loop exits.
const MAX_CONSECUTIVE_ERRORS: u32 = 10;
/// Maximum backoff cap in seconds after repeated errors.
const MAX_BACKOFF_SECS: u64 = 30;

/// Sentinel key embedded in the returned JSON to tell the caller that
/// output was already emitted by the watch loop (so `emit_success` should
/// be skipped in `main.rs`).
pub const WATCH_SENTINEL: &str = "__watch_completed";

/// Run a polling watch loop.
///
/// `fetch` is called on every tick to produce a JSON snapshot.
/// The loop continues until Ctrl-C or `--once` mode finishes.
pub async fn watch_loop<F, Fut>(
    config: WatchConfig,
    ctx: &ExecutionContext,
    domain: &str,
    fetch: F,
) -> Result<Value>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<Value>>,
{
    let mut seq: u64 = 0;
    let mut consecutive_errors: u32 = 0;

    loop {
        seq += 1;
        let now = Utc::now().to_rfc3339();

        match fetch().await {
            Ok(data) => {
                consecutive_errors = 0;

                if ctx.json || ctx.quiet {
                    // Structured JSON snapshot per tick.
                    let snapshot = json!({
                        "ok": true,
                        "domain": domain,
                        "seq": seq,
                        "timestamp": now,
                        "data": data,
                    });
                    if ctx.quiet {
                        println!("{}", serde_json::to_string(&snapshot)?);
                    } else {
                        println!("{}", serde_json::to_string(&snapshot)?);
                    }
                } else {
                    // Human-friendly output.
                    if seq > 1 {
                        println!("--- {} watch  {} ---", domain, now);
                    } else {
                        println!("--- {} watch started  {} ---", domain, now);
                    }
                    emit_success(ctx, domain, &data)?;
                }
            },
            Err(err) => {
                consecutive_errors += 1;
                let backoff = std::cmp::min((consecutive_errors as u64) * 2, MAX_BACKOFF_SECS);

                if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                    return Err(anyhow!(
                        "{} watch: {} consecutive errors, last: {}",
                        domain,
                        consecutive_errors,
                        err
                    ));
                }

                if ctx.json || ctx.quiet {
                    let snapshot = json!({
                        "ok": false,
                        "domain": domain,
                        "seq": seq,
                        "timestamp": now,
                        "error": err.to_string(),
                        "consecutive_errors": consecutive_errors,
                        "retry_in_seconds": backoff,
                    });
                    println!("{}", serde_json::to_string(&snapshot)?);
                } else {
                    warn!(
                        "{} watch: error ({}/{}): {} — retrying in {}s",
                        domain, consecutive_errors, MAX_CONSECUTIVE_ERRORS, err, backoff
                    );
                    eprintln!(
                        "warning: {} (retry {}/{}, next in {}s)",
                        err, consecutive_errors, MAX_CONSECUTIVE_ERRORS, backoff
                    );
                }

                if config.once {
                    return Err(err);
                }

                sleep(Duration::from_secs(backoff)).await;
                continue; // skip the normal interval sleep
            },
        }

        if config.once {
            break;
        }

        // Race the interval sleep against Ctrl-C.
        tokio::select! {
            _ = sleep(Duration::from_secs(config.interval)) => {}
            _ = tokio::signal::ctrl_c() => {
                if !ctx.json && !ctx.quiet {
                    eprintln!("\n{} watch stopped", domain);
                }
                break;
            }
        }
    }

    Ok(json!({ WATCH_SENTINEL: true }))
}
