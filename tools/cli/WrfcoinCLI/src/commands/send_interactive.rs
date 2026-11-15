use anyhow::Result;
use colored::Colorize;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::cli::commands::{ExecutionContext, Transaction, Wallet};
use crate::commands::wrf_amount_to_units;
use wrfcoin_cli::output::format_wrf_units;
use wrfcoin_cli::{ApiClient, ApiConfig};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RiskWarning {
    pub level: RiskLevel,
    pub code: String,
    pub message: String,
}

/// What a transparent transfer costs. Until the chain schedules
/// `transfer_fee_burn`, core4 applies a transfer as a balance move of exactly
/// `amount` and the preview shows no fee. Once the node reports the fee in
/// force at the next block (`/network/info` `transfer_fee`), the preview shows
/// it, the remaining balance subtracts it, and the submitted body carries it.
/// The fee is burned: nobody receives it.
pub const NETWORK_FEE_NONE: &str = "none";
pub const NO_NETWORK_FEE_NOTE: &str = "This testnet charges no network fee: the recipient receives exactly the amount, and your balance drops by exactly the amount.";
pub const BURNED_NETWORK_FEE_NOTE: &str = "The network fee is burned (destroyed), not paid to anyone. The recipient receives exactly the amount; your balance drops by the amount plus the fee.";
/// Shown when the node could not be asked; never claim "no fee" then.
pub const NETWORK_FEE_UNKNOWN: &str = "unknown (the node did not report it; it charges at most its own fee)";

/// The preview's fee line for a lookup result.
pub fn network_fee_label(fee: &Result<Option<u64>>) -> String {
    match fee {
        Ok(None) => NETWORK_FEE_NONE.to_string(),
        Ok(Some(units)) => format!("{} (burned)", format_wrf_units(*units)),
        Err(_) => NETWORK_FEE_UNKNOWN.to_string(),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransactionPreview {
    pub from: String,
    pub to: String,
    pub amount: u64,
    pub network_fee: String,
    /// The burned fee in base units, when the node reports one in force.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_fee_base_units: Option<u64>,
    pub remaining_balance: u64,
    pub warnings: Vec<RiskWarning>,
}

/// `wallet send` options the chain cannot honour. They are refused before
/// any request, instead of being dropped silently: the submitted transparent
/// transfer has no memo field and pays no fee.
pub fn reject_unsupported_send_options(fee: Option<f64>, memo: Option<&str>) -> Result<()> {
    if memo.is_some() {
        anyhow::bail!(
            "--memo is not supported: a transparent transfer carries no memo on this chain, so it would be dropped. Send again without --memo."
        );
    }
    if fee.is_some() {
        anyhow::bail!(
            "--fee is not supported: the chain sets the network fee (shown in the preview; none until the burned transfer fee is switched on), so there is nothing to choose. Send again without --fee."
        );
    }
    Ok(())
}

/// Entry point for interactive/dry-run send.
pub async fn handle_send_interactive(
    to: &str,
    amount: f64,
    dry_run: bool,
    nonce_override: Option<u64>,
    ctx: &ExecutionContext,
) -> Result<Value> {
    let wallet = Arc::new(RwLock::new(Wallet::try_default()?));
    let w = wallet.read().await;

    // Resolve the primary address
    let from = w.get_primary_address().ok_or_else(|| {
        anyhow::anyhow!("No primary wallet address. Run: wrfcoin wallet new-address")
    })?;
    if !dry_run {
        w.primary_signing_address()?;
    }

    // Chain state is the only balance/nonce source of truth.
    let client = ApiClient::new(ApiConfig::default())?;
    let account = client.get_address(&from).await?;
    let balance = account
        .spendable_balance
        .parse::<u64>()
        .map_err(|_| anyhow::anyhow!("node returned an invalid spendable balance"))?;

    let amount_units = wrf_amount_to_units(amount)?;
    let fee = client.get_transfer_fee().await;
    let fee_units = fee.as_ref().ok().copied().flatten();
    let debit = amount_units.saturating_add(fee_units.unwrap_or(0));

    // Risk assessment
    let history = w.get_transaction_history();
    let mut warnings = assess_risks(debit, balance, to, &history);
    if debit > balance {
        warnings.push(RiskWarning {
            level: RiskLevel::High,
            code: "INSUFFICIENT_FOR_AMOUNT_AND_FEE".to_string(),
            message: format!(
                "Spendable balance {} does not cover {} plus the network fee; the chain will refuse this send",
                format_wrf_units(balance),
                format_wrf_units(amount_units)
            ),
        });
    }
    let remaining_balance = balance.saturating_sub(debit);

    // Build preview
    let preview = TransactionPreview {
        from: from.clone(),
        to: to.to_string(),
        amount: amount_units,
        network_fee: network_fee_label(&fee),
        network_fee_base_units: fee_units,
        remaining_balance,
        warnings,
    };

    // Dry-run: return immediately
    if dry_run {
        return Ok(json!({
            "dry_run": true,
            "preview": preview,
        }));
    }

    // Interactive confirmation
    if !ctx.non_interactive && !ctx.output_format.is_machine_readable() {
        render_preview(&preview);

        let confirmed = dialoguer::Confirm::new()
            .with_prompt("Proceed with this transaction?")
            .default(false)
            .interact()?;

        if !confirmed {
            return Ok(json!({
                "cancelled": true,
                "preview": preview,
            }));
        }
    }

    // Execute the transaction
    let nonce = match (account.nonce.as_deref(), nonce_override) {
        (Some(raw), _) => raw
            .parse::<u64>()
            .map_err(|_| anyhow::anyhow!("node returned an invalid account nonce"))?,
        (None, Some(value)) => value,
        (None, None) => {
            return Err(anyhow::anyhow!(
                "The node does not expose an account nonce yet; pass `--nonce <u64>` explicitly"
            ));
        },
    };
    let (from, auth_pubkey, auth_signature) =
        w.sign_transparent_transfer(to, amount_units, nonce)?;
    drop(w);
    // `security_key_spend`: a protected account also needs its key.
    let security_key_auth = crate::commands::security_key::spend_assertion_if_required(
        &client, &from, to, amount_units, nonce,
    )
    .await?;
    let accepted = client
        .submit_transparent_transfer_with_security_key(
            &from,
            to,
            amount_units,
            nonce,
            &auth_pubkey,
            &auth_signature,
            fee_units,
            security_key_auth.as_ref(),
        )
        .await?;

    Ok(json!({
        "summary": format!(
            "Submitted {} WRF to {} (tx_id {}) — not yet confirmed",
            format_wrf_units(amount_units),
            to,
            accepted.tx_id,
        ),
        "status": "submitted",
        "tx_id": accepted.tx_id,
        "preview": preview,
    }))
}

/// Assess risks for the transaction.
fn assess_risks(amount: u64, balance: u64, to: &str, history: &[Transaction]) -> Vec<RiskWarning> {
    let mut warnings = Vec::new();

    // Unknown recipient
    let known_addresses: HashSet<&str> = history
        .iter()
        .flat_map(|tx| [tx.from.as_str(), tx.to.as_str()])
        .collect();
    if !known_addresses.contains(to) {
        warnings.push(RiskWarning {
            level: RiskLevel::Low,
            code: "UNKNOWN_RECIPIENT".to_string(),
            message: "Recipient address has not appeared in your transaction history".to_string(),
        });
    }

    // Low remaining balance
    let remaining = balance.saturating_sub(amount);
    if balance > 0 && (remaining as f64 / balance as f64) < 0.10 {
        warnings.push(RiskWarning {
            level: RiskLevel::High,
            code: "LOW_BALANCE".to_string(),
            message: format!(
                "Only {:.4} WRF ({:.1}%) will remain after this transaction",
                remaining as f64 / wrfcoin_cli::output::WRF_UNITS_PER_COIN as f64,
                (remaining as f64 / balance as f64) * 100.0
            ),
        });
    }

    // Large transaction relative to balance
    if balance > 0 && amount as f64 / balance as f64 > 0.50 {
        warnings.push(RiskWarning {
            level: RiskLevel::Medium,
            code: "LARGE_TRANSACTION".to_string(),
            message: format!(
                "This transaction is {:.1}% of your total balance",
                (amount as f64 / balance as f64) * 100.0
            ),
        });
    }

    warnings
}

/// Render a human-readable preview to the terminal.
fn render_preview(preview: &TransactionPreview) {
    println!();
    println!("{}", "=== Transaction Preview ===".bold());
    println!("  {} {}", "From:".dimmed(), preview.from);
    println!("  {} {}", "To:".dimmed(), preview.to);
    println!(
        "  {} {}",
        "Amount:".dimmed(),
        format_wrf_units(preview.amount)
    );
    println!("  {} {}", "Network fee:".dimmed(), preview.network_fee);
    println!(
        "  {} {}",
        "Remaining balance:".dimmed(),
        format_wrf_units(preview.remaining_balance)
    );
    let note = if preview.network_fee_base_units.is_some() {
        BURNED_NETWORK_FEE_NOTE
    } else if preview.network_fee == NETWORK_FEE_NONE {
        NO_NETWORK_FEE_NOTE
    } else {
        ""
    };
    if !note.is_empty() {
        println!("  {}", note.dimmed());
    }

    if !preview.warnings.is_empty() {
        println!();
        println!("{}", "Warnings:".yellow().bold());
        for w in &preview.warnings {
            let prefix = match w.level {
                RiskLevel::Low => "  [LOW]".dimmed().to_string(),
                RiskLevel::Medium => "  [MEDIUM]".yellow().to_string(),
                RiskLevel::High => "  [HIGH]".red().bold().to_string(),
            };
            println!("{} {}", prefix, w.message);
        }
    }
    println!();
}
